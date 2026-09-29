//! Runtime-loaded libmpv client and render API bindings.

use crate::error::{Error, Result};
use libloading::{Library, Symbol};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::{Arc, OnceLock};

#[repr(C)]
pub struct mpv_handle {
    _p: [u8; 0],
}

#[cfg(not(windows))]
#[repr(C)]
pub struct mpv_render_context {
    _p: [u8; 0],
}

pub const FORMAT_STRING: c_int = 1;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_INT64: c_int = 4;
pub const FORMAT_DOUBLE: c_int = 5;

pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_LOG_MESSAGE: c_int = 2;
pub const EVENT_COMMAND_REPLY: c_int = 5;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;

#[cfg(not(windows))]
pub const RENDER_PARAM_API_TYPE: c_int = 1;
#[cfg(not(windows))]
pub const RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
#[cfg(not(windows))]
pub const RENDER_PARAM_OPENGL_FBO: c_int = 3;
#[cfg(not(windows))]
pub const RENDER_PARAM_FLIP_Y: c_int = 4;
#[cfg(target_os = "linux")]
pub const RENDER_PARAM_X11_DISPLAY: c_int = 8;
#[cfg(target_os = "linux")]
pub const RENDER_PARAM_WL_DISPLAY: c_int = 9;

#[repr(C)]
struct mpv_event {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct mpv_event_end_file {
    reason: c_int,
    error: c_int,
    playlist_entry_id: i64,
}

#[repr(C)]
struct mpv_event_log_message {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
    log_level: c_int,
}

#[cfg(not(windows))]
#[repr(C)]
pub struct mpv_render_param {
    pub type_: c_int,
    pub data: *mut c_void,
}

#[cfg(not(windows))]
pub type GetProcAddressFn =
    unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void;

#[cfg(not(windows))]
#[repr(C)]
pub struct mpv_opengl_init_params {
    pub get_proc_address: GetProcAddressFn,
    pub get_proc_address_ctx: *mut c_void,
}

#[cfg(not(windows))]
#[repr(C)]
pub struct mpv_opengl_fbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

#[cfg(not(windows))]
pub type UpdateFn = unsafe extern "C" fn(ctx: *mut c_void);

type CreateFn = unsafe extern "C" fn() -> *mut mpv_handle;
type InitializeFn = unsafe extern "C" fn(*mut mpv_handle) -> c_int;
type DestroyFn = unsafe extern "C" fn(*mut mpv_handle);
type SetOptionStringFn =
    unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int;
type CommandFn = unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char) -> c_int;
type CommandAsyncFn = unsafe extern "C" fn(*mut mpv_handle, u64, *mut *const c_char) -> c_int;
type SetPropertyFn =
    unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int;
type WaitEventFn = unsafe extern "C" fn(*mut mpv_handle, f64) -> *mut mpv_event;
type RequestLogFn = unsafe extern "C" fn(*mut mpv_handle, *const c_char) -> c_int;
type ErrorStringFn = unsafe extern "C" fn(c_int) -> *const c_char;
type VersionFn = unsafe extern "C" fn() -> std::ffi::c_ulong;
#[cfg(not(windows))]
type RenderCreateFn = unsafe extern "C" fn(
    *mut *mut mpv_render_context,
    *mut mpv_handle,
    *mut mpv_render_param,
) -> c_int;
#[cfg(not(windows))]
type RenderSetUpdateFn =
    unsafe extern "C" fn(*mut mpv_render_context, Option<UpdateFn>, *mut c_void);
#[cfg(not(windows))]
type RenderRenderFn = unsafe extern "C" fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int;
#[cfg(not(windows))]
type RenderFreeFn = unsafe extern "C" fn(*mut mpv_render_context);
#[cfg(not(windows))]
type RenderUpdateFn = unsafe extern "C" fn(*mut mpv_render_context) -> u64;

#[cfg(not(windows))]
pub const RENDER_UPDATE_FRAME: u64 = 1;

