//! Window monitors feed playback rules with the visible application windows.

pub mod ewmh;
pub mod kwin;
pub mod wlr;

use crate::paths::Paths;
use crate::platform::linux::MsgSender;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

pub enum Monitor {
    /// Holds the D-Bus connection the KWin script reports into.
    Kwin { _connection: kwin::Kwin },
    Ewmh,
    Wlr,
    None,
}

impl Monitor {
    pub fn name(&self) -> &'static str {
        match self {
            Monitor::Kwin { .. } => "kwin",
            Monitor::Ewmh => "x11",
            Monitor::Wlr => "wlr-foreign-toplevel",
            Monitor::None => "none",
        }
    }
}

/// Pick the richest backend the session offers: KWin scripting, then wlroots' foreign
/// toplevel protocol on Wayland, then EWMH on X11.
pub fn start(tx: MsgSender, interval: Arc<AtomicU64>, wayland: bool, paths: &Paths) -> Monitor {
    if kwin::available() {
        match kwin::Kwin::start(tx.clone(), paths) {
            Ok(k) => return Monitor::Kwin { _connection: k },
            Err(e) => log::warn!("KWin window monitor: {e}"),
        }
    }
    if wayland {
        if wlr::start(tx) {
            return Monitor::Wlr;
        }
        return Monitor::None;
    }
    if ewmh::start(tx, interval) { Monitor::Ewmh } else { Monitor::None }
}
