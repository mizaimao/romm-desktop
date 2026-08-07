// Entry point: wire the header controls and load the first view.

import { el, state, trail, invoke, listen } from "./state.js";
import { human, toast } from "./util.js";
import { showPlatforms, runSearch, setLayout, setZoom } from "./library.js";
import { setSidebar, installDetailResizer } from "./detail.js";
import { showThemes } from "./themes.js";
import { showSystems } from "./systems.js";
import { installTabs, showSection, activeSection } from "./tabs.js";
import { installKeys } from "./keys.js";
import { installGamepad } from "./gamepad.js";
import { startBackdrop } from "./backdrop.js";

el.back.addEventListener("click", () => {
  el.search.value = "";
  // Collections are three levels deep, so step back through them rather than
  // dropping straight to the platform grid.
  const up = trail.pop();
  if (up) return up();
  el.themesBtn.classList.remove("active");
  el.systemsBtn.classList.remove("active");
  // Back at the top of a section returns to that section, not always to the
  // library — the tab bar says where you are and it should stay true.
  showSection(activeSection());
});

el.zoom.addEventListener("input", (e) => setZoom(Number(e.target.value)));

el.layoutBtn.addEventListener("click", () =>
  setLayout(state.layout === "grid" ? "list" : "grid")
);

el.sidebarBtn.addEventListener("click", () => setSidebar(!state.sidebar));

el.systemsBtn.addEventListener("click", () =>
  state.view === "systems" ? showPlatforms() : showSystems()
);

el.themesBtn.addEventListener("click", () =>
  state.view === "themes" ? showPlatforms() : showThemes()
);

let searchTimer;
el.search.addEventListener("input", (e) => {
  clearTimeout(searchTimer);
  const v = e.target.value;
  searchTimer = setTimeout(() => runSearch(v), 200);
});

listen("download-progress", ({ payload }) => {
  const [id, done, total] = payload;
  const prog = document.getElementById("prog");
  if (prog && state.selected === id) {
    prog.hidden = false;
    prog.max = total || 1;
    prog.value = done;
  }
});

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
  if (localStorage.getItem("backdrop") === "on") startBackdrop();
  setZoom(state.zoom);
  setLayout(state.layout);
  setSidebar(state.sidebar);
  installTabs();
  await showSection("library");
  installDetailResizer();
  installKeys();
  installGamepad();
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