/// The loaded library and its entry points.
pub struct Lib {
    create: CreateFn,
    initialize: InitializeFn,
    terminate_destroy: DestroyFn,
    set_option_string: SetOptionStringFn,
    command: CommandFn,
    command_async: CommandAsyncFn,
    set_property: SetPropertyFn,
    wait_event: WaitEventFn,
    request_log_messages: RequestLogFn,
    error_string: ErrorStringFn,
    #[cfg(not(windows))]
    render_context_create: RenderCreateFn,
    #[cfg(not(windows))]
    render_context_set_update_callback: RenderSetUpdateFn,
    #[cfg(not(windows))]
    render_context_render: RenderRenderFn,
    #[cfg(not(windows))]
    render_context_free: RenderFreeFn,
    #[cfg(not(windows))]
    render_context_update: RenderUpdateFn,
    pub version: u64,
    _lib: Library,
}

static LIB: OnceLock<std::result::Result<Arc<Lib>, String>> = OnceLock::new();

fn candidates() -> Vec<std::path::PathBuf> {
    let mut v: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for n in NAMES {
                v.push(dir.join(n));
            }
        }
    }
    v.extend(NAMES.iter().map(std::path::PathBuf::from));
    #[cfg(target_os = "macos")]
    for prefix in ["/opt/homebrew/lib", "/usr/local/lib", "/opt/local/lib"] {
        v.push(std::path::Path::new(prefix).join("libmpv.dylib"));
        v.push(std::path::Path::new(prefix).join("libmpv.2.dylib"));
    }
    v
}

#[cfg(target_os = "linux")]
const NAMES: &[&str] = &["libmpv.so.2", "libmpv.so"];
#[cfg(target_os = "macos")]
const NAMES: &[&str] = &["libmpv.2.dylib", "libmpv.dylib"];
#[cfg(windows)]
const NAMES: &[&str] = &["libmpv-2.dll", "mpv-2.dll", "mpv-1.dll"];

#[cfg(target_os = "linux")]
const INSTALL_HINT: &str = "install the mpv package (provides libmpv.so.2)";
#[cfg(target_os = "macos")]
const INSTALL_HINT: &str = "install mpv with Homebrew: brew install mpv";
#[cfg(windows)]
const INSTALL_HINT: &str = "place libmpv-2.dll from the mpv-dev package next to deadlywp.exe";

/// Load libmpv once per process.
pub fn lib() -> Result<Arc<Lib>> {
    LIB.get_or_init(|| {
        let mut last = String::from("no candidate paths");
        for path in candidates() {
            // SAFETY: loading libmpv runs its constructors, which have no preconditions.
            match unsafe { Library::new(&path) } {
                Ok(l) => match Lib::bind(l) {
                    Ok(lib) => {
                        log::info!(
                            "libmpv client API {}.{} from {}",
                            lib.version >> 16,
                            lib.version & 0xffff,
                            path.display()
                        );
                        return Ok(Arc::new(lib));
                    }
                    Err(e) => last = format!("{}: {e}", path.display()),
                },
                Err(e) => last = format!("{}: {e}", path.display()),
            }
        }
        Err(format!("libmpv not found ({last}); {INSTALL_HINT}"))
    })
    .clone()
    .map_err(Error::Media)
}

