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

  vec3 base = mix(u_low, u_high, smoothstep(0.35, 0.75, n));

  // Darker towards the edges: the grid sits on top and needs the contrast.
  float d = distance(uv, vec2(0.5)) * 1.25;
  base *= 1.0 - smoothstep(0.35, 1.0, d) * 0.55;

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
const DEFAULTS = { speed: 1, strength: 1, low: "", high: "" };

export function backdropSettings() {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem("backdropSettings") || "{}") };
  } catch {
    return { ...DEFAULTS };
  }
}

export function saveBackdropSettings(next) {
  const merged = { ...backdropSettings(), ...next };
  localStorage.setItem("backdropSettings", JSON.stringify(merged));
  // Applied live rather than on restart: a colour picker you cannot see the
  // result of is a colour picker nobody can use.
  if (live) live(merged);
  return merged;
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
    // An unset colour falls back to the theme's, so the default follows
    // whatever palette is in force instead of being a second place to maintain.
    gl.useProgram(prog);
    gl.uniform3fv(uLow, rgb(cfg.low, "--bg", [0.05, 0.05, 0.07]));
    gl.uniform3fv(uHigh, rgb(cfg.high, "--accent", [0.18, 0.2, 0.36]));
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
