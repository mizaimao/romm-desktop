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
out vec4 color;

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

const VERTEX: &str = r#"
in vec2 pos;
void main() { gl_Position = vec4(pos, 0.0, 1.0); }
"#;

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
            let version = version_line(&reported(gl::SHADING_LANGUAGE_VERSION))?;
            let fragment = format!("{version}\nprecision highp float;\n{HEAD}{}{TAIL}", chosen.body);
            let vertex = format!("{version}\n{VERTEX}");

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
            let (mut vao, mut vbo) = (0, 0);
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                size_of_val(&corners) as isize,
                corners.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );
            let pos = CString::new("pos").unwrap();
            let at = gl::GetAttribLocation(program, pos.as_ptr()) as u32;
            gl::EnableVertexAttribArray(at);
            gl::VertexAttribPointer(at, 2, gl::FLOAT, gl::FALSE, 0, std::ptr::null());
            gl::BindVertexArray(0);

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
                speed: 1.0,
                pace: chosen.pace,
                label: chosen.label,
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
            gl::BindVertexArray(self.vao);
            gl::DrawArrays(gl::TRIANGLES, 0, 6);
            gl::BindVertexArray(0);
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
            gl::DeleteVertexArrays(1, &self.vao);
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
fn version_line(shading_language: &str) -> Result<&'static str> {
    // "OpenGL ES GLSL ES 3.00" or "3.30" / "4.10 Metal - 90".
    let es = shading_language.contains("ES");
    let number: f32 = shading_language
        .split_whitespace()
        .find_map(|word| word.parse().ok())
        .unwrap_or(0.0);

    match (es, number) {
        (true, n) if n >= 3.0 => Ok("#version 300 es"),
        (false, n) if n >= 3.3 => Ok("#version 330 core"),
        _ => Err(anyhow!(
            "this context is GLSL {shading_language:?}, and the backdrop needs \
             GLSL ES 3.00 or GLSL 3.30 — SDL was asked for one and given another"
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
    fn the_shader_version_follows_the_context() {
        assert_eq!(version_line("OpenGL ES GLSL ES 3.20").unwrap(), "#version 300 es");
        assert_eq!(version_line("OpenGL ES GLSL ES 3.00").unwrap(), "#version 300 es");
        assert_eq!(version_line("3.30").unwrap(), "#version 330 core");
        assert_eq!(version_line("4.10 Metal - 90.1").unwrap(), "#version 330 core");
    }

    /// The context macOS hands back when nobody asks for a better one. It has
    /// to say so rather than emit a version line the driver will reject.
    #[test]
    fn a_context_too_old_says_which_rather_than_failing_in_the_compiler() {
        for old in ["1.20", "OpenGL ES GLSL ES 1.00", ""] {
            let err = version_line(old).unwrap_err().to_string();
            assert!(err.contains("GLSL"), "{old:?} gave an unhelpful message: {err}");
        }
    }

    #[test]
    fn an_unknown_style_falls_back_rather_than_failing() {
        assert_eq!(style("nonsense").id, STYLES[0].id);
        assert_eq!(style("aurora").id, "aurora");
    }
}
