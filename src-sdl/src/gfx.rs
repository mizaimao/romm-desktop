// Drawing, on a context we own.
//
// SDL's 2D renderer went away with this file arriving, and the reason is not
// that it was slow. It creates its *own* GL context and gives no say in what
// version — on macOS a legacy 2.1 one, on Linux and Android a GLES 2.0 one —
// so a shader had to be written in whichever dialect it happened to hand back,
// and multi-pass work had to be interleaved with a batcher that does not know
// about it. The glass is render-to-texture and a blur, and that is not a thing
// to do through somebody else's batching.
//
// So: one context, asked for outright, GL 3.3 core on a desktop and GLES 3.0
// on the handheld. Both speak the same GLSL, so there is one dialect again.
// What is left is a couple of hundred lines that draw a rectangle with an
// optional texture on it, which is everything an interface of cards and words
// actually needs.

use anyhow::{Context, Result, anyhow};
use std::ffi::CString;

/// The vertex shader: a quad in points, and where on a texture each corner
/// sits.
///
/// Positions arrive in the same points everything else is written in and are
/// turned into clip space here, so nothing above this file ever computes a
/// coordinate between -1 and 1.
const VERTEX: &str = r#"
in vec2 a_pos;
in vec2 a_uv;
out vec2 v_uv;
uniform vec2 u_screen;
void main() {
  v_uv = a_uv;
  vec2 clip = vec2(a_pos.x / u_screen.x * 2.0 - 1.0, 1.0 - a_pos.y / u_screen.y * 2.0);
  gl_Position = vec4(clip, 0.0, 1.0);
}
"#;

/// The fragment shader: a colour, optionally multiplied by a texture.
///
/// One program rather than two. Text is a coverage mask tinted at draw time
/// and artwork is a picture drawn as it is, and both are "a colour times a
/// sample" — with a 1x1 white texture standing in when there is no picture, so
/// a plain rectangle takes the same path as everything else.
const FRAGMENT: &str = r#"
in vec2 v_uv;
out vec4 color;
uniform sampler2D u_texture;
uniform vec4 u_tint;
void main() {
  color = texture(u_texture, v_uv) * u_tint;
}
"#;

/// A colour, 0 to 1.
#[derive(Debug, Clone, Copy)]
pub struct Rgba(pub f32, pub f32, pub f32, pub f32);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
    }
    pub const WHITE: Rgba = Rgba(1.0, 1.0, 1.0, 1.0);
}

/// An image on the card. Deletes itself.
pub struct Texture {
    id: u32,
    /// Its own size in pixels. Not what it is drawn at — a cover is stretched
    /// to the card — but what it would be at one to one.
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
}

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe { gl::DeleteTextures(1, &self.id) }
    }
}

pub struct Gfx {
    program: u32,
    vao: u32,
    vbo: u32,
    u_screen: i32,
    u_tint: i32,
    /// One white pixel, so a rectangle with no picture on it is the same draw
    /// as one with.
    blank: Texture,
    width: f32,
    height: f32,
}

impl Gfx {
    /// # Safety
    ///
    /// A context must be current, and must stay current for the life of this.
    pub unsafe fn new(video: &sdl2::VideoSubsystem) -> Result<Self> {
        unsafe {
            gl::load_with(|name| video.gl_get_proc_address(name) as *const _);
            let version = version_line().context("this machine's OpenGL")?;
            let program = link(
                &format!("{version}\n{}{VERTEX}", precision(version)),
                &format!("{version}\n{}{FRAGMENT}", precision(version)),
            )?;

            let (mut vao, mut vbo) = (0, 0);
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            // Four floats a vertex — two of position, two of texture — and six
            // vertices a quad. Filled per draw; there is no static geometry
            // in an interface where everything moves.
            gl::BufferData(gl::ARRAY_BUFFER, (24 * 4) as isize, std::ptr::null(), gl::STREAM_DRAW);
            bind_attribute(program, "a_pos", 0)?;
            bind_attribute(program, "a_uv", 2)?;
            gl::BindVertexArray(0);

            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl::Disable(gl::DEPTH_TEST);

            let blank = upload(1, 1, &[255, 255, 255, 255]);
            Ok(Gfx {
                u_screen: uniform(program, "u_screen"),
                u_tint: uniform(program, "u_tint"),
                program,
                vao,
                vbo,
                blank,
                width: 1.0,
                height: 1.0,
            })
        }
    }

