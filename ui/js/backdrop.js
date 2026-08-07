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

/// Colours are read from the stylesheet rather than hardcoded, so the shader
/// follows whatever theme is in force instead of being a second place colours
/// have to be kept in sync.
function themeColour(name, fallback) {
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
// colours, plus a vignette so the edges fall away and the grid stays readable.
const FRAGMENT = `#version 300 es
precision mediump float;
out vec4 colour;

uniform vec2  u_size;
uniform float u_time;
uniform vec3  u_low;
uniform vec3  u_high;
uniform float u_strength;
uniform float u_speed;

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

// Value noise: hash the four lattice corners and smoothly blend between them.
float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), f.x),
             mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), f.x), f.y);
}

void main() {
  vec2 uv = gl_FragCoord.xy / u_size;
  vec2 aspect = vec2(u_size.x / u_size.y, 1.0);

  // Two layers at different scales and speeds, so the motion never reads as one
  // repeating shape.
  float t = u_time * u_speed;
  float a = noise(uv * aspect * 3.0 + vec2(t * 0.02, t * 0.013));
  float b = noise(uv * aspect * 6.0 - vec2(t * 0.011, t * 0.017));
  float n = a * 0.65 + b * 0.35;

  vec3 base = mix(u_low, u_high, smoothstep(0.30, 0.85, n));

  // Darker towards the edges, and never fully bright in the middle either. The
  // grid sits on top of all of it and text has to stay readable over the
  // brightest pixel this can produce, not the average one.
  float d = distance(uv, vec2(0.5)) * 1.15;
  base *= 1.0 - smoothstep(0.15, 1.0, d) * 0.75;

  colour = vec4(base * u_strength, 1.0);
}`;

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
const DEFAULTS = { speed: 4, strength: 0.32, low: "", high: "", preset: "midnight" };

/// Dark palettes, because this sits behind cover art at night on a television.
///
/// Each is a dark base and a slightly-lit accent; the shader blends between
/// them, so the pair is the whole scheme. Kept deliberately low in value — a
/// bright pair produces a backdrop that competes with the artwork no matter
/// what the brightness slider says.
export const PRESETS = [
  { id: "midnight",  label: "Midnight",  low: "#0b0d16", high: "#2a3566" },
  { id: "ember",     label: "Ember",     low: "#140b09", high: "#5c2418" },
  { id: "moss",      label: "Moss",      low: "#0a1210", high: "#1f4a37" },
  { id: "plum",      label: "Plum",      low: "#120a16", high: "#452b5e" },
  { id: "slate",     label: "Slate",     low: "#0f1113", high: "#333a42" },
  { id: "rust",      label: "Rust",      low: "#150f09", high: "#5e3a17" },
  { id: "abyss",     label: "Abyss",     low: "#06090c", high: "#12414d" },
  { id: "wine",      label: "Wine",      low: "#130a0e", high: "#54203a" },
  { id: "custom",    label: "Custom",    low: null,      high: null },
];

/// The colours a preset resolves to, or the user's own for "custom".
export function presetColours(cfg) {
  const p = PRESETS.find((x) => x.id === cfg.preset);
  if (!p || p.id === "custom") return { low: cfg.low, high: cfg.high };
  return { low: p.low, high: p.high };
}

/// Motion, as the slider expresses it.
///
/// Below about 3 the drift is too slow to read as movement at all — it looks
/// like a still image with a rendering cost. The slider covers the range that
/// actually differs.
export const SPEED_MIN = 3;
export const SPEED_MAX = 7;

export function backdropSettings() {
  let stored;
  try {
    stored = { ...DEFAULTS, ...JSON.parse(localStorage.getItem("backdropSettings") || "{}") };
  } catch {
    stored = { ...DEFAULTS };
  }
  // Settings saved before the scale changed hold values below the new floor,
  // which would leave the slider pinned at one end showing a speed it cannot
  // express.
  stored.speed = Math.min(SPEED_MAX, Math.max(SPEED_MIN, Number(stored.speed) || DEFAULTS.speed));
  return stored;
}

/// Apply settings to the running shader. Does not store and does not announce.
///
/// The receiving end of the event, and separate from `saveBackdropSettings` for
/// exactly that reason: the listener used to call the saving version, which
/// emitted again, which the emitting window also received. Every drag of a
/// colour slider fed itself back round and the backdrop flickered.
export function applyBackdropSettings(cfg) {
  if (live) live({ ...backdropSettings(), ...(cfg || {}) });
}

export function saveBackdropSettings(next) {
  const merged = { ...backdropSettings(), ...next };
  localStorage.setItem("backdropSettings", JSON.stringify(merged));
  // Applied live rather than on restart: a colour picker you cannot see the
  // result of is a colour picker nobody can use.
  if (live) live(merged);
  // ...and told to the other window, because the controls live in Settings and
  // the shader lives in the library. Calling startBackdrop from the settings
  // window put the canvas in *that* document, so changing the app's background
  // changed the background of the settings panel and nothing else.
  window.__TAURI__?.event?.emit?.("backdrop-settings", merged);
  return merged;
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
  return themeColour(fallbackVar, fallbackRgb);
}

/// Start the backdrop. Returns a stop function, or null when it could not run —
/// an old driver, a software renderer, a webview with WebGL switched off. The
/// app is fully usable without it, so every failure here is silent.
export function startBackdrop() {
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

  const vs = compile(gl, gl.VERTEX_SHADER, VERTEX);
  const fs = compile(gl, gl.FRAGMENT_SHADER, FRAGMENT);
  if (!vs || !fs) return null;

  const prog = gl.createProgram();
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    console.warn("backdrop link failed:", gl.getProgramInfoLog(prog));
    return null;
  }
  gl.useProgram(prog);

  // One triangle covering the viewport. A triangle rather than two for a quad:
  // fewer vertices and no seam down the diagonal.
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, "pos");
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);

  const uSize = gl.getUniformLocation(prog, "u_size");
  const uTime = gl.getUniformLocation(prog, "u_time");
  const uLow = gl.getUniformLocation(prog, "u_low");
  const uHigh = gl.getUniformLocation(prog, "u_high");
  const uStrength = gl.getUniformLocation(prog, "u_strength");
  const uSpeed = gl.getUniformLocation(prog, "u_speed");

  const apply = (cfg) => {
    // A preset supplies the pair; "custom" falls through to the user's own, and
    // an unset custom colour falls through again to the theme's — so the
    // default follows the palette rather than being a second place to maintain.
    const { low, high } = presetColours(cfg);
    gl.useProgram(prog);
    gl.uniform3fv(uLow, rgb(low, "--bg", [0.05, 0.05, 0.07]));
    gl.uniform3fv(uHigh, rgb(high, "--accent", [0.18, 0.2, 0.36]));
    gl.uniform1f(uStrength, cfg.strength);
    gl.uniform1f(uSpeed, cfg.speed);
  };
  apply(backdropSettings());
  live = apply;

  const resize = () => {
    // Half resolution: this is out-of-focus noise, and full-resolution costs
    // four times the pixels for something nobody is looking directly at.
    const scale = 0.5;
    canvas.width = Math.max(2, Math.floor(window.innerWidth * scale));
    canvas.height = Math.max(2, Math.floor(window.innerHeight * scale));
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.uniform2f(uSize, canvas.width, canvas.height);
  };
  resize();
  window.addEventListener("resize", resize);

  let frame = 0;
  let stopped = false;
  const start = performance.now();
  const draw = (now) => {
    if (stopped) return;
    frame = requestAnimationFrame(draw);
    gl.uniform1f(uTime, (now - start) / 1000);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };

  document.body.prepend(canvas);
  // Without this the page's own opaque background sits on top of the canvas and
  // the shader renders perfectly where nobody can see it.
  document.documentElement.classList.add("backdrop-on");
  frame = requestAnimationFrame(draw);

  running = () => {
    stopped = true;
    cancelAnimationFrame(frame);
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

/// Is the backdrop currently on screen?
export function backdropRunning() {
  return running !== null;
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
// Vista's "Window Color and Appearance" let you pick the colour of the glass,
// and that choice is most of why two Vista machines looked different from each
// other. Same idea: one colour drives the bars, the button gel, the hover glow
// and the focus ring, because in Aero they were all the same light.

export const GLASS_PRESETS = [
  { id: "aero",     label: "Aero blue",  colour: "#4d8fd6" },
  { id: "frost",    label: "Frost",      colour: "#8fb8d8" },
  { id: "graphite", label: "Graphite",   colour: "#6d7681" },
  { id: "jade",     label: "Jade",       colour: "#3f9e86" },
  { id: "amber",    label: "Amber",      colour: "#c8873c" },
  { id: "ruby",     label: "Ruby",       colour: "#b04a55" },
  { id: "violet",   label: "Violet",     colour: "#7b62c4" },
];

const GLASS_KEY = "glassTint";

export function glassTint() {
  return localStorage.getItem(GLASS_KEY) || GLASS_PRESETS[0].colour;
}

/// Apply the tint to this document, and tell the other window.
///
/// Both windows want it: the library has the bars, and Settings has the same
/// controls in its own document. A tint applied in one and not the other is
/// worse than no tint, because it looks like a bug.
export function setGlassTint(colour, { announce = true } = {}) {
  const value = /^#[0-9a-f]{6}$/i.test(colour) ? colour : GLASS_PRESETS[0].colour;
  document.documentElement.style.setProperty("--glass", value);
  if (announce) {
    localStorage.setItem(GLASS_KEY, value);
    window.__TAURI__?.event?.emit?.("glass-tint", value);
  }
  return value;
}

/// Called at startup in every window that has chrome to tint.
export function applyStoredGlassTint() {
  setGlassTint(glassTint(), { announce: false });
}
