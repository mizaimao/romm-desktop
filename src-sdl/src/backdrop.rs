// The animated backdrop, as a shader.
//
// `ui/js/backdrop.js` is not CSS — it is WebGL2 with GLSL fragment shaders, a
// full-screen quad, and a handful of uniforms. On Mali-G52 the shading
// language maps to it one for one, which is why
// docs/handheld-frontend.md says not to leave this until last: it is the most
// portable thing in the whole front end, and getting it up early is what
// proves the GL context works on the device at all.
//
// The shader is the same source, with `#version 300 es` swapped for the
// desktop's `#version 330 core` — GLES 3.0 and GL 3.3 differ in the version
// line and in whether precision qualifiers are required, and in nothing else
// that this uses.

use anyhow::{Result, anyhow};
use std::ffi::CString;

/// Shared by every style: the uniforms, the noise, and the vignette.
///
/// One program per style would mean a compilation each at startup and a copy
/// each of the colour handling, so a style is a body spliced into this frame
/// and they cannot drift apart on how they read `u_strength` or how they
/// darken at the edges.
const HEAD: &str = r#"
uniform vec2  u_size;
uniform float u_time;
uniform vec3  u_low;
uniform vec3  u_mid;
uniform vec3  u_high;
uniform float u_strength;
uniform float u_speed;

float hash(vec2 p) {
  // Wrapped before the sine so the argument stays small however far the
  // coordinate has drifted. Time is unbounded here, and without the wrap the
  // sine is being asked for a value it cannot resolve after a few minutes of
  // running — which is what made the noisier styles band and march.
  p = fract(p * vec2(0.3183099, 0.3678794));
  p += dot(p, p + 19.19);
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), f.x),
             mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), f.x), f.y);
}

// Two octaves is enough for a backdrop and half the cost of four. This is
// drawn behind a library, not in a demo.
float fbm(vec2 p) {
  return noise(p) * 0.65 + noise(p * 2.1 + 4.7) * 0.35;
}

// Low -> mid -> high, so a scheme can be three colours rather than two.
vec3 ramp(float k) {
  k = clamp(k, 0.0, 1.0);
  return k < 0.5 ? mix(u_low, u_mid, k * 2.0) : mix(u_mid, u_high, (k - 0.5) * 2.0);
}

void main() {
  vec2 uv = gl_FragCoord.xy / u_size;
  vec2 aspect = vec2(u_size.x / u_size.y, 1.0);
  float t = u_time * u_speed;
  vec3 base;
"#;

const TAIL: &str = r#"
  // Darker towards the edges, and never fully bright in the middle either.
  // The grid sits on top of all of it, and text has to stay readable over the
  // brightest pixel this can produce rather than the average one.
  float d = distance(uv, vec2(0.5)) * 1.15;
  base *= 1.0 - smoothstep(0.15, 1.0, d) * 0.75;
  color = vec4(base * u_strength, 1.0);
}
"#;

const VERTEX_MODERN: &str = r#"
in vec2 pos;
void main() { gl_Position = vec4(pos, 0.0, 1.0); }
"#;

const VERTEX_LEGACY: &str = r#"
attribute vec2 pos;
void main() { gl_Position = vec4(pos, 0.0, 1.0); }
"#;

