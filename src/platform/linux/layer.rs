//! wlr-layer-shell background surfaces on GDK windows. GDK is told to leave the window's
//! `wl_surface` unmanaged and we give it the layer-surface role ourselves.

use crate::error::{Error, Result};
use crate::platform::linux::{set_custom_surface, wl_display_ptr, wl_output_ptr, wl_surface_ptr};
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_output::WlOutput, wl_registry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{self, Layer, ZwlrLayerShellV1};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1};

const LAYER_SHELL: &str = "zwlr_layer_shell_v1";

struct Probe;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Probe {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

/// Before GTK starts: `None` when not on Wayland, otherwise whether the compositor offers a
/// layer shell.
pub fn probe() -> Option<bool> {
    std::env::var_os("WAYLAND_DISPLAY")?;
    let conn = Connection::connect_to_env().ok()?;
    let (globals, _queue) = registry_queue_init::<Probe>(&conn).ok()?;
    Some(globals.contents().with_list(|l| l.iter().any(|g| g.interface == LAYER_SHELL)))
}

#[derive(Default)]
struct State {
    globals: Vec<(u32, String, u32)>,
    windows: HashMap<ObjectId, glib::WeakRef<gtk::Window>>,
    configured: HashMap<ObjectId, (i32, i32)>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(state: &mut Self, _: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            state.globals.push((name, interface, version));
        }
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for State {
    fn event(_: &mut Self, _: &ZwlrLayerShellV1, _: zwlr_layer_shell_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for State {
    fn event(state: &mut Self, surface: &ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                surface.ack_configure(serial);
                let (w, h) = (width as i32, height as i32);
                state.configured.insert(surface.id(), (w, h));
                if let Some(win) = state.windows.get(&surface.id()).and_then(|w| w.upgrade()) {
                    if w > 0 && h > 0 {
                        win.set_size_request(w, h);
                        win.resize(w, h);
                    }
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                if let Some(win) = state.windows.remove(&surface.id()).and_then(|w| w.upgrade()) {
                    win.hide();
                }
            }
            _ => {}
        }
    }
}

/// One background surface; dropping it destroys the role and hides the window.
pub struct LayerSurface {
    proxy: ZwlrLayerSurfaceV1,
    shell: Rc<Inner>,
}

impl Drop for LayerSurface {
    fn drop(&mut self) {
        self.shell.state.borrow_mut().windows.remove(&self.proxy.id());
        self.proxy.destroy();
        let _ = self.shell.conn.flush();
    }
}

struct Inner {
    conn: Connection,
    queue: RefCell<EventQueue<State>>,
    qh: QueueHandle<State>,
    state: RefCell<State>,
    shell: ZwlrLayerShellV1,
}

impl Inner {
    fn dispatch(&self) {
        if let Some(guard) = self.queue.borrow().prepare_read() {
            let _ = guard.read();
        }
        let mut state = self.state.borrow_mut();
        if let Err(e) = self.queue.borrow_mut().dispatch_pending(&mut state) {
            log::warn!("layer shell dispatch: {e}");
        }
        let _ = self.conn.flush();
    }
}

pub struct LayerShell {
    inner: Rc<Inner>,
    layer: Layer,
    _watch: glib::SourceId,
}

/// KWin keeps the Plasma desktop above every other desktop-layer surface, so on KDE the
/// wallpaper must sit on the `bottom` layer to be visible at all; elsewhere `background`
/// stays underneath any desktop icon layer the shell provides.
fn preferred_layer() -> Layer {
    let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_ascii_lowercase().contains("kde"));
    if kde || crate::platform::linux::monitor::kwin::available() { Layer::Bottom } else { Layer::Background }
}

impl LayerShell {
    /// Attach to GDK's Wayland connection. `Ok(None)` when not on Wayland.
    pub fn attach(display: &gdk::Display) -> Result<Option<LayerShell>> {
        let ptr = wl_display_ptr(display);
        if ptr.is_null() {
            return Ok(None);
        }
        // SAFETY: GDK owns this wl_display for the life of the process.
        let backend = unsafe { Backend::from_foreign_display(ptr as *mut _) };
        let conn = Connection::from_backend(backend);
        let mut queue = conn.new_event_queue::<State>();
        let qh = queue.handle();
        let registry = conn.display().get_registry(&qh, ());
        let mut state = State::default();
        queue.roundtrip(&mut state).map_err(|e| Error::Platform(format!("wayland registry: {e}")))?;
        let (name, version) = state
            .globals
            .iter()
            .find(|(_, i, _)| i == LAYER_SHELL)
            .map(|(n, _, v)| (*n, *v))
            .ok_or_else(|| Error::Platform("compositor does not offer wlr-layer-shell".into()))?;
        let shell: ZwlrLayerShellV1 = registry.bind(name, version.min(4), &qh, ());
        let raw = std::os::fd::AsRawFd::as_raw_fd(&conn.backend().poll_fd());
        let inner = Rc::new(Inner { conn, queue: RefCell::new(queue), qh, state: RefCell::new(state), shell });
        let watched = inner.clone();
        let watch = glib::unix_fd_add_local(raw, glib::IOCondition::IN, move |_, _| {
            watched.dispatch();
            glib::ControlFlow::Continue
        });
        let layer = preferred_layer();
        log::info!("wayland layer shell v{} on the {:?} layer", version.min(4), layer);
        Ok(Some(LayerShell { inner, layer, _watch: watch }))
    }

