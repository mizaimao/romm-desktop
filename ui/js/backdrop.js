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
/// What every style shares: the uniforms, the noise, and the vignette.
///
/// One program per style would mean five compilations at startup and five
/// times the code that reads a colour scheme. Instead each style is a body
/// that fills `base`, spliced into this frame — so they cannot drift apart on
/// how they read `u_strength`, how they darken at the edges, or how they take
/// the two scheme colours.
const SHADER_HEAD = `#version 300 es
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

// Two octaves is enough for a backdrop and half the cost of four. This is
// drawn behind a library, not in a demo.
float fbm(vec2 p) {
  return noise(p) * 0.65 + noise(p * 2.1 + 4.7) * 0.35;
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

  colour = vec4(base * u_strength, 1.0);
}`;

/// The styles, as the body each one contributes.
///
/// There was one. "Blobs" is a fair description of it and a poor choice of
/// only option: the whole point of a backdrop is that it suits the room, and
/// one shape suits one room. Each of these takes the same two scheme colours
/// so switching style does not also change the palette.
export const BACKDROPS = [
  {
    id: "blobs",
    label: "Blobs",
    hint: "Soft clouds drifting at two scales. The original, and the quietest.",
    body: `
      float a = noise(uv * aspect * 3.0 + vec2(t * 0.02, t * 0.013));
      float b = noise(uv * aspect * 6.0 - vec2(t * 0.011, t * 0.017));
      base = mix(u_low, u_high, smoothstep(0.30, 0.85, a * 0.65 + b * 0.35));`,
  },
  {
    id: "aurora",
    label: "Aurora",
    hint: "Slow vertical curtains, brighter where they fold over each other.",
    body: `
      float band = uv.y * 2.2 + fbm(vec2(uv.x * 2.0, t * 0.05)) * 1.6;
      float curtain = sin(band * 3.14159) * 0.5 + 0.5;
      curtain *= smoothstep(1.0, 0.15, uv.y);
      base = mix(u_low, u_high, pow(curtain, 1.6));`,
  },
  {
    id: "plasma",
    label: "Plasma",
    hint: "Interfering sine waves. The oldest trick on this list, and the one "
      + "that looks most like a demo from 1993.",
    body: `
      vec2 p = (uv - 0.5) * aspect * 4.0;
      float v = sin(p.x + t * 0.35)
              + sin(p.y * 1.3 - t * 0.28)
              + sin((p.x + p.y) * 0.7 + t * 0.2)
              + sin(length(p) * 1.6 - t * 0.4);
      base = mix(u_low, u_high, smoothstep(-1.2, 2.4, v));`,
  },
  {
    id: "grid",
    label: "Grid",
    hint: "A horizon and a grid running to it. Steadier than the rest: the "
      + "lines do not move, only the light on them.",
    body: `
      vec2 p = (uv - vec2(0.5, 0.18)) * aspect;
      float horizon = smoothstep(0.0, 0.55, p.y);
      vec2 g = vec2(p.x / max(p.y, 0.04), 1.0 / max(p.y, 0.04) + t * 0.25);
      vec2 line = abs(fract(g * 2.0) - 0.5) / fwidth(g * 2.0);
      float mesh = 1.0 - min(min(line.x, line.y), 1.0);
      base = mix(u_low, u_high, mesh * 0.8 * (1.0 - horizon) + (1.0 - horizon) * 0.12);`,
  },
  {
    id: "stars",
    label: "Drift",
    hint: "Points of light, sparse and slow. Almost nothing on screen, which "
      + "on an OLED is almost nothing lit.",
    body: `
      vec2 p = uv * aspect * 18.0 + vec2(0.0, t * 0.08);
      vec2 cell = floor(p);
      float star = hash(cell);
      vec2 pos = fract(p) - vec2(hash(cell + 3.1), hash(cell + 7.7));
      float glow = smoothstep(0.42, 0.0, length(pos)) * step(0.955, star);
      float twinkle = 0.6 + 0.4 * sin(t * 1.6 + star * 40.0);
      base = mix(u_low, u_high, glow * twinkle);`,
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
/// An unknown id falls through to the stored colours as well: schemes can be
/// renamed or dropped, and a settings file naming one that no longer exists
/// should leave the window looking like something rather than nothing.
export function presetColours(cfg) {
  const p = SCHEMES.find((x) => x.id === cfg.preset);
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
      high: gl.getUniformLocation(p, "u_high"),
      strength: gl.getUniformLocation(p, "u_strength"),
      speed: gl.getUniformLocation(p, "u_speed"),
    };
  };
  let u = bind(prog);

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
    // an unset custom colour falls through again to the theme's — so the
    // default follows the palette rather than being a second place to maintain.
    const { low, high } = presetColours(cfg);
    gl.useProgram(active);
    gl.uniform3fv(u.low, rgb(low, "--bg", [0.05, 0.05, 0.07]));
    gl.uniform3fv(u.high, rgb(high, "--accent", [0.18, 0.2, 0.36]));
    gl.uniform1f(u.strength, cfg.strength);
    gl.uniform1f(u.speed, cfg.speed);
    if (!sized) resize();
  };
  let sized = false;
  apply(backdropSettings());
  live = apply;

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
// Vista's "Window Color and Appearance" let you pick the colour of the glass,
// and that choice is most of why two Vista machines looked different from each
// other. Same idea: one colour drives the bars, the button gel, the hover glow
// and the focus ring, because in Aero they were all the same light.