/// Which GLSL this context speaks.
///
/// Two, and not by choice. SDL's renderer creates its own context and its GL
/// backend is a legacy 2.1 one on macOS — the attributes asked for before the
/// window are for a context we then do not get to use. Owning the context
/// outright is where this ends up (the glass needs render-to-texture anyway),
/// and until then the shader speaks both.
///
/// The body is identical in both. What differs is three lines of preamble:
/// where the fragment colour goes, what the vertex input is called, and
/// whether precision qualifiers are allowed or required.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dialect {
    /// GLSL 3.30 core, or GLSL ES 3.00.
    Modern(&'static str),
    /// GLSL 1.20, or GLSL ES 1.00.
    Legacy(&'static str),
}

impl Dialect {
    fn fragment_preamble(self) -> &'static str {
        match self {
            Dialect::Modern("#version 300 es") => "precision highp float;
out vec4 color;
",
            Dialect::Modern(_) => "out vec4 color;
",
            // ES 1.00 must declare a precision and highp is optional in
            // fragment shaders, so it is asked for and fallen back on. This is
            // the whole of what the doc means by "a GLSL ES 1.00 rewrite with
            // mediump fragment precision": at mediump the hash below loses the
            // bits that make it noise, and the backdrop bands and marches.
            // Mali-G52 has highp; Mali-400 does not.
            Dialect::Legacy("#version 100") => {
                "#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#define color gl_FragColor
"
            }
            // GLSL 1.20 has no precision statements at all — they arrived in
            // 1.30 — so asking for one is a compile error rather than a hint.
            Dialect::Legacy(_) => "#define color gl_FragColor
",
        }
    }

    fn vertex(self) -> &'static str {
        match self {
            Dialect::Modern(_) => VERTEX_MODERN,
            // No precision statement here even for ES 1.00: vertex shaders
            // default to highp, and only fragment shaders have to be told.
            Dialect::Legacy(_) => VERTEX_LEGACY,
        }
    }

    fn version(self) -> &'static str {
        match self {
            Dialect::Modern(v) | Dialect::Legacy(v) => v,
        }
    }

    fn uses_vertex_arrays(self) -> bool {
        matches!(self, Dialect::Modern(_))
    }
}

/// One shape, and what the motion slider means for it.
///
/// `pace` exists because one slider across five shapes was a lie: every body
/// writes its own multipliers on `t`, and they are two decades apart. Blobs
/// drifts at 0.015 of it and Plasma sweeps at 0.31.
pub struct Style {
    pub id: &'static str,
    pub label: &'static str,
    pub pace: f32,
    body: &'static str,
}

pub const STYLES: &[Style] = &[
    Style {
        id: "blobs",
        label: "Blobs",
        pace: 1.7,
        body: r#"
      float a = noise(uv * aspect * 3.0 + vec2(t * 0.02, t * 0.013));
      float b = noise(uv * aspect * 6.0 - vec2(t * 0.011, t * 0.017));
      base = ramp(smoothstep(0.30, 0.85, a * 0.65 + b * 0.35));"#,
    },
    Style {
        id: "aurora",
        label: "Aurora",
        pace: 1.4,
        body: r#"
      float band = uv.y * 2.2 + fbm(vec2(uv.x * 2.0, t * 0.05)) * 1.6;
      float curtain = sin(band * 3.14159) * 0.5 + 0.5;
      curtain *= smoothstep(1.0, 0.15, uv.y);
      base = ramp(pow(curtain, 1.6));"#,
    },
    Style {
        id: "plasma",
        label: "Plasma",
        pace: 0.1,
        body: r#"
      vec2 p = (uv - 0.5) * aspect * 4.0;
      float v = sin(p.x + t * 0.35)
              + sin(p.y * 1.3 - t * 0.28)
              + sin((p.x + p.y) * 0.7 + t * 0.2)
              + sin(length(p) * 1.6 - t * 0.4);
      base = ramp(smoothstep(-1.2, 2.4, v));"#,
    },
];

/// What the motion slider sits at before anyone touches it.
///
/// From `ui/js/backdrop.js`, where it is shared across styles and each one's
/// `pace` scales it. The range there is 0 to 7.
pub const DEFAULT_SPEED: f32 = 4.0;

pub fn style(id: &str) -> &'static Style {
    STYLES.iter().find(|s| s.id == id).unwrap_or(&STYLES[0])
}

/// A colour scheme, as the three stops the ramp blends between.
#[derive(Debug, Clone, Copy)]
pub struct Scheme {
    pub low: [f32; 3],
    pub mid: [f32; 3],
    pub high: [f32; 3],
}

