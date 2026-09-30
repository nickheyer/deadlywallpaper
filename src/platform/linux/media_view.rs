//! libmpv rendering into a GtkGLArea.

use crate::content::{Content, View};
use crate::error::{Error, Result};
use crate::geom::Size;
use crate::media::glquad::{Framebuffer, Quad, render_frame};
use crate::media::looper::{Frame, Loop};
use crate::media::mpv::{
    Handle, RENDER_PARAM_WL_DISPLAY, RENDER_PARAM_X11_DISPLAY, RENDER_UPDATE_FRAME, RenderContext,
};
use crate::media::player::{MediaContent, MediaSurface, PlayerOptions, Vo, event_bridge};
use crate::platform::ContentSpec;
use crate::platform::linux::canvas::Slot;
use crate::platform::linux::gl;
use crate::platform::linux::{MsgSender, is_wayland, wl_display_ptr, x11_display_ptr};
use glib::SendWeakRef;
use gtk::prelude::*;
use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Arc;

pub fn spawn(
    spec: &ContentSpec<'_>,
    slot: &Slot,
    tx: MsgSender,
    display: &gdk::Display,
) -> Result<Box<dyn Content>> {
    let wp = spec.wallpaper;
    if !wp.kind().is_online() && !std::path::Path::new(&wp.source).is_file() {
        return Err(Error::NotFound(format!("{} does not exist", wp.source)));
    }
    let (events, pending) = event_bridge(spec.id, tx.clone());
    let looper = Arc::new(Loop::spawn(
        PlayerOptions {
            kind: wp.kind(),
            source: &wp.source,
            audio: spec.audio,
            volume: spec.volume,
            hw_accel: spec.settings.video.hw_accel,
            scaler: spec.settings.video.scaler,
            stream_quality: spec.settings.video.stream_quality,
            vo: Vo::Render,
            slot: slot.size,
        },
        true,
        events,
    )?);
    let view = MediaView::new(looper.clone(), slot, display)?;
    looper.load()?;
    Ok(Box::new(MediaContent::new(
        Box::new(view),
        looper,
        pending,
        spec.id,
        tx,
    )))
}

struct Render {
    looper: Arc<Loop>,
    handles: Vec<Arc<Handle>>,
    /// One per core, in the looper's order; empty until the GL context exists.
    ctxs: Vec<RenderContext>,
    /// What the last redraw showed, so captures repeat it rather than advancing the loop.
    frame: Frame,
    capture: Option<gl::Capture>,
    /// Draws the frame through the view; absent when the program could not be built.
    quad: Option<Quad>,
    view: View,
    slot: Size,
    callback_ctx: *mut SendWeakRef<gtk::GLArea>,
    prefer: *mut c_void,
    x11: *mut c_void,
    wl: *mut c_void,
}

pub struct MediaView {
    area: gtk::GLArea,
    render: Rc<RefCell<Render>>,
}

const RENDER_KEY: &str = "deadlywp-render";

/// Let libmpv run pending render-thread work (frame delivery, screenshots) on the GL
/// context, and request a redraw only when a new frame exists. Runs on the main thread
/// even while the compositor withholds frame callbacks from an occluded surface.
fn pump(area: &gtk::GLArea) {
    // SAFETY: the key holds an `Rc<RefCell<Render>>` stored in `MediaView::new` for the
    // lifetime of the widget and is only read on the main thread.
    let Some(render) = (unsafe { area.data::<Rc<RefCell<Render>>>(RENDER_KEY) }) else {
        return;
    };
    let render = unsafe { render.as_ref() }.clone();
    let r = render.borrow();
    if !r.ctxs.is_empty() && area.is_realized() {
        area.make_current();
        let mut fresh = false;
        for ctx in &r.ctxs {
            fresh |= ctx.update() & RENDER_UPDATE_FRAME != 0;
        }
        if fresh {
            area.queue_render();
        }
    }
}

impl MediaView {
    /// Create the view and its render context synchronously; fails when OpenGL is unavailable.
    pub fn new(looper: Arc<Loop>, slot: &Slot, display: &gdk::Display) -> Result<MediaView> {
        let area = gtk::GLArea::new();
        area.set_has_alpha(false);
        area.set_has_depth_buffer(false);
        area.set_has_stencil_buffer(false);
        area.set_auto_render(false);
        area.set_size_request(slot.size.w, slot.size.h);
        area.set_hexpand(true);
        area.set_vexpand(true);
        slot.container.pack_start(&area, true, true, 0);
        let wayland = is_wayland(display);
        let handles: Vec<Arc<Handle>> = looper
            .players()
            .iter()
            .map(|p| p.handle().clone())
            .collect();
        let render = Rc::new(RefCell::new(Render {
            looper,
            handles,
            ctxs: Vec::new(),
            frame: Frame {
                active: 0,
                fade: None,
            },
            capture: None,
            quad: None,
            view: View::whole(slot.size),
            slot: slot.size,
            callback_ctx: std::ptr::null_mut(),
            prefer: if wayland {
                gl::PREFER_EGL
            } else {
                gl::PREFER_GLX
            },
            x11: x11_display_ptr(display),
            wl: wl_display_ptr(display),
        }));

        // SAFETY: set once before any reader; removed with the widget.
        unsafe { area.set_data(RENDER_KEY, render.clone()) };
        let r = render.clone();
        area.connect_realize(move |area| ensure_context(area, &r));

        let r = render.clone();
        area.connect_render(move |area, _| {
            let mut r = r.borrow_mut();
            let scale = area.scale_factor();
            let (w, h) = (
                area.allocated_width() * scale,
                area.allocated_height() * scale,
            );
            let Render {
                looper,
                ctxs,
                frame,
                capture,
                quad,
                view,
                slot,
                ..
            } = &mut *r;
            if let (false, Some(gl)) = (ctxs.is_empty(), *capture) {
                for ctx in ctxs.iter() {
                    ctx.update();
                }
                *frame = looper.tick();
                if w > 0 && h > 0 {
                    let screen = Framebuffer {
                        fbo: gl.current_fbo(),
                        w,
                        h,
                        flip_y: true,
                    };
                    render_frame(ctxs, *frame, quad, view, *slot, screen, scale as f64);
                }
            }
            glib::Propagation::Stop
        });

        let r = render.clone();
        area.connect_unrealize(move |area| {
            area.make_current();
            release(&mut r.borrow_mut());
        });

        area.show();
        area.realize();
        ensure_context(&area, &render);
        if render.borrow().ctxs.is_empty() {
            slot.container.remove(&area);
            return Err(Error::Media(
                "OpenGL rendering is unavailable for this display".into(),
            ));
        }
        Ok(MediaView { area, render })
    }
}

