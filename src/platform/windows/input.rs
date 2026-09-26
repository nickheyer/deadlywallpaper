//! Low-level mouse hook: desktop clicks and motion reach interactive wallpapers, which sit
//! behind the icon layer and never receive input directly.

use crate::content::PointerKind;
use crate::msg::Msg;
use crate::platform::MsgSenderApi;
use crate::platform::windows::{MsgSender, class_name};
use std::sync::OnceLock;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetSystemMetrics, HHOOK, MSLLHOOKSTRUCT, SM_SWAPBUTTON, SetWindowsHookExW, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP,
};

static TX: OnceLock<MsgSender> = OnceLock::new();
static HOOK: OnceLock<isize> = OnceLock::new();

pub fn install(tx: MsgSender) {
    let _ = TX.set(tx);
    // SAFETY: installing a hook in this process for the lifetime of the daemon.
    unsafe {
        if let Ok(module) = GetModuleHandleW(None) {
            if let Ok(h) = SetWindowsHookExW(WH_MOUSE_LL, Some(hook), Some(module.into()), 0) {
                let _ = HOOK.set(h.0 as isize);
            }
        }
    }
}

/// True while the desktop (icon layer or wallpaper) is the foreground window.
pub fn desktop_foreground() -> bool {
    // SAFETY: plain query.
    let fg = unsafe { GetForegroundWindow() };
    if fg.0.is_null() {
        return true;
    }
    matches!(class_name(fg).as_str(), "WorkerW" | "Progman" | "DeadlyWallpaperSurface")
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for WH_MOUSE_LL, lparam points at an MSLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        // SAFETY: plain query.
        let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
        let kind = match wparam.0 as u32 {
            WM_MOUSEMOVE => Some(PointerKind::Move),
            WM_LBUTTONDOWN if !swapped => Some(PointerKind::Down),
            WM_RBUTTONDOWN if swapped => Some(PointerKind::Down),
            WM_LBUTTONUP if !swapped => Some(PointerKind::Up),
            WM_RBUTTONUP if swapped => Some(PointerKind::Up),
            _ => None,
        };
        if let (Some(kind), Some(tx)) = (kind, TX.get()) {
            if kind == PointerKind::Move || desktop_foreground() {
                tx.send(Msg::Pointer { x: info.pt.x, y: info.pt.y, kind });
            }
        }
    }
    // SAFETY: standard hook chaining.
    unsafe { CallNextHookEx(Some(HHOOK(HOOK.get().copied().unwrap_or(0) as *mut _)), code, wparam, lparam) }
}
