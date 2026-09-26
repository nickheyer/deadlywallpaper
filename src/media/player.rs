use crate::content::{Content, ContentEvent, ContentId, PointerEvent, Seek};
use crate::error::{Error, Result};
use crate::media::mpv::{self, Handle};
use crate::model::props::ControlKind;
use crate::model::settings::{Scaler, StreamQuality};
use crate::model::{Control, Kind};
use crate::msg::Msg;
use crate::platform::{MsgSender, MsgSenderApi};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Where mpv draws.
pub enum Vo {
    /// Render API: the platform view pulls frames through a [`mpv::RenderContext`].
    #[cfg_attr(windows, allow(dead_code, reason = "Windows embeds mpv by window id"))]
    Render,
    /// mpv creates its own child window inside this native window id.
    #[cfg_attr(not(windows), allow(dead_code, reason = "render-API backends never embed by window id"))]
    Wid(i64),
}

pub struct PlayerOptions<'a> {
    pub kind: Kind,
    pub source: &'a str,
    pub audio: bool,
    pub volume: u8,
    pub hw_accel: bool,
    pub scaler: Scaler,
    pub stream_quality: StreamQuality,
    pub vo: Vo,
}

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
    next_id: u64,
}

impl Player {
    pub fn new(opts: PlayerOptions<'_>, on_event: impl Fn(PlayerEvent) + Send + 'static) -> Result<Player> {
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
            ("loop-file", "inf".into()),
            ("keep-open", "yes".into()),
            ("idle", "yes".into()),
            ("image-display-duration", "inf".into()),
            ("volume", opts.volume.to_string()),
            ("aid", if opts.audio { "auto" } else { "no" }.into()),
            ("hwdec", if opts.hw_accel { "auto-safe" } else { "no" }.into()),
            ("ytdl", "yes".into()),
        ];
        match opts.vo {
            Vo::Render => options.push(("vo", "libmpv".into())),
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
        Ok(Player { handle, kind: opts.kind, source: opts.source.to_string(), next_id: 1 })
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

    pub fn set_volume(&self, volume: u8) {
        report(self.handle.set_f64("volume", volume.min(100) as f64));
    }

    /// Engine mute disables the audio track so a user "mute" control stays independent.
    pub fn set_engine_muted(&self, muted: bool) {
        report(self.handle.set_str("aid", if muted { "no" } else { "auto" }));
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
        for (k, v) in scaler.mpv_properties() {
            report(self.handle.set_str(k, v));
        }
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
            ControlKind::Textbox { .. } | ControlKind::Color { .. } | ControlKind::FolderDropdown { .. } => {
                if let Some(s) = value.as_str() {
                    report(self.handle.set_str(name, s));
                }
            }
            ControlKind::Button { .. } | ControlKind::Label { .. } => {}
        }
    }

    /// Start an asynchronous frame capture; completion arrives as `CommandDone { id }`.
    pub fn screenshot(&mut self, path: &std::path::Path) -> Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.handle.command_async(id, &["screenshot-to-file", &path.to_string_lossy(), "window"])?;
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
    let _ = std::thread::Builder::new().name("mpv-events".into()).spawn(move || {
        loop {
            match handle.wait_event(1.0) {
                mpv::Event::Shutdown => {
                    on_event(PlayerEvent::Shutdown);
                    break;
                }
                mpv::Event::FileLoaded => on_event(PlayerEvent::Loaded),
                mpv::Event::EndFile { reason, error } => {
                    if reason == 4 {
                        on_event(PlayerEvent::Ended { error: Some(handle.error_string(error)) });
                    }
                }
                mpv::Event::CommandReply { id, error } => {
                    let error = (error < 0).then(|| handle.error_string(error));
                    on_event(PlayerEvent::CommandDone { id, error });
                }
                mpv::Event::Log { prefix, level, text } => match level.as_str() {
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
pub fn event_bridge(id: ContentId, tx: MsgSender) -> (impl Fn(PlayerEvent) + Send + 'static, Pending) {
    let pending: Pending = Arc::default();
    let p2 = pending.clone();
    let f = move |ev: PlayerEvent| match ev {
        PlayerEvent::Loaded => tx.send(Msg::Content(id, ContentEvent::Loaded)),
        PlayerEvent::Ended { error: Some(e) } => tx.send(Msg::Content(id, ContentEvent::Exited { reason: e })),
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
}

/// A media wallpaper: a platform view (dropped first, so the render context goes before the
/// core) and the player driving it.
pub struct MediaContent {
    view: Box<dyn MediaSurface>,
    player: Player,
    pending: Pending,
    id: ContentId,
    tx: MsgSender,
}

impl MediaContent {
    pub fn new(view: Box<dyn MediaSurface>, player: Player, pending: Pending, id: ContentId, tx: MsgSender) -> MediaContent {
        MediaContent { view, player, pending, id, tx }
    }
}

impl Content for MediaContent {
    fn set_paused(&mut self, paused: bool) {
        self.player.set_paused(paused);
    }

    fn set_volume(&mut self, volume: u8) {
        self.player.set_volume(volume);
    }

    fn set_muted(&mut self, muted: bool) {
        self.player.set_engine_muted(muted);
    }

    fn seek(&mut self, seek: Seek) {
        self.player.seek(seek);
    }

    fn apply(&mut self, name: &str, control: &Control, value: Option<&Value>) {
        self.player.apply(name, control, value);
    }

    fn screenshot(&mut self, path: PathBuf) {
        if let Some(result) = self.view.capture(&path) {
            self.tx.send(Msg::Content(self.id, ContentEvent::Screenshot { path, result }));
            return;
        }
        match self.player.screenshot(&path) {
            Ok(cmd) => {
                if let Ok(mut m) = self.pending.lock() {
                    m.insert(cmd, path);
                }
            }
            Err(e) => self.tx.send(Msg::Content(self.id, ContentEvent::Screenshot { path, result: Err(e) })),
        }
    }

    fn pointer(&mut self, _ev: PointerEvent) {}

    fn set_input_enabled(&mut self, _enabled: bool) {}

    fn audio_data(&mut self, _bins: &[f32]) {}
}
