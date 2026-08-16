// Entry point: wire the header controls and load the first view.

import { el, state, trail, invoke, listen } from "./state.js";
import { askDownload } from "./bulk.js";
import { openSortMenu } from "./sort.js";
import { chooseMode, storedMode } from "./shell.js";
import { human, toast } from "./util.js";
import { showPlatforms, runSearch, setLayout, setZoom, renderRows } from "./library.js";
import { setSidebar, installDetailResizer } from "./detail.js";
import { installTabs, showSection, resetSection, activeSection } from "./tabs.js";
import { installKeys } from "./keys.js";
import { installGamepad } from "./gamepad.js";
import { warmRefresh } from "./actions.js";
import {
  startBackdrop, stopBackdrop, applyBackdropSettings,
  applyStoredGlassTint, setGlassTint, setGlassStrength,
} from "./backdrop.js";

el.back.addEventListener("click", () => {
  el.search.value = "";
  // Collections are three levels deep, so step back through them rather than
  // dropping straight to the platform grid.
  const up = trail.pop();
  if (up) return up();
  // Back at the top of a section returns to that section, not always to the
  // library — the tab bar says where you are and it should stay true.
  resetSection();
});

// Settings runs in its own document and cannot touch this one, so the artwork
// choice comes back as an event. Redrawn from the backend rather than from
// what is on screen: the images themselves have changed, not their layout.
listen("art-changed", () => {
  if (state.view === "platforms") return;
  if (state.rows.length) renderRows(state.rows, state.view === "search");
});

// The console pictures, which are the other half: `art-changed` deliberately
// skips the console grid because game artwork is not what changed there. This
// is the one that redraws it.
// Chosen in the settings window, which cannot reach this document.
listen("shell-mode", async ({ payload }) => {
  chooseMode(String(payload), { announce: false });
  // Redrawn from scratch: the console list has moved to a different element,
  // and everything on screen was laid out for the old arrangement.
  await showSection(activeSection(), { force: true });
});

listen("icons-changed", () => {
  if (state.view === "platforms") showPlatforms();
});

el.sortBtn.addEventListener("click", () => openSortMenu(el.sortBtn));

el.zoom.addEventListener("input", (e) => setZoom(Number(e.target.value)));

el.layoutBtn.addEventListener("click", () =>
  setLayout(state.layout === "grid" ? "list" : "grid")
);

el.sidebarBtn.addEventListener("click", () => setSidebar(!state.sidebar));

// Whatever is on screen is what gets taken: a console page downloads that
// console, a collection page that collection.
// This is an application, not a page. The native menu offers "Open Image in
// New Window", "Copy Image" and "Share" for a console icon, none of which mean
// anything here, and it comes up over anything the app puts in its place.
//
// Text fields keep theirs: cut, copy, paste and the spelling menu are the
// system's job and there is no reason to reimplement them.
window.addEventListener("contextmenu", (ev) => {
  if (ev.target.closest("input, textarea, [contenteditable]")) return;
  ev.preventDefault();
});

el.grabBtn.addEventListener("click", () =>
  askDownload(
    state.view === "collection-roms" && state.collection
      ? { collection: state.collection, name: state.collectionName }
      : { platform: state.platform }
  )
);

let searchTimer;
el.search.addEventListener("input", (e) => {
  clearTimeout(searchTimer);
  const v = e.target.value;
  searchTimer = setTimeout(() => runSearch(v), 200);
});

// The backdrop's controls are in the Settings window and its canvas is here, so
// the two have to talk. Without this, toggling it there rendered a shader into
// the settings panel and left the library untouched.
listen("glass-tint", ({ payload }) => setGlassTint(payload, { announce: false }));
listen("glass-strength", ({ payload }) => setGlassStrength(payload, { announce: false }));

listen("backdrop-toggle", ({ payload }) => {
  if (payload) startBackdrop();
  else stopBackdrop();
});
listen("backdrop-settings", ({ payload }) => {
  // Apply, never re-save: saving emits, and this window would then answer its
  // own message. That round trip is what made the backdrop flicker while a
  // colour was being dragged.
  applyBackdropSettings(payload);
});

/// Anything worth watching gets a speed and a size, not just a moving bar.
///
/// Below a few megabytes it is over before it can be read, so the threshold
/// keeps a bar from flashing up with numbers nobody had time to see. A disc
/// image is a different matter: minutes of waiting with no idea whether it is
/// moving at 2 MB/s or 40.
const PROGRESS_DETAIL_BYTES = 5 * 1024 * 1024;

/// Rate is measured from the first tick of *this* transfer rather than from
/// zero, so a resumed download reports what it is doing now instead of
/// crediting itself with the part it did not transfer.
let dlStart = null;

