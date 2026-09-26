//! Global mouse monitor: desktop input reaches interactive wallpapers sitting under the
//! Finder icon layer.

use crate::content::PointerKind;
use crate::msg::Msg;
use crate::platform::MsgSenderApi;
use crate::platform::macos::{MsgSender, displays, mainloop};
use block2::RcBlock;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
use std::ptr::NonNull;

pub fn install(tx: MsgSender) {
    let mask = NSEventMask::MouseMoved | NSEventMask::LeftMouseDown | NSEventMask::LeftMouseUp | NSEventMask::LeftMouseDragged;
    let block = RcBlock::new(move |event: NonNull<NSEvent>| {
        // SAFETY: AppKit passes a live event for the duration of the callback.
        let event = unsafe { event.as_ref() };
        let kind = match event.r#type() {
            NSEventType::MouseMoved | NSEventType::LeftMouseDragged => PointerKind::Move,
            NSEventType::LeftMouseDown => PointerKind::Down,
            NSEventType::LeftMouseUp => PointerKind::Up,
            _ => return,
        };
        if kind != PointerKind::Move && !desktop_frontmost() {
            return;
        }
        let p = NSEvent::mouseLocation();
        let y = displays::primary_height() - p.y;
        tx.send(Msg::Pointer { x: p.x.round() as i32, y: y.round() as i32, kind });
    });
    // The returned token is intentionally leaked so the monitor lives as long as the daemon.
    let token = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &block);
    std::mem::forget(token);
}

/// Finder is frontmost and the click therefore lands on the desktop.
pub fn desktop_frontmost() -> bool {
    mainloop::frontmost().is_none_or(|(_, bundle)| bundle == "com.apple.finder")
}
