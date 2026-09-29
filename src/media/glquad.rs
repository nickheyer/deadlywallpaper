//! Render libmpv into a texture, then apply View to the slot. Requires a current GL context.

use crate::content::View;
use crate::geom::Size;
use crate::media::mpv::{GetProcAddressFn, RenderContext};
use std::ffi::{CStr, c_char, c_void};

const GL_VERSION: u32 = 0x1F02;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_TEXTURE0: u32 = 0x84C0;
const GL_RGBA: u32 = 0x1908;
const GL_RGBA8: u32 = 0x8058;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_FLOAT: u32 = 0x1406;
const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_TEXTURE_WRAP_S: u32 = 0x2802;
const GL_TEXTURE_WRAP_T: u32 = 0x2803;
const GL_LINEAR: i32 = 0x2601;
const GL_CLAMP_TO_EDGE: i32 = 0x812F;
const GL_VERTEX_SHADER: u32 = 0x8B31;
const GL_FRAGMENT_SHADER: u32 = 0x8B30;
const GL_COMPILE_STATUS: u32 = 0x8B81;
const GL_LINK_STATUS: u32 = 0x8B82;
const GL_ARRAY_BUFFER: u32 = 0x8892;
const GL_STATIC_DRAW: u32 = 0x88E4;
const GL_TRIANGLE_STRIP: u32 = 0x0005;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
const GL_BLEND: u32 = 0x0BE2;
const GL_SCISSOR_TEST: u32 = 0x0C11;
const GL_DEPTH_TEST: u32 = 0x0B71;
const GL_CULL_FACE: u32 = 0x0B44;

/// Unit square as a strip; texture coordinates follow the position (v flipped in the shader).
const VERTICES: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];

const VERTEX_GL: &str = "#version 150\nin vec2 pos;\nout vec2 uv;\nuniform mat3 m;\nvoid main() {\n  uv = vec2(pos.x, 1.0 - pos.y);\n  vec3 p = m * vec3(pos, 1.0);\n  gl_Position = vec4(p.xy, 0.0, 1.0);\n}\n";
const FRAGMENT_GL: &str = "#version 150\nin vec2 uv;\nout vec4 color;\nuniform sampler2D tex;\nvoid main() {\n  color = texture(tex, uv);\n}\n";
const VERTEX_ES: &str = "#version 300 es\nprecision highp float;\nin vec2 pos;\nout vec2 uv;\nuniform mat3 m;\nvoid main() {\n  uv = vec2(pos.x, 1.0 - pos.y);\n  vec3 p = m * vec3(pos, 1.0);\n  gl_Position = vec4(p.xy, 0.0, 1.0);\n}\n";
const FRAGMENT_ES: &str = "#version 300 es\nprecision mediump float;\nin vec2 uv;\nout vec4 color;\nuniform sampler2D tex;\nvoid main() {\n  color = texture(tex, uv);\n}\n";

