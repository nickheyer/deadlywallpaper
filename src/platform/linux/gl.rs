//! OpenGL entry point lookup for libmpv's render API, independent of the GL context flavor.

use libloading::Library;
use std::ffi::{CStr, c_char, c_void};
use std::sync::OnceLock;

type GetProc = unsafe extern "C" fn(*const c_char) -> *mut c_void;

struct Loaders {
    egl: Option<(Library, GetProc)>,
    glx: Option<(Library, GetProc)>,
    gl: Option<Library>,
}

fn loaders() -> &'static Loaders {
    static L: OnceLock<Loaders> = OnceLock::new();
    L.get_or_init(|| {
        // SAFETY: loading vendor GL libraries has no preconditions; symbols are looked up by
        // their standard names with matching signatures.
        unsafe {
            let load = |lib: &str, sym: &[u8]| -> Option<(Library, GetProc)> {
                let l = Library::new(lib).ok()?;
                let f = *l.get::<GetProc>(sym).ok()?;
                Some((l, f))
            };
            Loaders {
                egl: load("libEGL.so.1", b"eglGetProcAddress\0"),
                glx: load("libGL.so.1", b"glXGetProcAddressARB\0"),
                gl: Library::new("libGL.so.1").ok(),
            }
        }
    })
}

pub const PREFER_EGL: *mut c_void = std::ptr::without_provenance_mut(1);
pub const PREFER_GLX: *mut c_void = std::ptr::without_provenance_mut(2);

/// `mpv_opengl_init_params::get_proc_address`; `ctx` is [`PREFER_EGL`] or [`PREFER_GLX`].
pub unsafe extern "C" fn get_proc_address(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let l = loaders();
    let order: [&Option<(Library, GetProc)>; 2] = if ctx == PREFER_GLX {
        [&l.glx, &l.egl]
    } else {
        [&l.egl, &l.glx]
    };
    for (_, f) in order.into_iter().flatten() {
        // SAFETY: name is a NUL-terminated string supplied by libmpv.
        let p = unsafe { f(name) };
        if !p.is_null() {
            return p;
        }
    }
    if let Some(gl) = &l.gl {
        // SAFETY: name is NUL-terminated; dlsym lookup of a plain symbol.
        let bytes = unsafe { CStr::from_ptr(name) }.to_bytes_with_nul();
        if let Ok(sym) = unsafe { gl.get::<*mut c_void>(bytes) } {
            return *sym;
        }
    }
    std::ptr::null_mut()
}

pub use crate::media::glcap::Capture;
