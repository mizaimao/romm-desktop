// Shared UI state and cached element handles.
//
// A plain mutable object rather than a store: the app is small and every view
// reads the same handful of fields.

export const state = {
  view: "platforms", // platforms | roms | search | themes | collections | collection-roms
  platform: null,
  /// The console last opened, so returning to the grid puts the cursor back on
  /// it instead of the top-left card.
  lastPlatform: localStorage.getItem("lastPlatform") || null,
  rows: [],
  selected: null,
  /// Platform slug -> cover aspect (w/h), measured by the backend.
  aspects: {},
  /// The collection currently open, so its position can be remembered too.
  collection: null,
  /// List key -> the rom id last selected in it. Coming back to a console or a
  /// collection puts the cursor where you left it rather than at the top, which
  /// otherwise means re-finding your place on every single trip in and out.
  lastRom: JSON.parse(localStorage.getItem("lastRom") || "{}"),
  layout: localStorage.getItem("layout") || "grid",
  sidebar: localStorage.getItem("sidebar") !== "off",
  /// Base card width in px; the grid scales from this.
  zoom: Number(localStorage.getItem("zoom")) || 150,
  gamepad: null,
};

/// Where "back" goes, as a stack. Collections browse three levels deep while
/// every other view returns straight to the platform grid. Lives here rather
/// than in collections.js so library.js can clear it without an import cycle.
export const trail = [];

export const el = {
  list: document.getElementById("list"),
  detail: document.getElementById("detail"),
  title: document.getElementById("title"),
  back: document.getElementById("back"),
  search: document.getElementById("search"),
  status: document.getElementById("status"),
  toast: document.getElementById("toast"),
  themesBtn: document.getElementById("themes-btn"),
  collectionsBtn: document.getElementById("collections-btn"),
  systemsBtn: document.getElementById("systems-btn"),
  layoutBtn: document.getElementById("layout-btn"),
  sidebarBtn: document.getElementById("sidebar-btn"),
  lb: document.getElementById("lightbox"),
  settingsBtn: document.getElementById("settings-btn"),
  zoom: document.getElementById("zoom"),
  zoomWrap: document.getElementById("zoom-wrap"),
};

// Tauri globals, exposed once so modules do not each reach into window.
export const { invoke, convertFileSrc } = window.__TAURI__.core;
export const { listen } = window.__TAURI__.event;

/// Which list is on screen, as a key that survives a restart.
///
/// Search is deliberately excluded: the term changes, so a remembered position
/// in one search means nothing in the next.
export function listKey() {
  if (state.view === "roms" && state.platform) return `platform:${state.platform}`;
  if (state.view === "collection-roms" && state.collection) return `collection:${state.collection}`;
  return null;
}

/// Record the cursor position for the list currently on screen.
export function rememberRom(id) {
  const key = listKey();
  if (!key || id == null) return;
  state.lastRom[key] = id;
  try {
    localStorage.setItem("lastRom", JSON.stringify(state.lastRom));
  } catch {
    // A full or disabled localStorage costs the memory across restarts, not
    // within the session, so it is not worth failing a selection over.
  }
}

/// The rom to put the cursor on when a list is rendered, if it is still there.
export function rememberedRom(rows) {
  const key = listKey();
  if (!key) return null;
  const want = state.lastRom[key];
  return rows.some((r) => r.id === want) ? want : null;
}