listen("download-progress", ({ payload }) => {
  const [id, done, total] = payload;
  if (state.selected !== id) return;

  const prog = document.getElementById("prog");
  if (prog) {
    prog.hidden = false;
    prog.max = total || 1;
    prog.value = done;
  }

  const label = document.getElementById("prog-text");
  if (!label) return;
  if (total < PROGRESS_DETAIL_BYTES) {
    label.hidden = true;
    return;
  }

  const now = performance.now();
  if (!dlStart || done < dlStart.done) dlStart = { at: now, done };
  const seconds = (now - dlStart.at) / 1000;
  const rate = seconds > 0.4 ? (done - dlStart.done) / seconds : 0;
  const left = rate > 0 ? (total - done) / rate : null;

  label.hidden = false;
  label.textContent =
    `${human(done)} of ${human(total)}` +
    ` · ${total ? Math.round((done / total) * 100) : 0}%` +
    (rate > 0 ? ` · ${human(rate)}/s` : "") +
    (left !== null && left > 2 ? ` · ${formatEta(left)} left` : "");

  // A finished transfer is reported as (1, 1); clear the baseline so the next
  // one measures itself rather than inheriting this one's start.
  if (done >= total) dlStart = null;
});

function formatEta(seconds) {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  return m < 60 ? `${m}m ${Math.round(seconds % 60)}s` : `${Math.floor(m / 60)}h ${m % 60}m`;
}

(async function init() {
  try {
    const s = await invoke("status");
    // "not set up" and "server unreachable" look identical otherwise — both
    // give an empty library — so say which it is rather than leaving the user
    // to guess whether the app is broken.
    const server = !s.configured ? "no config.toml" : s.connected ? s.server : "offline";
    el.status.textContent = [
      server,
      `${s.roms_cached} roms`,
      s.retroarch ? `${s.cores_installed} cores` : "no RetroArch",
      `${human(s.disk_bytes)} on disk`,
    ].join(" · ");
    // Both of these are only otherwise discovered by pressing play and having
    // it fail, which is a poor way to learn the app was never configured.
    if (s.crowded_folder) {
      // Before anything is created, not after. The app is about to write a
      // library folder, a cache and a config beside itself, and doing that in
      // someone's Downloads is how a launcher earns a reputation for mess.
      showFolderWarning(s);
    } else if (!s.configured) {
      // Naming the exact path it looked at: "it cannot find my config" is
      // otherwise unanswerable, and the answer is rarely the directory the
      // user expected.
      toast(`No config.toml at ${s.config_path} — copy config.example.toml there`);
    } else if (!s.retroarch) {
      toast("RetroArch not found — set its location in Settings");
    }
    el.status.title =
      `Config:     ${s.config_path}\nData dir:   ${s.data_dir}\n` +
      `Downloads:  ${s.roms_dir}\nArtwork:    ${s.media_dir}\n\n` +
      `Everything this app downloads lives there. Delete that folder to reclaim the space.`;
  } catch (e) {
    el.status.textContent = "backend error";
  }
  // Off by default: it is a preference, and starting a GPU loop uninvited on
  // someone's machine is not a decision this app should make for them.
  // Before anything is drawn: which arrangement decides where things go.
  chooseMode(storedMode(), { announce: false });
  if (localStorage.getItem("backdrop") === "on") startBackdrop();
  applyStoredGlassTint();
  setZoom(state.zoom);
  setLayout(state.layout);
  setSidebar(state.sidebar);
  installTabs();
  await showSection("library", { force: true });
  installDetailResizer();
  installKeys();
  installGamepad();
  // Measure the display now. It costs 24 animation frames, and taken on demand
  // that wait landed between pressing play and the game being asked for.
  warmRefresh();
})();

/// "You have put me somewhere I am going to make a mess of."
///
/// Shown once, before the first sync or download creates anything. Dismissable
/// rather than blocking: it is advice, and someone who meant to put the app
/// there should not have to argue with it.
function showFolderWarning(status) {
  if (localStorage.getItem("folderWarningSeen") === "yes") return;

  const box = document.createElement("div");
  box.id = "conflict-overlay";
  box.innerHTML = `<div class="conflict-box">
      <header><span class="icon icon-info-on"></span><h2>Give me my own folder</h2></header>
      <p class="lead">This app writes everything beside its own executable —
        downloaded games, cover art, the library index, your config.</p>
      <p class="why">Right now that is:<br><code>${status.data_dir}</code><br>
        which already has ${status.folder_entries} other item${
          status.folder_entries === 1 ? "" : "s"
        } in it.</p>
      <p class="note">Move the app into a folder of its own and everything it
        creates stays together — and deleting that one folder reclaims all of
        it. Nothing has been written yet.</p>
      <div class="sides">
        <button class="side" data-ack="ok"><span class="who">Got it</span>
          <span class="when">do not show this again</span></button>
        <button class="side" data-ack="later"><span class="who">Remind me</span>
          <span class="when">ask again next launch</span></button>
      </div>
    </div>`;

  box.addEventListener("click", (ev) => {
    const btn = ev.target.closest("[data-ack]");
    if (!btn) return;
    if (btn.dataset.ack === "ok") localStorage.setItem("folderWarningSeen", "yes");
    box.remove();
  });
  document.addEventListener(
    "keydown",
    (ev) => {
      if (ev.key === "Escape" && box.isConnected) box.remove();
    },
    { once: true }
  );
  document.body.appendChild(box);
  box.querySelector("[data-ack]").focus();
}