/// One palette for both surfaces.
///
/// The glass tint and the shader backdrop were two dropdowns of seven and eight
/// colours, chosen separately, and every sensible combination was a pair that
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


const GLASS_KEY = "glassTint";
const TINT_KEY = "glassStrength";
const CLEAR_KEY = "paneClarity";

/// How much of the tint shows through the glass, as a percentage.
export function glassStrength() {
  const n = Number(localStorage.getItem(TINT_KEY));
  return Number.isFinite(n) && n > 0 ? n : 18;
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
  // GLASS_PRESETS, which was deleted when the two colour dropdowns were merged
  // — so on a machine with no stored tint, which is every new install, the
  // first call threw before anything had been painted.
  return localStorage.getItem(GLASS_KEY) || SCHEMES[0].glass;
}

/// Apply the tint to this document, and tell the other window.
///
/// Both windows want it: the library has the cards, Settings has its own
/// controls. A tint applied in one and not the other is worse than no tint,
/// because it looks like a bug.
export function setGlassTint(colour, { announce = true } = {}) {
  const value = /^#[0-9a-f]{6}$/i.test(colour) ? colour : SCHEMES[0].glass;
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

/// How much of what is behind the preview pane shows through it, 0–80%.
///
/// The pane was the last solid surface in the window: every card, the header
/// and the tab row are glass over whatever is behind them, and then a
/// 460-pixel slab of flat panel down the right-hand side. Capped at 80 because
/// past that the text has nothing to sit on — a preview you cannot read is not
/// a preview, and the artwork behind it is not what you opened it for.
export function paneClarity() {
  const n = Number(localStorage.getItem(CLEAR_KEY));
  return Number.isFinite(n) && n >= 0 ? Math.min(80, n) : 55;
}

export function setPaneClarity(pct, { announce = true } = {}) {
  const value = Math.max(0, Math.min(80, Math.round(Number(pct) || 0)));
  // Stored as clarity and used as opacity: "how much shows through" is the
  // question somebody moving a slider is asking, and the stylesheet wants the
  // other end of it.
  document.documentElement.style.setProperty("--pane-solid", `${100 - value}%`);
  if (announce) {
    localStorage.setItem(CLEAR_KEY, String(value));
    window.__TAURI__?.event?.emit?.("pane-clarity", value);
  }
  return value;
}
