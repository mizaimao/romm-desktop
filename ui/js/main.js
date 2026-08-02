// Entry point: wire the header controls and load the first view.

import { el, state, invoke, listen } from "./state.js";
import { human } from "./util.js";
import { showPlatforms, runSearch, setLayout, setZoom } from "./library.js";
import { setSidebar } from "./detail.js";
import { showThemes } from "./themes.js";
import { showSystems } from "./systems.js";
import { installKeys } from "./keys.js";
import { installGamepad } from "./gamepad.js";

el.back.addEventListener("click", () => {
  el.search.value = "";
  el.themesBtn.classList.remove("active");
  el.systemsBtn.classList.remove("active");
  showPlatforms();
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
    el.status.textContent = [
      s.connected ? s.server : "offline",
      `${s.roms_cached} roms`,
      s.retroarch ? `${s.cores_installed} cores` : "no RetroArch",
      `${human(s.disk_bytes)} on disk`,
    ].join(" · ");
    el.status.title =
      `Downloads:  ${s.roms_dir}\nArtwork:    ${s.media_dir}\n\n` +
      `Everything this app downloads lives there. Delete that folder to reclaim the space.`;
  } catch (e) {
    el.status.textContent = "backend error";
  }
  setZoom(state.zoom);
  setLayout(state.layout);
  setSidebar(state.sidebar);
  await showPlatforms();
  installKeys();
  installGamepad();
})();
