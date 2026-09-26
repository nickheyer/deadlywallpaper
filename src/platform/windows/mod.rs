pub mod displays;
pub mod input;
pub mod mainloop;
pub mod monitor;
pub mod program;
pub mod shell;

pub use mainloop::{MainLoop, MsgSender, Runtime};
pub use shell::{Shell, Slot};

use windows::Win32::Foundation::HWND;
use windows::core::PCWSTR;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn pcwstr(buf: &[u16]) -> PCWSTR {
    PCWSTR(buf.as_ptr())
}

pub fn class_name(hwnd: HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buf = [0u16; 256];
    // SAFETY: buffer of the stated length.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}
