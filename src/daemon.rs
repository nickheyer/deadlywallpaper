use crate::engine::Engine;
use crate::error::Result;
use crate::ipc::server::Server;
use crate::model::{Layout, Settings};
use crate::msg::Msg;
use crate::paths::Paths;
use crate::platform::{MainLoopApi, MsgSenderApi, Runtime, RuntimeApi};
use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

static STOP: AtomicBool = AtomicBool::new(false);

pub fn run() -> Result<()> {
    let paths = Paths::discover()?;
    crate::logger::init(Some(&paths.log_file()), std::io::stderr().is_terminal());
    log::info!("deadlywp {} starting", env!("CARGO_PKG_VERSION"));
    let (rt, main_loop) = Runtime::init(&paths)?;
    let settings = Settings::load(&paths.settings_file(), &paths.default_library_dir());
    let layout = Layout::load(&paths.layout_file());
    let tx = rt.sender();
    let server = Server::start(Arc::new(move |req, reply| {
        tx.send(Msg::Request(req, reply))
    }))?;
    let mut engine = Engine::new(rt, paths, settings, layout, server)?;
    install_signal_handlers();
    let interval = Arc::new(AtomicU64::new(engine.interval()));
    let tick_tx = engine.sender();
    let tick_interval = interval.clone();
    std::thread::Builder::new()
        .name("tick".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(
                    tick_interval.load(Ordering::Relaxed).max(100),
                ));
                if STOP.load(Ordering::Relaxed) {
                    tick_tx.send(Msg::Quit);
                    return;
                }
                tick_tx.send(Msg::Tick);
            }
        })
        .map_err(|e| crate::error::Error::Platform(e.to_string()))?;
    log::info!("ready");
    main_loop.run(Box::new(move |msg| {
        let keep = engine.handle(msg);
        interval.store(engine.interval(), Ordering::Relaxed);
        keep
    }));
    log::info!("stopped");
    Ok(())
}

#[cfg(unix)]
fn install_signal_handlers() {
    extern "C" fn on_signal(_: libc::c_int) {
        STOP.store(true, Ordering::Relaxed);
    }
    // SAFETY: the handler only stores to an atomic.
    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGHUP, handler);
    }
}

#[cfg(windows)]
fn install_signal_handlers() {
    use windows::Win32::System::Console::{CTRL_C_EVENT, CTRL_CLOSE_EVENT, SetConsoleCtrlHandler};
    unsafe extern "system" fn on_ctrl(kind: u32) -> windows::core::BOOL {
        if kind == CTRL_C_EVENT || kind == CTRL_CLOSE_EVENT {
            STOP.store(true, Ordering::Relaxed);
        }
        windows::core::BOOL(1)
    }
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(on_ctrl), true);
    }
}
