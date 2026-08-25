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

use anyhow::Result;
use std::ffi::CString;

/// Shared by every style: the uniforms, the noise, and the vignette.
///
/// One program per style would mean a compilation each at startup and a copy
/// each of the color handling, so a style is a body spliced into this frame
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

// Low -> mid -> high, so a scheme can be three colors rather than two.
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
    pub body: &'static str,
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
    Style {
        id: "grid",
        label: "Grid",
        pace: 0.7,
        body: r#"
      vec2 q = (uv - vec2(0.5, 0.55)) * aspect;
      float depth = max(-q.y, 1e-3);
      vec2 g = vec2(q.x / depth, 1.0 / depth + t * 0.25);
      vec2 w = max(fwidth(g * 2.0), 1e-4);
      vec2 line = abs(fract(g * 2.0) - 0.5) / w;
      float mesh = 1.0 - min(min(line.x, line.y), 1.0);
      // Nothing above the horizon but a glow, and the mesh fades in below it:
      // the lines converge faster than the pixels can hold them right at the
      // vanishing line, and drawn hard that reads as noise rather than as
      // distance.
      float floorMask = step(0.0, -q.y) * smoothstep(0.0, 0.06, depth);
      float sky = 1.0 - smoothstep(0.0, 0.30, max(q.y, 0.0));
      base = ramp(mesh * 0.8 * floorMask + sky * 0.12);"#,
    },
    Style {
        id: "stars",
        label: "Drift",
        pace: 1.0,
        body: r#"
      vec2 p = uv * aspect * 18.0 + vec2(0.0, t * 0.08);
      vec2 cell = floor(p), f = fract(p);
      float glow = 0.0;
      for (int j = -1; j <= 1; j++) {
        for (int i = -1; i <= 1; i++) {
          vec2 o = vec2(float(i), float(j));
          vec2 c = cell + o;
          float star = hash(c);
          vec2 pos = f - o - vec2(hash(c + 3.1), hash(c + 7.7));
          float twinkle = 0.6 + 0.4 * sin(t * 1.6 + star * 40.0);
          glow += smoothstep(0.42, 0.0, length(pos)) * step(0.955, star) * twinkle;
        }
      }
      base = ramp(min(glow, 1.0));"#,
    },
    Style {
        id: "starfield",
        label: "Starfield",
        pace: 1.1,
        body: r#"
      base = u_low;
      for (int L = 0; L < 3; L++) {
        float f = float(L);
        float scale = 10.0 + f * 12.0;
        float speed = (f + 1.0) * 0.06;
        vec2 p = uv * aspect * scale + vec2(t * speed, 0.0);
        vec2 cell = floor(p), fr = fract(p);
        // The neighbours too. A star is scattered anywhere in its cell and its
        // glow has a radius, so one near an edge had the rest of it drawn by a
        // cell that was never asked — the same bug Drift had, and it looks the
        // same: points sliced flat along invisible straight lines.
        float glow = 0.0;
        for (int j = -1; j <= 1; j++) {
          for (int i = -1; i <= 1; i++) {
            vec2 o = vec2(float(i), float(j));
            vec2 c = cell + o;
            float star = hash(c + f * 17.0);
            vec2 off = fr - o - vec2(hash(c + 3.1), hash(c + 7.7));
            glow += smoothstep(0.30, 0.0, length(off)) * step(0.972, star);
          }
        }
        base = mix(base, u_high, min(glow, 1.0) * (0.35 + f * 0.32));
      }"#,
    },
    Style {
        id: "towers",
        label: "Towers",
        pace: 0.3,
        body: r#"
      // The rows cycle in depth: one reaching the front wraps to the back,
      // with the phases spread so they arrive evenly rather than in a pulse.
      // Additive, because the columns are light rather than surfaces — which
      // also means they never need sorting, and that is what lets a row wrap
      // past its neighbours without a seam.
      float horizon = 0.58;
      vec2 q = vec2((uv.x - 0.5) * aspect.x, uv.y);
      float ypx = max(fwidth(q.y), 1e-5);
      float glow = 0.0;

      for (int L = 0; L < 6; L++) {
        float turn = t * 0.06 + float(L) / 6.0;
        float phase = fract(turn);
        float z = mix(6.0, 0.55, phase);
        float s = 0.42 / z;

        // Where this row's floor meets the screen, and how much world reaches
        // across one screen unit at that depth.
        float yb = horizon - s * 0.9;
        float wx = q.x / s;

        // The row is slid sideways by its own hash, and re-seeded each time it
        // wraps: rows sharing an offset line up into corridors, and the same
        // rank of columns coming round again is a carousel.
        float row = float(L) * 13.0 + floor(turn);
        float slide = hash(vec2(row, 3.0)) * 7.0;
        float cx = floor(wx + slide);
        float fx = fract(wx + slide) - 0.5;

        // Most cells empty. A solid rank is a fence; the boot screen was a
        // scatter, because it was drawing what was on the memory card.
        float seed = hash(vec2(cx, row));
        float there = step(0.7, seed);
        float height = (0.35 + hash(vec2(cx, row + 1.7)) * 1.5) * s;
        float tall = max(height, 1e-4);

        // The bar, and a wider soft falloff around it. The halo is most of
        // what reads as translucent light rather than as a white rectangle.
        float wpx = max(fwidth(wx), 1e-4);
        float core = 1.0 - smoothstep(0.10 - wpx, 0.10 + wpx, abs(fx));
        float halo = exp(-abs(fx) * 9.0) * 0.35;

        // Up the column, in blocks. The seams flatten out once a block is
        // thinner than a pixel, which at the far end of the range they are —
        // drawn anyway they stop being blocks and become a shimmer.
        float up = q.y - yb;
        float k = clamp(up / tall, 0.0, 1.0);
        float bs = max(s * 0.22, 1e-4);
        float band = 0.3 * smoothstep(0.004, 0.022, bs);
        float box = (1.0 - band) + band * cos(up / bs * 6.28318);
        // Soft at the foot and at the cap rather than cut off. The canvas is
        // drawn at half the window's resolution and scaled up, and a hard step
        // along a vertical edge is the one place that shows.
        float column = smoothstep(-ypx, ypx, up)
                     * smoothstep(height + ypx, height - ypx, up);
        float lit = column * (mix(1.0, 0.4, k) * box + smoothstep(0.8, 1.0, k) * 0.9);

        // A little of the light escaping past the top of the stack.
        lit += step(height, up) * exp(-max(up - height, 0.0) * 26.0) * 0.35;

        // The reflection, mirrored in the plane and dimmer the further it
        // falls from the foot of the column.
        float below = yb - q.y;
        lit += step(0.0, below) * step(below, height) * exp(-below * 5.0) * 0.5;

        // Haze towards the back, and a fade at both ends of the depth range so
        // a row never pops in at either edge of it. Depth is a straight
        // function of the phase here, so the haze can be read off that rather
        // than off z.
        glow += lit * (core + halo) * there * (0.45 + 0.55 * phase)
              * smoothstep(0.0, 0.18, phase) * smoothstep(1.0, 0.82, phase);
      }

      // The plane itself: not black, and brightest where it meets the horizon,
      // which is what stops the lower half of the screen reading as a hole
      // rather than as a floor.
      float plane = step(q.y, horizon) * smoothstep(0.0, 0.9, q.y / horizon) * 0.13;
      plane += exp(-abs(q.y - horizon) * 90.0) * 0.1;
      base = ramp(min(glow * 0.8 + plane, 1.0));
      // The hottest cores go past the top of the ramp, towards white. Most of
      // these schemes have a dark navy for their high, and a light source that
      // never gets brighter than the wall behind it does not read as a light
      // source — it reads as a painted stripe. Only where several columns'
      // haloes overlap, so the field stays the color the scheme asked for.
      base += smoothstep(0.75, 1.6, glow) * 0.35;"#,
    },
    Style {
        id: "tunnel",
        label: "Tunnel",
        pace: 0.35,
        body: r#"
      vec2 q = (uv - 0.5) * aspect;
      float r = max(length(q), 0.02);
      float a = atan(q.y, q.x);
      // 1/r is the depth: rings bunch towards the center because that is
      // where the tunnel is furthest away.
      float rings = sin(1.0 / r * 3.0 - t * 0.8) * 0.5 + 0.5;
      float spokes = sin(a * 8.0 + t * 0.15) * 0.5 + 0.5;
      base = ramp(rings * 0.7 * (0.55 + spokes * 0.45) * smoothstep(0.0, 0.35, r));"#,
    },
    Style {
        id: "waves",
        label: "Waves",
        pace: 0.5,
        body: r#"
      vec2 q = (uv - vec2(0.5, 0.35)) * aspect;
      float depth = max(0.75 - uv.y, 0.05);
      float w = sin(q.x * 4.0 + t * 0.4) * 0.5
              + sin(q.x * 7.3 - t * 0.27) * 0.3
              + sin(q.x * 2.1 + t * 0.13) * 0.2;
      float ridge = smoothstep(0.05, 0.0, abs(fract((uv.y + w * 0.06) * 9.0) - 0.5) * depth);
      base = ramp(ridge * 0.8 * smoothstep(0.95, 0.25, uv.y));"#,
    },
    Style {
        id: "sweep",
        label: "Sweep",
        pace: 0.9,
        body: r#"
      // Named band, not d: SHADER_TAIL declares its own float d for the
      // vignette, and a body declaring one too is a redeclaration — the shader
      // fails to compile, programFor returns null, and the style simply never
      // switches, with nothing on screen to say why.
      // (No backticks anywhere in here: this is inside a JS template literal.)
      float band = (uv.x * aspect.x + uv.y) * 0.7;
      base = ramp(0.5 + 0.5 * sin(band * 2.2 - t * 0.5));"#,
    },
    Style {
        id: "static",
        label: "Static",
        pace: 2.0,
        body: r#"
      // Three fields at incommensurable scales, each drifting on its own
      // irrational-ish vector, plus a slow wander of the sampling grid itself.
      //
      // Two fields on whole-number scales still shared a period: 3 and 7 line
      // up every 21 pixels and the eye finds that. These are 3.0, 5.7 and
      // 11.3, and the drift vectors are picked so no two are rational
      // multiples of each other — the combined pattern has no repeat short
      // enough to see. The last term moves the whole lattice, so even a static
      // pixel is sampling somewhere new each frame.
      vec2 wander = vec2(sin(t * 0.13), cos(t * 0.17)) * 40.0;
      float g1 = hash(floor((uv * u_size + wander) / 3.0)  + vec2(t * 37.7, t * 11.3));
      float g2 = hash(floor((uv * u_size + wander) / 5.7)  - vec2(t * 19.1, t * 43.9));
      float g3 = hash(floor((uv * u_size - wander) / 11.3) + vec2(t * 29.3, t * 7.1));
      base = ramp(g1 * 0.34 + g2 * 0.20 + g3 * 0.16 + 0.08);"#,
    },
];

