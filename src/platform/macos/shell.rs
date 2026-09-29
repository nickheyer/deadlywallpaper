//! One borderless window per display at the desktop window level, below the icon layer.

use crate::error::{Error, Result};
use crate::geom::{Rect, Size};
use crate::ipc::Capabilities;
use crate::model::Display;
use crate::platform::ShellApi;
use crate::platform::macos::{displays, ns_rect};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use raw_window_handle::{AppKitWindowHandle, HasWindowHandle, RawWindowHandle, WindowHandle};
use std::ptr::NonNull;

struct Canvas {
    window: Retained<NSWindow>,
    display_id: String,
    rect: Rect,
}

impl Drop for Canvas {
    fn drop(&mut self) {
        self.window.orderOut(None);
    }
}

pub struct Shell {
    mtm: MainThreadMarker,
    canvases: Vec<Canvas>,
}

pub struct Slot {
    pub view: Retained<NSView>,
    pub size: Size,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}

impl HasWindowHandle for Slot {
    fn window_handle(
        &self,
    ) -> std::result::Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        let ptr = NonNull::new(Retained::as_ptr(&self.view) as *mut std::ffi::c_void)
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        // SAFETY: the view lives as long as the slot.
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::AppKit(AppKitWindowHandle::new(ptr)))
        })
    }
}

impl Shell {
    pub fn new(mtm: MainThreadMarker) -> Shell {
        Shell {
            mtm,
            canvases: Vec::new(),
        }
    }

    fn create(&self, display: &Display) -> Result<Canvas> {
        let mtm = self.mtm;
        let screen = NSScreen::screens(mtm)
            .iter()
            .find(|s| {
                let f = s.frame();
                let top = displays::primary_height() - (f.origin.y + f.size.height);
                f.origin.x.round() as i32 == display.rect.x && top.round() as i32 == display.rect.y
            })
            .ok_or_else(|| Error::Platform(format!("no screen for display {}", display.name)))?;
        let frame = screen.frame();
        // SAFETY: standard window construction on the main thread; the window is never
        // released by close because we keep it retained.
        let window = unsafe {
            let w = NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc::<NSWindow>(),
                ns_rect(0.0, 0.0, frame.size.width, frame.size.height),
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            );
            w.setReleasedWhenClosed(false);
            w.setLevel(CGWindowLevelForKey(CGWindowLevelKey::DesktopWindowLevelKey) as isize);
            w.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::Stationary
                    | NSWindowCollectionBehavior::IgnoresCycle,
            );
            w.setIgnoresMouseEvents(true);
            w.setHasShadow(false);
            w.setOpaque(true);
            w.setBackgroundColor(Some(&NSColor::blackColor()));
            w.setFrame_display(frame, true);
            w.orderFrontRegardless();
            w
        };
        Ok(Canvas {
            window,
            display_id: display.id.clone(),
            rect: display.rect,
        })
    }
}

impl ShellApi for Shell {
    type Slot = Slot;

    fn spans_displays(&self) -> bool {
        false
    }

    fn sync_displays(&mut self, displays: &[Display]) -> Result<bool> {
        self.canvases.retain(|c| {
            displays
                .iter()
                .any(|d| d.id == c.display_id && d.rect == c.rect)
        });
        for d in displays {
            if !self.canvases.iter().any(|c| c.display_id == d.id) {
                let canvas = self.create(d)?;
                self.canvases.push(canvas);
            }
        }
        Ok(false)
    }

    fn slot(&mut self, display: &Display, region: Rect) -> Result<Slot> {
        let canvas = self
            .canvases
            .iter()
            .find(|c| c.display_id == display.id)
            .ok_or_else(|| Error::Platform(format!("no window for display {}", display.name)))?;
        let content = canvas
            .window
            .contentView()
            .ok_or_else(|| Error::Platform("window has no content view".into()))?;
        let x = (region.x - canvas.rect.x) as f64;
        let y_top = (region.y - canvas.rect.y) as f64;
        let y = canvas.rect.h as f64 - (y_top + region.h as f64);
        let view = NSView::initWithFrame(
            self.mtm.alloc::<NSView>(),
            ns_rect(x, y, region.w as f64, region.h as f64),
        );
        view.setWantsLayer(true);
        content.addSubview(&view);
        Ok(Slot {
            view,
            size: Size {
                w: region.w,
                h: region.h,
            },
        })
    }

    /// Finder keeps drawing its own desktop above these windows; nothing is handed back.
    fn settle(&mut self) {}

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            presenter: "quartz".into(),
            pointer_motion: true,
            pointer_clicks: true,
            global_pointer: true,
            programs: false,
            web_devtools: true,
            rotate_web: true,
        }
    }
}
