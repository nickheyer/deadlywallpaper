//! Offscreen frame capture through a handful of OpenGL entry points, shared by the render-API
//! backends.

use crate::media::mpv::GetProcAddressFn;
use std::ffi::{c_char, c_void};

pub type GetIntegerv = unsafe extern "C" fn(u32, *mut i32);
pub const GL_DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_RGBA: u32 = 0x1908;
const GL_RGBA8: u32 = 0x8058;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_LINEAR: i32 = 0x2601;
const GL_PACK_ALIGNMENT: u32 = 0x0D05;

#[derive(Clone, Copy)]
pub struct Capture {
    pub get_integerv: GetIntegerv,
    gen_framebuffers: unsafe extern "C" fn(i32, *mut u32),
    bind_framebuffer: unsafe extern "C" fn(u32, u32),
    delete_framebuffers: unsafe extern "C" fn(i32, *const u32),
    gen_textures: unsafe extern "C" fn(i32, *mut u32),
    bind_texture: unsafe extern "C" fn(u32, u32),
    delete_textures: unsafe extern "C" fn(i32, *const u32),
    tex_image_2d: unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    tex_parameteri: unsafe extern "C" fn(u32, u32, i32),
    framebuffer_texture_2d: unsafe extern "C" fn(u32, u32, u32, u32, i32),
    check_framebuffer_status: unsafe extern "C" fn(u32) -> u32,
    read_pixels: unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut c_void),
    pixel_storei: unsafe extern "C" fn(u32, i32),
    finish: unsafe extern "C" fn(),
}

impl Capture {
    /// Resolve the entry points with the same loader libmpv uses; the GL context must be
    /// current.
    pub fn load(get_proc: GetProcAddressFn, ctx: *mut c_void) -> Option<Capture> {
        macro_rules! f {
            ($name:literal) => {{
                // SAFETY: standard GL entry point cast to its standard signature.
                let p = unsafe { get_proc(ctx, concat!($name, "\0").as_ptr() as *const c_char) };
                if p.is_null() {
                    return None;
                }
                unsafe { std::mem::transmute_copy::<*mut c_void, _>(&p) }
            }};
        }
        Some(Capture {
            get_integerv: f!("glGetIntegerv"),
            gen_framebuffers: f!("glGenFramebuffers"),
            bind_framebuffer: f!("glBindFramebuffer"),
            delete_framebuffers: f!("glDeleteFramebuffers"),
            gen_textures: f!("glGenTextures"),
            bind_texture: f!("glBindTexture"),
            delete_textures: f!("glDeleteTextures"),
            tex_image_2d: f!("glTexImage2D"),
            tex_parameteri: f!("glTexParameteri"),
            framebuffer_texture_2d: f!("glFramebufferTexture2D"),
            check_framebuffer_status: f!("glCheckFramebufferStatus"),
            read_pixels: f!("glReadPixels"),
            pixel_storei: f!("glPixelStorei"),
            finish: f!("glFinish"),
        })
    }

    /// Current draw framebuffer binding.
    pub fn current_fbo(&self) -> i32 {
        let mut fbo: i32 = 0;
        // SAFETY: valid output pointer on a current context.
        unsafe { (self.get_integerv)(GL_DRAW_FRAMEBUFFER_BINDING, &mut fbo) };
        fbo
    }

    /// Render through `draw(fbo, w, h)` into a fresh texture and return its RGBA rows.
    pub fn render_offscreen(&self, w: i32, h: i32, draw: impl FnOnce(i32, i32, i32)) -> Option<Vec<u8>> {
        // SAFETY: plain GL calls on the current context with valid buffers; every object
        // created here is deleted before returning and the previous binding is restored.
        unsafe {
            let previous = self.current_fbo();
            let mut tex: u32 = 0;
            let mut fbo: u32 = 0;
            (self.gen_textures)(1, &mut tex);
            (self.bind_texture)(GL_TEXTURE_2D, tex);
            (self.tex_image_2d)(GL_TEXTURE_2D, 0, GL_RGBA8 as i32, w, h, 0, GL_RGBA, GL_UNSIGNED_BYTE, std::ptr::null());
            (self.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            (self.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
            (self.gen_framebuffers)(1, &mut fbo);
            (self.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
            (self.framebuffer_texture_2d)(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, tex, 0);
            let complete = (self.check_framebuffer_status)(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE;
            let mut pixels = None;
            if complete {
                draw(fbo as i32, w, h);
                (self.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
                (self.finish)();
                (self.pixel_storei)(GL_PACK_ALIGNMENT, 1);
                let mut buf = vec![0u8; (w * h * 4) as usize];
                (self.read_pixels)(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, buf.as_mut_ptr() as *mut c_void);
                pixels = Some(buf);
            }
            (self.bind_framebuffer)(GL_FRAMEBUFFER, previous as u32);
            (self.delete_framebuffers)(1, &fbo);
            (self.delete_textures)(1, &tex);
            pixels
        }
    }
}
