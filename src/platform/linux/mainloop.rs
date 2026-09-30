use crate::content::Content;
use crate::error::{Error, Result};
use crate::model::{Display, Kind};
use crate::msg::Msg;
use crate::paths::Paths;
use crate::platform::linux::{
    canvas, displays, is_wayland, layer, media_view, monitor, plasma, program, session,
    shell::Shell, shell::Slot,
};
use crate::platform::{ContentSpec, MainLoopApi, MsgHandler, MsgSenderApi, RuntimeApi};
use crate::web::WebContent;
use gtk::prelude::*;
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
    fn run(self, mut handler: MsgHandler) {
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
        let on_plasma = plasma::available();
        if !on_plasma && layer::probe() == Some(false) {
            log::info!("compositor has no wlr-layer-shell; using the X11 backend through Xwayland");
            gdk::set_allowed_backends("x11");
        }
        gtk::init().map_err(|e| Error::Platform(format!("gtk init: {e}")))?;
        let display = gdk::Display::default()
            .ok_or_else(|| Error::Platform("no display connection".into()))?;
        #[allow(deprecated)]
        let (tx, rx) = glib::MainContext::channel(glib::Priority::DEFAULT);
        let tx = MsgSender(tx);
        displays::watch(&display, tx.clone());
        session::watch(tx.clone());
        let shell = if on_plasma {
            Shell::Plasma(plasma::Shell::new(paths, tx.clone())?)
        } else {
            Shell::Canvas(canvas::Shell::new(&display)?)
        };
        log::info!("presenting wallpapers through {}", shell.presenter());
        let rt = Runtime {
            tx,
            display,
            shell,
            monitor: monitor::Monitor::None,
            interval: Arc::new(AtomicU64::new(500)),
            paths: paths.clone(),
        };
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
        if is_wayland(&self.display) {
            "wayland".into()
        } else {
            "x11".into()
        }
    }

    fn start_window_monitor(&mut self, interval_ms: u64, track_pointer: bool) -> String {
        self.interval.store(interval_ms, Ordering::Relaxed);
        self.monitor = monitor::start(
            self.tx.clone(),
            self.interval.clone(),
            is_wayland(&self.display),
            &self.paths,
            track_pointer,
        );
        self.monitor.name().to_string()
    }

    fn set_monitor_interval(&mut self, interval_ms: u64) {
        self.interval.store(interval_ms, Ordering::Relaxed);
    }

    fn set_pointer_tracking(&mut self, track: bool) {
        self.monitor.set_pointer_tracking(track);
    }

    fn spawn_content(&mut self, spec: &ContentSpec<'_>, slot: &Slot) -> Result<Box<dyn Content>> {
        let kind = spec.wallpaper.kind();
        match (&mut self.shell, slot) {
            (Shell::Plasma(shell), Slot::Plasma(slot)) => {
                plasma::content::spawn(spec, slot, self.tx.clone(), shell)
            }
            (Shell::Canvas(shell), Slot::Canvas(slot)) => match kind {
                k if k.is_media() => media_view::spawn(spec, slot, self.tx.clone(), &self.display),
                k if k.is_web() => {
                    let builder = crate::web::builder(spec, self.tx.clone())?;
                    // The page keeps the image's size and moves inside this layout, which
                    // clips it to the display.
                    let inner =
                        gtk::Layout::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
                    inner.set_size_request(slot.size.w, slot.size.h);
                    slot.container.pack_start(&inner, true, true, 0);
                    inner.show();
                    let webview = builder
                        .build_gtk(&inner)
                        .map_err(|e| Error::Web(format!("web view: {e}")))?;
                    tune_webkit(&webview);
                    let page = {
                        use wry::WebViewExtUnix;
                        webview.webview()
                    };
                    let slot_size = slot.size;
                    let hook = move |v: &crate::content::View| -> Result<()> {
                        if v.rotation != 0.0 {
                            return Err(Error::Unsupported(
                                "web wallpapers cannot be rotated on this desktop".into(),
                            ));
                        }
                        use webkit2gtk::WebViewExt;
                        let (x, y) = v.origin(slot_size);
                        let (w, h) = (
                            (v.width as f64 * v.scale).round().max(1.0) as i32,
                            (v.height as f64 * v.scale).round().max(1.0) as i32,
                        );
                        page.set_size_request(w, h);
                        inner.move_(&page, x.round() as i32, y.round() as i32);
                        page.set_zoom_level(v.scale);
                        Ok(())
                    };
                    Ok(Box::new(
                        WebContent::new(webview, kind, spec.id, self.tx.clone(), slot.size)
                            .with_view_hook(Box::new(hook)),
                    ))
                }
                Kind::Program => program::spawn(spec, slot, self.tx.clone(), !shell.is_wayland()),
                _ => Err(Error::Unsupported(format!(
                    "{} wallpapers are not supported",
                    kind.label()
                ))),
            },
            _ => Err(Error::Platform(
                "the wallpaper slot belongs to another presenter".into(),
            )),
        }
    }
}

fn tune_webkit(webview: &wry::WebView) {
    use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt, WebViewExt};
    use wry::WebViewExtUnix;
    let wv = webview.webview();
    if let Some(s) = WebViewExt::settings(&wv) {
        s.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Always);
        s.set_media_playback_requires_user_gesture(false);
        s.set_enable_webgl(true);
        s.set_enable_webaudio(true);
        s.set_enable_write_console_messages_to_stdout(false);
    }
}
