use crate::geom::Rect;
use crate::model::Display;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::core::BOOL;
use windows::Win32::Graphics::Gdi::{DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};

const EDD_GET_DEVICE_INTERFACE_NAME: u32 = 0x0000_0001;
const MONITORINFOF_PRIMARY: u32 = 0x0000_0001;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::core::PCWSTR;

fn rect(r: RECT) -> Rect {
    Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
}

unsafe extern "system" fn collect(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    // SAFETY: data is the Vec pointer passed to EnumDisplayMonitors below.
    let out = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
    out.push(hmon);
    BOOL(1)
}

pub fn list() -> Vec<Display> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: callback and pointer live for the duration of the call.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    let mut out = Vec::new();
    for (i, hmon) in handles.into_iter().enumerate() {
        let mut info = MONITORINFOEXW { monitorInfo: MONITORINFO { cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32, ..Default::default() }, ..Default::default() };
        // SAFETY: info is sized correctly for the extended struct.
        if !unsafe { GetMonitorInfoW(hmon, &mut info.monitorInfo) }.as_bool() {
            continue;
        }
        let device = String::from_utf16_lossy(&info.szDevice).trim_end_matches('\0').to_string();
        let mut dd = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        let wide: Vec<u16> = device.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: device name is NUL-terminated and dd is sized.
        let ok = unsafe { EnumDisplayDevicesW(PCWSTR(wide.as_ptr()), 0, &mut dd, EDD_GET_DEVICE_INTERFACE_NAME) }.as_bool();
        let (id, name) = if ok {
            let id = String::from_utf16_lossy(&dd.DeviceID).trim_end_matches('\0').to_string();
            let name = String::from_utf16_lossy(&dd.DeviceString).trim_end_matches('\0').to_string();
            (if id.is_empty() { device.clone() } else { id }, if name.is_empty() { device.clone() } else { name })
        } else {
            (device.clone(), device.clone())
        };
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        // SAFETY: output pointers are valid.
        let _ = unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        out.push(Display {
            id: crate::paths::slug(&id.replace(['\\', '#', '{', '}'], "-")) + &format!("-{i}"),
            name,
            rect: rect(info.monitorInfo.rcMonitor),
            workarea: rect(info.monitorInfo.rcWork),
            scale: dpi_x as f64 / 96.0,
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    out
}
