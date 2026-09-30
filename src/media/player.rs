use crate::content::{Content, ContentEvent, ContentId, PointerEvent, Seek, View};
use crate::error::{Error, Result};
use crate::geom::Size;
use crate::media::looper::Loop;
use crate::media::mpv::{self, Handle};
use crate::model::props::ControlKind;
use crate::model::settings::{Scaler, StreamQuality};
use crate::model::{Control, Kind};
use crate::msg::Msg;
use crate::platform::{MsgSender, MsgSenderApi};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Where mpv draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vo {
    /// Render API: the platform view pulls frames through a [`mpv::RenderContext`].
    #[cfg_attr(windows, allow(dead_code, reason = "Windows embeds mpv by window id"))]
    Render,
    /// mpv creates its own child window inside this native window id.
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "render-API backends never embed by window id")
    )]
    Wid(i64),
    /// No output at all: tests drive the cores by the clock alone.
    #[cfg(test)]
    Null,
}

#[derive(Clone, Copy)]
pub struct PlayerOptions<'a> {
    pub kind: Kind,
    pub source: &'a str,
    pub audio: bool,
    pub volume: u8,
    pub hw_accel: bool,
    pub scaler: Scaler,
    pub stream_quality: StreamQuality,
    pub vo: Vo,
    /// Logical size of the surface mpv draws into.
    pub slot: Size,
}

/// How a core handles the end of its clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Looping {
    /// mpv starts the file over itself.
    Native,
    /// One pass, then hold the last frame while a [`Loop`] hands over to the other core. A
    /// standby core starts paused on its first frame.
    Pass { standby: bool },
}

/// Reply ids for asynchronous commands, unique across every core in the process.
static NEXT_COMMAND_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub enum PlayerEvent {
    Loaded,
    Ended { error: Option<String> },
    Shutdown,
    CommandDone { id: u64, error: Option<String> },
}

