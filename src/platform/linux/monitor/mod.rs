//! Window monitors feed playback rules with the visible application windows; on KWin the
//! same script also streams the pointer position to interactive wallpapers.

pub mod ewmh;
pub mod kwin;
pub mod wlr;

use crate::paths::Paths;
use crate::platform::linux::MsgSender;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

pub enum Monitor {
    Kwin(kwin::Kwin),
    Ewmh,
    Wlr,
    None,
}

impl Monitor {
    pub fn name(&self) -> &'static str {
        match self {
            Monitor::Kwin(_) => "kwin",
            Monitor::Ewmh => "x11",
            Monitor::Wlr => "wlr-foreign-toplevel",
            Monitor::None => "none",
        }
    }

    /// Only KWin reports pointer motion through the daemon; the other backends' wallpaper
    /// surfaces receive pointer input from the display server directly.
    pub fn set_pointer_tracking(&mut self, track: bool) {
        if let Monitor::Kwin(k) = self {
            k.set_pointer_tracking(track);
        }
    }
}

/// Pick the richest backend the session offers: KWin scripting, then wlroots' foreign
/// toplevel protocol on Wayland, then EWMH on X11.
pub fn start(
    tx: MsgSender,
    interval: Arc<AtomicU64>,
    wayland: bool,
    paths: &Paths,
    track_pointer: bool,
) -> Monitor {
    if kwin::available() {
        match kwin::Kwin::start(tx.clone(), paths, track_pointer) {
            Ok(k) => return Monitor::Kwin(k),
            Err(e) => log::warn!("KWin window monitor: {e}"),
        }
    }
    if wayland {
        if wlr::start(tx) {
            return Monitor::Wlr;
        }
        return Monitor::None;
    }
    if ewmh::start(tx, interval) {
        Monitor::Ewmh
    } else {
        Monitor::None
    }
}
