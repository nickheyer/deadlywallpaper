use crate::content::Content;
use crate::error::{Error, Result};
use crate::media::player::{MediaContent, MediaSurface, Player, PlayerOptions, Vo, event_bridge};
use crate::model::{Display, Kind};
use crate::msg::Msg;
use crate::paths::Paths;
use crate::platform::windows::{
    displays, input, monitor, pcwstr, program, shell::Shell, shell::Slot, wide,
};
use crate::platform::{ContentSpec, MainLoopApi, MsgSenderApi, RuntimeApi};
use crate::web::WebContent;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, TranslateMessage, WINDOW_EX_STYLE,
    WM_APP, WM_DISPLAYCHANGE, WM_ENDSESSION, WM_SETTINGCHANGE, WM_WTSSESSION_CHANGE, WNDCLASSW,
    WS_OVERLAPPED,
};

const WM_WAKE: u32 = WM_APP + 1;
const WTS_SESSION_LOCK: usize = 7;
const WTS_SESSION_UNLOCK: usize = 8;
const CLASS: &str = "DeadlyWallpaperPump";

thread_local! {
    static LOOP: RefCell<Option<(Receiver<Msg>, Box<dyn FnMut(Msg) -> bool>)>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: RefCell<u32> = const { RefCell::new(0) };
}

static PUMP_TX: std::sync::OnceLock<Sender<Msg>> = std::sync::OnceLock::new();

#[derive(Clone)]
pub struct MsgSender {
    tx: Sender<Msg>,
    hwnd: isize,
}

impl MsgSenderApi for MsgSender {
    fn send(&self, msg: Msg) {
        if self.tx.send(msg).is_ok() {
            // SAFETY: posting to a window we created; a dead window just fails.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(self.hwnd as *mut _)),
                    WM_WAKE,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        }
    }
}

pub struct MainLoop {
    rx: Receiver<Msg>,
}