/// One libmpv core playing a wallpaper. Call [`Player::load`] once the video output exists
/// (after the render context is created for [`Vo::Render`]).
pub struct Player {
    handle: Arc<Handle>,
    kind: Kind,
    source: String,
    slot: Size,
    scaler: Mutex<Scaler>,
    /// The view realized through mpv's own filters and zoom, for surfaces that leave it to mpv.
    view: Mutex<Option<View>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Player {
    pub fn new(
        opts: PlayerOptions<'_>,
        looping: Looping,
        on_event: impl Fn(PlayerEvent) + Send + 'static,
    ) -> Result<Player> {
        let handle = Arc::new(Handle::new(mpv::lib()?)?);
        let mut options: Vec<(&str, String)> = vec![
            ("config", "no".into()),
            ("terminal", "no".into()),
            ("msg-level", "all=warn".into()),
            ("input-default-bindings", "no".into()),
            ("input-cursor", "no".into()),
            ("input-vo-keyboard", "no".into()),
            ("osc", "no".into()),
            ("osd-level", "0".into()),
            ("cursor-autohide", "no".into()),
            ("stop-screensaver", "no".into()),
            ("audio-client-name", "Deadly Wallpaper".into()),
            (
                "loop-file",
                if looping == Looping::Native {
                    "inf"
                } else {
                    "no"
                }
                .into(),
            ),
            ("keep-open", "yes".into()),
            ("idle", "yes".into()),
            ("image-display-duration", "inf".into()),
            ("volume", opts.volume.to_string()),
            ("aid", if opts.audio { "auto" } else { "no" }.into()),
            (
                "hwdec",
                if opts.hw_accel { "auto-safe" } else { "no" }.into(),
            ),
            ("ytdl", "yes".into()),
        ];
        match opts.vo {
            Vo::Render => options.push(("vo", "libmpv".into())),
            #[cfg(test)]
            Vo::Null => options.push(("vo", "null".into())),
            Vo::Wid(id) => {
                options.push(("wid", id.to_string()));
                options.push(("force-window", "yes".into()));
                options.push(("border", "no".into()));
                options.push(("ontop", "no".into()));
            }
        }
        if opts.kind == Kind::Gif {
            options.push(("scale", "nearest".into()));
        }
        if looping == (Looping::Pass { standby: true }) {
            options.push(("pause", "yes".into()));
        }
        if opts.kind == Kind::VideoStream {
            options.push(("ytdl-format", opts.stream_quality.ytdl_format()));
        }
        for (k, v) in opts.scaler.mpv_properties() {
            options.push((k, (*v).into()));
        }
        for (k, v) in &options {
            handle.set_option(k, v)?;
        }
        handle.initialize()?;
        handle.request_log("warn");
        spawn_event_thread(handle.clone(), on_event);
        Ok(Player {
            handle,
            kind: opts.kind,
            source: opts.source.to_string(),
            slot: opts.slot,
            scaler: Mutex::new(opts.scaler),
            view: Mutex::new(None),
        })
    }

    pub fn slot(&self) -> Size {
        self.slot
    }

    /// Start playback of the configured source.
    pub fn load(&self) -> Result<()> {
        self.handle.command(&["loadfile", &self.source])
    }

    #[cfg(not(windows))]
    pub fn handle(&self) -> &Arc<Handle> {
        &self.handle
    }

    pub fn set_paused(&self, paused: bool) {
        report(self.handle.set_flag("pause", paused));
    }

    /// `volume` scaled by `gain` (0 to 1), for cross-fading two cores.
    pub fn set_volume_scaled(&self, volume: u8, gain: f64) {
        report(
            self.handle
                .set_f64("volume", volume.min(100) as f64 * gain.clamp(0.0, 1.0)),
        );
    }

    /// Back to the first frame, precisely; a paused core shows it.
    pub fn rewind(&self) {
        report(self.handle.command(&["seek", "0", "absolute+exact"]));
    }

    /// Length of the loaded clip in seconds, once known.
    pub fn duration(&self) -> Option<f64> {
        self.handle.get_f64("duration").filter(|d| *d > 0.0)
    }

    /// Playback position in seconds; zero before the first frame.
    pub fn position(&self) -> f64 {
        self.handle.get_f64("time-pos").unwrap_or(0.0)
    }

    /// Frames per second as the container declares them, or as decoded frames suggest; 30
    /// until either is known.
    pub fn fps(&self) -> f64 {
        self.handle
            .get_f64("container-fps")
            .or_else(|| self.handle.get_f64("estimated-vf-fps"))
            .filter(|f| *f >= 1.0)
            .unwrap_or(30.0)
    }

    /// The clip played to its end and holds the last frame.
    pub fn eof_reached(&self) -> bool {
        self.handle.get_flag("eof-reached").unwrap_or(false)
    }

    /// Engine mute disables the audio track so a user "mute" control stays independent.
    pub fn set_engine_muted(&self, muted: bool) {
        report(
            self.handle
                .set_str("aid", if muted { "no" } else { "auto" }),
        );
    }

    pub fn seek(&self, seek: Seek) {
        if self.kind == Kind::Picture {
            return;
        }
        let (v, mode) = match seek {
            Seek::Absolute(p) => (p, "absolute-percent"),
            Seek::Relative(p) => (p, "relative-percent"),
        };
        report(self.handle.command(&["seek", &format!("{v}"), mode]));
    }

    pub fn set_scaler(&self, scaler: Scaler) {
        *lock(&self.scaler) = scaler;
        match *lock(&self.view) {
            Some(view) => self.apply_view(&view, scaler),
            None => {
                for (k, v) in scaler.mpv_properties() {
                    report(self.handle.set_str(k, v));
                }
            }
        }
    }

    /// Show `view` of the image through mpv itself: the frame is fitted to the image size and
    /// turned by a filter, then zoomed and panned into place.
    pub fn set_view(&self, view: &View) {
        if view.is_whole(self.slot) {
            if lock(&self.view).take().is_some() {
                report(self.handle.set_str("vf", ""));
                report(self.handle.set_f64("video-zoom", 0.0));
                report(self.handle.set_f64("video-pan-x", 0.0));
                report(self.handle.set_f64("video-pan-y", 0.0));
                let scaler = *lock(&self.scaler);
                for (k, v) in scaler.mpv_properties() {
                    report(self.handle.set_str(k, v));
                }
            }
            return;
        }
        *lock(&self.view) = Some(*view);
        self.apply_view(view, *lock(&self.scaler));
    }

    fn apply_view(&self, view: &View, scaler: Scaler) {
        let (w, h) = (view.width.max(1), view.height.max(1));
        let fit = match scaler {
            Scaler::Fill => format!("scale={w}:{h}"),
            Scaler::Uniform => format!(
                "scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black"
            ),
            Scaler::UniformFill => {
                format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}")
            }
            Scaler::None => {
                format!("crop='min(iw,{w})':'min(ih,{h})',pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black")
            }
        };
        let radians = view.rotation.to_radians();
        let graph = if view.rotation == 0.0 {
            fit
        } else {
            format!("{fit},rotate={radians}:ow=rotw({radians}):oh=roth({radians}):c=black")
        };
        report(self.handle.set_str("vf", &format!("lavfi=[{graph}]")));
        // The turned frame's bounding box, which mpv now shows one to one before zoom and pan.
        let (sin, cos) = radians.sin_cos();
        let (bw, bh) = (
            w as f64 * cos.abs() + h as f64 * sin.abs(),
            w as f64 * sin.abs() + h as f64 * cos.abs(),
        );
        report(self.handle.set_str("video-unscaled", "yes"));
        report(self.handle.set_str("keepaspect", "yes"));
        report(self.handle.set_str("panscan", "0.0"));
        report(self.handle.set_f64("video-zoom", view.scale.log2()));
        report(
            self.handle
                .set_f64("video-pan-x", view.x / (bw * view.scale)),
        );
        report(
            self.handle
                .set_f64("video-pan-y", view.y / (bh * view.scale)),
        );
    }

