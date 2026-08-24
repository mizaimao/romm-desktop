// An animated shader behind the library.
//
// WebGL2, which every webview this app runs in has had for years — WKWebView on
// macOS, Chromium's WebView2 on Windows, WebKitGTK on Linux. Not WebGPU, which
// is the newer API and still patchy across those three.
//
// It is a single full-screen quad running one fragment shader. There is no
// geometry, no textures and no library: the whole scene is arithmetic on the
// pixel coordinate, which is why it costs almost nothing and needs no
// dependency.
//
// Deliberately quiet. This sits behind cover art that the eye is supposed to be
// on, so it moves slowly and stays dark. A backdrop that competes with the
// artwork is worse than no backdrop.

/// Colors are read from the stylesheet rather than hardcoded, so the shader
/// follows whatever theme is in force instead of being a second place colors
/// have to be kept in sync.
function themeColor(name, fallback) {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const m = raw.match(/^#?([0-9a-f]{6})$/i);
  if (!m) return fallback;
  const n = parseInt(m[1], 16);
  return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

const VERTEX = `#version 300 es
in vec2 pos;
void main() { gl_Position = vec4(pos, 0.0, 1.0); }`;

// Two layers of drifting value-noise, tinted between the theme's own two
// colors, plus a vignette so the edges fall away and the grid stays readable.
/// What every style shares: the uniforms, the noise, and the vignette.
///
/// One program per style would mean five compilations at startup and five
/// times the code that reads a color scheme. Instead each style is a body
/// that fills `base`, spliced into this frame — so they cannot drift apart on
/// how they read `u_strength`, how they darken at the edges, or how they take
/// the two scheme colors.
const SHADER_HEAD = `#version 300 es
precision highp float;

// highp, not mediump, and this is the whole reason Static and Rain looked like
// they repeated every few frames.
//
// hash() below is sin(dot(p, k)) * 43758.5453 taken fract. Static feeds it
// coordinates in the hundreds plus a time term that grows without bound, and
// mediump carries about three decimal digits — so sin of a large argument
// loses every bit that mattered, fract of the result lands on a handful of
// values, and the "noise" becomes a visible band pattern marching across the
// screen. It was never the shape of the effect; it was the arithmetic.
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
  // coordinate has drifted. Time is unbounded here — Static adds t * 37 — and
  // without the wrap the sine is being asked for a value it cannot resolve
  // even at highp after a few minutes of running.
  p = fract(p * vec2(0.3183099, 0.3678794));
  p += dot(p, p + 19.19);
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

// Value noise: hash the four lattice corners and smoothly blend between them.
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
//
// Every style already blends between two colors; this is the same call with
// a stop in the middle, so a two-color scheme sets the middle halfway and
// nothing changes for it. No backticks in here: this whole block is inside a
// JS template literal, and one would end the string.
vec3 ramp(float k) {
  k = clamp(k, 0.0, 1.0);
  return k < 0.5 ? mix(u_low, u_mid, k * 2.0) : mix(u_mid, u_high, (k - 0.5) * 2.0);
}

void main() {
  vec2 uv = gl_FragCoord.xy / u_size;
  vec2 aspect = vec2(u_size.x / u_size.y, 1.0);
  float t = u_time * u_speed;
  vec3 base;
`;

const SHADER_TAIL = `
  // Darker towards the edges, and never fully bright in the middle either. The
  // grid sits on top of all of it and text has to stay readable over the
  // brightest pixel this can produce, not the average one.
  float d = distance(uv, vec2(0.5)) * 1.15;
  base *= 1.0 - smoothstep(0.15, 1.0, d) * 0.75;

  color = vec4(base * u_strength, 1.0);
}`;

/// The styles, as the body each one contributes.
///
/// There was one. "Blobs" is a fair description of it and a poor choice of
/// only option: the whole point of a backdrop is that it suits the room, and
/// one shape suits one room. Each of these takes the same two scheme colors
/// so switching style does not also change the palette.
///
/// `pace` is what the Motion slider means for this style, and it exists
/// because the slider was a lie shared between five shapes. Every body writes
/// its own multipliers on `t`, and they are two decades apart: Blobs drifts at
/// 0.015 of it and Plasma sweeps at 0.31, twenty times faster for the same
/// number on the slider. Setting it where Plasma was watchable left Blobs and
/// Aurora looking like still images. The slider now says how fast *this*
/// backdrop should go, and each style carries the factor that makes that true
/// — so the setting survives switching style, which is the whole point of one
/// slider rather than five.
export const BACKDROPS = [
  {
    id: "blobs",
    label: "Blobs",
    hint: "Soft clouds drifting at two scales. The original, and the quietest.",
    pace: 1.7,
    defaults: { strength: 0.5 },
    body: `
      float a = noise(uv * aspect * 3.0 + vec2(t * 0.02, t * 0.013));
      float b = noise(uv * aspect * 6.0 - vec2(t * 0.011, t * 0.017));
      base = ramp(smoothstep(0.30, 0.85, a * 0.65 + b * 0.35));`,
  },
  {
    id: "aurora",
    label: "Aurora",
    hint: "Slow vertical curtains, brighter where they fold over each other.",
    pace: 1.4,
    defaults: { strength: 0.6 },
    body: `
      float band = uv.y * 2.2 + fbm(vec2(uv.x * 2.0, t * 0.05)) * 1.6;
      float curtain = sin(band * 3.14159) * 0.5 + 0.5;
      curtain *= smoothstep(1.0, 0.15, uv.y);
      base = ramp(pow(curtain, 1.6));`,
  },
  {
    id: "plasma",
    label: "Plasma",
    hint: "Interfering sine waves. The oldest trick on this list, and the one "
      + "that looks most like a demo from 1993.",
    // The one that made the slider a lie: four sines at 0.2–0.4 of `t`, where
    // Blobs drifts at 0.015 of it.
    pace: 0.1,
    defaults: { strength: 0.8 },
    body: `
      vec2 p = (uv - 0.5) * aspect * 4.0;
      float v = sin(p.x + t * 0.35)
              + sin(p.y * 1.3 - t * 0.28)
              + sin((p.x + p.y) * 0.7 + t * 0.2)
              + sin(length(p) * 1.6 - t * 0.4);
      base = ramp(smoothstep(-1.2, 2.4, v));`,
  },
  {
    id: "grid",
    label: "Grid",
    hint: "A horizon and a grid running to it. Steadier than the rest: the "
      + "lines do not move, only the light on them.",
    // `gl_FragCoord.y` counts from the bottom, so the floor is where `uv.y` is
    // *small* — and the old horizon at 0.18 put it below the bottom of the
    // screen. Everything actually on show was the clamp: `max(p.y, 0.04)`
    // froze the perspective divide across the whole lower fifth, so the lines
    // stopped converging at a fixed height and ran straight down from it, and
    // `fwidth` of a frozen coordinate is zero — a divide by zero smeared along
    // that same seam. Horizon in the upper half, depth measured downwards from
    // it, and the divide guarded rather than the input clamped.
    pace: 0.7,
    defaults: { strength: 1.0 },
    body: `
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
      base = ramp(mesh * 0.8 * floorMask + sky * 0.12);`,
  },
  {
    id: "stars",
    label: "Drift",
    hint: "Points of light, sparse and slow. Almost nothing on screen, which "
      + "on an OLED is almost nothing lit.",
    // A star is scattered inside a cell of an invisible grid, and its glow has
    // a radius of 0.42 of a cell — so any star sitting nearer than that to a
    // cell edge had the rest of its glow drawn by the neighbouring cell, which
    // was not asking about it. What showed was a field of points sliced flat
    // along straight lines, always the same lines, because the grid does not
    // move with the drift. Every point has to ask its eight neighbours too.
    pace: 1,
    defaults: { speed: 3.0, strength: 1.0 },
    body: `
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
      base = ramp(min(glow, 1.0));`,
  },
  {
    id: "starfield",
    label: "Starfield",
    hint: "Three layers of stars at different speeds. The attract-mode look, "
      + "and the one that reads as depth without moving much.",
    pace: 1.1,
    defaults: { speed: 3.0, strength: 1.0 },
    body: `
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
      }`,
  },
  {
    id: "tunnel",
    label: "Tunnel",
    hint: "Rings running away from the middle. The oldest perspective trick "
      + "there is, and still the most hypnotic.",
    pace: 0.35,
    defaults: { strength: 0.7 },
    body: `
      vec2 q = (uv - 0.5) * aspect;
      float r = max(length(q), 0.02);
      float a = atan(q.y, q.x);
      // 1/r is the depth: rings bunch towards the centre because that is
      // where the tunnel is furthest away.
      float rings = sin(1.0 / r * 3.0 - t * 0.8) * 0.5 + 0.5;
      float spokes = sin(a * 8.0 + t * 0.15) * 0.5 + 0.5;
      base = ramp(rings * 0.7 * (0.55 + spokes * 0.45) * smoothstep(0.0, 0.35, r));`,
  },
  {
    id: "waves",
    label: "Waves",
    hint: "Ridges seen at a low angle, rolling towards you.",
    pace: 0.5,
    defaults: { strength: 0.7 },
    body: `
      vec2 q = (uv - vec2(0.5, 0.35)) * aspect;
      float depth = max(0.75 - uv.y, 0.05);
      float w = sin(q.x * 4.0 + t * 0.4) * 0.5
              + sin(q.x * 7.3 - t * 0.27) * 0.3
              + sin(q.x * 2.1 + t * 0.13) * 0.2;
      float ridge = smoothstep(0.05, 0.0, abs(fract((uv.y + w * 0.06) * 9.0) - 0.5) * depth);
      base = ramp(ridge * 0.8 * smoothstep(0.95, 0.25, uv.y));`,
  },
  {
    id: "sweep",
    label: "Sweep",
    hint: "A slow diagonal wash. The cheapest thing here — two sines and no "
      + "noise at all — and the quietest behind artwork.",
    pace: 0.9,
    defaults: { speed: 2.0, strength: 0.7 },
    body: `
      // Named band, not d: SHADER_TAIL declares its own float d for the
      // vignette, and a body declaring one too is a redeclaration — the shader
      // fails to compile, programFor returns null, and the style simply never
      // switches, with nothing on screen to say why.
      // (No backticks anywhere in here: this is inside a JS template literal.)
      float band = (uv.x * aspect.x + uv.y) * 0.7;
      base = ramp(0.5 + 0.5 * sin(band * 2.2 - t * 0.5));`,
  },
  {
    id: "static",
    label: "Static",
    hint: "Untuned television. One hash per pixel, so it costs almost nothing "
      + "— and on an OLED it is the busiest thing on this list.",
    pace: 2.0,
    defaults: { speed: 6.0, strength: 1.0 },
    // No motion control. The field is re-hashed from scratch every frame at
    // any speed, so the slider changes the numbers and nothing you can see —
    // and a control that does nothing is worse than no control. The default
    // is the cheapest rate that still looks like static.
    motion: false,
    body: `
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
      base = ramp(g1 * 0.34 + g2 * 0.20 + g3 * 0.16 + 0.08);`,
  },
];

export function backdropStyle(id) {
  return BACKDROPS.find((b) => b.id === id) ?? BACKDROPS[0];
}

function fragmentFor(id) {
  return SHADER_HEAD + backdropStyle(id).body + SHADER_TAIL;
}

function compile(gl, type, src) {
  const sh = gl.createShader(type);
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    console.warn("backdrop shader failed:", gl.getShaderInfoLog(sh));
    gl.deleteShader(sh);
    return null;
  }
  return sh;
}

let running = null;

/// What the backdrop looks like, as the user set it.
///
/// Kept in localStorage rather than config.toml: it is a per-screen preference,
/// and the machine driving a television wants different values from the laptop.
// Strength well below 1. At full it is a bright glow across the middle of the
// screen that the cover art has to compete with — the backdrop's whole job is
// to sit behind the artwork, not next to it.
const DEFAULTS = {
  speed: 4,
  strength: 0.32,
  low: "",
  high: "",
  preset: "midnight",
  style: "blobs",
};

/// Dark palettes, because this sits behind cover art at night on a television.
///
/// Each is a dark base and a slightly-lit accent; the shader blends between
/// them, so the pair is the whole scheme. Kept deliberately low in value — a
/// bright pair produces a backdrop that competes with the artwork no matter
/// what the brightness slider says.
/// The gradient a scheme resolves to, or the user's own on "custom".
///
/// An unknown id falls through to the stored colors as well: schemes can be
/// renamed or dropped, and a settings file naming one that no longer exists
/// should leave the window looking like something rather than nothing.
export function presetColors(cfg) {
  // Every scheme, not just the two-color pairs. This searched `SCHEMES`, so a
  // single color or a spectrum found nothing, returned the config's own empty
  // strings, and fell back to the theme — which is why all eighteen of them
  // rendered identically and looked like the mono scheme.
  const p = ALL_SCHEMES.filter(Boolean).find((x) => x.id === cfg.preset);
  if (!p || p.id === "custom") return { low: cfg.low, mid: cfg.mid, high: cfg.high };
  return { low: p.low, mid: p.mid, high: p.high };
}

/// Motion, as the slider expresses it.
///
/// Below about 3 the drift is too slow to read as movement at all — it looks
/// like a still image with a rendering cost. The slider covers the range that
/// actually differs.
export const SPEED_MIN = 0;
export const SPEED_MAX = 7;

/// Settings for one style, layered over the shared ones.
///
/// Each shape wants different numbers and the single set could not hold them:
/// Scanlines at the brightness that suits Drift is a white screen, and Tunnel
/// at Blobs' speed is a migraine. Stored under the style's own id and layered
/// over the shared values, so anything never touched for a style still follows
/// the shared setting rather than freezing at whatever it was the first time
/// that style was opened.
function perStyle() {
  try {
    return JSON.parse(localStorage.getItem("backdropPerStyle") || "{}");
  } catch {
    return {};
  }
}

/// Store `patch` against `style`, keeping the rest of that style's overrides.
export function saveStyleSettings(style, patch) {
  const all = perStyle();
  all[style] = { ...(all[style] || {}), ...patch };
  localStorage.setItem("backdropPerStyle", JSON.stringify(all));
  const merged = backdropSettings();
  if (live) live(merged);
  window.__TAURI__?.event?.emit?.("backdrop-settings", merged);
  return merged;
}

/// What this style overrides, if anything. Empty object when it follows the
/// shared settings for everything.
export function styleSettings(style) {
  return perStyle()[style] || {};
}

/// What a style starts at, before anyone touches it.
///
/// Some shapes are wrong at the shared numbers and always will be: Scanlines
/// at 32% brightness is a lit grey screen, Static at 32% is a snowstorm over
/// the artwork. The style carries its own answer and the shared settings are
/// the fallback for everything that has no opinion.
export function styleDefaults(style) {
  const own = BACKDROPS.find((b) => b.id === style)?.defaults || {};
  let shared;
  try {
    shared = { ...DEFAULTS, ...JSON.parse(localStorage.getItem("backdropSettings") || "{}") };
  } catch {
    shared = { ...DEFAULTS };
  }
  return { speed: shared.speed, strength: shared.strength, ...own };
}

/// Forget one style's overrides, putting it back on its own defaults.
export function clearStyleSettings(style) {
  const all = perStyle();
  // Back to the style's own defaults rather than to nothing: dropping the
  // overrides entirely would leave Scanlines on the shared brightness, which
  // is the setting it exists to disagree with.
  const own = BACKDROPS.find((b) => b.id === style)?.defaults;
  if (own) all[style] = { ...own };
  else delete all[style];
  localStorage.setItem("backdropPerStyle", JSON.stringify(all));
  const merged = backdropSettings();
  if (live) live(merged);
  window.__TAURI__?.event?.emit?.("backdrop-settings", merged);
  return merged;
}

export function backdropSettings() {
  let stored;
  try {
    stored = { ...DEFAULTS, ...JSON.parse(localStorage.getItem("backdropSettings") || "{}") };
  } catch {
    stored = { ...DEFAULTS };
  }
  // The style's own defaults sit under its overrides, and both over the
  // shared settings.
  const own = BACKDROPS.find((b) => b.id === stored.style)?.defaults || {};
  stored = { ...stored, ...own, ...(perStyle()[stored.style] || {}) };
  // Only a ceiling now. The floor was 3 — 300% — which is why the motion
  // slider refused to go below that however far it was dragged, and why a
  // style whose default is slower could not keep it.
  const raw = Number(stored.speed);
  stored.speed = Number.isFinite(raw) ? Math.max(0, raw) : DEFAULTS.speed;
  return stored;
}

/// Apply settings to the running shader. Does not store and does not announce.
///
/// The receiving end of the event, and separate from `saveBackdropSettings` for
/// exactly that reason: the listener used to call the saving version, which
/// emitted again, which the emitting window also received. Every drag of a
/// color slider fed itself back round and the backdrop flickered.
export function applyBackdropSettings(cfg) {
  if (live) live({ ...backdropSettings(), ...(cfg || {}) });
}

export function saveBackdropSettings(next) {
  // Built from the *shared* settings, never from `backdropSettings()`: that
  // one layers the current style's overrides on top, so saving anything would
  // copy those overrides into the shared values and every later style would
  // inherit them. Switching from Tunnel to Rain carried Tunnel's brightness
  // across and then kept it for good.
  let shared;
  try {
    shared = { ...DEFAULTS, ...JSON.parse(localStorage.getItem("backdropSettings") || "{}") };
  } catch {
    shared = { ...DEFAULTS };
  }
  const merged = { ...shared, ...next };
  localStorage.setItem("backdropSettings", JSON.stringify(merged));
  // What the shader is given still includes the chosen style's overrides.
  const effective = backdropSettings();
  // Applied live rather than on restart: a color picker you cannot see the
  // result of is a color picker nobody can use.
  if (live) live(effective);
  // ...and told to the other window, because the controls live in Settings and
  // the shader lives in the library. Calling startBackdrop from the settings
  // window put the canvas in *that* document, so changing the app's background
  // changed the background of the settings panel and nothing else.
  window.__TAURI__?.event?.emit?.("backdrop-settings", effective);
  return effective;
}

/// Whether the library window is showing a backdrop.
///
/// Read from storage rather than from `running`, so the settings window — which
/// never renders one — reports the state of the window that does. localStorage
/// is shared between them; they are the same origin.
export function backdropWanted() {
  return localStorage.getItem("backdrop") === "on";
}

/// Turn it on or off in whichever window renders the library.
export function setBackdropWanted(on) {
  localStorage.setItem("backdrop", on ? "on" : "off");
  window.__TAURI__?.event?.emit?.("backdrop-toggle", on);
  // In the library window itself, act immediately rather than waiting for the
  // event to come back around.
  if (document.getElementById("list")) {
    if (on) startBackdrop();
    else stopBackdrop();
  }
  return on;
}

/// Set while running, so settings changes reach the shader without restarting
/// it — recreating the GL context on every slider tick would stutter.
let live = null;

function rgb(hex, fallbackVar, fallbackRgb) {
  const m = String(hex || "").match(/^#?([0-9a-f]{6})$/i);
  if (m) {
    const n = parseInt(m[1], 16);
    return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
  }
  return themeColor(fallbackVar, fallbackRgb);
}

/// Start the backdrop. Returns a stop function, or null when it could not run —
/// an old driver, a software renderer, a webview with WebGL switched off. The
/// app is fully usable without it, so every failure here is silent.
export function startBackdrop() {
  // A measuring switch, never set in normal use. The shader runs on a canvas
  // the size of the window and never stops, so it is one of the few things in
  // the app that could plausibly cost tens of megabytes on its own — and the
  // only way to know is to be able to turn it off.
  if (globalThis.__ROMM_FLAGS?.includes("no-backdrop")) return;
  try {
    return build();
  } catch (e) {
    // The module says every failure in here is silent, and for one version it
    // was not: a reference error thrown out of this function escaped into the
    // init that called it and took the tab row, the settings button and the
    // page background with it. Decoration cannot be allowed to do that,
    // whatever mistake is in it.
    console.warn("backdrop failed to start:", e);
    return null;
  }
}

function build() {
  if (running) return running;

  const canvas = document.createElement("canvas");
  canvas.id = "backdrop";
  const gl = canvas.getContext("webgl2", {
    antialias: false,
    // The backdrop is drawn once per frame and never read back; telling the
    // driver so lets it skip preserving the buffer between frames.
    preserveDrawingBuffer: false,
    powerPreference: "low-power",
  });
  if (!gl) return null;

  // One program per style, built when that style is first asked for and kept:
  // switching back and forth in Settings should not recompile a shader every
  // time, and five of them is a few kilobytes of driver state.
  const programs = new Map();
  const vs = compile(gl, gl.VERTEX_SHADER, VERTEX);
  if (!vs) return null;

  const programFor = (id) => {
    if (programs.has(id)) return programs.get(id);
    const fs = compile(gl, gl.FRAGMENT_SHADER, fragmentFor(id));
    if (!fs) return programs.get("blobs") ?? null;
    const p = gl.createProgram();
    gl.attachShader(p, vs);
    gl.attachShader(p, fs);
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
      console.warn("backdrop link failed:", gl.getProgramInfoLog(p));
      return null;
    }
    programs.set(id, p);
    return p;
  };

  let styleId = backdropSettings().style ?? "blobs";
  const prog = programFor(styleId);
  if (!prog) return null;
  gl.useProgram(prog);

  // One triangle covering the viewport. A triangle rather than two for a quad:
  // fewer vertices and no seam down the diagonal.
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  // Uniform locations belong to a program, so they are looked up per program
  // rather than once: a location from one is meaningless in another, and the
  // symptom of getting that wrong is a backdrop that goes black on the second
  // style you try.
  let active = prog;
  const bind = (p) => {
    gl.useProgram(p);
    const loc = gl.getAttribLocation(p, "pos");
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    return {
      size: gl.getUniformLocation(p, "u_size"),
      time: gl.getUniformLocation(p, "u_time"),
      low: gl.getUniformLocation(p, "u_low"),
      mid: gl.getUniformLocation(p, "u_mid"),
      high: gl.getUniformLocation(p, "u_high"),
      strength: gl.getUniformLocation(p, "u_strength"),
      speed: gl.getUniformLocation(p, "u_speed"),
    };
  };
  let u = bind(prog);

  // Declared before `apply`, which calls it.
  //
  // It was the other way round for one version, and `const` bindings are in
  // the dead zone until their line runs — so the first call threw
  // ReferenceError, out of startBackdrop, out of the init that called it, and
  // took the tab row, the settings button and everything else after it with
  // it. A backdrop is decoration; it managed to be fatal.
  let sized = false;
  const resize = () => {
    // Half resolution: this is out-of-focus noise, and full-resolution costs
    // four times the pixels for something nobody is looking directly at.
    const scale = 0.5;
    canvas.width = Math.max(2, Math.floor(window.innerWidth * scale));
    canvas.height = Math.max(2, Math.floor(window.innerHeight * scale));
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.useProgram(active);
    gl.uniform2f(u.size, canvas.width, canvas.height);
    sized = true;
  };

  const apply = (cfg) => {
    const want = cfg.style ?? "blobs";
    if (want !== styleId) {
      const next = programFor(want);
      if (next) {
        styleId = want;
        active = next;
        u = bind(next);
        sized = false;
      }
    }
    // A preset supplies the pair; "custom" falls through to the user's own, and
    // an unset custom color falls through again to the theme's — so the
    // default follows the palette rather than being a second place to maintain.
    const { low, mid, high } = presetColors(cfg);
    gl.useProgram(active);
    // A two-color scheme has no middle, so the midpoint of the pair stands in
    // and the ramp behaves exactly as the old two-stop mix did.
    const lo = rgb(low, "--bg", [0.05, 0.05, 0.07]);
    const hi = rgb(high, "--accent", [0.18, 0.2, 0.36]);
    const md = mid ? rgb(mid, "--accent", hi) : lo.map((v, i) => (v + hi[i]) / 2);
    gl.uniform3fv(u.low, lo);
    gl.uniform3fv(u.mid, md);
    gl.uniform3fv(u.high, hi);
    gl.uniform1f(u.strength, cfg.strength);
    // Scaled by the style's own pace, not handed over raw: one number on the
    // slider has to mean the same amount of movement whichever shape is drawing.
    gl.uniform1f(u.speed, cfg.speed * (backdropStyle(styleId).pace ?? 1));
    if (!sized) resize();
  };
  apply(backdropSettings());
  live = apply;

  resize();
  window.addEventListener("resize", resize);

  let frame = 0;
  let stopped = false;
  const start = performance.now();
  let lastDraw = 0;

  // A drifting gradient at 30fps and the same gradient at 120fps are the same
  // picture. On a ProMotion display the loop was running four times faster than
  // anything in it changes, which is four times the GPU for no difference —
  // and this thing is on screen the entire time the app is open.
  const MIN_GAP_MS = 1000 / 30;

  const draw = (now) => {
    if (stopped) return;
    frame = requestAnimationFrame(draw);
    if (now - lastDraw < MIN_GAP_MS) return;
    lastDraw = now;
    gl.useProgram(active);
    gl.uniform1f(u.time, (now - start) / 1000);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };

  // Nothing to draw for while the window is not on screen. WebKit does not
  // reliably throttle an occluded window's animation frames here, so an app
  // left open behind something else kept a shader running at full rate for as
  // long as it was open — which is most of the day.
  const onVisibility = () => {
    if (document.hidden) {
      cancelAnimationFrame(frame);
    } else if (!stopped) {
      // Reset the clock so the gradient carries on from where it was rather
      // than jumping forward by however long the window was covered.
      lastDraw = 0;
      frame = requestAnimationFrame(draw);
    }
  };
  document.addEventListener("visibilitychange", onVisibility);

  document.body.prepend(canvas);
  // Without this the page's own opaque background sits on top of the canvas and
  // the shader renders perfectly where nobody can see it.
  document.documentElement.classList.add("backdrop-on");
  frame = requestAnimationFrame(draw);

  running = () => {
    stopped = true;
    cancelAnimationFrame(frame);
    document.removeEventListener("visibilitychange", onVisibility);
    window.removeEventListener("resize", resize);
    canvas.remove();
    document.documentElement.classList.remove("backdrop-on");
    live = null;
    running = null;
  };
  return running;
}

export function stopBackdrop() {
  if (running) running();
}


/// Whether this machine can run it at all, without starting anything.
///
/// Used by Settings to say "not supported here" rather than offering a switch
/// that silently does nothing.
export function backdropSupported() {
  try {
    return !!document.createElement("canvas").getContext("webgl2");
  } catch {
    return false;
  }
}

// ---- Glass tint -----------------------------------------------------------
//
// Vista's "Window Color and Appearance" let you pick the color of the glass,
// and that choice is most of why two Vista machines looked different from each
// other. Same idea: one color drives the bars, the button gel, the hover glow
// and the focus ring, because in Aero they were all the same light.

/// One palette for both surfaces.
///
/// The glass tint and the shader backdrop were two dropdowns of seven and eight
/// colors, chosen separately, and every sensible combination was a pair that
/// already matched — "Aero blue" glass over the "Midnight" backdrop, "Jade"
/// over "Moss". Two controls whose only correct settings are a diagonal of the
/// grid they span is one control.
///
/// `glass` tints the cards, the selection glow and the controls; `low` and
/// `high` are the two ends of the gradient the shader draws. Custom keeps all
/// three separately settable, because someone who wants an unmatched pair
/// should still be able to have one.
export const SCHEMES = [
  { id: "midnight", label: "Midnight", glass: "#4d8fd6", low: "#0b0d16", high: "#2a3566" },
  { id: "frost",    label: "Frost",    glass: "#8fb8d8", low: "#0b0f14", high: "#33506b" },
  { id: "abyss",    label: "Abyss",    glass: "#3aa0b5", low: "#06090c", high: "#12414d" },
  { id: "moss",     label: "Moss",     glass: "#3f9e86", low: "#0a1210", high: "#1f4a37" },
  { id: "ember",    label: "Ember",    glass: "#c8873c", low: "#140b09", high: "#5c2418" },
  { id: "rust",     label: "Rust",     glass: "#b06a35", low: "#150f09", high: "#5e3a17" },
  { id: "wine",     label: "Wine",     glass: "#b04a55", low: "#130a0e", high: "#54203a" },
  { id: "plum",     label: "Plum",     glass: "#7b62c4", low: "#120a16", high: "#452b5e" },
  { id: "slate",    label: "Slate",    glass: "#6d7681", low: "#0f1113", high: "#333a42" },
  { id: "custom",   label: "Custom",   glass: null,      low: null,      high: null },
];

/// Three-color schemes, kept apart from the pairs above.
///
/// A gradient through a third color is a different thing from a tint between
/// two: `sunset` is not "orange with more orange", it is red through orange to
/// yellow, and the middle is what makes it read as a sweep. Named after what
/// they look like rather than after a vendor's lighting preset.
export const TRIPLES = [
  { id: "sunset",   label: "Sunset",   glass: "#e0794a", low: "#150a12", mid: "#7a2540", high: "#e8a24a" },
  { id: "vapor",    label: "Vapourwave", glass: "#ff77c8", low: "#160a2a", mid: "#8a2b8f", high: "#39d7e8" },
  { id: "aurora3",  label: "Aurora",   glass: "#54d6a0", low: "#04120f", mid: "#166b57", high: "#8ef0c0" },
  { id: "ember3",   label: "Furnace",  glass: "#ff8a3d", low: "#160604", mid: "#8f2a09", high: "#ffc44d" },
  { id: "ocean",    label: "Deep water", glass: "#3fa9d8", low: "#03080f", mid: "#0d3a5e", high: "#63d0f0" },
  { id: "spectrum", label: "Spectrum", glass: "#7c8cff", low: "#2a0d4a", mid: "#0d4a86", high: "#3fbf8f" },
  { id: "magma",    label: "Magma",    glass: "#e8543d", low: "#0a0406", mid: "#5e1020", high: "#ff9a3c" },
  { id: "toxic",    label: "Toxic",    glass: "#9ee84a", low: "#07110a", mid: "#2f6b17", high: "#d4ff5c" },
];

/// Full-spectrum sweeps, the kind RGB lighting software ships as presets.
///
/// These run right round the hue circle rather than between two ends of it,
/// which the three-stop ramp can only approximate — low, mid and high are
/// picked so the two halves of the ramp cover opposite sides of the wheel and
/// the result reads as a rainbow rather than as a gradient.
export const RAINBOWS = [
  // Not the three raw primaries. #e02020 / #20c040 / #2060e0 is a television
  // test card: fully saturated red, green and blue have no common ground, so
  // the ramp between them passes through mud rather than through a hue sweep.
  // Pulled off the corners and matched in lightness, which is what makes the
  // sweep read as one thing.
  { id: "rgb",      label: "RGB",        glass: "#5f8fe0", low: "#e2445c", mid: "#3fc98a", high: "#5a7fe8" },
  { id: "prism",    label: "Prism",      glass: "#8f5fd8", low: "#c01ad0", mid: "#20b0e0", high: "#e8d020" },
  { id: "neon",     label: "Neon",       glass: "#ff4fd8", low: "#ff2bd6", mid: "#00e5ff", high: "#b026ff" },
  { id: "candy",    label: "Candy",      glass: "#ff7ab8", low: "#ff5fa2", mid: "#ffd166", high: "#5fd3ff" },
  { id: "heat",     label: "Heat map",   glass: "#ff9a3c", low: "#1020a0", mid: "#20c060", high: "#ff3020" },
  { id: "pastel",   label: "Pastel",     glass: "#b8a6e8", low: "#f2a6c2", mid: "#a6e8c8", high: "#a6c2f2" },
];

/// Every scheme the dropdown offers, pairs first.
export const ALL_SCHEMES = [
  ...SCHEMES.filter((s) => s.id !== "custom"),
  ...TRIPLES,
  ...RAINBOWS,
  SCHEMES.find((s) => s.id === "custom"),
];

/// The groups the dropdown draws, so the list of forty is browsable.
export const SCHEME_GROUPS = [
  ["Pairs", SCHEMES.filter((s) => s.id !== "custom")],
  ["Spectrums", TRIPLES],
  ["Rainbows", RAINBOWS],
  ["Your own", [SCHEMES.find((s) => s.id === "custom")]],
];


const GLASS_KEY = "glassTint";
const TINT_KEY = "glassStrength";

/// How opaque the glass is, as a percentage — every card, the selected row,
/// the cover art behind a game, and the preview pane, which is one of them.
///
/// This is the transparency control, and calling it "tint strength" was the
/// reason nobody could find it. `--tint` is the *opacity* the surfaces mix
/// their color in at and it has never been anything else: there are five
/// `var(--tint)` in the stylesheet and all five are the percentage in a
/// `color-mix` against `transparent`. So the preview pane's own second slider
/// was a second control over one idea, which is how the two surfaces came to
/// disagree. One number, every sheet of glass in the window.
///
/// Read by asking whether the key is there rather than what it coerces to:
/// `n > 0` kept an unset key from reading as zero, but it also threw away a
/// deliberate zero, so dragging the glass to clear put it back to 18 on the
/// next start.
export function glassStrength() {
  const raw = localStorage.getItem(TINT_KEY);
  if (raw === null) return 18;
  const n = Number(raw);
  return Number.isFinite(n) && n >= 0 ? Math.min(60, n) : 18;
}

export function setGlassStrength(pct, { announce = true } = {}) {
  const value = Math.max(0, Math.min(60, Math.round(Number(pct) || 0)));
  document.documentElement.style.setProperty("--tint", `${value}%`);
  if (announce) {
    localStorage.setItem(TINT_KEY, String(value));
    window.__TAURI__?.event?.emit?.("glass-strength", value);
  }
  return value;
}

export function glassTint() {
  // The first scheme's glass when nothing has been chosen. This read
  // GLASS_PRESETS, which was deleted when the two color dropdowns were merged
  // — so on a machine with no stored tint, which is every new install, the
  // first call threw before anything had been painted.
  return localStorage.getItem(GLASS_KEY) || SCHEMES[0].glass;
}

/// Apply the tint to this document, and tell the other window.
///
/// Both windows want it: the library has the cards, Settings has its own
/// controls. A tint applied in one and not the other is worse than no tint,
/// because it looks like a bug.
export function setGlassTint(color, { announce = true } = {}) {
  const value = /^#[0-9a-f]{6}$/i.test(color) ? color : SCHEMES[0].glass;
  document.documentElement.style.setProperty("--glass", value);
  if (announce) {
    localStorage.setItem(GLASS_KEY, value);
    window.__TAURI__?.event?.emit?.("glass-tint", value);
  }
  return value;
}

/// Called at startup in every window with bars and cards to tint.
export function applyStoredGlassTint() {
  setGlassTint(glassTint(), { announce: false });
  setGlassStrength(glassStrength(), { announce: false });
}

