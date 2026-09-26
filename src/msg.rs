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
    Session { locked: bool },
    Content(ContentId, ContentEvent),
    Tray(TrayAction),
    /// Global pointer event in desktop coordinates (platforms that inject input).
    #[cfg_attr(target_os = "linux", allow(dead_code, reason = "Linux delivers pointer input to wallpaper surfaces natively"))]
    Pointer { x: i32, y: i32, kind: PointerKind },
    /// Audio spectrum bins for visualizer wallpapers.
    Audio(Vec<f32>),
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
