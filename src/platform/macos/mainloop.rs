use crate::content::Content;
use crate::error::{Error, Result};
use crate::model::{Display, Kind};
use crate::msg::Msg;
use crate::paths::Paths;
use crate::platform::macos::{displays, input, media_view, monitor, shell::Shell, shell::Slot};
use crate::platform::{ContentSpec, MainLoopApi, MsgSenderApi, RuntimeApi};
use crate::web::WebContent;
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDidChangeScreenParametersNotification, NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

thread_local! {
    static LOOP: RefCell<Option<(Receiver<Msg>, Box<dyn FnMut(Msg) -> bool>)>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub struct MsgSender {
    tx: Sender<Msg>,
}

impl MsgSenderApi for MsgSender {
    fn send(&self, msg: Msg) {
        if self.tx.send(msg).is_ok() {
            dispatch2::DispatchQueue::main().exec_async(drain);
        }
    }
}

/// Deliver queued messages on the main thread; stops the application loop when asked.
fn drain() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let stop = LOOP.with(|l| {
        let mut guard = l.borrow_mut();
        let Some((rx, handler)) = guard.as_mut() else { return false };
        while let Ok(msg) = rx.try_recv() {
            if !handler(msg) {
                return true;
            }
        }
        false
    });
    if stop {
        LOOP.with(|l| l.borrow_mut().take());
        let app = NSApplication::sharedApplication(mtm);
        // stop() takes effect after the next event, so post one to wake the loop.
        {
            app.stop(None);
            if let Some(event) = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                NSEventType::ApplicationDefined,
                NSPoint::ZERO,
                NSEventModifierFlags::empty(),
                0.0,
                0,
                None,
                0,
                0,
                0,
            ) {
                app.postEvent_atStart(&event, true);
            }
        }
    }
}

pub struct MainLoop {
    rx: Receiver<Msg>,
    mtm: MainThreadMarker,
}

impl MainLoopApi for MainLoop {
    fn run(self, handler: Box<dyn FnMut(Msg) -> bool>) {
        LOOP.with(|l| *l.borrow_mut() = Some((self.rx, handler)));
        drain();
        let app = NSApplication::sharedApplication(self.mtm);
        app.run();
    }
}

pub struct Runtime {
    tx: MsgSender,
    shell: Shell,
    interval: Arc<AtomicU64>,
    mtm: MainThreadMarker,
    _observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl RuntimeApi for Runtime {
    fn init(_paths: &Paths) -> Result<(Runtime, MainLoop)> {
        let mtm = MainThreadMarker::new().ok_or_else(|| Error::Platform("the daemon must start on the main thread".into()))?;
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let (tx, rx) = channel();
        let tx = MsgSender { tx };
        let mut observers = Vec::new();
        {
            let t = tx.clone();
            let block = RcBlock::new(move |_: NonNull<NSNotification>| t.send(Msg::Displays(displays::list())));
            // SAFETY: notification center call with a retained block; the token is kept.
            let token = unsafe { NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(Some(NSApplicationDidChangeScreenParametersNotification), None, None, &block) };
            observers.push(token);
        }
        for (name, locked) in [("com.apple.screenIsLocked", true), ("com.apple.screenIsUnlocked", false)] {
            let t = tx.clone();
            let block = RcBlock::new(move |_: NonNull<NSNotification>| t.send(Msg::Session { locked }));
            // SAFETY: as above, on the distributed center.
            let token = unsafe { NSDistributedNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(Some(&NSString::from_str(name)), None, None, &block) };
            observers.push(token);
        }
        input::install(tx.clone());
        let shell = Shell::new(mtm);
        Ok((Runtime { tx, shell, interval: Arc::new(AtomicU64::new(500)), mtm, _observers: observers }, MainLoop { rx, mtm }))
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
        "quartz".into()
    }

    /// The global event monitor installed at startup always reports motion; the engine
    /// decides what reaches wallpapers.
    fn start_window_monitor(&mut self, interval_ms: u64, _track_pointer: bool) -> String {
        self.interval.store(interval_ms, Ordering::Relaxed);
        monitor::start(self.tx.clone(), self.interval.clone());
        "quartz".into()
    }

    fn set_monitor_interval(&mut self, interval_ms: u64) {
        self.interval.store(interval_ms, Ordering::Relaxed);
    }

    fn set_pointer_tracking(&mut self, _track: bool) {}

    fn spawn_content(&mut self, spec: &ContentSpec<'_>, slot: &Slot) -> Result<Box<dyn Content>> {
        let kind = spec.wallpaper.kind();
        match kind {
            k if k.is_media() => media_view::spawn(spec, slot, self.tx.clone(), self.mtm),
            k if k.is_web() => {
                let builder = crate::web::builder(spec, self.tx.clone())?;
                let bounds = wry::Rect { position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(), size: wry::dpi::LogicalSize::new(slot.size.w as f64, slot.size.h as f64).into() };
                let webview = builder.with_bounds(bounds).build_as_child(slot).map_err(|e| Error::Web(format!("web view: {e}")))?;
                // The slot view becomes the image: framed at the scaled size, laid out at the
                // image size, turned about its centre. The desktop window clips it.
                let view = slot.view.clone();
                let home = view.frame();
                let slot_size = slot.size;
                let hook = move |v: &crate::content::View| -> Result<()> {
                    let (x, y) = v.origin(slot_size);
                    let (w, h) = (v.width as f64 * v.scale, v.height as f64 * v.scale);
                    view.setFrameCenterRotation(0.0);
                    view.setFrame(NSRect::new(NSPoint::new(home.origin.x + x, home.origin.y + home.size.height - y - h), NSSize::new(w, h)));
                    view.setBoundsSize(NSSize::new(v.width as f64, v.height as f64));
                    view.setFrameCenterRotation(-v.rotation);
                    Ok(())
                };
                Ok(Box::new(WebContent::new(webview, kind, spec.id, self.tx.clone(), slot.size).with_view_hook(Box::new(hook))))
            }
            Kind::Program => Err(Error::Unsupported("program wallpapers are not supported on macOS: another application's window cannot be embedded".into())),
            _ => Err(Error::Unsupported(format!("{} wallpapers are not supported", kind.label()))),
        }
    }
}

/// Bundle identifier of the frontmost application, if any.
pub fn frontmost() -> Option<(i32, String)> {
    let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
    let app = ws.frontmostApplication()?;
    let bundle = app.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default();
    Some((app.processIdentifier(), bundle))
}

pub fn any_object_string(o: &AnyObject) -> Option<String> {
    o.downcast_ref::<NSString>().map(|s| s.to_string())
}