impl Lib {
    fn bind(lib: Library) -> std::result::Result<Lib, String> {
        // SAFETY: every symbol is looked up by its documented name and cast to the
        // signature published in libmpv's headers (client API 2.x).
        unsafe {
            fn sym<T: Copy>(lib: &Library, name: &[u8]) -> std::result::Result<T, String> {
                let s: Symbol<T> = unsafe { lib.get(name) }.map_err(|e| e.to_string())?;
                Ok(*s)
            }
            let version: VersionFn = sym(&lib, b"mpv_client_api_version\0")?;
            let v = version() as u64;
            if v >> 16 < 2 {
                return Err(format!(
                    "libmpv client API {}.{} is too old (need 2.0)",
                    v >> 16,
                    v & 0xffff
                ));
            }
            Ok(Lib {
                create: sym(&lib, b"mpv_create\0")?,
                initialize: sym(&lib, b"mpv_initialize\0")?,
                terminate_destroy: sym(&lib, b"mpv_terminate_destroy\0")?,
                set_option_string: sym(&lib, b"mpv_set_option_string\0")?,
                command: sym(&lib, b"mpv_command\0")?,
                command_async: sym(&lib, b"mpv_command_async\0")?,
                set_property: sym(&lib, b"mpv_set_property\0")?,
                wait_event: sym(&lib, b"mpv_wait_event\0")?,
                request_log_messages: sym(&lib, b"mpv_request_log_messages\0")?,
                error_string: sym(&lib, b"mpv_error_string\0")?,
                #[cfg(not(windows))]
                render_context_create: sym(&lib, b"mpv_render_context_create\0")?,
                #[cfg(not(windows))]
                render_context_set_update_callback: sym(
                    &lib,
                    b"mpv_render_context_set_update_callback\0",
                )?,
                #[cfg(not(windows))]
                render_context_render: sym(&lib, b"mpv_render_context_render\0")?,
                #[cfg(not(windows))]
                render_context_free: sym(&lib, b"mpv_render_context_free\0")?,
                #[cfg(not(windows))]
                render_context_update: sym(&lib, b"mpv_render_context_update\0")?,
                version: v,
                _lib: lib,
            })
        }
    }

    fn err(&self, code: c_int, what: &str) -> Error {
        // SAFETY: mpv_error_string returns a static string for any code.
        let s = unsafe { CStr::from_ptr((self.error_string)(code)) };
        Error::Media(format!("{what}: {}", s.to_string_lossy()))
    }
}

/// Events surfaced from `mpv_wait_event`.
#[derive(Debug)]
pub enum Event {
    None,
    Shutdown,
    Log {
        prefix: String,
        level: String,
        text: String,
    },
    FileLoaded,
    EndFile {
        reason: c_int,
        error: c_int,
    },
    CommandReply {
        id: u64,
        error: c_int,
    },
    Other,
}

/// An mpv core. Thread-safe; commands may be issued from any thread.
pub struct Handle {
    lib: Arc<Lib>,
    ptr: *mut mpv_handle,
}

// SAFETY: libmpv documents its handle as safe to use from multiple threads.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

