//! Visible top-level windows via Win32, polled at the rule interval.

use crate::geom::Rect;
use crate::msg::Msg;
use crate::platform::windows::{MsgSender, class_name};
use crate::platform::{MsgSenderApi, Snapshot, WindowInfo, WindowPlacement};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed, WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;
use windows::core::PWSTR;

const EXCLUDED: &[&str] = &[
    "WorkerW",
    "Progman",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
    "MultitaskingViewFrame",
    "XamlExplorerHostIslandWindow",
    "WindowsDashboard",
    "NotifyIconOverflowWindow",
    "RainmeterMeterWindow",
    "_cls_desk_",
    "DeadlyWallpaperSurface",
    "DeadlyWallpaperPump",
];

pub fn start(tx: MsgSender, interval: Arc<AtomicU64>) {
    let _ = std::thread::Builder::new()
        .name("win32-monitor".into())
        .spawn(move || {
            loop {
                tx.send(Msg::Windows(snapshot()));
                std::thread::sleep(Duration::from_millis(
                    interval.load(Ordering::Relaxed).max(100),
                ));
            }
        });
}

unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: data is the Vec pointer passed to EnumWindows.
    unsafe { &mut *(data.0 as *mut Vec<HWND>) }.push(hwnd);
    BOOL(1)
}

fn rect(r: RECT) -> Rect {
    Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
}

fn process_name(pid: u32) -> String {
    // SAFETY: handle is closed before returning; buffer length is passed correctly.
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
                .is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

pub fn snapshot() -> Snapshot {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: pointer outlives the enumeration.
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize)) };
    // SAFETY: plain query.
    let foreground = unsafe { GetForegroundWindow() };
    let mut windows = Vec::new();
    for hwnd in handles {
        // SAFETY: window queries on handles from EnumWindows; stale handles just fail.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                continue;
            }
            if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
                continue;
            }
            let mut cloaked: u32 = 0;
            if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4)
                .is_ok()
                && cloaked != 0
            {
                continue;
            }
            let class = class_name(hwnd);
            if EXCLUDED.iter().any(|c| c.eq_ignore_ascii_case(&class)) {
                continue;
            }
            let mut frame = RECT::default();
            let bounds = if DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut frame as *mut RECT as *mut _,
                std::mem::size_of::<RECT>() as u32,
            )
            .is_ok()
            {
                rect(frame)
            } else if GetWindowRect(hwnd, &mut frame).is_ok() {
                rect(frame)
            } else {
                continue;
            };
            if bounds.is_empty() {
                continue;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let fullscreen = GetMonitorInfoW(hmon, &mut info).as_bool() && {
                let m = rect(info.rcMonitor);
                bounds.x <= m.x
                    && bounds.y <= m.y
                    && bounds.right() >= m.right()
                    && bounds.bottom() >= m.bottom()
            };
            windows.push(WindowInfo {
                placement: WindowPlacement::Rect(bounds),
                fullscreen,
                maximized: IsZoomed(hwnd).as_bool(),
                focused: hwnd == foreground,
                app: process_name(pid),
                pid: (pid > 0).then_some(pid),
            });
        }
    }
    Snapshot { windows }
}