pub struct Quad {
    get_string: unsafe extern "C" fn(u32) -> *const u8,
    gen_textures: unsafe extern "C" fn(i32, *mut u32),
    bind_texture: unsafe extern "C" fn(u32, u32),
    delete_textures: unsafe extern "C" fn(i32, *const u32),
    tex_image_2d: unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    tex_parameteri: unsafe extern "C" fn(u32, u32, i32),
    active_texture: unsafe extern "C" fn(u32),
    gen_framebuffers: unsafe extern "C" fn(i32, *mut u32),
    bind_framebuffer: unsafe extern "C" fn(u32, u32),
    delete_framebuffers: unsafe extern "C" fn(i32, *const u32),
    framebuffer_texture_2d: unsafe extern "C" fn(u32, u32, u32, u32, i32),
    check_framebuffer_status: unsafe extern "C" fn(u32) -> u32,
    create_shader: unsafe extern "C" fn(u32) -> u32,
    shader_source: unsafe extern "C" fn(u32, i32, *const *const c_char, *const i32),
    compile_shader: unsafe extern "C" fn(u32),
    get_shaderiv: unsafe extern "C" fn(u32, u32, *mut i32),
    delete_shader: unsafe extern "C" fn(u32),
    create_program: unsafe extern "C" fn() -> u32,
    attach_shader: unsafe extern "C" fn(u32, u32),
    link_program: unsafe extern "C" fn(u32),
    get_programiv: unsafe extern "C" fn(u32, u32, *mut i32),
    use_program: unsafe extern "C" fn(u32),
    delete_program: unsafe extern "C" fn(u32),
    get_uniform_location: unsafe extern "C" fn(u32, *const c_char) -> i32,
    get_attrib_location: unsafe extern "C" fn(u32, *const c_char) -> i32,
    uniform_matrix3fv: unsafe extern "C" fn(i32, i32, u8, *const f32),
    uniform1i: unsafe extern "C" fn(i32, i32),
    gen_vertex_arrays: unsafe extern "C" fn(i32, *mut u32),
    bind_vertex_array: unsafe extern "C" fn(u32),
    delete_vertex_arrays: unsafe extern "C" fn(i32, *const u32),
    gen_buffers: unsafe extern "C" fn(i32, *mut u32),
    bind_buffer: unsafe extern "C" fn(u32, u32),
    buffer_data: unsafe extern "C" fn(u32, isize, *const c_void, u32),
    delete_buffers: unsafe extern "C" fn(i32, *const u32),
    enable_vertex_attrib_array: unsafe extern "C" fn(u32),
    vertex_attrib_pointer: unsafe extern "C" fn(u32, i32, u32, u8, i32, *const c_void),
    draw_arrays: unsafe extern "C" fn(u32, i32, i32),
    viewport: unsafe extern "C" fn(i32, i32, i32, i32),
    clear_color: unsafe extern "C" fn(f32, f32, f32, f32),
    clear: unsafe extern "C" fn(u32),
    disable: unsafe extern "C" fn(u32),
    get_integerv: unsafe extern "C" fn(u32, *mut i32),
    program: u32,
    vao: u32,
    vbo: u32,
    u_matrix: i32,
    texture: u32,
    fbo: u32,
    size: (i32, i32),
}

impl Quad {
    /// Resolve the entry points with the same loader libmpv uses and build the program.
    pub fn load(get_proc: GetProcAddressFn, ctx: *mut c_void) -> Option<Quad> {
        macro_rules! f {
            ($name:literal) => {{
                // SAFETY: standard GL entry point cast to its standard signature.
                let p = unsafe { get_proc(ctx, concat!($name, "\0").as_ptr() as *const c_char) };
                if p.is_null() {
                    log::error!("OpenGL entry point {} is missing", $name);
                    return None;
                }
                unsafe { std::mem::transmute_copy::<*mut c_void, _>(&p) }
            }};
        }
        let mut q = Quad {
            get_string: f!("glGetString"),
            gen_textures: f!("glGenTextures"),
            bind_texture: f!("glBindTexture"),
            delete_textures: f!("glDeleteTextures"),
            tex_image_2d: f!("glTexImage2D"),
            tex_parameteri: f!("glTexParameteri"),
            active_texture: f!("glActiveTexture"),
            gen_framebuffers: f!("glGenFramebuffers"),
            bind_framebuffer: f!("glBindFramebuffer"),
            delete_framebuffers: f!("glDeleteFramebuffers"),
            framebuffer_texture_2d: f!("glFramebufferTexture2D"),
            check_framebuffer_status: f!("glCheckFramebufferStatus"),
            create_shader: f!("glCreateShader"),
            shader_source: f!("glShaderSource"),
            compile_shader: f!("glCompileShader"),
            get_shaderiv: f!("glGetShaderiv"),
            delete_shader: f!("glDeleteShader"),
            create_program: f!("glCreateProgram"),
            attach_shader: f!("glAttachShader"),
            link_program: f!("glLinkProgram"),
            get_programiv: f!("glGetProgramiv"),
            use_program: f!("glUseProgram"),
            delete_program: f!("glDeleteProgram"),
            get_uniform_location: f!("glGetUniformLocation"),
            get_attrib_location: f!("glGetAttribLocation"),
            uniform_matrix3fv: f!("glUniformMatrix3fv"),
            uniform1i: f!("glUniform1i"),
            gen_vertex_arrays: f!("glGenVertexArrays"),
            bind_vertex_array: f!("glBindVertexArray"),
            delete_vertex_arrays: f!("glDeleteVertexArrays"),
            gen_buffers: f!("glGenBuffers"),
            bind_buffer: f!("glBindBuffer"),
            buffer_data: f!("glBufferData"),
            delete_buffers: f!("glDeleteBuffers"),
            enable_vertex_attrib_array: f!("glEnableVertexAttribArray"),
            vertex_attrib_pointer: f!("glVertexAttribPointer"),
            draw_arrays: f!("glDrawArrays"),
            viewport: f!("glViewport"),
            clear_color: f!("glClearColor"),
            clear: f!("glClear"),
            disable: f!("glDisable"),
            get_integerv: f!("glGetIntegerv"),
            program: 0,
            vao: 0,
            vbo: 0,
            u_matrix: -1,
            texture: 0,
            fbo: 0,
            size: (0, 0),
        };
        if q.build() { Some(q) } else { None }
    }