fn cstr(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

impl Handle {
    pub fn new(lib: Arc<Lib>) -> Result<Handle> {
        // libmpv refuses to start unless numeric formatting is the C locale; toolkits
        // (GTK) switch it to the user's locale during init.
        #[cfg(unix)]
        // SAFETY: setlocale with a valid static string.
        unsafe {
            libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
        }
        // SAFETY: mpv_create has no preconditions.
        let ptr = unsafe { (lib.create)() };
        if ptr.is_null() {
            return Err(Error::Media("mpv_create failed".into()));
        }
        Ok(Handle { lib, ptr })
    }

    pub fn set_option(&self, name: &str, value: &str) -> Result<()> {
        let (n, v) = (cstr(name), cstr(value));
        // SAFETY: valid handle and NUL-terminated strings.
        let r = unsafe { (self.lib.set_option_string)(self.ptr, n.as_ptr(), v.as_ptr()) };
        if r < 0 {
            Err(self.lib.err(r, &format!("option {name}={value}")))
        } else {
            Ok(())
        }
    }

    pub fn initialize(&self) -> Result<()> {
        // SAFETY: valid handle.
        let r = unsafe { (self.lib.initialize)(self.ptr) };
        if r < 0 {
            Err(self.lib.err(r, "mpv_initialize"))
        } else {
            Ok(())
        }
    }

    pub fn request_log(&self, level: &str) {
        let l = cstr(level);
        // SAFETY: valid handle and string.
        unsafe { (self.lib.request_log_messages)(self.ptr, l.as_ptr()) };
    }

    fn argv(args: &[&str]) -> (Vec<CString>, Vec<*const c_char>) {
        let owned: Vec<CString> = args.iter().map(|a| cstr(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        (owned, ptrs)
    }

    pub fn command(&self, args: &[&str]) -> Result<()> {
        let (_owned, mut ptrs) = Handle::argv(args);
        // SAFETY: NULL-terminated argv of valid C strings.
        let r = unsafe { (self.lib.command)(self.ptr, ptrs.as_mut_ptr()) };
        if r < 0 {
            Err(self.lib.err(r, &args.join(" ")))
        } else {
            Ok(())
        }
    }

    pub fn command_async(&self, id: u64, args: &[&str]) -> Result<()> {
        let (_owned, mut ptrs) = Handle::argv(args);
        // SAFETY: as in `command`.
        let r = unsafe { (self.lib.command_async)(self.ptr, id, ptrs.as_mut_ptr()) };
        if r < 0 {
            Err(self.lib.err(r, &args.join(" ")))
        } else {
            Ok(())
        }
    }

    pub fn set_str(&self, name: &str, value: &str) -> Result<()> {
        let n = cstr(name);
        let v = cstr(value);
        let mut p = v.as_ptr();
        // SAFETY: MPV_FORMAT_STRING expects a pointer to a `char*`.
        let r = unsafe {
            (self.lib.set_property)(
                self.ptr,
                n.as_ptr(),
                FORMAT_STRING,
                &mut p as *mut _ as *mut c_void,
            )
        };
        if r < 0 {
            Err(self.lib.err(r, &format!("set {name}")))
        } else {
            Ok(())
        }
    }

    pub fn set_f64(&self, name: &str, mut value: f64) -> Result<()> {
        let n = cstr(name);
        // SAFETY: MPV_FORMAT_DOUBLE expects a pointer to a double.
        let r = unsafe {
            (self.lib.set_property)(
                self.ptr,
                n.as_ptr(),
                FORMAT_DOUBLE,
                &mut value as *mut f64 as *mut c_void,
            )
        };
        if r < 0 {
            Err(self.lib.err(r, &format!("set {name}")))
        } else {
            Ok(())
        }
    }

    pub fn set_i64(&self, name: &str, mut value: i64) -> Result<()> {
        let n = cstr(name);
        // SAFETY: MPV_FORMAT_INT64 expects a pointer to an int64_t.
        let r = unsafe {
            (self.lib.set_property)(
                self.ptr,
                n.as_ptr(),
                FORMAT_INT64,
                &mut value as *mut i64 as *mut c_void,
            )
        };
        if r < 0 {
            Err(self.lib.err(r, &format!("set {name}")))
        } else {
            Ok(())
        }
    }

    pub fn set_flag(&self, name: &str, value: bool) -> Result<()> {
        let n = cstr(name);
        let mut v: c_int = value as c_int;
        // SAFETY: MPV_FORMAT_FLAG expects a pointer to an int.
        let r = unsafe {
            (self.lib.set_property)(
                self.ptr,
                n.as_ptr(),
                FORMAT_FLAG,
                &mut v as *mut c_int as *mut c_void,
            )
        };
        if r < 0 {
            Err(self.lib.err(r, &format!("set {name}")))
        } else {
            Ok(())
        }
    }

    /// Block up to `timeout` seconds for the next event.
    pub fn wait_event(&self, timeout: f64) -> Event {
        // SAFETY: the returned event is valid until the next wait_event call on this handle,
        // and we copy everything out before returning.
        unsafe {
            let ev = (self.lib.wait_event)(self.ptr, timeout);
            if ev.is_null() {
                return Event::None;
            }
            let ev = &*ev;
            match ev.event_id {
                0 => Event::None,
                EVENT_SHUTDOWN => Event::Shutdown,
                EVENT_FILE_LOADED => Event::FileLoaded,
                EVENT_END_FILE => {
                    let d = &*(ev.data as *const mpv_event_end_file);
                    Event::EndFile {
                        reason: d.reason,
                        error: d.error,
                    }
                }
                EVENT_COMMAND_REPLY => Event::CommandReply {
                    id: ev.reply_userdata,
                    error: ev.error,
                },
                EVENT_LOG_MESSAGE => {
                    let d = &*(ev.data as *const mpv_event_log_message);
                    let s = |p: *const c_char| {
                        if p.is_null() {
                            String::new()
                        } else {
                            CStr::from_ptr(p).to_string_lossy().trim_end().to_string()
                        }
                    };
                    Event::Log {
                        prefix: s(d.prefix),
                        level: s(d.level),
                        text: s(d.text),
                    }
                }
                _ => Event::Other,
            }
        }
    }

    pub fn error_string(&self, code: c_int) -> String {
        // SAFETY: static string.
        unsafe {
            CStr::from_ptr((self.lib.error_string)(code))
                .to_string_lossy()
                .into_owned()
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: valid handle; every render context bound to it was freed first
        // (RenderContext is always dropped before its Handle by construction).
        unsafe { (self.lib.terminate_destroy)(self.ptr) };
    }
}

/// OpenGL render context bound to one [`Handle`]. Must be created, used and dropped with the
/// target GL context current on the calling thread.
#[cfg(not(windows))]
pub struct RenderContext {
    lib: Arc<Lib>,
    ptr: *mut mpv_render_context,
}

#[cfg(not(windows))]
impl RenderContext {
    /// `extra` carries display pointers for hardware decoding interop
    /// (`RENDER_PARAM_X11_DISPLAY` / `RENDER_PARAM_WL_DISPLAY`).
    pub fn new(
        handle: &Handle,
        get_proc_address: GetProcAddressFn,
        gpa_ctx: *mut c_void,
        extra: &[(c_int, *mut c_void)],
    ) -> Result<RenderContext> {
        let api = c"opengl";
        let mut init = mpv_opengl_init_params {
            get_proc_address,
            get_proc_address_ctx: gpa_ctx,
        };
        let mut params = vec![
            mpv_render_param {
                type_: RENDER_PARAM_API_TYPE,
                data: api.as_ptr() as *mut c_void,
            },
            mpv_render_param {
                type_: RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init as *mut _ as *mut c_void,
            },
        ];
        for (t, d) in extra {
            if !d.is_null() {
                params.push(mpv_render_param {
                    type_: *t,
                    data: *d,
                });
            }
        }
        params.push(mpv_render_param {
            type_: 0,
            data: std::ptr::null_mut(),
        });
        let mut ptr: *mut mpv_render_context = std::ptr::null_mut();
        // SAFETY: params is NUL-terminated and every pointer outlives the call.
        let r = unsafe {
            (handle.lib.render_context_create)(&mut ptr, handle.ptr, params.as_mut_ptr())
        };
        if r < 0 || ptr.is_null() {
            return Err(handle.lib.err(r, "mpv_render_context_create"));
        }
        Ok(RenderContext {
            lib: handle.lib.clone(),
            ptr,
        })
    }

    /// `cb` runs on an arbitrary mpv thread whenever a new frame should be drawn.
    pub fn set_update_callback(&self, cb: Option<UpdateFn>, ctx: *mut c_void) {
        // SAFETY: valid context; ctx lifetime is the caller's responsibility.
        unsafe { (self.lib.render_context_set_update_callback)(self.ptr, cb, ctx) };
    }

    /// Process pending work on the render thread; the result carries
    /// [`RENDER_UPDATE_FRAME`] when a new frame is ready.
    pub fn update(&self) -> u64 {
        // SAFETY: valid context; called on the render thread.
        unsafe { (self.lib.render_context_update)(self.ptr) }
    }

    /// Draw the current frame into `fbo` of the given size.
    pub fn render(&self, fbo: c_int, w: c_int, h: c_int, flip_y: bool) {
        let mut target = mpv_opengl_fbo {
            fbo,
            w,
            h,
            internal_format: 0,
        };
        let mut flip: c_int = flip_y as c_int;
        let mut params = [
            mpv_render_param {
                type_: RENDER_PARAM_OPENGL_FBO,
                data: &mut target as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: RENDER_PARAM_FLIP_Y,
                data: &mut flip as *mut c_int as *mut c_void,
            },
            mpv_render_param {
                type_: 0,
                data: std::ptr::null_mut(),
            },
        ];
        // SAFETY: GL context is current per the type's contract; params outlive the call.
        unsafe { (self.lib.render_context_render)(self.ptr, params.as_mut_ptr()) };
    }
}

#[cfg(not(windows))]
impl Drop for RenderContext {
    fn drop(&mut self) {
        // SAFETY: valid context, GL context current per contract.
        unsafe { (self.lib.render_context_free)(self.ptr) };
    }
}
