pub mod displays;
pub mod input;
pub mod mainloop;
pub mod media_view;
pub mod monitor;
pub mod shell;

pub use mainloop::{MainLoop, MsgSender, Runtime};
pub use shell::{Shell, Slot};

use objc2_foundation::{NSPoint, NSRect, NSSize};

pub fn ns_rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