    /// Turn a realized, unmapped toplevel into a background layer surface on `monitor`, then
    /// show it once the compositor has configured its size.
    pub fn make_layer_surface(&self, window: &gtk::Window, monitor: &gdk::Monitor) -> Result<LayerSurface> {
        window.realize();
        let gdk_window = window.window().ok_or_else(|| Error::Platform("window has no GDK surface".into()))?;
        set_custom_surface(&gdk_window);
        let inner = &self.inner;
        // SAFETY: both pointers are live proxies owned by GDK; the ids only wrap them.
        let (surface, output) = unsafe {
            let s = ObjectId::from_ptr(WlSurface::interface(), wl_surface_ptr(&gdk_window) as *mut _).map_err(|e| Error::Platform(format!("wl_surface: {e}")))?;
            let o = ObjectId::from_ptr(WlOutput::interface(), wl_output_ptr(monitor) as *mut _).map_err(|e| Error::Platform(format!("wl_output: {e}")))?;
            (WlSurface::from_id(&inner.conn, s), WlOutput::from_id(&inner.conn, o))
        };
        let surface = surface.map_err(|e| Error::Platform(format!("wl_surface proxy: {e}")))?;
        let output = output.map_err(|e| Error::Platform(format!("wl_output proxy: {e}")))?;
        let proxy = inner.shell.get_layer_surface(&surface, Some(&output), self.layer, crate::paths::APP_ID.into(), &inner.qh, ());
        proxy.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        proxy.set_exclusive_zone(-1);
        proxy.set_keyboard_interactivity(KeyboardInteractivity::None);
        proxy.set_size(0, 0);
        inner.state.borrow_mut().windows.insert(proxy.id(), window.downgrade());
        surface.commit();
        inner.conn.flush().map_err(|e| Error::Platform(format!("wayland flush: {e}")))?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            {
                let mut state = inner.state.borrow_mut();
                let mut queue = inner.queue.borrow_mut();
                queue.blocking_dispatch(&mut state).map_err(|e| Error::Platform(format!("layer surface configure: {e}")))?;
                if state.configured.contains_key(&proxy.id()) {
                    break;
                }
            }
            if Instant::now() > deadline {
                proxy.destroy();
                return Err(Error::Platform("compositor did not configure the background surface".into()));
            }
        }
        window.show_all();
        Ok(LayerSurface { proxy, shell: inner.clone() })
    }
}