    /// Tell it how big the drawable is, in pixels.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        unsafe { gl::Viewport(0, 0, self.width as i32, self.height as i32) };
    }

    pub fn clear(&self, color: Rgba) {
        unsafe {
            gl::ClearColor(color.0, color.1, color.2, color.3);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
    }

    /// A filled rectangle, in pixels from the top left.
    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32, color: Rgba) {
        self.quad(&self.blank, x, y, w, h, color);
    }

    /// An image, stretched to fit.
    ///
    /// `tint` multiplies it, which is what makes one text raster serve every
    /// colour it is drawn in — white with the coverage as alpha, tinted here.
    pub fn image(&self, texture: &Texture, x: f32, y: f32, w: f32, h: f32, tint: Rgba) {
        self.quad(texture, x, y, w, h, tint);
    }

    fn quad(&self, texture: &Texture, x: f32, y: f32, w: f32, h: f32, tint: Rgba) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let (l, t, r, b) = (x, y, x + w, y + h);
        #[rustfmt::skip]
        let vertices: [f32; 24] = [
            l, t, 0.0, 0.0,
            r, t, 1.0, 0.0,
            l, b, 0.0, 1.0,
            l, b, 0.0, 1.0,
            r, t, 1.0, 0.0,
            r, b, 1.0, 1.0,
        ];
        unsafe {
            gl::UseProgram(self.program);
            gl::Uniform2f(self.u_screen, self.width, self.height);
            gl::Uniform4f(self.u_tint, tint.0, tint.1, tint.2, tint.3);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, texture.id);
            gl::BindVertexArray(self.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl::BufferSubData(
                gl::ARRAY_BUFFER,
                0,
                size_of_val(&vertices) as isize,
                vertices.as_ptr() as *const _,
            );
            gl::DrawArrays(gl::TRIANGLES, 0, 6);
            gl::BindVertexArray(0);
        }
    }

    /// Upload a picture: four bytes a pixel, top row first.
    pub fn upload_rgba(&self, width: u32, height: u32, pixels: &[u8]) -> Texture {
        unsafe { upload(width, height, pixels) }
    }

    /// Upload a coverage mask as white with that coverage for alpha.
    ///
    /// What text is. Kept white here so the colour is decided at draw time and
    /// one raster serves a name however it is drawn — dim in a metadata line,
    /// bright under the cursor.
    pub fn upload_coverage(&self, width: u32, height: u32, coverage: &[u8]) -> Texture {
        let mut rgba = Vec::with_capacity(coverage.len() * 4);
        for &value in coverage {
            rgba.extend_from_slice(&[255, 255, 255, value]);
        }
        unsafe { upload(width, height, &rgba) }
    }
}

impl Drop for Gfx {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.program);
            gl::DeleteBuffers(1, &self.vbo);
            gl::DeleteVertexArrays(1, &self.vao);
        }
    }
}

unsafe fn upload(width: u32, height: u32, pixels: &[u8]) -> Texture {
    unsafe {
        let mut id = 0;
        gl::GenTextures(1, &mut id);
        gl::BindTexture(gl::TEXTURE_2D, id);
        // Linear, because a cover is drawn at whatever size the zoom asks for
        // and nearest makes that look like a screenshot of a screenshot.
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        // Clamped, or the edge of a card samples the opposite edge of its own
        // artwork and every cover gets a thin wrong-coloured line round it.
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        // Rows are not padded. The default is four-byte alignment, which
        // shears any image whose width is not a multiple of four — and a text
        // raster is exactly as wide as the word in it.
        gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA as i32,
            width as i32,
            height as i32,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            pixels.as_ptr() as *const _,
        );
        gl::BindTexture(gl::TEXTURE_2D, 0);
        Texture { id, width, height }
    }
}

