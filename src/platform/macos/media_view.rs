//! libmpv rendering through the render API on an NSOpenGL context attached to the slot view.
#![allow(deprecated)]

use crate::content::{Content, View};
use crate::error::{Error, Result};
use crate::geom::Size;
use crate::media::glcap::Capture;
use crate::media::glquad::{Quad, render_view};
use crate::media::mpv::{GetProcAddressFn, Handle, RENDER_UPDATE_FRAME, RenderContext};
use crate::media::player::{MediaContent, MediaSurface, Player, PlayerOptions, Vo, event_bridge};
use crate::platform::ContentSpec;
use crate::platform::macos::{MsgSender, Slot};
use libloading::Library;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSOpenGLContext, NSOpenGLPixelFormat};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

const NS_OPENGL_PFA_DOUBLE_BUFFER: u32 = 5;
const NS_OPENGL_PFA_COLOR_SIZE: u32 = 8;
const NS_OPENGL_PFA_ACCELERATED: u32 = 73;
const NS_OPENGL_PFA_ALLOW_OFFLINE_RENDERERS: u32 = 96;
const NS_OPENGL_PFA_OPENGL_PROFILE: u32 = 99;
const NS_OPENGL_PROFILE_VERSION_3_2_CORE: u32 = 0x3200;

fn opengl() -> Option<&'static Library> {
    static LIB: OnceLock<Option<Library>> = OnceLock::new();
    // SAFETY: loading the system OpenGL framework has no preconditions.
    LIB.get_or_init(|| unsafe { Library::new("/System/Library/Frameworks/OpenGL.framework/OpenGL").ok() }).as_ref()
}

unsafe extern "C" fn get_proc_address(_: *mut c_void, name: *const c_char) -> *mut c_void {
    let Some(lib) = opengl() else { return std::ptr::null_mut() };
    // SAFETY: name is NUL-terminated per libmpv's contract.
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes_with_nul();
    // SAFETY: plain symbol lookup.
    match unsafe { lib.get::<*mut c_void>(bytes) } {
        Ok(sym) => *sym,
        Err(_) => std::ptr::null_mut(),
    }
}

const GPA: GetProcAddressFn = get_proc_address;

pub fn spawn(spec: &ContentSpec<'_>, slot: &Slot, tx: MsgSender, mtm: MainThreadMarker) -> Result<Box<dyn Content>> {
    let wp = spec.wallpaper;
    if !wp.kind().is_online() && !std::path::Path::new(&wp.source).is_file() {
        return Err(Error::NotFound(format!("{} does not exist", wp.source)));
    }
    let (events, pending) = event_bridge(spec.id, tx.clone());
    let player = Player::new(
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
        events,
    )?;
    let view = MediaView::new(player.handle().clone(), slot, mtm)?;
    player.load()?;
    Ok(Box::new(MediaContent::new(Box::new(view), player, pending, spec.id, tx)))
}

/// Wraps the context pointer so it can cross to the render thread; NSOpenGLContext may be
/// made current on any single thread at a time.
struct GlContext(Retained<NSOpenGLContext>);
// SAFETY: the context is only used from the render thread after creation, and dropped there.
unsafe impl Send for GlContext {}

enum Job {
    Frame,
    View(View),
    Capture(std::path::PathBuf, Sender<Result<()>>),
    Stop,
}

pub struct MediaView {
    jobs: SyncSender<Job>,
    thread: Option<JoinHandle<()>>,
    alive: Arc<AtomicBool>,
    /// The render thread built the quad renderer, so views other than the whole image work.
    quad_ready: Arc<AtomicBool>,
    slot: Size,
    handle_ptr: *mut c_void,
}

impl MediaView {
    pub fn new(handle: Arc<Handle>, slot: &Slot, mtm: MainThreadMarker) -> Result<MediaView> {
        let attrs: [u32; 9] = [
            NS_OPENGL_PFA_OPENGL_PROFILE,
            NS_OPENGL_PROFILE_VERSION_3_2_CORE,
            NS_OPENGL_PFA_DOUBLE_BUFFER,
            NS_OPENGL_PFA_COLOR_SIZE,
            24,
            NS_OPENGL_PFA_ACCELERATED,
            NS_OPENGL_PFA_ALLOW_OFFLINE_RENDERERS,
            0,
            0,
        ];
        // SAFETY: attribute list is zero-terminated; view attachment happens on the main thread.
        let context = unsafe {
            let format = NSOpenGLPixelFormat::initWithAttributes(mtm.alloc::<NSOpenGLPixelFormat>(), NonNull::from(&attrs[0]))
                .ok_or_else(|| Error::Media("no OpenGL pixel format available".into()))?;
            let ctx = NSOpenGLContext::initWithFormat_shareContext(mtm.alloc::<NSOpenGLContext>(), &format, None)
                .ok_or_else(|| Error::Media("OpenGL context creation failed".into()))?;
            ctx.setView(Some(&slot.view), mtm);
            ctx
        };
        let scale = slot.view.window().map(|w| w.backingScaleFactor()).unwrap_or(1.0);
        let (w, h) = ((slot.size.w as f64 * scale) as i32, (slot.size.h as f64 * scale) as i32);
        let (jobs, rx) = sync_channel::<Job>(4);
        let alive = Arc::new(AtomicBool::new(true));
        let quad_ready = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = channel::<Result<()>>();
        let wake: Arc<Mutex<Option<SyncSender<Job>>>> = Arc::new(Mutex::new(Some(jobs.clone())));
        let handle_ptr = Box::into_raw(Box::new(wake.clone())) as *mut c_void;
        let gl = GlContext(context);
        let thread_alive = alive.clone();
        let thread_quad = quad_ready.clone();
        let wake_addr = handle_ptr as usize;
        let target = Target { w, h, slot: slot.size, dpi: scale };
        let thread = std::thread::Builder::new()
            .name("mpv-render".into())
            .spawn(move || render_thread(gl, handle, rx, ready_tx, target, wake_addr as *mut c_void, thread_alive, thread_quad))
            .map_err(|e| Error::Media(e.to_string()))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(MediaView { jobs, thread: Some(thread), alive, quad_ready, slot: slot.size, handle_ptr }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(Error::Media("render thread failed to start".into())),
        }
    }
}