    /// Map a property control onto an mpv property of the same name.
    pub fn apply(&self, name: &str, control: &Control, value: Option<&Value>) {
        let Some(value) = value else { return };
        match &control.kind {
            ControlKind::ScalerDropdown { .. } => {
                if let Some(i) = value.as_i64() {
                    self.set_scaler(Scaler::from_index(i));
                }
            }
            ControlKind::Slider { step, .. } => {
                if let Some(v) = value.as_f64() {
                    if step.fract() == 0.0 {
                        report(self.handle.set_i64(name, v.round() as i64));
                    } else {
                        report(self.handle.set_f64(name, v));
                    }
                }
            }
            ControlKind::Checkbox { .. } => {
                if let Some(b) = value.as_bool() {
                    report(self.handle.set_flag(name, b));
                }
            }
            ControlKind::Dropdown { .. } => {
                if let Some(i) = value.as_i64() {
                    report(self.handle.set_i64(name, i));
                }
            }
            ControlKind::Textbox { .. }
            | ControlKind::Color { .. }
            | ControlKind::FolderDropdown { .. } => {
                if let Some(s) = value.as_str() {
                    report(self.handle.set_str(name, s));
                }
            }
            ControlKind::Button { .. } | ControlKind::Label { .. } => {}
        }
    }

    /// Start an asynchronous frame capture; completion arrives as `CommandDone { id }`.
    pub fn screenshot(&self, path: &std::path::Path) -> Result<u64> {
        let id = NEXT_COMMAND_ID.fetch_add(1, Ordering::Relaxed);
        self.handle.command_async(
            id,
            &["screenshot-to-file", &path.to_string_lossy(), "window"],
        )?;
        Ok(id)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.handle.command(&["quit"]);
    }
}

fn report(r: Result<()>) {
    if let Err(e) = r {
        log::debug!("mpv: {e}");
    }
}

fn spawn_event_thread(handle: Arc<Handle>, on_event: impl Fn(PlayerEvent) + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("mpv-events".into())
        .spawn(move || {
            loop {
                match handle.wait_event(1.0) {
                    mpv::Event::Shutdown => {
                        on_event(PlayerEvent::Shutdown);
                        break;
                    }
                    mpv::Event::FileLoaded => on_event(PlayerEvent::Loaded),
                    mpv::Event::EndFile { reason, error } => {
                        if reason == 4 {
                            on_event(PlayerEvent::Ended {
                                error: Some(handle.error_string(error)),
                            });
                        }
                    }
                    mpv::Event::CommandReply { id, error } => {
                        let error = (error < 0).then(|| handle.error_string(error));
                        on_event(PlayerEvent::CommandDone { id, error });
                    }
                    mpv::Event::Log {
                        prefix,
                        level,
                        text,
                    } => match level.as_str() {
                        "fatal" | "error" => log::warn!("mpv[{prefix}]: {text}"),
                        _ => log::debug!("mpv[{prefix}]: {text}"),
                    },
                    mpv::Event::None | mpv::Event::Other => {}
                }
            }
        });
}

