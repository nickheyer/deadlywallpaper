use crate::content::{ContentEvent, ContentId, PointerKind};
use crate::engine::Engine;
use crate::ipc::Request;
use crate::ipc::server::Reply;
use crate::model::Display;
use crate::platform::Snapshot;

/// Everything the engine reacts to, delivered on the platform main thread.
pub enum Msg {
    Request(Request, Reply),
    Tick,
    Displays(Vec<Display>),
    Windows(Snapshot),
    Session {
        locked: bool,
    },
    Content(ContentId, ContentEvent),
    Tray(TrayAction),
    /// Global pointer event in desktop coordinates (platforms that track the pointer).
    Pointer {
        x: i32,
        y: i32,
        kind: PointerKind,
    },
    /// The desktop reassigned its wallpaper areas (Plasma restarted or switched activity).
    #[cfg_attr(
        not(target_os = "linux"),
        allow(dead_code, reason = "sent by the Plasma presenter")
    )]
    DesktopChanged,
    /// The desktop's own settings replaced the wallpaper on a display.
    #[cfg_attr(
        not(target_os = "linux"),
        allow(dead_code, reason = "sent by the Plasma presenter")
    )]
    WallpaperDismissed {
        display: String,
    },
    /// Audio spectrum for visualizer wallpapers.
    Audio(crate::audio::Spectrum),
    /// The system media session changed, for wallpapers with media listeners.
    Media(crate::nowplaying::MediaEvent),
    /// Result of background work, applied on the engine thread.
    Job(Box<dyn FnOnce(&mut Engine) + Send>),
    Quit,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TrayAction {
    OpenUi,
    TogglePause,
    CloseAll,
    Random,
    Quit,
}
