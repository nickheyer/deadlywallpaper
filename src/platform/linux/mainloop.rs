use crate::content::Content;
use crate::error::{Error, Result};
use crate::model::{Display, Kind};
use crate::msg::Msg;
use crate::paths::Paths;
use crate::platform::linux::{displays, layer, media_view, monitor, program, session, shell::Shell, shell::Slot};
use crate::platform::{ContentSpec, MainLoopApi, MsgSenderApi, RuntimeApi};
use crate::web::WebContent;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use wry::WebViewBuilderExtUnix;

#[derive(Clone)]
pub struct MsgSender(glib::Sender<Msg>);

impl MsgSenderApi for MsgSender {
    fn send(&self, msg: Msg) {
        let _ = self.0.send(msg);
    }
}

pub struct MainLoop {
    rx: glib::Receiver<Msg>,
}

impl MainLoopApi for MainLoop {
    fn run(self, mut handler: Box<dyn FnMut(Msg) -> bool>) {
        self.rx.attach(None, move |msg| {
            if handler(msg) {
                glib::ControlFlow::Continue
            } else {
                gtk::main_quit();
                glib::ControlFlow::Break
            }
        });
        gtk::main();
    }
}

pub struct Runtime {
    tx: MsgSender,
    display: gdk::Display,
    shell: Shell,
    monitor: monitor::Monitor,
    interval: Arc<AtomicU64>,
    paths: Paths,
}

impl RuntimeApi for Runtime {
    fn init(paths: &Paths) -> Result<(Runtime, MainLoop)> {
        glib::set_prgname(Some(crate::paths::APP_ID));
        glib::set_application_name(crate::paths::APP_NAME);
        // GTK3 commits both EGL and shared-memory buffers on one surface; with the NVIDIA
        // driver's explicit sync enabled the compositor rejects the mixed commits.
        // SAFETY: called before any other thread exists.
        unsafe { std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1") };
        if layer::probe() == Some(false) {
            log::info!("compositor has no wlr-layer-shell; using the X11 backend through Xwayland");
            gdk::set_allowed_backends("x11");
        }
        gtk::init().map_err(|e| Error::Platform(format!("gtk init: {e}")))?;
        let display = gdk::Display::default().ok_or_else(|| Error::Platform("no display connection".into()))?;
        #[allow(deprecated)]
        let (tx, rx) = glib::MainContext::channel(glib::Priority::DEFAULT);
        let tx = MsgSender(tx);
        displays::watch(&display, tx.clone());
        session::watch(tx.clone());
        let shell = Shell::new(&display)?;
        let rt = Runtime { tx, display, shell, monitor: monitor::Monitor::None, interval: Arc::new(AtomicU64::new(500)), paths: paths.clone() };
        Ok((rt, MainLoop { rx }))
    }

    fn sender(&self) -> MsgSender {
        self.tx.clone()
    }

    fn displays(&self) -> Vec<Display> {
        displays::list(&self.display)
    }

    fn shell(&mut self) -> &mut Shell {
        &mut self.shell
    }

    fn session(&self) -> String {
        if self.shell.is_wayland() { "wayland".into() } else { "x11".into() }
    }

    fn start_window_monitor(&mut self, interval_ms: u64) -> String {
        self.interval.store(interval_ms, Ordering::Relaxed);
        self.monitor = monitor::start(self.tx.clone(), self.interval.clone(), self.shell.is_wayland(), &self.paths);
        self.monitor.name().to_string()
    }

    fn set_monitor_interval(&mut self, interval_ms: u64) {
        self.interval.store(interval_ms, Ordering::Relaxed);
    }

    fn spawn_content(&mut self, spec: &ContentSpec<'_>, slot: &Slot) -> Result<Box<dyn Content>> {
        let kind = spec.wallpaper.kind();
        match kind {
            k if k.is_media() => media_view::spawn(spec, slot, self.tx.clone(), &self.display),
            k if k.is_web() => {
                let builder = crate::web::builder(spec, self.tx.clone())?;
                let webview = builder.build_gtk(&slot.container).map_err(|e| Error::Web(format!("web view: {e}")))?;
                tune_webkit(&webview);
                Ok(Box::new(WebContent::new(webview, kind, spec.id, self.tx.clone())))
            }
            Kind::Program => program::spawn(spec, slot, self.tx.clone(), !self.shell.is_wayland()),
            _ => Err(Error::Unsupported(format!("{} wallpapers are not supported", kind.label()))),
        }
    }
}

fn tune_webkit(webview: &wry::WebView) {
    use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt, WebViewExt};
    use wry::WebViewExtUnix;
    let wv = webview.webview();
    if let Some(s) = wv.settings() {
        s.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Always);
        s.set_media_playback_requires_user_gesture(false);
        s.set_enable_webgl(true);
        s.set_enable_webaudio(true);
        s.set_enable_write_console_messages_to_stdout(false);
    }
}