impl Default for Scheme {
    /// The one the app opens on: a deep blue rising to a warm highlight.
    fn default() -> Self {
        Scheme {
            low: [0.04, 0.05, 0.10],
            mid: [0.10, 0.14, 0.30],
            high: [0.35, 0.30, 0.55],
        }
    }
}

pub struct Backdrop {
    program: u32,
    vao: u32,
    vbo: u32,
    uniforms: Uniforms,
    pub scheme: Scheme,
    pub strength: f32,
    pub speed: f32,
    pace: f32,
    label: &'static str,
    dialect: Dialect,
    /// Where `pos` lives, for the legacy path that re-describes the buffer
    /// every draw.
    attribute: u32,
}

struct Uniforms {
    size: i32,
    time: i32,
    low: i32,
    mid: i32,
    high: i32,
    strength: i32,
    speed: i32,
}

impl Backdrop {
    /// Compile it for the style named, against a context that is already
    /// current.
    ///
    /// # Safety
    ///
    /// Every call here is a GL call, and GL has no notion of which thread or
    /// which context it is being asked from — the caller has made one current
    /// and must not change it under us.
    pub unsafe fn build(video: &sdl2::VideoSubsystem, style_id: &str) -> Result<Self> {
        unsafe {
            gl::load_with(|name| video.gl_get_proc_address(name) as *const _);

            let chosen = style(style_id);
            let dialect = dialect_for(&reported(gl::SHADING_LANGUAGE_VERSION))?;
            let version = dialect.version();
            let fragment = format!(
                "{version}\n{}{HEAD}{}{TAIL}",
                dialect.fragment_preamble(),
                chosen.body
            );
            let vertex = format!("{version}\n{}", dialect.vertex());

            println!(
                "gl: {} · {}",
                reported(gl::VERSION),
                reported(gl::SHADING_LANGUAGE_VERSION)
            );
            let program = link(&vertex, &fragment)?;

            // One quad, as two triangles covering the clip volume. The vertex
            // shader passes it through: everything interesting happens per
            // fragment.
            let corners: [f32; 12] =
                [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0];
            // Vertex array objects are 3.0 and later. A legacy context binds
            // the buffer and describes it on every draw instead.
            let (mut vao, mut vbo) = (0, 0);
            if dialect.uses_vertex_arrays() {
                gl::GenVertexArrays(1, &mut vao);
                gl::BindVertexArray(vao);
            }
            gl::GenBuffers(1, &mut vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                size_of_val(&corners) as isize,
                corners.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );
            let pos = CString::new("pos").unwrap();
            let at = gl::GetAttribLocation(program, pos.as_ptr()).max(0) as u32;
            gl::EnableVertexAttribArray(at);
            gl::VertexAttribPointer(at, 2, gl::FLOAT, gl::FALSE, 0, std::ptr::null());
            if dialect.uses_vertex_arrays() {
                gl::BindVertexArray(0);
            }

            let uniform = |name: &str| {
                let c = CString::new(name).unwrap();
                gl::GetUniformLocation(program, c.as_ptr())
            };
            Ok(Backdrop {
                program,
                vao,
                vbo,
                uniforms: Uniforms {
                    size: uniform("u_size"),
                    time: uniform("u_time"),
                    low: uniform("u_low"),
                    mid: uniform("u_mid"),
                    high: uniform("u_high"),
                    strength: uniform("u_strength"),
                    speed: uniform("u_speed"),
                },
                scheme: Scheme::default(),
                strength: 0.5,
                // The webview's own default, and it matters: this is
                // multiplied by the style's `pace` and then by the small
                // constants in the body — Blobs drifts at 0.02 of it — so at
                // 1.0 the picture moves and nobody can tell.
                speed: DEFAULT_SPEED,
                pace: chosen.pace,
                label: chosen.label,
                dialect,
                attribute: at,
            })
        }
    }

