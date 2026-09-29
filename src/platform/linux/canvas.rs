//! GTK wallpaper surfaces using wlr-layer-shell or X11 keep-below windows.

use crate::error::{Error, Result};
use crate::geom::{Rect, Size};
use crate::ipc::Capabilities;
use crate::model::Display;
use crate::platform::ShellApi;
use crate::platform::linux::layer::{LayerShell, LayerSurface};
use crate::platform::linux::{displays, is_wayland};
use gtk::prelude::*;

/// A background window covering one display (Wayland) or the whole virtual screen (X11).
struct Canvas {
    window: gtk::Window,
    layout: gtk::Layout,
    rect: Rect,
    display_id: Option<String>,
    _layer: Option<LayerSurface>,
}

impl Drop for Canvas {
    fn drop(&mut self) {
        self.window.hide();
        // The window is destroyed explicitly so GTK releases it now rather than at exit.
        unsafe { self.window.destroy() };
    }
}

pub struct Shell {
    display: gdk::Display,
    layer: Option<LayerShell>,
    canvases: Vec<Canvas>,
}

/// A rectangle inside a canvas that content widgets attach to.
pub struct Slot {
    layout: glib::WeakRef<gtk::Layout>,
    pub container: gtk::Box,
    pub size: Size,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(layout) = self.layout.upgrade() {
            layout.remove(&self.container);
        }
    }
}

impl Shell {
    pub fn new(display: &gdk::Display) -> Result<Shell> {
        let layer = LayerShell::attach(display)?;
        Ok(Shell { display: display.clone(), layer, canvases: Vec::new() })
    }

    pub fn is_wayland(&self) -> bool {
        self.layer.is_some() && is_wayland(&self.display)
    }

    fn new_window(&self) -> (gtk::Window, gtk::Layout) {
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title(crate::paths::APP_NAME);
        window.set_decorated(false);
        window.set_resizable(true);
        window.set_accept_focus(false);
        window.set_focus_on_map(false);
        window.set_skip_taskbar_hint(true);
        window.set_skip_pager_hint(true);
        window.connect_delete_event(|_, _| glib::Propagation::Stop);
        let layout = gtk::Layout::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        window.add(&layout);
        layout.show();
        (window, layout)
    }

    /// EWMH keep-below windows sit above desktop windows and below applications.
    fn x11_canvas(&mut self, display: &Display) -> Result<()> {
        let (window, layout) = self.new_window();
        let r = display.rect;
        window.set_type_hint(gdk::WindowTypeHint::Normal);
        window.move_(r.x, r.y);
        window.set_default_size(r.w, r.h);
        layout.set_size(r.w as u32, r.h as u32);
        // The window manager only honors state requests once it manages the window, which is
        // after the map completes.
        window.connect_map_event(|window, _| {
            if let Some(w) = window.window() {
                if let Err(e) = pin_x11(&w) {
                    log::warn!("wallpaper window state: {e}");
                }
                w.lower();
            }
            glib::Propagation::Proceed
        });
        window.realize();
        if let Some(w) = window.window() {
            all_desktops_hint(&w);
        }
        window.show_all();
        window.set_keep_below(true);
        window.resize(r.w, r.h);
        window.move_(r.x, r.y);
        self.canvases.push(Canvas { window, layout, rect: r, display_id: Some(display.id.clone()), _layer: None });
        Ok(())
    }

    fn wayland_canvas(&mut self, display: &Display) -> Result<()> {
        let layer = self.layer.as_ref().ok_or_else(|| Error::Platform("layer shell missing".into()))?;
        let monitor = displays::monitor_for(&self.display, display).ok_or_else(|| Error::Platform(format!("display {} has no GDK monitor", display.id)))?;
        let (window, layout) = self.new_window();
        window.set_default_size(display.rect.w, display.rect.h);
        layout.set_size(display.rect.w as u32, display.rect.h as u32);
        let surface = layer.make_layer_surface(&window, &monitor)?;
        self.canvases.push(Canvas { window, layout, rect: display.rect, display_id: Some(display.id.clone()), _layer: Some(surface) });
        Ok(())
    }
}