    fn build(&mut self) -> bool {
        // SAFETY: plain GL object creation on the current context; failures are checked and
        // every object is released on the error path.
        unsafe {
            let version = (self.get_string)(GL_VERSION);
            let es = !version.is_null()
                && CStr::from_ptr(version as *const c_char)
                    .to_string_lossy()
                    .starts_with("OpenGL ES");
            let (vs_src, fs_src) = if es {
                (VERTEX_ES, FRAGMENT_ES)
            } else {
                (VERTEX_GL, FRAGMENT_GL)
            };
            let Some(vs) = self.shader(GL_VERTEX_SHADER, vs_src) else {
                return false;
            };
            let Some(fs) = self.shader(GL_FRAGMENT_SHADER, fs_src) else {
                (self.delete_shader)(vs);
                return false;
            };
            let program = (self.create_program)();
            (self.attach_shader)(program, vs);
            (self.attach_shader)(program, fs);
            (self.link_program)(program);
            (self.delete_shader)(vs);
            (self.delete_shader)(fs);
            let mut ok = 0;
            (self.get_programiv)(program, GL_LINK_STATUS, &mut ok);
            if ok == 0 {
                log::error!("view shader program failed to link");
                (self.delete_program)(program);
                return false;
            }
            self.program = program;
            self.u_matrix = (self.get_uniform_location)(program, c"m".as_ptr());
            let u_tex = (self.get_uniform_location)(program, c"tex".as_ptr());
            let a_pos = (self.get_attrib_location)(program, c"pos".as_ptr());
            (self.gen_vertex_arrays)(1, &mut self.vao);
            (self.bind_vertex_array)(self.vao);
            (self.gen_buffers)(1, &mut self.vbo);
            (self.bind_buffer)(GL_ARRAY_BUFFER, self.vbo);
            (self.buffer_data)(
                GL_ARRAY_BUFFER,
                std::mem::size_of_val(&VERTICES) as isize,
                VERTICES.as_ptr() as *const c_void,
                GL_STATIC_DRAW,
            );
            (self.enable_vertex_attrib_array)(a_pos as u32);
            (self.vertex_attrib_pointer)(a_pos as u32, 2, GL_FLOAT, 0, 0, std::ptr::null());
            (self.bind_vertex_array)(0);
            (self.use_program)(program);
            (self.uniform1i)(u_tex, 0);
            (self.use_program)(0);
        }
        true
    }

    unsafe fn shader(&self, kind: u32, source: &str) -> Option<u32> {
        // SAFETY: caller holds a current context; the source pointer and length are valid for
        // the call.
        unsafe {
            let shader = (self.create_shader)(kind);
            let ptr = source.as_ptr() as *const c_char;
            let len = source.len() as i32;
            (self.shader_source)(shader, 1, &ptr, &len);
            (self.compile_shader)(shader);
            let mut ok = 0;
            (self.get_shaderiv)(shader, GL_COMPILE_STATUS, &mut ok);
            if ok == 0 {
                log::error!("view shader failed to compile");
                (self.delete_shader)(shader);
                return None;
            }
            Some(shader)
        }
    }

