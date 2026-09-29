//! On-screen windows from the Quartz window server, polled at the rule interval.

use crate::geom::Rect;
use crate::msg::Msg;
use crate::platform::macos::{MsgSender, mainloop};
use crate::platform::{MsgSenderApi, Snapshot, WindowInfo, WindowPlacement};
use objc2::runtime::AnyObject;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub fn start(tx: MsgSender, interval: Arc<AtomicU64>) {
    let _ = std::thread::Builder::new()
        .name("quartz-monitor".into())
        .spawn(move || {
            loop {
                tx.send(Msg::Windows(snapshot()));
                std::thread::sleep(Duration::from_millis(
                    interval.load(Ordering::Relaxed).max(100),
                ));
            }
        });
}

fn value(dict: &NSDictionary, key: &str) -> Option<objc2::rc::Retained<AnyObject>> {
    let key = NSString::from_str(key);
    dict.objectForKey(key.as_ref())
}

fn number(dict: &NSDictionary, key: &str) -> Option<f64> {
    value(dict, key)
        .and_then(|o| o.downcast::<NSNumber>().ok())
        .map(|n| n.doubleValue())
}

fn text(dict: &NSDictionary, key: &str) -> Option<String> {
    value(dict, key).and_then(|o| mainloop::any_object_string(&o))
}

pub fn snapshot() -> Snapshot {
    let own_pid = std::process::id() as i32;
    let frontmost = mainloop::frontmost();
    let Some(list): Option<CFRetained<objc2_core_foundation::CFArray>> = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        0,
    ) else {
        return Snapshot::default();
    };
    // SAFETY: CFArray of CFDictionary is toll-free bridged to NSArray<NSDictionary>.
    let array: &NSArray<NSDictionary> =
        unsafe { &*(CFRetained::as_ptr(&list).as_ptr() as *const NSArray<NSDictionary>) };
    let mut windows = Vec::new();
    let mut focused_taken = false;
    for dict in array.iter() {
        let layer = number(&dict, "kCGWindowLayer").unwrap_or(0.0) as i64;
        if layer != 0 {
            continue;
        }
        let pid = number(&dict, "kCGWindowOwnerPID").unwrap_or(0.0) as i32;
        if pid == own_pid {
            continue;
        }
        if number(&dict, "kCGWindowAlpha").unwrap_or(1.0) <= 0.0 {
            continue;
        }
        let Some(bounds) =
            value(&dict, "kCGWindowBounds").and_then(|o| o.downcast::<NSDictionary>().ok())
        else {
            continue;
        };
        let rect = Rect::new(
            number(&bounds, "X").unwrap_or(0.0) as i32,
            number(&bounds, "Y").unwrap_or(0.0) as i32,
            number(&bounds, "Width").unwrap_or(0.0) as i32,
            number(&bounds, "Height").unwrap_or(0.0) as i32,
        );
        if rect.w < 50 || rect.h < 50 {
            continue;
        }
        let app = text(&dict, "kCGWindowOwnerName").unwrap_or_default();
        let is_front = frontmost.as_ref().is_some_and(|(fp, _)| *fp == pid);
        let focused = is_front && !focused_taken;
        if focused {
            focused_taken = true;
        }
        windows.push(WindowInfo {
            placement: WindowPlacement::Rect(rect),
            fullscreen: false,
            maximized: false,
            focused,
            app,
            pid: Some(pid as u32),
        });
    }
    let displays = crate::platform::macos::displays::list();
    for w in &mut windows {
        if let WindowPlacement::Rect(r) = &w.placement {
            w.fullscreen = displays.iter().any(|d| {
                r.x <= d.rect.x
                    && r.y <= d.rect.y
                    && r.right() >= d.rect.right()
                    && r.bottom() >= d.rect.bottom()
            });
        }
    }
    Snapshot { windows }
}