    /// Draw one frame of it, filling whatever is bound.
    ///
    /// # Safety
    ///
    /// As `build`. The caller must also have flushed anything the SDL renderer
    /// had pending, or its batched state and ours interleave.
    pub unsafe fn draw(&self, width: f32, height: f32, seconds: f32) {
        unsafe {
            gl::UseProgram(self.program);
            gl::Uniform2f(self.uniforms.size, width, height);
            gl::Uniform1f(self.uniforms.time, seconds);
            gl::Uniform3fv(self.uniforms.low, 1, self.scheme.low.as_ptr());
            gl::Uniform3fv(self.uniforms.mid, 1, self.scheme.mid.as_ptr());
            gl::Uniform3fv(self.uniforms.high, 1, self.scheme.high.as_ptr());
            gl::Uniform1f(self.uniforms.strength, self.strength);
            gl::Uniform1f(self.uniforms.speed, self.speed * self.pace);
            gl::Disable(gl::BLEND);
            gl::Disable(gl::DEPTH_TEST);
            if self.dialect.uses_vertex_arrays() {
                gl::BindVertexArray(self.vao);
            } else {
                gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
                gl::EnableVertexAttribArray(self.attribute);
                gl::VertexAttribPointer(
                    self.attribute, 2, gl::FLOAT, gl::FALSE, 0, std::ptr::null(),
                );
            }
            gl::DrawArrays(gl::TRIANGLES, 0, 6);
            if self.dialect.uses_vertex_arrays() {
                gl::BindVertexArray(0);
            } else {
                gl::DisableVertexAttribArray(self.attribute);
                gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            }
            gl::UseProgram(0);
        }
    }
}

impl Backdrop {
    pub fn style_label(&self) -> &'static str {
        self.label
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.program);
            gl::DeleteBuffers(1, &self.vbo);
            if self.dialect.uses_vertex_arrays() {
                gl::DeleteVertexArrays(1, &self.vao);
            }
        }
    }
}

/// The version line, chosen from what the context actually is.
///
/// Asked rather than assumed, because the answer is not the platform's to
/// give: SDL hands back whatever context it was configured for, and a request
/// for core 3.3 that was not honoured produces "version '330' is not
/// supported" — a true statement about a context nobody asked for, and an
/// unhelpful one.
///
/// Everything below this line — the noise, the ramp, the vignette — is
/// identical in both dialects, which is the whole reason the doc calls this
/// the most portable thing in the front end.
fn dialect_for(shading_language: &str) -> Result<Dialect> {
    // "OpenGL ES GLSL ES 3.00", or "3.30", or Apple's "4.10 Metal - 90.1".
    let es = shading_language.contains("ES");
    let number: f32 = shading_language
        .split_whitespace()
        .find_map(|word| word.parse().ok())
        .unwrap_or(0.0);

    match (es, number) {
        (true, n) if n >= 3.0 => Ok(Dialect::Modern("#version 300 es")),
        (true, n) if n >= 1.0 => Ok(Dialect::Legacy("#version 100")),
        (false, n) if n >= 3.3 => Ok(Dialect::Modern("#version 330 core")),
        (false, n) if n >= 1.2 => Ok(Dialect::Legacy("#version 120")),
        _ => Err(anyhow!(
            "this context reports GLSL {shading_language:?}, which is older than \
             anything the backdrop can be written in"
        )),
    }
}

unsafe fn reported(name: u32) -> String {
    unsafe {
        let raw = gl::GetString(name);
        if raw.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(raw as *const _).to_string_lossy().into_owned()
    }
}

unsafe fn link(vertex: &str, fragment: &str) -> Result<u32> {
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
            return Err(anyhow!("linking the backdrop: {}", log(program, true)));
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
            // The whole log, not a summary. A shader that will not compile on
            // a device we do not have is a message somebody has to read from a
            // photograph of a 4" screen.
            return Err(anyhow!("compiling the backdrop: {}", log(shader, false)));
        }
        Ok(shader)
    }
}

