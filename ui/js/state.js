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
