//! Reconcile settings and layout with running wallpapers; handle CLI and UI requests.

pub mod library;
pub mod playback;
pub mod stream;

use crate::audio::Capture;
use crate::content::{Content, ContentEvent, ContentId, PointerEvent, PointerKind, Seek, View};
use crate::error::{Error, Result, ctx};
use crate::geom::Size;
use crate::ipc::server::{Reply, Server};
use crate::ipc::{ActiveInfo, Capabilities, Event, InfoPatch, Request, Response, Status};
use crate::model::display;
use crate::model::props::Properties;
use crate::model::wallpaper::PropertySource;
use crate::model::{Arrangement, Display, Kind, Layout, Placement, Settings, Wallpaper};
use crate::msg::{Msg, TrayAction};
use crate::paths::Paths;
use crate::platform::{
    ContentSpec, MsgSender, MsgSenderApi, Runtime, RuntimeApi, ShellApi, Slot, Snapshot,
};
use crate::tray::Tray;
use library::{Library, THUMBNAIL};
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const RESET_BUTTON: &str = "lively_default_settings_reload";

struct Active {
    id: ContentId,
    placement: Placement,
    wallpaper: Wallpaper,
    _slot: Slot,
    content: Box<dyn Content>,
    view: View,
    props_path: Option<PathBuf>,
    loaded: bool,
    paused: Option<bool>,
    volume: Option<u8>,
    started: Instant,
    thumbnail_pending: bool,
}

struct PendingShot {
    id: ContentId,
    path: PathBuf,
    thumbnail: Option<String>,
    reply: Reply,
    deadline: Instant,
}

pub struct Engine {
    rt: Runtime,
    paths: Paths,
    settings: Settings,
    layout: Layout,
    displays: Vec<Display>,
    library: Library,
    active: Vec<Active>,
    next_id: ContentId,
    windows: Option<Snapshot>,
    locked: bool,
    battery: Option<starship_battery::Manager>,
    on_battery: bool,
    ticks: u64,
    user_paused: bool,
    server: Server,
    tray: Option<Tray>,
    audio: Option<Capture>,
    monitor_name: String,
    capabilities: Capabilities,
    pending_shots: Vec<PendingShot>,
}

impl Engine {
    pub fn new(
        rt: Runtime,
        paths: Paths,
        settings: Settings,
        layout: Layout,
        server: Server,
    ) -> Result<Engine> {
        let library = Library {
            dir: settings.library_dir.clone(),
        };
        ctx(std::fs::create_dir_all(&library.dir), library.dir.display())?;
        let mut displays = rt.displays();
        display::sort(&mut displays);
        let mut engine = Engine {
            rt,
            paths,
            settings,
            layout,
            displays,
            library,
            active: Vec::new(),
            next_id: 1,
            windows: None,
            locked: false,
            battery: starship_battery::Manager::new().ok(),
            on_battery: false,
            ticks: 0,
            user_paused: false,
            server,
            tray: None,
            audio: None,
            monitor_name: String::new(),
            capabilities: Capabilities::default(),
            pending_shots: Vec::new(),
        };
        engine.rt.shell().sync_displays(&engine.displays)?;
        engine.capabilities = engine.rt.shell().capabilities();
        engine.monitor_name = engine.rt.start_window_monitor(
            engine.settings.rules.interval_ms,
            engine.settings.input.forward_mouse,
        );
        log::info!(
            "session {} presented by {} with window monitor '{}', {} display(s)",
            engine.rt.session(),
            engine.capabilities.presenter,
            engine.monitor_name,
            engine.displays.len()
        );
        if engine.settings.tray {
            match Tray::new(engine.rt.sender(), false) {
                Ok(t) => engine.tray = Some(t),
                Err(e) => log::warn!("{e}"),
            }
        }
        if let Err(e) = crate::autostart::apply(engine.settings.autostart) {
            log::warn!("{e}");
        }
        engine.refresh_battery();
        engine.reconcile();
        Ok(engine)
    }

    pub fn sender(&self) -> MsgSender {
        self.rt.sender()
    }

    pub fn interval(&self) -> u64 {
        self.settings.rules.interval_ms
    }

    /// Handle one message; `false` ends the daemon.
    pub fn handle(&mut self, msg: Msg) -> bool {
        let keep = self.dispatch(msg);
        self.expire_captures();
        self.rt.shell().settle();
        keep
    }