unsafe fn log(object: u32, program: bool) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every style has to splice into the shared frame, and the frame declares
    /// the things they all use. A body naming something the head does not
    /// define is a shader that fails to compile on the device and nowhere
    /// else, because nothing here compiles it without a context.
    #[test]
    fn every_style_only_uses_what_the_frame_provides() {
        for style in STYLES {
            for name in ["uv", "aspect", "t", "base"] {
                assert!(
                    HEAD.contains(name) || style.body.contains(name),
                    "{}: {name} is used by nobody", style.id
                );
            }
            assert!(style.body.contains("base ="), "{} never fills base", style.id);
            assert!(style.pace > 0.0, "{} has no pace", style.id);
        }
    }

    /// The motion slider means the same thing to every shape, which is what
    /// `pace` is for — and the numbers are two decades apart, so a typo is
    /// invisible.
    #[test]
    fn the_paces_span_the_range_they_are_meant_to() {
        let fastest = STYLES.iter().map(|s| s.pace).fold(f32::MIN, f32::max);
        let slowest = STYLES.iter().map(|s| s.pace).fold(f32::MAX, f32::min);
        assert!(fastest / slowest > 10.0, "the paces are all the same, so the slider is a lie");
    }

    /// The version line comes from the context, and the strings are what
    /// drivers actually report — including Apple's, which puts "Metal" in it.
    #[test]
    fn the_dialect_follows_the_context() {
        assert_eq!(dialect_for("OpenGL ES GLSL ES 3.20").unwrap(), Dialect::Modern("#version 300 es"));
        assert_eq!(dialect_for("3.30").unwrap(), Dialect::Modern("#version 330 core"));
        assert_eq!(dialect_for("4.10 Metal - 90.1").unwrap(), Dialect::Modern("#version 330 core"));
        // The one SDL's own renderer hands back on macOS, which is what this
        // whole second dialect exists for.
        assert_eq!(dialect_for("1.20").unwrap(), Dialect::Legacy("#version 120"));
        assert_eq!(dialect_for("OpenGL ES GLSL ES 1.00").unwrap(), Dialect::Legacy("#version 100"));
    }

    /// Where the fragment colour goes is the preamble's business and nobody
    /// else's. It was declared in both — the shared frame *and* the modern
    /// preamble — which is a duplicate on one dialect and a compile error on
    /// the other, and the error names a line the body does not contain.
    #[test]
    fn the_shared_frame_does_not_declare_the_output() {
        assert!(!HEAD.contains("out vec4"), "the frame declares the output as well");
        assert!(!HEAD.contains("gl_FragColor"), "the frame names a legacy-only builtin");
    }

    /// The two dialects differ in three lines of preamble and nothing else,
    /// and each has to declare what the body uses.
    #[test]
    fn each_dialect_declares_what_the_body_needs() {
        for d in [
            Dialect::Modern("#version 330 core"),
            Dialect::Modern("#version 300 es"),
            Dialect::Legacy("#version 120"),
            Dialect::Legacy("#version 100"),
        ] {
            let pre = d.fragment_preamble();
            assert!(
                pre.contains("out vec4 color") || pre.contains("#define color"),
                "{d:?} never says where the colour goes"
            );
            assert!(d.vertex().contains("pos"), "{d:?} has no vertex input");
        }
        // GLSL 1.20 has no precision statements; asking for one is an error.
        assert!(!Dialect::Legacy("#version 120").fragment_preamble().contains("precision"));
        // GLSL ES 1.00 must have one.
        assert!(Dialect::Legacy("#version 100").fragment_preamble().contains("precision"));
    }

    #[test]
    fn a_context_too_old_says_so() {
        let err = dialect_for("").unwrap_err().to_string();
        assert!(err.contains("GLSL"), "unhelpful message: {err}");
    }

    #[test]
    fn an_unknown_style_falls_back_rather_than_failing() {
        assert_eq!(style("nonsense").id, STYLES[0].id);
        assert_eq!(style("aurora").id, "aurora");
    }
}