/// Create one mpv render context per core on the area's GL context, once.
fn ensure_context(area: &gtk::GLArea, render: &Rc<RefCell<Render>>) {
    if !area.is_realized() || !render.borrow().ctxs.is_empty() {
        return;
    }
    area.make_current();
    if let Some(e) = area.error() {
        log::error!("GL context: {e}");
        return;
    }
    let mut r = render.borrow_mut();
    let extra = [
        (RENDER_PARAM_X11_DISPLAY, r.x11),
        (RENDER_PARAM_WL_DISPLAY, r.wl),
    ];
    let mut ctxs = Vec::new();
    for handle in &r.handles {
        match RenderContext::new(handle, gl::get_proc_address, r.prefer, &extra) {
            Ok(ctx) => ctxs.push(ctx),
            Err(e) => {
                log::error!("mpv render context: {e}");
                return;
            }
        }
    }
    let weak: Box<SendWeakRef<gtk::GLArea>> = Box::new(area.downgrade().into());
    let ptr = Box::into_raw(weak);
    for ctx in &ctxs {
        ctx.set_update_callback(Some(on_update), ptr as *mut c_void);
    }
    r.callback_ctx = ptr;
    r.capture = gl::Capture::load(gl::get_proc_address, r.prefer);
    r.quad = Quad::load(gl::get_proc_address, r.prefer);
    r.ctxs = ctxs;
    log::debug!(
        "{} mpv render context(s) ready ({:?})",
        r.ctxs.len(),
        area.context().map(|c| c.version())
    );
}

fn release(r: &mut Render) {
    if let Some(mut quad) = r.quad.take() {
        quad.destroy();
    }
    for ctx in r.ctxs.drain(..) {
        ctx.set_update_callback(None, std::ptr::null_mut());
        drop(ctx);
    }
    if !r.callback_ctx.is_null() {
        // SAFETY: the pointer came from Box::into_raw in realize and the callback is unset.
        drop(unsafe { Box::from_raw(r.callback_ctx) });
        r.callback_ctx = std::ptr::null_mut();
    }
}

impl MediaSurface for MediaView {
    /// Render the current frame into an offscreen buffer on our own GL context, so captures
    /// work with hardware-decoded frames and while the surface is occluded.
    fn capture(&self, path: &std::path::Path) -> Option<Result<()>> {
        let mut r = self.render.borrow_mut();
        let Render {
            ctxs,
            frame,
            capture,
            quad,
            view,
            slot,
            ..
        } = &mut *r;
        if ctxs.is_empty() {
            return None;
        }
        let gl = (*capture)?;
        if !self.area.is_realized() {
            return Some(Err(Error::Media("wallpaper surface is not ready".into())));
        }
        let scale = self.area.scale_factor();
        let (w, h) = (
            self.area.allocated_width() * scale,
            self.area.allocated_height() * scale,
        );
        if w <= 0 || h <= 0 {
            return Some(Err(Error::Media("wallpaper surface has no size".into())));
        }
        self.area.make_current();
        let pixels = gl.render_offscreen(w, h, |fbo, w, h| {
            let offscreen = Framebuffer {
                fbo,
                w,
                h,
                flip_y: false,
            };
            render_frame(ctxs, *frame, quad, view, *slot, offscreen, scale as f64)
        });
        let image = pixels.and_then(|p| crate::capture::from_gl_pixels(w as u32, h as u32, &p));
        Some(match image {
            Some(img) => crate::capture::save_rgba(img, path),
            None => Err(Error::Media("offscreen frame capture failed".into())),
        })
    }

    fn set_view(&self, view: &View, _slot: Size) -> Option<Result<()>> {
        let mut r = self.render.borrow_mut();
        if !view.is_whole(r.slot) && r.quad.is_none() {
            return Some(Err(Error::Media("OpenGL is unavailable for this display, so the wallpaper cannot be moved, scaled or turned".into())));
        }
        r.view = *view;
        self.area.queue_render();
        Some(Ok(()))
    }
}

impl Drop for MediaView {
    fn drop(&mut self) {
        if self.area.is_realized() {
            self.area.make_current();
        }
        release(&mut self.render.borrow_mut());
        if let Some(parent) = self
            .area
            .parent()
            .and_then(|p| p.downcast::<gtk::Container>().ok())
        {
            parent.remove(&self.area);
        }
    }
}

/// Called by libmpv on one of its threads when a new frame is ready.
unsafe extern "C" fn on_update(ctx: *mut c_void) {
    // SAFETY: ctx is the SendWeakRef box installed in realize and freed only after the
    // callback has been unset.
    let weak = unsafe { &*(ctx as *const SendWeakRef<gtk::GLArea>) }.clone();
    glib::idle_add_once(move || {
        if let Some(area) = weak.upgrade() {
            pump(&area);
        }
    });
}
