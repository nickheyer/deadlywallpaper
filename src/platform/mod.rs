//! Platform contracts. Exactly one backend compiles; the engine only sees these types.

use crate::content::{Content, ContentId};
use crate::error::Result;
use crate::geom::Rect;
use crate::ipc::Capabilities;
use crate::model::{Display, Settings, Wallpaper};
use crate::msg::Msg;
use crate::paths::Paths;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux as imp;

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub use windows as imp;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos as imp;

pub use imp::{MainLoop, MsgSender, Runtime, Shell, Slot};

/// One visible window as seen by the window monitor. Backends report only windows that can
/// cover the desktop: mapped, not minimized, not docks or desktop layers.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowInfo {
    pub placement: WindowPlacement,
    pub fullscreen: bool,
    pub maximized: bool,
    pub focused: bool,
    /// Application identifier (process name, class, or app id).
    pub app: String,
    pub pid: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WindowPlacement {
    /// Frame geometry in desktop coordinates.
    Rect(Rect),
    /// Only the origins of the outputs the window is on are known.
    #[cfg_attr(
        not(target_os = "linux"),
        allow(
            dead_code,
            reason = "reported only by the wlroots foreign-toplevel monitor"
        )
    )]
    Outputs(Vec<(i32, i32)>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub windows: Vec<WindowInfo>,
}

/// What the engine needs to build a content instance.
pub struct ContentSpec<'a> {
    pub id: ContentId,
    pub wallpaper: &'a Wallpaper,
    pub audio: bool,
    pub volume: u8,
    pub settings: &'a Settings,
    /// Wallpaper Engine's `assets` folder, which scene wallpapers load shaders, effects and
    /// stock textures from; `None` when it was not found.
    pub assets: Option<&'a std::path::Path>,
}

/// Main-thread runtime: native windowing state used by the engine.
pub trait RuntimeApi: Sized {
    /// Initialize the toolkit on the main thread.
    fn init(paths: &Paths) -> Result<(Self, MainLoop)>;
    fn sender(&self) -> MsgSender;
    fn displays(&self) -> Vec<Display>;
    fn shell(&mut self) -> &mut Shell;
    /// Display server name for status output.
    fn session(&self) -> String;
    /// Start the window monitor; returns its backend name. `track_pointer` asks backends
    /// that report pointer motion through the daemon to do so.
    fn start_window_monitor(&mut self, interval_ms: u64, track_pointer: bool) -> String;
    fn set_monitor_interval(&mut self, interval_ms: u64);
    fn set_pointer_tracking(&mut self, track: bool);
    fn spawn_content(&mut self, spec: &ContentSpec<'_>, slot: &Slot) -> Result<Box<dyn Content>>;
}

/// The engine's message handler; returns `false` to stop the main loop.
pub type MsgHandler = Box<dyn FnMut(Msg) -> bool>;

/// The native event loop; delivers every [`Msg`] to the handler on the main thread.
pub trait MainLoopApi {
    /// Run until the handler returns `false`.
    fn run(self, handler: MsgHandler);
}

/// Desktop background layer: presents content regions on displays.
pub trait ShellApi {
    type Slot;
    /// Whether one slot can span several displays (X11 root-sized window, Windows WorkerW).
    fn spans_displays(&self) -> bool;
    /// Returns `true` when the background surfaces had to be recreated, which invalidates
    /// every slot handed out before.
    fn sync_displays(&mut self, displays: &[Display]) -> Result<bool>;
    /// A surface for `region` on `display`; content draws its view of the image inside it.
    fn slot(&mut self, display: &Display, region: Rect) -> Result<Self::Slot>;
    /// Called after every engine message: hand desktop areas no slot holds any more back to
    /// the desktop's own wallpaper.
    fn settle(&mut self);
    fn capabilities(&self) -> Capabilities;
}

pub trait MsgSenderApi: Clone + Send + 'static {
    fn send(&self, msg: Msg);
}