type Pending = Arc<Mutex<HashMap<u64, PathBuf>>>;

/// Route player events to the engine as content events for `id`.
pub fn event_bridge(
    id: ContentId,
    tx: MsgSender,
) -> (impl Fn(PlayerEvent) + Send + 'static, Pending) {
    let pending: Pending = Arc::default();
    let p2 = pending.clone();
    let f = move |ev: PlayerEvent| match ev {
        PlayerEvent::Loaded => tx.send(Msg::Content(id, ContentEvent::Loaded)),
        PlayerEvent::Ended { error: Some(e) } => {
            tx.send(Msg::Content(id, ContentEvent::Exited { reason: e }))
        }
        PlayerEvent::Ended { error: None } | PlayerEvent::Shutdown => {}
        PlayerEvent::CommandDone { id: cmd, error } => {
            let path = p2.lock().ok().and_then(|mut m| m.remove(&cmd));
            if let Some(path) = path {
                let result = match error {
                    Some(e) => Err(Error::Media(e)),
                    None => Ok(()),
                };
                tx.send(Msg::Content(id, ContentEvent::Screenshot { path, result }));
            }
        }
    };
    (f, pending)
}

/// The platform surface mpv draws into. Surfaces that own the GL context capture frames
/// themselves; others leave it to mpv's screenshot command.
pub trait MediaSurface {
    fn capture(&self, path: &std::path::Path) -> Option<Result<()>> {
        let _ = path;
        None
    }

    /// Show `view` of the image; `None` leaves it to the player.
    fn set_view(&self, view: &View, slot: Size) -> Option<Result<()>> {
        let _ = (view, slot);
        None
    }
}

/// A media wallpaper: a platform view (dropped first, so the render contexts go before the
/// cores) and the looping cores driving it.
pub struct MediaContent {
    view: Box<dyn MediaSurface>,
    looper: Arc<Loop>,
    pending: Pending,
    id: ContentId,
    tx: MsgSender,
}

impl MediaContent {
    pub fn new(
        view: Box<dyn MediaSurface>,
        looper: Arc<Loop>,
        pending: Pending,
        id: ContentId,
        tx: MsgSender,
    ) -> MediaContent {
        MediaContent {
            view,
            looper,
            pending,
            id,
            tx,
        }
    }
}

impl Content for MediaContent {
    fn set_paused(&mut self, paused: bool) {
        self.looper.set_paused(paused);
    }

    fn set_volume(&mut self, volume: u8) {
        self.looper.set_volume(volume);
    }

    fn set_muted(&mut self, muted: bool) {
        self.looper.set_engine_muted(muted);
    }

    fn seek(&mut self, seek: Seek) {
        self.looper.seek(seek);
    }

    fn apply(&mut self, name: &str, control: &Control, value: Option<&Value>) {
        self.looper.apply(name, control, value);
    }

    fn screenshot(&mut self, path: PathBuf) {
        if let Some(result) = self.view.capture(&path) {
            self.tx.send(Msg::Content(
                self.id,
                ContentEvent::Screenshot { path, result },
            ));
            return;
        }
        match self.looper.screenshot(&path) {
            Ok(cmd) => {
                if let Ok(mut m) = self.pending.lock() {
                    m.insert(cmd, path);
                }
            }
            Err(e) => self.tx.send(Msg::Content(
                self.id,
                ContentEvent::Screenshot {
                    path,
                    result: Err(e),
                },
            )),
        }
    }

    fn pointer(&mut self, _ev: PointerEvent) {}

    fn set_input_enabled(&mut self, _enabled: bool) {}

    fn audio_data(&mut self, _bins: &[f32]) {}

    fn set_view(&mut self, view: &View) -> Result<()> {
        match self.view.set_view(view, self.looper.slot()) {
            Some(result) => result,
            None => {
                self.looper.set_view(view);
                Ok(())
            }
        }
    }
}