impl ShellApi for Shell {
    type Slot = Slot;

    fn spans_displays(&self) -> bool {
        false
    }

    fn sync_displays(&mut self, displays: &[Display]) -> Result<bool> {
        self.canvases.retain(|c| c.display_id.as_ref().is_some_and(|id| displays.iter().any(|d| &d.id == id && d.rect == c.rect)));
        for d in displays {
            if self.canvases.iter().any(|c| c.display_id.as_deref() == Some(&d.id)) {
                continue;
            }
            if self.is_wayland() {
                self.wayland_canvas(d)?;
            } else {
                self.x11_canvas(d)?;
            }
        }
        Ok(false)
    }

    fn slot(&mut self, display: &Display, region: Rect) -> Result<Slot> {
        let canvas = self
            .canvases
            .iter()
            .find(|c| c.display_id.as_deref() == Some(&display.id))
            .ok_or_else(|| Error::Platform(format!("no background surface for display {}", display.name)))?;
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        container.set_size_request(region.w, region.h);
        canvas.layout.put(&container, region.x - canvas.rect.x, region.y - canvas.rect.y);
        container.show();
        Ok(Slot { layout: canvas.layout.downgrade(), container, size: Size { w: region.w, h: region.h } })
    }

    /// Canvases own their surfaces outright; there is nothing to hand back to the desktop.
    fn settle(&mut self) {}

    fn capabilities(&self) -> Capabilities {
        let wayland = self.is_wayland();
        Capabilities {
            presenter: if wayland { "layer-shell".into() } else { "x11".into() },
            pointer_motion: true,
            pointer_clicks: true,
            global_pointer: false,
            programs: !wayland,
            web_devtools: true,
            // A WebKitGTK widget is an axis-aligned rectangle; the page can be moved and zoomed but not turned.
            rotate_web: false,
        }
    }
}

/// Before mapping, request every desktop through the initial `_NET_WM_DESKTOP` hint; window
/// managers read it when they start managing the window.
fn all_desktops_hint(window: &gdk::Window) {
    let all = 0xFFFF_FFFFu32.to_ne_bytes();
    gdk::property_change(window, &gdk::Atom::intern("_NET_WM_DESKTOP"), &gdk::Atom::intern("CARDINAL"), 32, gdk::PropMode::Replace, gdk::ChangeData::UChars(&all));
}

/// Mapped windows require EWMH client messages to change desktop and stacking hints.
fn pin_x11(window: &gdk::Window) -> Result<()> {
    use glib::translate::ToGlibPtr;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt, EventMask};
    unsafe extern "C" {
        fn gdk_x11_window_get_xid(window: *mut gdk::ffi::GdkWindow) -> libc::c_ulong;
    }
    // SAFETY: the GDK window is a live X11 window.
    let xid = unsafe { gdk_x11_window_get_xid(window.to_glib_none().0) } as u32;
    let (conn, screen) = x11rb::connect(None).map_err(|e| Error::Platform(format!("X11: {e}")))?;
    let root = conn.setup().roots[screen].root;
    let atom = |name: &str| -> Result<u32> {
        conn.intern_atom(false, name.as_bytes())
            .map_err(|e| Error::Platform(e.to_string()))?
            .reply()
            .map(|r| r.atom)
            .map_err(|e| Error::Platform(e.to_string()))
    };
    let (desktop, state) = (atom("_NET_WM_DESKTOP")?, atom("_NET_WM_STATE")?);
    let (below, skip, attention) = (atom("_NET_WM_STATE_BELOW")?, atom("_KDE_NET_WM_STATE_SKIP_SWITCHER")?, atom("_NET_WM_STATE_DEMANDS_ATTENTION")?);
    let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
    for data in [(desktop, [0xFFFF_FFFF, 1, 0, 0, 0]), (state, [1, below, skip, 1, 0]), (state, [0, attention, 0, 1, 0])] {
        let event = ClientMessageEvent::new(32, xid, data.0, data.1);
        conn.send_event(false, root, mask, event).map_err(|e| Error::Platform(e.to_string()))?;
    }
    conn.flush().map_err(|e| Error::Platform(e.to_string()))?;
    Ok(())
}
