//! External program wallpapers re-parented into a wallpaper slot.

use crate::content::{Content, ContentEvent, PointerEvent, PointerKind, Seek, View};
use crate::error::{Error, Result};
use crate::geom::Size;
use crate::model::Control;
use crate::msg::Msg;
use crate::platform::windows::{MsgSender, Slot};
use crate::platform::{ContentSpec, MsgSenderApi};
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows::core::BOOL;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SUSPEND_RESUME, PROCESS_TERMINATE, TerminateProcess};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetParent,
    SetWindowLongPtrW, SetWindowPos, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WS_CAPTION, WS_CHILD, WS_EX_APPWINDOW, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};
use windows::core::s;

pub fn spawn(spec: &ContentSpec<'_>, slot: &Slot, tx: MsgSender) -> Result<Box<dyn Content>> {
    let wp = spec.wallpaper;
    let exe = PathBuf::from(&wp.source);
    if !exe.is_file() {
        return Err(Error::NotFound(format!("{} does not exist", exe.display())));
    }
    let child = Command::new(&exe)
        .args(wp.info.args())
        .current_dir(exe.parent().unwrap_or(std::path::Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::Platform(format!("start {}: {e}", exe.display())))?;
    let pid = child.id();
    let target = slot.hwnd.0 as isize;
    let window: Shared<Option<isize>> = Arc::new(Mutex::new(None));
    let geometry: Shared<(i32, i32, i32, i32)> = Arc::new(Mutex::new((0, 0, slot.size.w, slot.size.h)));
    let timeout = Duration::from_secs(spec.settings.video.load_timeout_secs);
    let id = spec.id;
    let finder_tx = tx.clone();
    let (found, placement) = (window.clone(), geometry.clone());
    std::thread::Builder::new()
        .name("program-window".into())
        .spawn(move || match find_window(pid, timeout) {
            Ok(hwnd) => {
                let raw = hwnd.0 as isize;
                finder_tx.send(Msg::Job(Box::new(move |_| {
                    let (x, y, w, h) = *lock(&placement);
                    attach(HWND(raw as *mut _), HWND(target as *mut _), x, y, w, h);
                    *lock(&found) = Some(raw);
                })));
                finder_tx.send(Msg::Content(id, ContentEvent::Loaded));
            }
            Err(e) => finder_tx.send(Msg::Content(id, ContentEvent::Exited { reason: e.to_string() })),
        })
        .map_err(|e| Error::Platform(e.to_string()))?;
    Ok(Box::new(ProgramContent::new(child, pid, id, tx, window, geometry, slot.size)))
}

type Shared<T> = Arc<Mutex<T>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Search {
    pid: u32,
    found: Option<HWND>,
}

unsafe extern "system" fn by_pid(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: data points at the Search struct passed by `find_window`.
    let s = unsafe { &mut *(data.0 as *mut Search) };
    let mut pid = 0u32;
    // SAFETY: plain queries.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == s.pid && IsWindowVisible(hwnd).as_bool() {
            s.found = Some(hwnd);
            return BOOL(0);
        }
    }
    BOOL(1)
}