unsafe fn bind_attribute(program: u32, name: &str, offset: usize) -> Result<()> {
    unsafe {
        let c = CString::new(name)?;
        let at = gl::GetAttribLocation(program, c.as_ptr());
        if at < 0 {
            return Err(anyhow!("the shader has no {name}"));
        }
        gl::EnableVertexAttribArray(at as u32);
        gl::VertexAttribPointer(
            at as u32,
            2,
            gl::FLOAT,
            gl::FALSE,
            (4 * std::mem::size_of::<f32>()) as i32,
            (offset * std::mem::size_of::<f32>()) as *const _,
        );
        Ok(())
    }
}

unsafe fn uniform(program: u32, name: &str) -> i32 {
    unsafe {
        let c = CString::new(name).unwrap();
        gl::GetUniformLocation(program, c.as_ptr())
    }
}

/// GLSL ES needs to be told a precision; desktop GLSL does not have the
/// statement at all before 1.30 and ignores it after.
fn precision(version: &str) -> &'static str {
    if version.contains(" es") { "precision highp float;\n" } else { "" }
}

/// Which GLSL this context speaks.
///
/// One answer now, not two: the context is ours, so it is the one we asked
/// for. A machine that cannot give it says so here rather than failing inside
/// a shader compiler.
pub fn version_line() -> Result<&'static str> {
    let reported = unsafe { reported(gl::SHADING_LANGUAGE_VERSION) };
    let es = reported.contains("ES");
    let number: f32 =
        reported.split_whitespace().find_map(|word| word.parse().ok()).unwrap_or(0.0);
    match (es, number) {
        (true, n) if n >= 3.0 => Ok("#version 300 es"),
        (false, n) if n >= 3.3 => Ok("#version 330 core"),
        _ => Err(anyhow!(
            "reports GLSL {reported:?}; this needs GLSL 3.30 or GLSL ES 3.00"
        )),
    }
}

pub unsafe fn reported(name: u32) -> String {
    unsafe {
        let raw = gl::GetString(name);
        if raw.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(raw as *const _).to_string_lossy().into_owned()
    }
}

pub unsafe fn link(vertex: &str, fragment: &str) -> Result<u32> {
    unsafe {
        let v = compile(gl::VERTEX_SHADER, vertex)?;
        let f = compile(gl::FRAGMENT_SHADER, fragment)?;
        let program = gl::CreateProgram();
        gl::AttachShader(program, v);
        gl::AttachShader(program, f);
        gl::LinkProgram(program);
        gl::DeleteShader(v);
        gl::DeleteShader(f);
        let mut ok = 0;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut ok);
        if ok == 0 {
            return Err(anyhow!("linking: {}", info_log(program, true)));
        }
        Ok(program)
    }
}

unsafe fn compile(kind: u32, source: &str) -> Result<u32> {
    unsafe {
        let shader = gl::CreateShader(kind);
        let c = CString::new(source)?;
        gl::ShaderSource(shader, 1, &c.as_ptr(), std::ptr::null());
        gl::CompileShader(shader);
        let mut ok = 0;
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut ok);
        if ok == 0 {
            // The whole log. A shader that will not compile on a device we do
            // not have is a message somebody reads off a photograph.
            return Err(anyhow!("compiling: {}", info_log(shader, false)));
        }
        Ok(shader)
    }
}

unsafe fn info_log(object: u32, program: bool) -> String {
    unsafe {
        let mut len = 0;
        if program {
            gl::GetProgramiv(object, gl::INFO_LOG_LENGTH, &mut len);
        } else {
            gl::GetShaderiv(object, gl::INFO_LOG_LENGTH, &mut len);
        }
        let mut buf = vec![0u8; len.max(1) as usize];
        if program {
            gl::GetProgramInfoLog(object, len, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _);
        } else {
            gl::GetShaderInfoLog(object, len, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _);
        }
        String::from_utf8_lossy(&buf).trim_end_matches('\0').trim().to_owned()
    }
}
