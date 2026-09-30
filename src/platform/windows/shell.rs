//! Wallpaper surfaces parented under Explorer's WorkerW, behind the desktop icons.

use crate::error::{Error, Result};
use crate::geom::{Rect, Size};
use crate::ipc::Capabilities;
use crate::model::Display;
use crate::model::display::virtual_bounds;
use crate::platform::ShellApi;
use crate::platform::windows::{class_name, pcwstr, wide};
use raw_window_handle::{HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use std::num::NonZeroIsize;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, FindWindowExW, FindWindowW,
    IsWindow, MoveWindow, RegisterClassW, SMTO_NORMAL, SendMessageTimeoutW, WNDCLASSW, WS_CHILD,
    WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_NOACTIVATE, WS_VISIBLE,
};
use windows::core::BOOL;

const CLASS: &str = "DeadlyWallpaperSurface";

pub struct Shell {
    hinstance: HINSTANCE,
    canvas: Option<(HWND, Rect)>,
}

pub struct Slot {
    pub hwnd: HWND,
    pub size: Size,
    hinstance: HINSTANCE,
}

impl Drop for Slot {
    fn drop(&mut self) {
        // SAFETY: we created this window on this thread.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

impl HasWindowHandle for Slot {
    fn window_handle(
        &self,
    ) -> std::result::Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        let mut h = Win32WindowHandle::new(
            NonZeroIsize::new(self.hwnd.0 as isize)
                .ok_or(raw_window_handle::HandleError::Unavailable)?,
        );
        h.hinstance = NonZeroIsize::new(self.hinstance.0 as isize);
        // SAFETY: the handle stays valid for the life of the slot.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(h)) })
    }
}

unsafe extern "system" fn find_worker(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: data points at the Option<HWND> passed by `desktop_parent`.
    let out = unsafe { &mut *(data.0 as *mut Option<HWND>) };
    // SAFETY: plain window queries.
    unsafe {
        if FindWindowExW(Some(hwnd), None, pcwstr(&wide("SHELLDLL_DefView")), None).is_ok() {
            if let Ok(w) = FindWindowExW(None, Some(hwnd), pcwstr(&wide("WorkerW")), None) {
                *out = Some(w);
                return BOOL(0);
            }
        }
    }
    BOOL(1)
}

/// The window that sits behind the desktop icons: Explorer's spawned WorkerW, or Progman
/// itself on builds where Progman hosts the icon view directly.
fn desktop_parent() -> Result<HWND> {
    // SAFETY: standard Explorer handshake; message 0x052C asks Progman to create WorkerW.
    unsafe {
        let progman = FindWindowW(pcwstr(&wide("Progman")), None)
            .map_err(|_| Error::Platform("Explorer's Progman window is not running".into()))?;
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            windows::Win32::Foundation::WPARAM(0xD),
            windows::Win32::Foundation::LPARAM(0x1),
            SMTO_NORMAL,
            1000,
            None,
        );
        let mut found: Option<HWND> = None;
        let _ = EnumWindows(Some(find_worker), LPARAM(&mut found as *mut _ as isize));
        Ok(found.unwrap_or(progman))
    }
}

unsafe extern "system" fn surface_proc(
    hwnd: HWND,
    msg: u32,
    w: windows::Win32::Foundation::WPARAM,
    l: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    // SAFETY: default processing.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

impl Shell {
    pub fn new(hinstance: HINSTANCE) -> Result<Shell> {
        let class = wide(CLASS);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(surface_proc),
            hInstance: hinstance,
            lpszClassName: pcwstr(&class),
            ..Default::default()
        };
        // SAFETY: registering our window class; duplicates are harmless.
        unsafe { RegisterClassW(&wc) };
        Ok(Shell {
            hinstance,
            canvas: None,
        })
    }

    fn create(&self, parent: HWND, bounds: Rect, origin: (i32, i32)) -> Result<HWND> {
        let class = wide(CLASS);
        // SAFETY: valid class and parent; the window is a child of the desktop layer.
        unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                pcwstr(&class),
                pcwstr(&wide("Deadly Wallpaper")),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                bounds.x - origin.0,
                bounds.y - origin.1,
                bounds.w,
                bounds.h,
                Some(parent),
                None,
                Some(self.hinstance),
                None,
            )
        }
        .map_err(|e| Error::Platform(format!("wallpaper surface: {e}")))
    }
}

impl ShellApi for Shell {
    type Slot = Slot;

    fn spans_displays(&self) -> bool {
        true
    }

    fn sync_displays(&mut self, displays: &[Display]) -> Result<bool> {
        let bounds = virtual_bounds(displays);
        // SAFETY: querying a handle we own; a destroyed window reports false.
        let alive = self
            .canvas
            .is_some_and(|(h, _)| unsafe { IsWindow(Some(h)) }.as_bool() && class_name(h) == CLASS);
        if bounds.is_empty() {
            self.canvas = None;
            return Ok(false);
        }
        if !alive {
            let parent = desktop_parent()?;
            let hwnd = self.create(parent, bounds, (bounds.x, bounds.y))?;
            let had = self.canvas.take().is_some();
            self.canvas = Some((hwnd, bounds));
            return Ok(had);
        }
        if let Some((hwnd, rect)) = self.canvas.as_mut() {
            if *rect != bounds {
                // SAFETY: moving our own child window; coordinates are relative to WorkerW,
                // whose client origin is the virtual screen origin.
                unsafe {
                    let _ = MoveWindow(*hwnd, 0, 0, bounds.w, bounds.h, true);
                }
                *rect = bounds;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn slot(&mut self, _display: &Display, region: Rect) -> Result<Slot> {
        let (canvas, bounds) = self
            .canvas
            .ok_or_else(|| Error::Platform("no wallpaper surface".into()))?;
        let hwnd = self.create(canvas, region, (bounds.x, bounds.y))?;
        Ok(Slot {
            hwnd,
            size: Size {
                w: region.w,
                h: region.h,
            },
            hinstance: self.hinstance,
        })
    }

    /// Explorer keeps drawing its own wallpaper under WorkerW; nothing is handed back.
    fn settle(&mut self) {}

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            presenter: "win32".into(),
            pointer_motion: true,
            pointer_clicks: true,
            global_pointer: true,
            programs: true,
            web_devtools: true,
            rotate_web: true,
            loop_blend: false,
        }
    }
}