/// What the motion slider sits at before anyone touches it.
///
/// From `ui/js/backdrop.js`, where it is shared across styles and each one's
/// `pace` scales it. The range there is 0 to 7.
pub const DEFAULT_SPEED: f32 = 4.0;

/// Just the ids and labels, for a settings list that must not depend on the
/// shader source being compiled.
pub const STYLE_LIST: &[(&str, &str)] = &[
    ("blobs", "Blobs"),
    ("aurora", "Aurora"),
    ("plasma", "Plasma"),
    ("grid", "Grid"),
    ("stars", "Drift"),
    ("starfield", "Starfield"),
    ("towers", "Towers"),
    ("tunnel", "Tunnel"),
    ("waves", "Waves"),
    ("sweep", "Sweep"),
    ("static", "Static"),
];

/// A named color scheme, as the webview has them.
pub struct Named {
    pub id: &'static str,
    pub label: &'static str,
    pub scheme: Scheme,
    /// What the panels are tinted with. The webview picks this per scheme and
    /// the panels here were a fixed gray regardless — which is most of why they
    /// read as slabs rather than glass.
    pub glass: [f32; 3],
}

/// The schemes, ported from `ui/js/backdrop.js` so the two front ends offer
/// the same names and the same colors. `custom` is deliberately absent: a
/// color picker is miserable on a d-pad, and the handheld drops it.
///
/// The webview stores only two stops and mixes the middle, so the same
/// midpoint is computed here — the ramp then behaves identically.
pub const SCHEMES: &[Named] = &[
    Named {
        id: "midnight",
        label: "Midnight",
        scheme: Scheme {
            low: [0.0431, 0.051, 0.0863],
            mid: [0.1039, 0.1294, 0.2432],
            high: [0.1647, 0.2078, 0.4],
        },
        glass: [0.302, 0.5608, 0.8392],
    },
    Named {
        id: "frost",
        label: "Frost",
        scheme: Scheme {
            low: [0.0431, 0.0588, 0.0784],
            mid: [0.1216, 0.1862, 0.249],
            high: [0.2, 0.3137, 0.4196],
        },
        glass: [0.5608, 0.7216, 0.8471],
    },
    Named {
        id: "abyss",
        label: "Abyss",
        scheme: Scheme {
            low: [0.0235, 0.0353, 0.0471],
            mid: [0.047, 0.1451, 0.1745],
            high: [0.0706, 0.2549, 0.302],
        },
        glass: [0.2275, 0.6275, 0.7098],
    },
    Named {
        id: "moss",
        label: "Moss",
        scheme: Scheme {
            low: [0.0392, 0.0706, 0.0627],
            mid: [0.0804, 0.1804, 0.1392],
            high: [0.1216, 0.2902, 0.2157],
        },
        glass: [0.2471, 0.6196, 0.5255],
    },
    Named {
        id: "ember",
        label: "Ember",
        scheme: Scheme {
            low: [0.0784, 0.0431, 0.0353],
            mid: [0.2196, 0.0921, 0.0647],
            high: [0.3608, 0.1412, 0.0941],
        },
        glass: [0.7843, 0.5294, 0.2353],
    },
    Named {
        id: "rust",
        label: "Rust",
        scheme: Scheme {
            low: [0.0824, 0.0588, 0.0353],
            mid: [0.2255, 0.1431, 0.0628],
            high: [0.3686, 0.2275, 0.0902],
        },
        glass: [0.6902, 0.4157, 0.2078],
    },
    Named {
        id: "wine",
        label: "Wine",
        scheme: Scheme {
            low: [0.0745, 0.0392, 0.0549],
            mid: [0.202, 0.0824, 0.1412],
            high: [0.3294, 0.1255, 0.2275],
        },
        glass: [0.6902, 0.2902, 0.3333],
    },
    Named {
        id: "plum",
        label: "Plum",
        scheme: Scheme {
            low: [0.0706, 0.0392, 0.0863],
            mid: [0.1706, 0.1039, 0.2274],
            high: [0.2706, 0.1686, 0.3686],
        },
        glass: [0.4824, 0.3843, 0.7686],
    },
    Named {
        id: "slate",
        label: "Slate",
        scheme: Scheme {
            low: [0.0588, 0.0667, 0.0745],
            mid: [0.1294, 0.1471, 0.1666],
            high: [0.2, 0.2275, 0.2588],
        },
        glass: [0.4275, 0.4627, 0.5059],
    },
];