fn find_window(pid: u32, timeout: Duration) -> Result<HWND> {
    let deadline = Instant::now() + timeout;
    loop {
        let mut s = Search { pid, found: None };
        // SAFETY: pointer outlives the enumeration.
        let _ = unsafe { EnumWindows(Some(by_pid), LPARAM(&mut s as *mut _ as isize)) };
        if let Some(h) = s.found {
            return Ok(h);
        }
        if Instant::now() > deadline {
            return Err(Error::Platform(format!("process {pid} showed no window within {}s", timeout.as_secs())));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn attach(hwnd: HWND, parent: HWND, x: i32, y: i32, w: i32, h: i32) {
    // SAFETY: restyling and re-parenting a window we located; failures are non-fatal.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & !(WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU | WS_POPUP).0 | WS_CHILD.0;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & !(WS_EX_APPWINDOW | WS_EX_DLGMODALFRAME | WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE).0;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
        let _ = SetParent(hwnd, Some(parent));
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

type NtProcessFn = unsafe extern "system" fn(HANDLE) -> i32;

fn nt(name: windows::core::PCSTR) -> Option<NtProcessFn> {
    // SAFETY: looking up a documented ntdll export and casting to its signature.
    unsafe {
        let ntdll = GetModuleHandleW(windows::core::w!("ntdll.dll")).ok()?;
        let f = GetProcAddress(ntdll, name)?;
        Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, NtProcessFn>(f))
    }
}

pub struct ProgramContent {
    pid: u32,
    /// The program's window once found and re-parented into the slot.
    window: Shared<Option<isize>>,
    /// Where the window sits inside the slot: x, y, width, height.
    geometry: Shared<(i32, i32, i32, i32)>,
    slot: Size,
    paused: bool,
}

impl ProgramContent {
    fn new(mut child: std::process::Child, pid: u32, id: crate::content::ContentId, tx: MsgSender, window: Shared<Option<isize>>, geometry: Shared<(i32, i32, i32, i32)>, slot: Size) -> ProgramContent {
        let _ = std::thread::Builder::new().name("program-wait".into()).spawn(move || {
            let reason = match child.wait() {
                Ok(s) => format!("program exited with {s}"),
                Err(e) => format!("program wait failed: {e}"),
            };
            tx.send(Msg::Content(id, ContentEvent::Exited { reason }));
        });
        ProgramContent { pid, window, geometry, slot, paused: false }
    }

    fn window(&self) -> Option<HWND> {
        lock(&self.window).map(|raw| HWND(raw as *mut _))
    }

    fn with_handle(&self, access: windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS, f: impl FnOnce(HANDLE)) {
        // SAFETY: handle closed after use.
        unsafe {
            if let Ok(h) = OpenProcess(access, false, self.pid) {
                f(h);
                let _ = windows::Win32::Foundation::CloseHandle(h);
            }
        }
    }
}

impl Drop for ProgramContent {
    fn drop(&mut self) {
        if self.paused {
            if let Some(resume) = nt(s!("NtResumeProcess")) {
                // SAFETY: valid handle inside the closure.
                self.with_handle(PROCESS_SUSPEND_RESUME, |h| unsafe {
                    resume(h);
                });
            }
        }
        // SAFETY: terminating the process we spawned.
        self.with_handle(PROCESS_TERMINATE, |h| unsafe {
            let _ = TerminateProcess(h, 0);
        });
    }
}

impl Content for ProgramContent {
    fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        let name = if paused { s!("NtSuspendProcess") } else { s!("NtResumeProcess") };
        if let Some(f) = nt(name) {
            // SAFETY: valid handle inside the closure.
            self.with_handle(PROCESS_SUSPEND_RESUME, |h| unsafe {
                f(h);
            });
            self.paused = paused;
        }
    }

    fn set_volume(&mut self, _volume: u8) {}

    fn set_muted(&mut self, _muted: bool) {}

    fn seek(&mut self, _seek: Seek) {}

    fn apply(&mut self, _name: &str, _control: &Control, _value: Option<&Value>) {}

    fn screenshot(&mut self, path: PathBuf) {
        let _ = path;
    }

    fn pointer(&mut self, ev: PointerEvent) {
        let Some(hwnd) = self.window() else { return };
        let (msg, wparam) = match ev.kind {
            PointerKind::Move => (WM_MOUSEMOVE, 0usize),
            PointerKind::Down => (WM_LBUTTONDOWN, 1usize),
            PointerKind::Up => (WM_LBUTTONUP, 0usize),
        };
        let lparam = ((ev.y as i32 as u32 as usize) << 16 | (ev.x as i32 as u16 as usize)) as isize;
        // SAFETY: posting to a window we embedded.
        let _ = unsafe { PostMessageW(Some(hwnd), msg, WPARAM(wparam), LPARAM(lparam)) };
    }

    fn set_input_enabled(&mut self, _enabled: bool) {}

    fn audio_data(&mut self, _bins: &[f32]) {}

    /// An embedded window can be placed but not scaled or turned; the slot clips it.
    fn set_view(&mut self, view: &View) -> Result<()> {
        if !view.is_plain() {
            return Err(Error::Unsupported("program wallpapers can be moved but not scaled or rotated".into()));
        }
        let (x, y) = view.origin(self.slot);
        let geometry = (x.round() as i32, y.round() as i32, view.width.max(1), view.height.max(1));
        *lock(&self.geometry) = geometry;
        if let Some(hwnd) = self.window() {
            // SAFETY: moving a child window we embedded; failures are non-fatal.
            let _ = unsafe { SetWindowPos(hwnd, None, geometry.0, geometry.1, geometry.2, geometry.3, SWP_NOZORDER | SWP_NOACTIVATE) };
        }
        Ok(())
    }
}
