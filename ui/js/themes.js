// Themes.
//
// A theme is a set of CSS custom properties and nothing else. There is no
// theme format, no loader and no template language — the stylesheet already
// reads every colour, corner and gap through a token, so a theme is data and
// adding one is a dozen lines here.
//
// That holds because of two decisions in the stylesheet. Roundness and density
// are *multipliers* rather than sizes: every corner in the app was drawn at its
// own value and the differences between them are deliberate, so a theme that
// set one radius everywhere would flatten distinctions that took a while to get
// right. Multiplying keeps the relationships. And the default of every token
// reproduces the original look exactly, so "no theme" and "the first theme" are
// the same thing rather than two designs that drift apart.
//
// What a theme cannot do is move anything. The layout lives in index.html, and
// a theme that could rearrange it would break every time the markup changed.

const KEY = "romm.theme";

/// Every token a theme sets, with the defaults that reproduce the original
/// look. A theme may set any subset; the rest fall back to these.
const BASE = {
  bg: "#14161a",
  panel: "#1b1e24",
  "panel-2": "#21252c",
  line: "#2c313a",
  text: "#e6e8ec",
  dim: "#8b93a1",
  accent: "#5b9dd9",
  ok: "#5fb37a",
  glass: "#4d8fd6",
  tint: "18%",
  round: "1",
  density: "1",
  font: '-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif',
};

/// The gradient the shader backdrop draws, per theme. Kept beside the palette
/// rather than chosen separately: they were two dropdowns once, and every
/// combination worth having was a matching pair.
export const THEMES = [
  {
    id: "aero",
    label: "Aero glass",
    note: "The original — blue glass over a dark slate",
    tokens: {},
    low: "#0b0d16",
    high: "#2a3566",
  },
  {
    id: "graphite",
    label: "Graphite",
    note: "Square corners, no colour, tight",
    tokens: {
      bg: "#101114",
      panel: "#17181c",
      "panel-2": "#1d1f24",
      line: "#2a2c33",
      text: "#e4e6ea",
      dim: "#878b94",
      accent: "#9aa3b2",
      glass: "#6d7681",
      tint: "8%",
      round: "0",
      density: "0.8",
    },
    low: "#0d0e11",
    high: "#2b2f36",
  },
  {
    id: "amber",
    label: "CRT amber",
    note: "Phosphor on black, soft corners, monospaced",
    tokens: {
      bg: "#0d0a06",
      panel: "#171009",
      "panel-2": "#1f160c",
      line: "#3a2a14",
      text: "#ffc46b",
      dim: "#a27738",
      accent: "#ffa32e",
      ok: "#b8d44a",
      glass: "#c8873c",
      tint: "22%",
      round: "0.5",
      font: 'ui-monospace, SFMono-Regular, Menlo, monospace',
    },
    low: "#0a0703",
    high: "#4a2a08",
  },
  {
    id: "paper",
    label: "Paper",
    note: "Light, roomy, high contrast",
    tokens: {
      bg: "#f4f2ee",
      panel: "#ffffff",
      "panel-2": "#eceae5",
      line: "#d8d4cc",
      text: "#1e2024",
      dim: "#6b6f77",
      accent: "#2f6fb5",
      ok: "#2f7d4f",
      glass: "#8fb8d8",
      tint: "10%",
      round: "1.4",
      density: "1.25",
    },
    low: "#e8e5df",
    high: "#c3cedd",
  },
  {
    id: "plum",
    label: "Plum",
    note: "Violet glass, generous corners",
    tokens: {
      bg: "#14101a",
      panel: "#1c1725",
      "panel-2": "#241d2f",
      line: "#352b45",
      text: "#e9e4f0",
      dim: "#948aa6",
      accent: "#a98cf0",
      glass: "#7b62c4",
      tint: "20%",
      round: "1.6",
    },
    low: "#120a16",
    high: "#452b5e",
  },
];

export function themeById(id) {
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}

/// The chosen theme's id. An id that no longer exists falls back to the first
/// rather than leaving the window with no palette at all.
export function currentThemeId() {
  const stored = localStorage.getItem(KEY);
  return THEMES.some((t) => t.id === stored) ? stored : THEMES[0].id;
}

export function currentTheme() {
  return themeById(currentThemeId());
}

/// The value of one token under the current theme, for code that needs a
/// colour rather than a stylesheet.
export function tokenOf(name, theme = currentTheme()) {
  return theme.tokens[name] ?? BASE[name];
}

/// Put a theme on the document.
///
/// Every token is written, including the ones the theme leaves alone: setting
/// only what a theme names would leave the previous theme's values behind on
/// everything else, so switching from a light theme to a dark one would keep
/// whichever colours the second happened not to mention.
export function applyTheme(id, { announce = true } = {}) {
  const theme = themeById(id);
  const root = document.documentElement;
  for (const name of Object.keys(BASE)) {
    root.style.setProperty(`--${name}`, tokenOf(name, theme));
  }
  if (announce) {
    localStorage.setItem(KEY, theme.id);
    // The settings window is a separate document and cannot reach this one.
    window.__TAURI__?.event?.emit?.("theme-changed", theme.id);
  }
  return theme;
}

/// Called at startup in every window that has anything to paint.
export function applyStoredTheme() {
  applyTheme(currentThemeId(), { announce: false });
}