/// The on-screen framebuffer: device pixels, logical slot size, and the ratio between them.
#[derive(Clone, Copy)]
struct Target {
    w: i32,
    h: i32,
    slot: Size,
    dpi: f64,
}

/// libmpv update callback: nudge the render thread.
unsafe extern "C" fn on_update(ctx: *mut c_void) {
    // SAFETY: ctx is the boxed wake sender created in `MediaView::new`, valid until the
    // callback is unset in the render thread's shutdown.
    let wake = unsafe { &*(ctx as *const Arc<Mutex<Option<SyncSender<Job>>>>) };
    if let Some(tx) = wake.lock().ok().and_then(|g| g.clone()) {
        let _ = tx.try_send(Job::Frame);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_thread(gl: GlContext, handle: Arc<Handle>, rx: Receiver<Job>, ready: Sender<Result<()>>, target: Target, wake_ptr: *mut c_void, alive: Arc<AtomicBool>, quad_ready: Arc<AtomicBool>) {
    let ctx = gl.0;
    ctx.makeCurrentContext();
    let render = match RenderContext::new(&handle, GPA, std::ptr::null_mut(), &[]) {
        Ok(r) => r,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let capture = Capture::load(GPA, std::ptr::null_mut());
    let mut quad = Quad::load(GPA, std::ptr::null_mut());
    quad_ready.store(quad.is_some(), Ordering::Relaxed);
    let mut view = View::whole(target.slot);
    let Target { w, h, slot, dpi } = target;
    render.set_update_callback(Some(on_update), wake_ptr);
    let _ = ready.send(Ok(()));
    while alive.load(Ordering::Relaxed) {
        match rx.recv() {
            Ok(Job::Frame) => {
                if render.update() & RENDER_UPDATE_FRAME != 0 {
                    render_view(&render, &mut quad, &view, slot, 0, w, h, dpi, true);
                    ctx.flushBuffer();
                }
            }
            Ok(Job::View(v)) => {
                view = v;
                render.update();
                render_view(&render, &mut quad, &view, slot, 0, w, h, dpi, true);
                ctx.flushBuffer();
            }
            Ok(Job::Capture(path, reply)) => {
                let result = match capture {
                    Some(gl) => match gl.render_offscreen(w, h, |fbo, w, h| render_view(&render, &mut quad, &view, slot, fbo, w, h, dpi, false)).and_then(|p| crate::capture::from_gl_pixels(w as u32, h as u32, &p)) {
                        Some(img) => crate::capture::save_rgba(img, &path),
                        None => Err(Error::Media("offscreen frame capture failed".into())),
                    },
                    None => Err(Error::Media("OpenGL capture entry points unavailable".into())),
                };
                let _ = reply.send(result);
            }
            Ok(Job::Stop) | Err(_) => break,
        }
    }
    if let Some(mut q) = quad.take() {
        q.destroy();
    }
    render.set_update_callback(None, std::ptr::null_mut());
    // SAFETY: the callback is unset, so the boxed wake sender has no more readers.
    drop(unsafe { Box::from_raw(wake_ptr as *mut Arc<Mutex<Option<SyncSender<Job>>>>) });
    drop(render);
    NSOpenGLContext::clearCurrentContext();
}

impl MediaSurface for MediaView {
    fn capture(&self, path: &std::path::Path) -> Option<Result<()>> {
        let (tx, rx) = channel();
        if self.jobs.send(Job::Capture(path.to_path_buf(), tx)).is_err() {
            return Some(Err(Error::Media("render thread is gone".into())));
        }
        Some(rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or_else(|_| Err(Error::Media("frame capture timed out".into()))))
    }

    fn set_view(&self, view: &View, _slot: Size) -> Option<Result<()>> {
        if !view.is_whole(self.slot) && !self.quad_ready.load(Ordering::Relaxed) {
            return Some(Err(Error::Media("the OpenGL view renderer is unavailable, so the wallpaper cannot be moved, scaled or turned".into())));
        }
        Some(self.jobs.send(Job::View(*view)).map_err(|_| Error::Media("render thread is gone".into())))
    }
}

impl Drop for MediaView {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        // SAFETY: the wake box is freed by the render thread; here we only stop new wakes.
        if let Some(w) = unsafe { (self.handle_ptr as *const Arc<Mutex<Option<SyncSender<Job>>>>).as_ref() } {
            if let Ok(mut g) = w.lock() {
                g.take();
            }
        }
        let _ = self.jobs.send(Job::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