    fn dispatch(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Request(req, reply) => return self.request(req, reply),
            Msg::Tick => self.tick(),
            Msg::Displays(list) => self.displays_changed(list),
            Msg::DesktopChanged => self.desktop_changed(),
            Msg::WallpaperDismissed { display } => self.wallpaper_dismissed(&display),
            Msg::Windows(snapshot) => {
                self.windows = Some(snapshot);
                self.evaluate();
            }
            Msg::Session { locked } => {
                if self.locked != locked {
                    log::info!("session {}", if locked { "locked" } else { "unlocked" });
                    self.locked = locked;
                    self.evaluate();
                }
            }
            Msg::Content(id, ev) => self.content_event(id, ev),
            Msg::Tray(action) => return self.tray_action(action),
            Msg::Pointer { x, y, kind } => self.pointer(x, y, kind),
            Msg::Audio(bins) => {
                for a in self
                    .active
                    .iter_mut()
                    .filter(|a| a.wallpaper.kind() == Kind::WebAudio)
                {
                    a.content.audio_data(&bins);
                }
            }
            Msg::Job(job) => job(self),
            Msg::Quit => return self.shutdown(),
        }
        true
    }

    fn shutdown(&mut self) -> bool {
        log::info!("shutting down");
        self.active.clear();
        self.rt.shell().settle();
        self.audio = None;
        self.tray = None;
        false
    }

    /// The desktop reassigned its wallpaper areas: re-map displays and rebuild what moved.
    fn desktop_changed(&mut self) {
        match self.rt.shell().sync_displays(&self.displays) {
            Ok(true) => {
                log::info!("desktop layout changed; restarting wallpapers");
                self.active.clear();
            }
            Ok(false) => {}
            Err(e) => self.report(&e),
        }
        self.reconcile();
    }

    fn wallpaper_dismissed(&mut self, display: &str) {
        let Some(d) = self.displays.iter().find(|d| d.id == display).cloned() else {
            return;
        };
        self.active.retain(|a| a.placement.display != d.id);
        self.layout.clear_display(&d.id);
        self.save_layout();
        self.audio_sync();
        self.broadcast(Event::Info {
            message: format!(
                "{} switched to another wallpaper in the desktop settings",
                d.name
            ),
        });
        self.broadcast(Event::Playback);
    }

    fn broadcast(&self, ev: Event) {
        self.server.broadcast(ev);
    }

    fn report(&self, e: &Error) {
        log::warn!("{e}");
        self.broadcast(Event::Error {
            message: e.to_string(),
        });
    }

    fn save_layout(&self) {
        if let Err(e) = self.layout.save(&self.paths.layout_file()) {
            log::warn!("save layout: {e}");
        }
        self.broadcast(Event::Layout);
    }

    fn display(&self, reference: Option<&str>) -> Result<Display> {
        match reference.map(str::trim).filter(|r| !r.is_empty()) {
            None => display::primary(&self.displays)
                .cloned()
                .ok_or_else(|| Error::NotFound("no displays connected".into())),
            Some(r) => display::find(&self.displays, r)
                .cloned()
                .ok_or_else(|| Error::NotFound(format!("no display '{r}'"))),
        }
    }

    fn slot_key(&self, display_id: &str) -> String {
        match self.layout.arrangement {
            Arrangement::Per => display_id.to_string(),
            Arrangement::Span => "span".into(),
            Arrangement::Duplicate => "duplicate".into(),
        }
    }

    /// Per-slot property copy, created from the wallpaper's template on first use. Built-in
    /// media copies are trimmed to the controls the wallpaper's kind has.
    fn ensure_props(&self, wp: &Wallpaper, slot: &str) -> Result<Option<PathBuf>> {
        let builtin = (wp.properties == PropertySource::BuiltinMedia)
            .then(|| crate::model::props::media_defaults(wp.kind()));
        let template = match &wp.properties {
            PropertySource::None => return Ok(None),
            PropertySource::File(p) => ctx(std::fs::read_to_string(p), p.display())?,
            PropertySource::BuiltinMedia => builtin
                .as_ref()
                .map(Properties::to_json)
                .transpose()?
                .unwrap_or_default(),
        };
        let dir = self.paths.properties_dir().join(&wp.id);
        ctx(std::fs::create_dir_all(&dir), dir.display())?;
        let path = dir.join(format!("{}.json", crate::paths::slug(slot)));
        if !path.is_file() {
            crate::paths::write(&path, template)?;
        } else if let Some(allowed) = builtin {
            let mut existing = Properties::load(&path)?;
            if existing.retain(|name| allowed.get(name).is_some()) {
                existing.save(&path)?;
            }
        }
        Ok(Some(path))
    }

    fn reset_props(&self, wp: &Wallpaper, slot: &str) -> Result<Option<PathBuf>> {
        if let Some(p) = self.ensure_props(wp, slot)? {
            let _ = std::fs::remove_file(&p);
        }
        self.ensure_props(wp, slot)
    }

    fn reconcile(&mut self) {
        let desired = self
            .layout
            .plan(&self.displays, self.rt.shell().spans_displays());
        self.active.retain(|a| desired.contains(&a.placement));
        for p in desired {
            if self.active.iter().any(|a| a.placement == p) {
                continue;
            }
            match self.spawn(p.clone()) {
                Ok(a) => self.active.push(a),
                Err(e) => {
                    self.report(&Error::Media(format!("{}: {e}", p.wallpaper)));
                    if matches!(e, Error::NotFound(_) | Error::Unsupported(_)) {
                        self.layout.remove_wallpaper(&p.wallpaper);
                    }
                }
            }
        }
        self.sync_views();
        self.evaluate();
        self.audio_sync();
        self.save_layout();
    }

    fn spawn(&mut self, p: Placement) -> Result<Active> {
        let wallpaper = self.library.get(&p.wallpaper)?;
        let display = self
            .displays
            .iter()
            .find(|d| d.id == p.display)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("display {} is gone", p.display)))?;
        let slot = self.rt.shell().slot(&display, p.region)?;
        let props_path = self.ensure_props(&wallpaper, &p.slot)?;
        let id = self.next_id;
        self.next_id += 1;
        let spec = ContentSpec {
            id,
            wallpaper: &wallpaper,
            audio: p.audio,
            volume: self.settings.volume,
            settings: &self.settings,
        };
        let mut content = self.rt.spawn_content(&spec, &slot)?;
        let view = self.view_of(&p, &display);
        content.set_view(&view)?;
        content.set_muted(!p.audio);
        content.set_input_enabled(self.settings.input.forward_mouse);
        log::info!(
            "started '{}' on {} ({}x{})",
            wallpaper.title(),
            display.name,
            p.region.w,
            p.region.h
        );
        Ok(Active {
            id,
            placement: p,
            wallpaper,
            _slot: slot,
            content,
            view,
            props_path,
            loaded: false,
            paused: None,
            volume: None,
            started: Instant::now(),
            thumbnail_pending: false,
        })
    }

    /// What an instance shows: its display's part of the spanning image, or all of its region.
    fn view_of(&self, p: &Placement, d: &Display) -> View {
        if p.spanning {
            self.layout.view_for(d, Layout::span_bounds(&self.displays))
        } else {
            View::whole(Size {
                w: p.region.w,
                h: p.region.h,
            })
        }
    }

    /// Push changed views to running instances without restarting them.
    fn sync_views(&mut self) {
        let wanted: Vec<View> = self
            .active
            .iter()
            .map(
                |a| match self.displays.iter().find(|d| d.id == a.placement.display) {
                    Some(d) => self.view_of(&a.placement, d),
                    None => a.view,
                },
            )
            .collect();
        let mut problems = Vec::new();
        for (a, view) in self.active.iter_mut().zip(wanted) {
            if a.view != view {
                match a.content.set_view(&view) {
                    Ok(()) => a.view = view,
                    Err(e) => problems.push(e),
                }
            }
        }
        for e in &problems {
            self.report(e);
        }
    }

    /// Whether the span wallpaper can show `layout`'s views on this desktop.
    fn check_alignment(&self, layout: &Layout) -> Result<()> {
        if layout.arrangement != Arrangement::Span {
            return Err(Error::Invalid(
                "alignment applies to the span arrangement".into(),
            ));
        }
        let Some(kind) = layout
            .shared
            .as_deref()
            .and_then(|id| self.library.get(id).ok())
            .map(|w| w.kind())
        else {
            return Ok(());
        };
        let bounds = Layout::span_bounds(&self.displays);
        for d in &self.displays {
            let v = layout.view_for(d, bounds);
            if kind == Kind::Program && !v.is_plain() {
                return Err(Error::Unsupported(
                    "program wallpapers can be moved but not scaled or rotated".into(),
                ));
            }
            if kind.is_web() && v.rotation.abs() > 1e-9 && !self.capabilities.rotate_web {
                return Err(Error::Unsupported(
                    "web wallpapers cannot be rotated on this desktop".into(),
                ));
            }
        }
        Ok(())
    }

    /// Apply `change` to the alignment when the running wallpaper can show the result.
    fn align(&mut self, change: impl FnOnce(&mut Layout)) -> Result<()> {
        let mut candidate = self.layout.clone();
        change(&mut candidate);
        self.check_alignment(&candidate)?;
        self.layout = candidate;
        self.reconcile();
        Ok(())
    }

    fn apply_properties(&mut self, idx: usize) {
        let Some(path) = self.active[idx].props_path.clone() else {
            return;
        };
        let props = match Properties::load(&path) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("{}: {e}", path.display());
                return;
            }
        };
        let a = &mut self.active[idx];
        for (name, control) in props.controls() {
            if control.is_interactive_only() {
                continue;
            }
            let value = control.value();
            a.content.apply(&name, &control, value.as_ref());
        }
    }

    fn evaluate(&mut self) {
        let global_pause = self.user_paused
            || (self.locked && self.settings.rules.lock_pause)
            || (self.on_battery && self.settings.rules.battery_pause);
        let decisions = playback::decide(&playback::Inputs {
            rules: &self.settings.rules,
            volume: self.settings.volume,
            audio_only_on_desktop: self.settings.audio_only_on_desktop,
            audio_output: &self.settings.audio_output,
            displays: &self.displays,
            windows: self.windows.as_ref(),
            global_pause,
        });
        let mut changed = false;
        for a in &mut self.active {
            let Some(d) = decisions.get(&a.placement.display) else {
                continue;
            };
            if a.paused != Some(d.pause) {
                a.content.set_paused(d.pause);
                a.paused = Some(d.pause);
                changed = true;
            }
            let volume = if a.placement.audio { d.volume } else { 0 };
            if a.volume != Some(volume) {
                a.content.set_volume(volume);
                a.volume = Some(volume);
            }
        }
        if changed {
            self.broadcast(Event::Playback);
        }
    }

    fn audio_sync(&mut self) {
        let wanted = self
            .active
            .iter()
            .any(|a| a.wallpaper.kind() == Kind::WebAudio);
        if wanted && self.audio.is_none() {
            match Capture::start(self.settings.audio_capture_device.clone(), self.rt.sender()) {
                Ok(c) => self.audio = Some(c),
                Err(e) => self.report(&e),
            }
        } else if !wanted {
            self.audio = None;
        }
    }

    fn refresh_battery(&mut self) {
        let Some(m) = &mut self.battery else { return };
        let discharging = m
            .batteries()
            .map(|it| {
                it.flatten()
                    .any(|b| b.state() == starship_battery::State::Discharging)
            })
            .unwrap_or(false);
        if discharging != self.on_battery {
            self.on_battery = discharging;
            self.evaluate();
        }
    }

    fn tick(&mut self) {
        self.ticks += 1;
        if self.ticks % 20 == 0 {
            self.refresh_battery();
        }
        let timeout = Duration::from_secs(self.settings.video.load_timeout_secs);
        let mut failed = Vec::new();
        for a in &self.active {
            if !a.loaded && a.started.elapsed() > timeout {
                failed.push((a.id, a.wallpaper.title()));
            }
        }
        for (id, title) in failed {
            self.active.retain(|a| a.id != id);
            self.report(&Error::Media(format!(
                "'{title}' did not load within {}s",
                timeout.as_secs()
            )));
        }
        self.evaluate();
    }

    fn displays_changed(&mut self, mut list: Vec<Display>) {
        display::sort(&mut list);
        if list == self.displays {
            return;
        }
        log::info!(
            "displays changed: {}",
            list.iter()
                .map(|d| format!(
                    "{} {}x{}+{}+{}",
                    d.name, d.rect.w, d.rect.h, d.rect.x, d.rect.y
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
        self.displays = list;
        match self.rt.shell().sync_displays(&self.displays) {
            Ok(true) => self.active.clear(),
            Ok(false) => {}
            Err(e) => self.report(&e),
        }
        self.reconcile();
        self.broadcast(Event::Displays);
    }

    fn content_event(&mut self, id: ContentId, ev: ContentEvent) {
        if let ContentEvent::Screenshot { path, result } = ev {
            self.finish_capture(id, &path, result);
            return;
        }
        let Some(idx) = self.active.iter().position(|a| a.id == id) else {
            return;
        };
        match ev {
            ContentEvent::Loaded => {
                if self.active[idx].loaded {
                    return;
                }
                self.active[idx].loaded = true;
                self.apply_properties(idx);
                // Navigation resets the page transform.
                let view = self.active[idx].view;
                if let Err(e) = self.active[idx].content.set_view(&view) {
                    self.report(&e);
                }
                self.active[idx].paused = None;
                self.active[idx].volume = None;
                self.evaluate();
                let a = &mut self.active[idx];
                if self.settings.thumbnails
                    && a.wallpaper.thumbnail.is_none()
                    && !a.thumbnail_pending
                {
                    a.thumbnail_pending = true;
                    let tx = self.rt.sender();
                    let cid = a.id;
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(2500));
                        tx.send(Msg::Job(Box::new(move |e| e.capture_thumbnail(cid))));
                    });
                }
                self.broadcast(Event::Playback);
            }
            ContentEvent::Exited { reason } => {
                let a = self.active.remove(idx);
                self.report(&Error::Media(format!(
                    "'{}' stopped: {reason}",
                    a.wallpaper.title()
                )));
                self.audio_sync();
                self.broadcast(Event::Playback);
            }
            ContentEvent::Screenshot { .. } => unreachable!(),
        }
    }

    fn capture(&mut self, idx: usize, path: PathBuf, thumbnail: Option<String>, reply: Reply) {
        if self.pending_shots.iter().any(|shot| shot.path == path) {
            reply(Response::error(&Error::Media(
                "capture already in progress for this file".into(),
            )));
            return;
        }
        let id = self.active[idx].id;
        self.pending_shots.push(PendingShot {
            id,
            path: path.clone(),
            thumbnail,
            reply,
            deadline: Instant::now() + Duration::from_secs(30),
        });
        self.active[idx].content.screenshot(path);
    }

    fn finish_capture(&mut self, id: ContentId, path: &std::path::Path, result: Result<()>) {
        let Some(pos) = self
            .pending_shots
            .iter()
            .position(|shot| shot.id == id && shot.path == path)
        else {
            return;
        };
        let shot = self.pending_shots.remove(pos);
        let result = result.and_then(|_| {
            if let Some(wallpaper) = shot.thumbnail {
                crate::capture::shrink(path, crate::media::thumb::WIDTH)?;
                self.set_thumbnail(&wallpaper, THUMBNAIL)?;
            }
            Ok(())
        });
        (shot.reply)(result.map_or_else(|e| Response::error(&e), |_| Response::Ok));
    }

    fn expire_captures(&mut self) {
        for shot in std::mem::take(&mut self.pending_shots) {
            let error = if !self.active.iter().any(|a| a.id == shot.id) {
                "wallpaper stopped before capture completed"
            } else if Instant::now() >= shot.deadline {
                "capture timed out"
            } else {
                self.pending_shots.push(shot);
                continue;
            };
            (shot.reply)(Response::error(&Error::Media(error.into())));
        }
    }

    fn capture_thumbnail(&mut self, id: ContentId) {
        if let Some(idx) = self.active.iter().position(|a| a.id == id) {
            let a = &self.active[idx];
            let path = a.wallpaper.dir.join(THUMBNAIL);
            let wallpaper = a.wallpaper.id.clone();
            self.capture(
                idx,
                path,
                Some(wallpaper),
                Box::new(|response| {
                    if let Response::Error { message, .. } = response {
                        log::warn!("thumbnail: {message}");
                    }
                }),
            );
        }
    }

    fn set_thumbnail(&mut self, wallpaper: &str, file: &str) -> Result<()> {
        let mut wp = self.library.get(wallpaper)?;
        wp.info.thumbnail = Some(file.into());
        wp.save_info()?;
        let updated = Wallpaper::from_info(&wp.dir, wp.info.clone());
        for a in self
            .active
            .iter_mut()
            .filter(|a| a.wallpaper.id == wallpaper)
        {
            a.wallpaper = updated.clone();
        }
        self.broadcast(Event::Library);
        Ok(())
    }

    fn pointer(&mut self, x: i32, y: i32, kind: PointerKind) {
        if !self.settings.input.forward_mouse {
            return;
        }
        if kind == PointerKind::Move
            && !self.settings.input.always_move
            && self
                .windows
                .as_ref()
                .is_some_and(|s| s.windows.iter().any(|w| w.focused))
        {
            return;
        }
        let Some(display) = display::at_point(&self.displays, x, y).cloned() else {
            return;
        };
        for a in self
            .active
            .iter_mut()
            .filter(|a| a.placement.display == display.id && a.wallpaper.kind().accepts_pointer())
        {
            let slot = Size {
                w: a.placement.region.w,
                h: a.placement.region.h,
            };
            let (ix, iy) = a.view.to_image(
                slot,
                (x - a.placement.region.x) as f64,
                (y - a.placement.region.y) as f64,
            );
            a.content.pointer(PointerEvent {
                x: ix.round() as i32,
                y: iy.round() as i32,
                kind,
            });
        }
    }

    fn tray_action(&mut self, action: TrayAction) -> bool {
        match action {
            TrayAction::OpenUi => self.open_ui(),
            TrayAction::TogglePause => self.set_user_paused(!self.user_paused),
            TrayAction::CloseAll => {
                self.layout.clear();
                self.reconcile();
            }
            TrayAction::Random => self.random_all(),
            TrayAction::Quit => return self.shutdown(),
        }
        true
    }

    fn set_user_paused(&mut self, paused: bool) {
        self.user_paused = paused;
        if let Some(t) = &self.tray {
            t.set_paused(paused);
        }
        self.evaluate();
        self.broadcast(Event::Playback);
    }

    fn open_ui(&self) {
        if let Ok(exe) = std::env::current_exe() {
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("ui")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0000_0008);
            }
            if let Err(e) = cmd.spawn() {
                log::warn!("open ui: {e}");
            }
        }
    }

    fn random(&self, exclude: Option<&str>) -> Option<Wallpaper> {
        let mut all = self.library.scan();
        if all.len() > 1 {
            all.retain(|w| Some(w.id.as_str()) != exclude);
        }
        if all.is_empty() {
            return None;
        }
        let pick =
            u64::from_str_radix(&crate::paths::nonce(), 16).unwrap_or(0) as usize % all.len();
        all.into_iter().nth(pick)
    }

    fn random_all(&mut self) {
        match self.layout.arrangement {
            Arrangement::Per => {
                let ids: Vec<String> = self.displays.iter().map(|d| d.id.clone()).collect();
                for id in ids {
                    let current = self.layout.wallpaper_for(&id).map(str::to_owned);
                    if let Some(w) = self.random(current.as_deref()) {
                        self.layout.assign(&id, &w.id);
                    }
                }
            }
            _ => {
                let current = self.layout.shared.clone();
                if let (Some(w), Some(d)) = (
                    self.random(current.as_deref()),
                    display::primary(&self.displays).map(|d| d.id.clone()),
                ) {
                    self.layout.assign(&d, &w.id);
                }
            }
        }
        self.reconcile();
    }

    /// Run `work` off the main thread and apply its result on it.
    fn job<R: Send + 'static>(
        &self,
        work: impl FnOnce() -> R + Send + 'static,
        then: impl FnOnce(&mut Engine, R) + Send + 'static,
    ) {
        let tx = self.rt.sender();
        std::thread::spawn(move || {
            let r = work();
            tx.send(Msg::Job(Box::new(move |e| then(e, r))));
        });
    }

    fn report_problems(&self, problems: &[String]) {
        for message in problems {
            self.broadcast(Event::Error {
                message: message.clone(),
            });
        }
    }

    fn request(&mut self, req: Request, reply: Reply) -> bool {
        let resp = match req {
            Request::Status => Response::Status(self.status()),
            Request::Displays => Response::Displays(self.displays.clone()),
            Request::Library => {
                Response::Library(self.library.scan().iter().map(Wallpaper::summary).collect())
            }
            Request::Settings => Response::Settings(self.settings.clone()),
            Request::SetSettings { settings } => self
                .set_settings(settings)
                .map_or_else(|e| Response::error(&e), |_| Response::Ok),
            Request::Layout => Response::Layout(self.layout.clone()),
            Request::SetArrangement {
                arrangement,
                display,
            } => {
                let preferred =
                    display.or_else(|| display::primary(&self.displays).map(|d| d.id.clone()));
                self.layout
                    .set_arrangement(arrangement, preferred.as_deref());
                self.reconcile();
                Response::Ok
            }
            Request::AlignImage { pose } => self
                .align(|l| l.set_image_pose(pose))
                .map_or_else(|e| Response::error(&e), |_| Response::Ok),
            Request::AlignDisplay { display, pose } => match self.display(Some(&display)) {
                Ok(d) => self
                    .align(|l| l.set_display_pose(&d.id, pose))
                    .map_or_else(|e| Response::error(&e), |_| Response::Ok),
                Err(e) => Response::error(&e),
            },
            Request::ResetAlignment => {
                self.layout.reset_alignment();
                self.reconcile();
                Response::Ok
            }
            Request::Set { target, display } => return self.set_target(target, display, reply),
            Request::Close { display } => self
                .close(display.as_deref())
                .map_or_else(|e| Response::error(&e), |_| Response::Ok),
            Request::Import { source } => {
                let lib = self.library.clone();
                let (copy, thumbnails, temp) = (
                    self.settings.copy_imports,
                    self.settings.thumbnails,
                    self.paths.temp_dir(),
                );
                let tx = self.rt.sender();
                self.job(
                    move || {
                        lib.import(
                            &source,
                            &library::ImportOptions {
                                copy,
                                thumbnails,
                                temp_dir: &temp,
                            },
                            &mut |_: &Wallpaper| {
                                tx.send(Msg::Job(Box::new(|e| e.broadcast(Event::Library))));
                            },
                        )
                    },
                    move |e, r| match r {
                        Ok(imported) => {
                            e.report_problems(&imported.problems);
                            reply(Response::Wallpapers(
                                imported.wallpapers.iter().map(Wallpaper::summary).collect(),
                            ));
                        }
                        Err(err) => reply(Response::error(&err)),
                    },
                );
                return true;
            }
            Request::Delete { wallpaper } => self
                .delete(&wallpaper)
                .map_or_else(|e| Response::error(&e), |_| Response::Ok),
            Request::Export { wallpaper, file } => match self.library.get(&wallpaper) {
                Ok(wp) => {
                    let lib = self.library.clone();
                    self.job(
                        move || lib.export(&wp, &file),
                        move |_, r| reply(r.map_or_else(|e| Response::error(&e), |_| Response::Ok)),
                    );
                    return true;
                }
                Err(e) => Response::error(&e),
            },
            Request::EditInfo { wallpaper, patch } => self
                .edit_info(&wallpaper, patch)
                .map_or_else(|e| Response::error(&e), Response::Wallpaper),
            Request::Properties { wallpaper, display } => {
                self.properties(&wallpaper, display.as_deref()).map_or_else(
                    |e| Response::error(&e),
                    |(path, controls)| Response::Controls { path, controls },
                )
            }
            Request::SetProperty {
                wallpaper,
                display,
                name,
                value,
            } => self
                .set_property(&wallpaper, display.as_deref(), &name, &value)
                .map_or_else(|e| Response::error(&e), |_| Response::Ok),
            Request::ResetProperties { wallpaper, display } => self
                .reset_properties(&wallpaper, display.as_deref())
                .map_or_else(|e| Response::error(&e), |_| Response::Ok),
            Request::Seek { display, value } => self
                .seek(display.as_deref(), &value)
                .map_or_else(|e| Response::error(&e), |_| Response::Ok),
            Request::Volume { value } => self
                .volume(&value)
                .map_or_else(|e| Response::error(&e), |_| Response::Ok),
            Request::Play { play } => {
                self.set_user_paused(!play);
                Response::Ok
            }
            Request::Screenshot { display, file } => {
                match self.display(display.as_deref()).and_then(|d| {
                    self.active_on(&d.id).ok_or_else(|| {
                        Error::NotFound(format!("no wallpaper running on {}", d.name))
                    })
                }) {
                    Ok(idx) => {
                        self.capture(idx, file, None, reply);
                        return true;
                    }
                    Err(e) => Response::error(&e),
                }
            }
            Request::Thumbnail { wallpaper } => return self.thumbnail(&wallpaper, reply),
            Request::AudioDevices => Response::Devices(crate::audio::devices()),
            Request::OpenUi => {
                self.open_ui();
                Response::Ok
            }
            Request::Subscribe => Response::Ok,
            Request::Quit => {
                reply(Response::Ok);
                return self.shutdown();
            }
        };
        reply(resp);
        true
    }

    fn status(&self) -> Status {
        Status {
            version: env!("CARGO_PKG_VERSION").into(),
            platform: std::env::consts::OS.into(),
            session: self.rt.session(),
            window_monitor: self.monitor_name.clone(),
            capabilities: self.capabilities.clone(),
            displays: self.displays.clone(),
            layout: self.layout.clone(),
            active: self
                .active
                .iter()
                .map(|a| ActiveInfo {
                    display: a.placement.display.clone(),
                    wallpaper: a.wallpaper.id.clone(),
                    title: a.wallpaper.title(),
                    kind: a.wallpaper.kind(),
                    loaded: a.loaded,
                    paused: a.paused.unwrap_or(false),
                    volume: a.volume.unwrap_or(0),
                    customizable: a.props_path.is_some(),
                })
                .collect(),
            paused: self.user_paused,
            locked: self.locked,
            on_battery: self.on_battery,
        }
    }

    fn active_on(&self, display_id: &str) -> Option<usize> {
        self.active
            .iter()
            .position(|a| a.placement.display == display_id)
    }

    fn set_settings(&mut self, mut s: Settings) -> Result<()> {
        s.normalize();
        if self.settings.library_dir != s.library_dir {
            ctx(
                std::fs::create_dir_all(&s.library_dir),
                s.library_dir.display(),
            )?;
        }
        s.save(&self.paths.settings_file())?;
        let old = std::mem::replace(&mut self.settings, s);
        let s = &self.settings;
        if old.library_dir != s.library_dir {
            self.library = Library {
                dir: s.library_dir.clone(),
            };
            self.broadcast(Event::Library);
        }
        if old.autostart != s.autostart {
            if let Err(e) = crate::autostart::apply(s.autostart) {
                self.report(&e);
            }
        }
        if old.tray != s.tray {
            match (&self.tray, s.tray) {
                (None, true) => {
                    self.tray = Tray::new(self.rt.sender(), self.user_paused)
                        .map_err(|e| log::warn!("{e}"))
                        .ok()
                }
                (Some(t), false) => {
                    t.set_visible(false);
                    self.tray = None;
                }
                _ => {}
            }
        }
        if old.rules.interval_ms != s.rules.interval_ms {
            self.rt.set_monitor_interval(s.rules.interval_ms);
        }
        if old.input != s.input {
            for a in &mut self.active {
                a.content.set_input_enabled(s.input.forward_mouse);
            }
            self.rt.set_pointer_tracking(s.input.forward_mouse);
        }
        if old.audio_capture_device != s.audio_capture_device {
            self.audio = None;
        }
        let restart_media = old.video != s.video;
        let restart_web = old.web != s.web;
        if restart_media || restart_web {
            self.active.retain(|a| {
                !((restart_media && a.wallpaper.kind().is_media())
                    || (restart_web && a.wallpaper.kind().is_web()))
            });
        }
        self.broadcast(Event::Settings);
        self.reconcile();
        Ok(())
    }

    fn set_target(&mut self, target: String, display: Option<String>, reply: Reply) -> bool {
        let d = match self.display(display.as_deref()) {
            Ok(d) => d,
            Err(e) => {
                reply(Response::error(&e));
                return true;
            }
        };
        match target.as_str() {
            "random" => {
                let current = self.layout.wallpaper_for(&d.id).map(str::to_owned);
                match self.random(current.as_deref()) {
                    Some(w) => {
                        self.layout.assign(&d.id, &w.id);
                        self.reconcile();
                        reply(Response::Ok);
                    }
                    None => reply(Response::error(&Error::NotFound(
                        "the library is empty".into(),
                    ))),
                }
            }
            "reload" => {
                if display.is_some() && self.layout.arrangement == Arrangement::Per {
                    self.active.retain(|a| a.placement.display != d.id);
                } else {
                    self.active.clear();
                }
                self.reconcile();
                reply(Response::Ok);
            }
            _ if self.library.get(&target).is_ok() => {
                self.layout.assign(&d.id, &target);
                self.reconcile();
                reply(Response::Ok);
            }
            _ => {
                let lib = self.library.clone();
                let (copy, thumbnails, temp) = (
                    self.settings.copy_imports,
                    self.settings.thumbnails,
                    self.paths.temp_dir(),
                );
                let did = d.id.clone();
                let tx = self.rt.sender();
                self.job(
                    move || {
                        lib.import(
                            &target,
                            &library::ImportOptions {
                                copy,
                                thumbnails,
                                temp_dir: &temp,
                            },
                            &mut |_: &Wallpaper| {
                                tx.send(Msg::Job(Box::new(|e| e.broadcast(Event::Library))));
                            },
                        )
                    },
                    move |e, r| match r {
                        Ok(imported) => {
                            e.report_problems(&imported.problems);
                            match imported.wallpapers.first() {
                                Some(w) => {
                                    e.layout.assign(&did, &w.id);
                                    e.reconcile();
                                    reply(Response::Wallpaper(w.summary()));
                                }
                                None => reply(Response::error(&Error::NotFound(
                                    "nothing was imported".into(),
                                ))),
                            }
                        }
                        Err(err) => reply(Response::error(&err)),
                    },
                );
            }
        }
        true
    }

    fn close(&mut self, display: Option<&str>) -> Result<()> {
        match display {
            Some(r) => {
                let d = self.display(Some(r))?;
                self.layout.clear_display(&d.id);
            }
            None => self.layout.clear(),
        }
        self.reconcile();
        Ok(())
    }

    fn delete(&mut self, wallpaper: &str) -> Result<()> {
        self.library.get(wallpaper)?;
        self.active.retain(|a| a.wallpaper.id != wallpaper);
        self.layout.remove_wallpaper(wallpaper);
        self.library
            .delete(wallpaper, &self.paths.properties_dir())?;
        self.reconcile();
        self.broadcast(Event::Library);
        Ok(())
    }

    fn edit_info(&mut self, wallpaper: &str, patch: InfoPatch) -> Result<crate::model::Summary> {
        let mut wp = self.library.get(wallpaper)?;
        let opt = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Some(t) = patch.title {
            wp.info.title = t.trim().to_string();
        }
        if let Some(v) = patch.desc {
            wp.info.desc = opt(Some(v));
        }
        if let Some(v) = patch.author {
            wp.info.author = opt(Some(v));
        }
        if let Some(v) = patch.contact {
            wp.info.contact = opt(Some(v));
        }
        if let Some(v) = patch.license {
            wp.info.license = opt(Some(v));
        }
        if let Some(v) = patch.arguments {
            wp.info.arguments = opt(Some(v));
        }
        wp.save_info()?;
        let updated = Wallpaper::from_info(&wp.dir, wp.info.clone());
        for a in self
            .active
            .iter_mut()
            .filter(|a| a.wallpaper.id == wallpaper)
        {
            a.wallpaper = updated.clone();
        }
        self.broadcast(Event::Library);
        Ok(updated.summary())
    }

    /// Resolve which property copy a request targets: a running instance on `display`, or the
    /// wallpaper's slot for that display.
    fn props_target(&self, wallpaper: &str, display: Option<&str>) -> Result<(Wallpaper, String)> {
        if wallpaper.is_empty() {
            let d = self.display(display)?;
            let a = self
                .active_on(&d.id)
                .map(|i| &self.active[i])
                .ok_or_else(|| Error::NotFound(format!("no wallpaper running on {}", d.name)))?;
            return Ok((a.wallpaper.clone(), a.placement.slot.clone()));
        }
        let wp = self.library.get(wallpaper)?;
        if let Some(a) = self.active.iter().find(|a| {
            a.wallpaper.id == wallpaper
                && display.is_none_or(|d| {
                    display::find(&self.displays, d).is_some_and(|dd| dd.id == a.placement.display)
                })
        }) {
            return Ok((wp, a.placement.slot.clone()));
        }
        let d = self.display(display)?;
        Ok((wp, self.slot_key(&d.id)))
    }

    fn properties(
        &self,
        wallpaper: &str,
        display: Option<&str>,
    ) -> Result<(PathBuf, Vec<(String, crate::model::Control)>)> {
        let (wp, slot) = self.props_target(wallpaper, display)?;
        let path = self.ensure_props(&wp, &slot)?.ok_or_else(|| {
            Error::Unsupported(format!("'{}' has no customization controls", wp.title()))
        })?;
        let props = Properties::load(&path)?;
        Ok((path, props.controls()))
    }

    fn set_property(
        &mut self,
        wallpaper: &str,
        display: Option<&str>,
        name: &str,
        value: &Value,
    ) -> Result<()> {
        if name == RESET_BUTTON {
            return self.reset_properties(wallpaper, display);
        }
        let (wp, slot) = self.props_target(wallpaper, display)?;
        let path = self.ensure_props(&wp, &slot)?.ok_or_else(|| {
            Error::Unsupported(format!("'{}' has no customization controls", wp.title()))
        })?;
        let mut props = Properties::load(&path)?;
        let control = props
            .get(name)
            .ok_or_else(|| Error::NotFound(format!("no control named '{name}'")))?;
        let sent = if control.is_interactive_only() {
            None
        } else {
            let v = props.set(name, value)?;
            props.save(&path)?;
            Some(v)
        };
        for a in self
            .active
            .iter_mut()
            .filter(|a| a.wallpaper.id == wp.id && a.placement.slot == slot)
        {
            a.content.apply(name, &control, sent.as_ref());
        }
        Ok(())
    }

    fn reset_properties(&mut self, wallpaper: &str, display: Option<&str>) -> Result<()> {
        let (wp, slot) = self.props_target(wallpaper, display)?;
        self.reset_props(&wp, &slot)?.ok_or_else(|| {
            Error::Unsupported(format!("'{}' has no customization controls", wp.title()))
        })?;
        let idxs: Vec<usize> = self
            .active
            .iter()
            .enumerate()
            .filter(|(_, a)| a.wallpaper.id == wp.id && a.placement.slot == slot)
            .map(|(i, _)| i)
            .collect();
        for i in idxs {
            self.apply_properties(i);
        }
        Ok(())
    }

    fn seek(&mut self, display: Option<&str>, value: &str) -> Result<()> {
        let seek = Seek::parse(value)
            .ok_or_else(|| Error::Invalid(format!("'{value}' is not a seek position")))?;
        let d = self.display(display)?;
        let all = display.is_none() || self.layout.arrangement != Arrangement::Per;
        let mut hit = false;
        for a in self
            .active
            .iter_mut()
            .filter(|a| all || a.placement.display == d.id)
        {
            a.content.seek(seek);
            hit = true;
        }
        if hit {
            Ok(())
        } else {
            Err(Error::NotFound(format!(
                "no wallpaper running on {}",
                d.name
            )))
        }
    }

    fn volume(&mut self, value: &str) -> Result<()> {
        let v = value.trim();
        let parsed = v
            .parse::<i32>()
            .map_err(|_| Error::Invalid(format!("'{value}' is not a volume")))?;
        let new = if v.starts_with(['+', '-']) {
            (self.settings.volume as i32).saturating_add(parsed)
        } else {
            parsed
        };
        let mut settings = self.settings.clone();
        settings.volume = new.clamp(0, 100) as u8;
        settings.save(&self.paths.settings_file())?;
        self.settings = settings;
        self.broadcast(Event::Settings);
        self.evaluate();
        Ok(())
    }

    fn thumbnail(&mut self, wallpaper: &str, reply: Reply) -> bool {
        let wp = match self.library.get(wallpaper) {
            Ok(w) => w,
            Err(e) => {
                reply(Response::error(&e));
                return true;
            }
        };
        let path = wp.dir.join(THUMBNAIL);
        if let Some(idx) = self.active.iter().position(|a| a.wallpaper.id == wp.id) {
            self.capture(idx, path, Some(wp.id), reply);
            return true;
        }
        if wp.kind().is_media() && !wp.kind().is_online() {
            let source = PathBuf::from(&wp.source);
            let kind = wp.kind();
            let temp = self.paths.temp_dir();
            let wid = wp.id.clone();
            self.job(
                move || crate::media::thumb::capture(&source, kind, &path, &temp),
                move |e, r| {
                    reply(
                        r.and_then(|_| e.set_thumbnail(&wid, THUMBNAIL))
                            .map_or_else(|err| Response::error(&err), |_| Response::Ok),
                    )
                },
            );
            return true;
        }
        reply(Response::error(&Error::Invalid(format!(
            "start '{}' to capture its thumbnail",
            wp.title()
        ))));
        true
    }
}
