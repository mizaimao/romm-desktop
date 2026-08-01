// Frontend for the RomM desktop client.
//
// No bundler and no framework: `withGlobalTauri` exposes the API on
// window.__TAURI__, so this is plain DOM code against the Rust commands in
// src-tauri/src/main.rs.

const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const el = {
  list: document.getElementById("list"),
  detail: document.getElementById("detail"),
  title: document.getElementById("title"),
  back: document.getElementById("back"),
  search: document.getElementById("search"),
  status: document.getElementById("status"),
  toast: document.getElementById("toast"),
};

let state = {
  view: "platforms", // platforms | roms | search
  platform: null,
  rows: [],
  selected: null,
  downloading: new Map(), // rom id -> {done, total}
};

const human = (b) => {
  if (!b) return "—";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let v = b, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(1)} ${u[i]}`;
};

let toastTimer;
function toast(msg, ms = 4000) {
  el.toast.textContent = msg;
  el.toast.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.toast.hidden = true), ms);
}

async function showPlatforms() {
  state.view = "platforms";
  state.platform = null;
  state.selected = null;
  el.back.hidden = true;
  el.detail.hidden = true;
  el.title.textContent = "Platforms";

  const items = await invoke("platforms");
  el.list.innerHTML = `<div class="grid">${items
    .map(
      (p) => `
      <div class="card" data-slug="${p.slug}">
        <div class="name">${escapeHtml(p.name)}</div>
        <div class="meta">
          <span class="dot ${p.playable ? "on" : ""}"></span>
          ${p.rom_count} games${p.playable ? "" : " · no core"}
        </div>
      </div>`
    )
    .join("")}</div>`;

  el.list.querySelectorAll(".card").forEach((c) =>
    c.addEventListener("click", () => showRoms(c.dataset.slug))
  );
}

async function showRoms(slug) {
  state.view = "roms";
  state.platform = slug;
  el.back.hidden = false;
  el.search.value = "";
  const rows = await invoke("roms", { platform: slug });
  state.rows = rows;
  el.title.textContent = `${slug} — ${rows.length} games`;
  renderRows(rows, false);
}

async function runSearch(term) {
  if (!term.trim()) {
    return state.platform ? showRoms(state.platform) : showPlatforms();
  }
  state.view = "search";
  el.back.hidden = false;
  const rows = await invoke("search", { term });
  state.rows = rows;
  el.title.textContent = `Search “${term}” — ${rows.length}`;
  renderRows(rows, true);
}

function renderRows(rows, showPlatform) {
  if (!rows.length) {
    el.list.innerHTML = `<div class="empty">Nothing here.</div>`;
    return;
  }
  el.list.innerHTML = `<div class="rows">${rows
    .map(
      (r) => `
      <div class="row" data-id="${r.id}">
        <span class="have">${r.downloaded ? "▣" : ""}</span>
        <span class="nm">${escapeHtml(r.name)}</span>
        ${showPlatform ? `<span class="pf">${r.platform}</span>` : ""}
        <span class="sz">${human(r.size_bytes)}</span>
      </div>`
    )
    .join("")}</div>`;

  el.list.querySelectorAll(".row").forEach((row) =>
    row.addEventListener("click", () => selectRom(Number(row.dataset.id)))
  );
}

async function selectRom(id) {
  state.selected = id;
  el.list.querySelectorAll(".row").forEach((r) =>
    r.classList.toggle("sel", Number(r.dataset.id) === id)
  );

  const d = await invoke("rom_detail", { id });
  const media = [];
  if (d.video) media.push(`<video src="${convertFileSrc(d.video)}" controls muted loop></video>`);
  if (d.cover) media.push(`<img src="${convertFileSrc(d.cover)}" alt="" />`);
  if (!d.video && d.screenshot)
    media.push(`<img src="${convertFileSrc(d.screenshot)}" alt="" />`);

  el.detail.hidden = false;
  el.detail.innerHTML = `
    <h2>${escapeHtml(d.name)}</h2>
    <div class="sub">${escapeHtml(d.fs_name)}</div>
    ${media.join("")}
    <dl>
      <dt>Platform</dt><dd>${d.platform}</dd>
      <dt>Size</dt><dd>${human(d.size_bytes)}</dd>
      <dt>Core</dt><dd>${d.core_label ? escapeHtml(d.core_label) : "<em>none installed</em>"}</dd>
      <dt>Local</dt><dd>${d.downloaded ? "yes" : "no"}</dd>
    </dl>
    <div class="actions">
      <button class="primary" id="play">${d.downloaded ? "Play" : "Download & Play"}</button>
      <button class="ghost" id="dl" ${d.downloaded ? "disabled" : ""}>Download</button>
    </div>
    <progress id="prog" hidden></progress>`;

  document.getElementById("play").addEventListener("click", () => play(d));
  document.getElementById("dl").addEventListener("click", () => download(d.id, false));
}

async function download(id, thenPlay) {
  const prog = document.getElementById("prog");
  if (prog) prog.hidden = false;
  try {
    const msg = await invoke("download_rom", { id });
    toast(msg);
    if (state.selected === id) await selectRom(id); // refresh "Local: yes"
    if (thenPlay) await launch(id);
  } catch (e) {
    toast(`Download failed — ${e}`, 8000);
  } finally {
    if (prog) prog.hidden = true;
  }
}

async function launch(id) {
  try {
    toast("Launching…");
    toast(await invoke("launch_rom", { id }));
  } catch (e) {
    toast(`Launch failed — ${e}`, 8000);
  }
}

async function play(d) {
  if (d.downloaded) return launch(d.id);
  return download(d.id, true);
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]
  );
}

// --- wiring --------------------------------------------------------------

el.back.addEventListener("click", () => {
  el.search.value = "";
  showPlatforms();
});

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
    ].join(" · ");
  } catch (e) {
    el.status.textContent = "backend error";
  }
  await showPlatforms();
})();