/// The panel tint for that scheme, or the first's.
pub fn glass_of(id: &str) -> [f32; 3] {
    SCHEMES.iter().find(|s| s.id == id).map(|s| s.glass).unwrap_or(SCHEMES[0].glass)
}

/// The scheme with that id, or the first.
pub fn scheme(id: &str) -> &'static Scheme {
    SCHEMES
        .iter()
        .find(|s| s.id == id)
        .map(|s| &s.scheme)
        .unwrap_or(&SCHEMES[0].scheme)
}

pub fn style(id: &str) -> &'static Style {
    STYLES.iter().find(|s| s.id == id).unwrap_or(&STYLES[0])
}

/// A color scheme, as the three stops the ramp blends between.
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
            // One dialect now: the context is ours, so it is the one we asked
            // for. This used to have to speak whatever SDL's renderer had
            // made, which on macOS was GLSL 1.20.
            let version = crate::gfx::version_line()?;
            let precision = if version.contains(" es") {
                "precision highp float;\n"
            } else {
                ""
            };
            let fragment = format!(
                "{version}\n{precision}out vec4 color;\n{HEAD}{}{TAIL}",
                chosen.body
            );
            let vertex = format!("{version}\n{VERTEX}");

            let program = crate::gfx::link(&vertex, &fragment)?;

            // One quad, as two triangles covering the clip volume. The vertex
            // shader passes it through: everything interesting happens per
            // fragment.
            let corners: [f32; 12] = [
                -1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0,
            ];
            // Vertex array objects are 3.0 and later. A legacy context binds
            // the buffer and describes it on every draw instead.
            let (mut vao, mut vbo) = (0, 0);
            gl::GenVertexArrays(1, &mut vao);
            gl::BindVertexArray(vao);
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
                // The webview's own default, and it matters: this is
                // multiplied by the style's `pace` and then by the small
                // constants in the body — Blobs drifts at 0.02 of it — so at
                // 1.0 the picture moves and nobody can tell.
                speed: DEFAULT_SPEED,
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
                    "{}: {name} is used by nobody",
                    style.id
                );
            }
            assert!(
                style.body.contains("base ="),
                "{} never fills base",
                style.id
            );
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
        assert!(
            fastest / slowest > 10.0,
            "the paces are all the same, so the slider is a lie"
        );
    }

    /// Where the fragment color goes is the preamble's business and nobody
    /// else's. It was declared in both — the shared frame *and* the modern
    /// preamble — which is a duplicate on one dialect and a compile error on
    /// the other, and the error names a line the body does not contain.
    /// Where the fragment color goes is `build`'s business and nobody
    /// else's. It was declared in both once — the shared frame and the
    /// preamble — which is a duplicate on one dialect and a compile error on
    /// the other, naming a line the body does not contain.
    #[test]
    fn the_shared_frame_does_not_declare_the_output() {
        assert!(
            !HEAD.contains("out vec4"),
            "the frame declares the output as well"
        );
    }

    #[test]
    fn an_unknown_style_falls_back_rather_than_failing() {
        assert_eq!(style("nonsense").id, STYLES[0].id);
        assert_eq!(style("aurora").id, "aurora");
    }
}