    /// The framebuffer the image is rendered into, `w`×`h` device pixels; rebuilt when the
    /// size changes. Returns `None` when the framebuffer cannot be completed.
    pub fn target(&mut self, w: i32, h: i32) -> Option<i32> {
        if self.fbo != 0 && self.size == (w, h) {
            return Some(self.fbo as i32);
        }
        self.release_target();
        // SAFETY: plain GL calls on the current context; the previous framebuffer binding is
        // restored before returning.
        unsafe {
            let mut previous = 0;
            (self.get_integerv)(
                crate::media::glcap::GL_DRAW_FRAMEBUFFER_BINDING,
                &mut previous,
            );
            (self.gen_textures)(1, &mut self.texture);
            (self.bind_texture)(GL_TEXTURE_2D, self.texture);
            (self.tex_image_2d)(
                GL_TEXTURE_2D,
                0,
                GL_RGBA8 as i32,
                w,
                h,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
            (self.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            (self.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
            (self.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
            (self.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
            (self.gen_framebuffers)(1, &mut self.fbo);
            (self.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (self.framebuffer_texture_2d)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                self.texture,
                0,
            );
            let complete =
                (self.check_framebuffer_status)(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE;
            (self.bind_framebuffer)(GL_FRAMEBUFFER, previous as u32);
            if !complete {
                log::error!("image framebuffer {w}x{h} is incomplete");
                self.release_target();
                return None;
            }
        }
        self.size = (w, h);
        Some(self.fbo as i32)
    }

    /// Draw the image texture into `fbo` (`w`×`h` device pixels) through `view` on a slot of
    /// `slot` logical pixels; `dpi` device pixels per logical pixel. `flip_y` mirrors the flag
    /// the image was rendered with, so a whole view is an exact copy.
    pub fn draw(&self, fbo: i32, w: i32, h: i32, view: &View, slot: Size, dpi: f64, flip_y: bool) {
        let (sin, cos) = view.rotation.to_radians().sin_cos();
        let k = view.scale;
        let (iw, ih) = (view.width as f64, view.height as f64);
        let ax = 2.0 * dpi / w as f64;
        let ay = if flip_y {
            -2.0 * dpi / h as f64
        } else {
            2.0 * dpi / h as f64
        };
        let oy = if flip_y { 1.0 } else { -1.0 };
        // Unit square → image pixels → slot pixels (scaled, turned, shifted) → device → clip.
        let tx = slot.w as f64 / 2.0 + view.x - k * (cos * iw / 2.0 - sin * ih / 2.0);
        let ty = slot.h as f64 / 2.0 + view.y - k * (sin * iw / 2.0 + cos * ih / 2.0);
        let m: [f32; 9] = [
            (ax * k * cos * iw) as f32,
            (ay * k * sin * iw) as f32,
            0.0,
            (ax * -k * sin * ih) as f32,
            (ay * k * cos * ih) as f32,
            0.0,
            (ax * tx - 1.0) as f32,
            (ay * ty + oy) as f32,
            1.0,
        ];
        // SAFETY: plain GL draw calls on the current context with objects this struct owns.
        unsafe {
            (self.bind_framebuffer)(GL_FRAMEBUFFER, fbo as u32);
            (self.viewport)(0, 0, w, h);
            (self.disable)(GL_BLEND);
            (self.disable)(GL_SCISSOR_TEST);
            (self.disable)(GL_DEPTH_TEST);
            (self.disable)(GL_CULL_FACE);
            (self.clear_color)(0.0, 0.0, 0.0, 1.0);
            (self.clear)(GL_COLOR_BUFFER_BIT);
            (self.use_program)(self.program);
            (self.uniform_matrix3fv)(self.u_matrix, 1, 0, m.as_ptr());
            (self.active_texture)(GL_TEXTURE0);
            (self.bind_texture)(GL_TEXTURE_2D, self.texture);
            (self.bind_vertex_array)(self.vao);
            (self.draw_arrays)(GL_TRIANGLE_STRIP, 0, 4);
            (self.bind_vertex_array)(0);
            (self.bind_texture)(GL_TEXTURE_2D, 0);
            (self.use_program)(0);
        }
    }

    fn release_target(&mut self) {
        // SAFETY: deleting objects this struct created; zero ids are ignored by GL.
        unsafe {
            if self.fbo != 0 {
                (self.delete_framebuffers)(1, &self.fbo);
            }
            if self.texture != 0 {
                (self.delete_textures)(1, &self.texture);
            }
        }
        self.fbo = 0;
        self.texture = 0;
        self.size = (0, 0);
    }

    /// Release every GL object; the context must be current.
    pub fn destroy(&mut self) {
        self.release_target();
        // SAFETY: deleting objects this struct created.
        unsafe {
            if self.vbo != 0 {
                (self.delete_buffers)(1, &self.vbo);
            }
            if self.vao != 0 {
                (self.delete_vertex_arrays)(1, &self.vao);
            }
            if self.program != 0 {
                (self.delete_program)(self.program);
            }
        }
        self.vbo = 0;
        self.vao = 0;
        self.program = 0;
    }
}

/// Render the current frame into `fbo` (`w`×`h` device pixels): straight when the view is the
/// whole image, otherwise through `quad`. Nothing is drawn when the view needs the quad and
/// there is none.
#[allow(clippy::too_many_arguments)]
pub fn render_view(
    ctx: &RenderContext,
    quad: &mut Option<Quad>,
    view: &View,
    slot: Size,
    fbo: i32,
    w: i32,
    h: i32,
    dpi: f64,
    flip_y: bool,
) {
    if view.is_whole(slot) {
        ctx.render(fbo, w, h, flip_y);
        return;
    }
    let Some(q) = quad else { return };
    let (tw, th) = (
        ((view.width as f64) * dpi).round() as i32,
        ((view.height as f64) * dpi).round() as i32,
    );
    if let Some(target) = q.target(tw.max(1), th.max(1)) {
        ctx.render(target, tw.max(1), th.max(1), flip_y);
        q.draw(fbo, w, h, view, slot, dpi, flip_y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_matrix_is_a_copy_for_a_whole_view() {
        let slot = Size { w: 1920, h: 1080 };
        let whole = View::whole(slot);
        let m = matrix(&whole, slot, 1920, 1080, 1.0, true);
        let apply =
            |m: &[f32; 9], x: f32, y: f32| (m[0] * x + m[3] * y + m[6], m[1] * x + m[4] * y + m[7]);
        assert_eq!(apply(&m, 0.0, 0.0), (-1.0, 1.0));
        assert_eq!(apply(&m, 1.0, 1.0), (1.0, -1.0));
        let shifted = View {
            x: 480.0,
            y: -270.0,
            scale: 0.5,
            ..whole
        };
        let m = matrix(&shifted, slot, 1920, 1080, 1.0, true);
        let (cx, cy) = apply(&m, 0.5, 0.5);
        assert!(
            (cx - 0.5).abs() < 1e-5 && (cy - 0.5).abs() < 1e-5,
            "{cx},{cy}"
        );
        let (x0, y0) = apply(&m, 0.0, 0.0);
        assert!(
            (x0 - 0.0).abs() < 1e-5 && (y0 - 1.0).abs() < 1e-5,
            "{x0},{y0}"
        );
    }

    fn matrix(view: &View, slot: Size, w: i32, h: i32, dpi: f64, flip_y: bool) -> [f32; 9] {
        let (sin, cos) = view.rotation.to_radians().sin_cos();
        let k = view.scale;
        let (iw, ih) = (view.width as f64, view.height as f64);
        let ax = 2.0 * dpi / w as f64;
        let ay = if flip_y {
            -2.0 * dpi / h as f64
        } else {
            2.0 * dpi / h as f64
        };
        let oy = if flip_y { 1.0 } else { -1.0 };
        let tx = slot.w as f64 / 2.0 + view.x - k * (cos * iw / 2.0 - sin * ih / 2.0);
        let ty = slot.h as f64 / 2.0 + view.y - k * (sin * iw / 2.0 + cos * ih / 2.0);
        [
            (ax * k * cos * iw) as f32,
            (ay * k * sin * iw) as f32,
            0.0,
            (ax * -k * sin * ih) as f32,
            (ay * k * cos * ih) as f32,
            0.0,
            (ax * tx - 1.0) as f32,
            (ay * ty + oy) as f32,
            1.0,
        ]
    }
}