impl MainLoopApi for MainLoop {
    fn run(self, handler: Box<dyn FnMut(Msg) -> bool>) {
        LOOP.with(|l| *l.borrow_mut() = Some((self.rx, handler)));
        drain();
        let mut msg = MSG::default();
        // SAFETY: standard message pump on the thread that owns the windows.
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

/// Deliver queued messages to the engine; quits the loop when the engine asks to stop.
fn drain() {
    let stop = LOOP.with(|l| {
        let mut guard = l.borrow_mut();
        let Some((rx, handler)) = guard.as_mut() else {
            return false;
        };
        while let Ok(msg) = rx.try_recv() {
            if !handler(msg) {
                return true;
            }
        }
        false
    });
    if stop {
        LOOP.with(|l| l.borrow_mut().take());
        // SAFETY: ends the message loop on this thread.
        unsafe { PostQuitMessage(0) };
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let post = |m: Msg| {
        if let Some(tx) = PUMP_TX.get() {
            let _ = tx.send(m);
        }
    };
    match msg {
        WM_WAKE => {
            drain();
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            post(Msg::Displays(displays::list()));
            drain();
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            post(Msg::Displays(displays::list()));
            post(Msg::ColorScheme {
                dark: crate::scheme::prefers_dark(),
            });
            drain();
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            match wparam.0 {
                WTS_SESSION_LOCK => post(Msg::Session { locked: true }),
                WTS_SESSION_UNLOCK => post(Msg::Session { locked: false }),
                _ => {}
            }
            drain();
            LRESULT(0)
        }
        WM_ENDSESSION => {
            post(Msg::Quit);
            drain();
            LRESULT(0)
        }
        m if m == TASKBAR_CREATED.with(|t| *t.borrow()) && m != 0 => {
            post(Msg::Displays(displays::list()));
            drain();
            LRESULT(0)
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub struct Runtime {
    tx: MsgSender,
    shell: Shell,
    interval: Arc<AtomicU64>,
    _paths: Paths,
}

impl RuntimeApi for Runtime {
    fn init(paths: &Paths) -> Result<(Runtime, MainLoop)> {
        // SAFETY: process-wide setup on the main thread before any window exists.
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(|e| Error::Platform(format!("COM init: {e}")))?;
        }
        let hinstance =
            unsafe { GetModuleHandleW(None) }.map_err(|e| Error::Platform(e.to_string()))?;
        let class = wide(CLASS);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: pcwstr(&class),
            ..Default::default()
        };
        // SAFETY: valid class description; a hidden top-level window receives broadcasts.
        let hwnd = unsafe {
            RegisterClassW(&wc);
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                pcwstr(&class),
                pcwstr(&wide("Deadly Wallpaper")),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinstance.into()),
                None,
            )
        }
        .map_err(|e| Error::Platform(format!("message window: {e}")))?;
        // SAFETY: registering for session notifications on our own window.
        unsafe {
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            TASKBAR_CREATED.with(|t| {
                *t.borrow_mut() = RegisterWindowMessageW(pcwstr(&wide("TaskbarCreated")))
            });
        }
        let (tx, rx) = channel();
        let _ = PUMP_TX.set(tx.clone());
        let sender = MsgSender {
            tx,
            hwnd: hwnd.0 as isize,
        };
        input::install(sender.clone());
        let shell = Shell::new(hinstance.into())?;
        Ok((
            Runtime {
                tx: sender,
                shell,
                interval: Arc::new(AtomicU64::new(500)),
                _paths: paths.clone(),
            },
            MainLoop { rx },
        ))
    }

    fn sender(&self) -> MsgSender {
        self.tx.clone()
    }

    fn displays(&self) -> Vec<Display> {
        displays::list()
    }

    fn shell(&mut self) -> &mut Shell {
        &mut self.shell
    }

    fn session(&self) -> String {
        "win32".into()
    }

    /// The low-level mouse hook installed at startup always reports motion; the engine
    /// decides what reaches wallpapers.
    fn start_window_monitor(&mut self, interval_ms: u64, _track_pointer: bool) -> String {
        self.interval.store(interval_ms, Ordering::Relaxed);
        monitor::start(self.tx.clone(), self.interval.clone());
        "win32".into()
    }

    fn set_monitor_interval(&mut self, interval_ms: u64) {
        self.interval.store(interval_ms, Ordering::Relaxed);
    }

    fn set_pointer_tracking(&mut self, _track: bool) {}

    fn spawn_content(&mut self, spec: &ContentSpec<'_>, slot: &Slot) -> Result<Box<dyn Content>> {
        let kind = spec.wallpaper.kind();
        match kind {
            k if k.is_media() => {
                let wp = spec.wallpaper;
                if !k.is_online() && !std::path::Path::new(&wp.source).is_file() {
                    return Err(Error::NotFound(format!("{} does not exist", wp.source)));
                }
                let (events, pending) = event_bridge(spec.id, self.tx.clone());
                let player = Player::new(
                    PlayerOptions {
                        kind,
                        source: &wp.source,
                        audio: spec.audio,
                        volume: spec.volume,
                        hw_accel: spec.settings.video.hw_accel,
                        scaler: spec.settings.video.scaler,
                        stream_quality: spec.settings.video.stream_quality,
                        vo: Vo::Wid(slot.hwnd.0 as isize as i64),
                        slot: slot.size,
                    },
                    events,
                )?;
                player.load()?;
                struct Surface;
                impl MediaSurface for Surface {}
                Ok(Box::new(MediaContent::new(
                    Box::new(Surface),
                    player,
                    pending,
                    spec.id,
                    self.tx.clone(),
                )))
            }
            k if k.is_web() => {
                let builder = crate::web::builder(spec, self.tx.clone())?;
                let bounds = wry::Rect {
                    position: wry::dpi::PhysicalPosition::new(0, 0).into(),
                    size: wry::dpi::PhysicalSize::new(slot.size.w as u32, slot.size.h as u32)
                        .into(),
                };
                let webview = builder
                    .with_bounds(bounds)
                    .build_as_child(slot)
                    .map_err(|e| Error::Web(format!("web view: {e}")))?;
                let controller = {
                    use wry::WebViewExtWindows;
                    webview.controller()
                };
                let slot_size = slot.size;
                let hook = move |v: &crate::content::View| -> Result<()> {
                    emulate_viewport(
                        &controller,
                        (!v.is_whole(slot_size)).then_some((v.width, v.height)),
                    )
                };
                Ok(Box::new(
                    WebContent::new(webview, kind, spec.id, self.tx.clone(), slot.size)
                        .with_view_hook(Box::new(hook))
                        .with_css_view(true),
                ))
            }
            Kind::Program => program::spawn(spec, slot, self.tx.clone()),
            _ => Err(Error::Unsupported(format!(
                "{} wallpapers are not supported",
                kind.label()
            ))),
        }
    }
}

/// Lay the page out at `size` regardless of its window, through the DevTools protocol, so a
/// spanning page keeps the image's viewport while the window shows this display's part of it.
fn emulate_viewport(
    controller: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
    size: Option<(i32, i32)>,
) -> Result<()> {
    use crate::platform::windows::{pcwstr, wide};
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    let (method, params) = match size {
        Some((w, h)) => (
            "Emulation.setDeviceMetricsOverride",
            format!(r#"{{"width":{w},"height":{h},"deviceScaleFactor":0,"mobile":false}}"#),
        ),
        None => ("Emulation.clearDeviceMetricsOverride", "{}".to_string()),
    };
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
        move |hr: windows::core::Result<()>, _json: String| {
            if let Err(e) = hr {
                log::warn!("{method}: {e}");
            }
            Ok(())
        },
    ));
    let (m, p) = (wide(method), wide(&params));
    // SAFETY: COM calls on the thread that owns the controller; WebView2 keeps the handler alive
    // until the call completes.
    unsafe {
        let core = controller
            .CoreWebView2()
            .map_err(|e| Error::Web(format!("webview2: {e}")))?;
        core.CallDevToolsProtocolMethod(pcwstr(&m), pcwstr(&p), &handler)
            .map_err(|e| Error::Web(format!("{method}: {e}")))
    }
}
