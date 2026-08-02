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
  themesBtn: document.getElementById("themes-btn"),
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
  el.themesBtn.classList.remove("active");
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
        <div class="logo">${
          p.logo
            ? `<img src="${convertFileSrc(p.logo)}" alt="" />`
            : `<span class="ph">${escapeHtml(p.slug)}</span>`
        }</div>
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

// --- theme picker --------------------------------------------------------

async function showThemes() {
  state.view = "themes";
  el.back.hidden = false;
  el.detail.hidden = true;
  el.themesBtn.classList.add("active");
  el.title.textContent = "Console icon themes";
  el.list.innerHTML = `<div class="empty">Loading themes…</div>`;

  let themes;
  try {
    themes = await invoke("themes_available");
  } catch (e) {
    el.list.innerHTML = `<div class="empty">Could not load the themes list.<br>${escapeHtml(String(e))}</div>`;
    return;
  }
  let styles = [];
  try {
    styles = await invoke("icon_styles");
  } catch (e) {
    /* non-fatal: the picker just won't show */
  }
  renderThemes(themes, styles);
}

function renderThemes(themes, styles = []) {
  const bar = styles.length
    ? `<div class="stylebar">
         <span class="lbl">Console icons:</span>
         ${styles
           .map(
             (s) =>
               `<button data-style="${s.key}" class="${s.selected ? "on" : ""}" ${
                 s.available ? "" : "disabled"
               }>${escapeHtml(s.label)} <span style="opacity:.6">${s.available}</span></button>`
           )
           .join("")}
       </div>`
    : "";

  el.list.innerHTML = bar + `<div class="themes">${themes
    .map(
      (t) => `
      <div class="tcard ${t.installed ? "on" : ""}" data-repo="${t.reponame}">
        <div class="shot">${
          t.screenshot
            ? `<img src="${t.screenshot}" alt="" loading="lazy" />`
            : ""
        }</div>
        <div class="body">
          <div class="tname">${escapeHtml(t.name)}
            ${t.installed ? `<span class="tbadge">· installed ${human(t.size_bytes)}</span>` : ""}
          </div>
          <div class="tby">${escapeHtml(t.author || "unknown")}${
            t.variants?.length ? ` · ${t.variants.length} variants` : ""
          }</div>
          <div class="tacts">
            <button class="go" data-act="icons">Use icons</button>
            <button data-act="${t.installed ? "remove" : "full"}">${
              t.installed ? "Remove" : "Full download"
            }</button>
          </div>
        </div>
      </div>`
    )
    .join("")}</div>`;

  el.list.querySelectorAll(".stylebar button").forEach((b) =>
    b.addEventListener("click", async () => {
      try {
        const label = await invoke("set_icon_style", { key: b.dataset.style });
        el.list.querySelectorAll(".stylebar button").forEach((x) =>
          x.classList.toggle("on", x === b)
        );
        toast(`Console icons: ${label}`);
      } catch (e) {
        toast(String(e), 6000);
      }
    })
  );

  el.list.querySelectorAll(".tcard button").forEach((b) =>
    b.addEventListener("click", (ev) => {
      ev.stopPropagation();
      const repo = b.closest(".tcard").dataset.repo;
      themeAction(repo, b.dataset.act, b);
    })
  );
}

async function themeAction(reponame, act, btn) {
  const card = btn.closest(".tcard");
  card.querySelectorAll("button").forEach((b) => (b.disabled = true));
  const original = btn.textContent;
  btn.textContent = act === "remove" ? "Removing…" : "Working…";
  try {
    let msg;
    if (act === "remove") {
      msg = await invoke("theme_remove", { reponame });
    } else {
      // "Use icons" clones, extracts the platform logos, then deletes the
      // checkout — themes are hundreds of MB and we render a few hundred KB.
      msg = await invoke("theme_download", { reponame, logosOnly: act === "icons" });
    }
    toast(msg, 6000);
    await showThemes();
  } catch (e) {
    toast(`${reponame} — ${e}`, 9000);
    btn.textContent = original;
    card.querySelectorAll("button").forEach((b) => (b.disabled = false));
  }
}

async function selectRom(id) {
  state.selected = id;
  el.list.querySelectorAll(".row").forEach((r) =>
    r.classList.toggle("sel", Number(r.dataset.id) === id)
  );

  const d = await invoke("rom_detail", { id });
  const shots = d.screenshots || [];

  // Screenshots on top (cycled if there is more than one), cover below.
  const top = shots.length
    ? `<div class="shots" id="shots">
         ${shots
           .map(
             (s, i) =>
               `<img src="${convertFileSrc(s)}" class="${i === 0 ? "on" : ""}" alt="" />`
           )
           .join("")}
         ${
           shots.length > 1
             ? `<div class="dots">${shots
                 .map((_, i) => `<span class="${i === 0 ? "on" : ""}"></span>`)
                 .join("")}</div>
                <button class="nav prev">‹</button><button class="nav next">›</button>`
             : ""
         }
       </div>`
    : d.video
      ? `<video src="${convertFileSrc(d.video)}" controls muted loop></video>`
      : "";

  const bottom = d.cover ? `<img class="cover" src="${convertFileSrc(d.cover)}" alt="" />` : "";
  const vid =
    shots.length && d.video
      ? `<video src="${convertFileSrc(d.video)}" controls muted loop></video>`
      : "";

  el.detail.hidden = false;
  el.detail.innerHTML = `
    <h2>${escapeHtml(d.name)}</h2>
    <div class="sub">${escapeHtml(d.fs_name)}</div>
    ${top}
    ${bottom}
    ${vid}
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

  if (shots.length > 1) startSlideshow(shots.length);

  document.getElementById("play").addEventListener("click", () => play(d));
  document.getElementById("dl").addEventListener("click", () => download(d.id, false));
}

let slideTimer;
function startSlideshow(count) {
  clearInterval(slideTimer);
  const box = document.getElementById("shots");
  if (!box) return;
  const imgs = [...box.querySelectorAll("img")];
  const dots = [...box.querySelectorAll(".dots span")];
  let i = 0;

  const show = (n) => {
    i = (n + count) % count;
    imgs.forEach((im, k) => im.classList.toggle("on", k === i));
    dots.forEach((dt, k) => dt.classList.toggle("on", k === i));
  };
  const auto = () => {
    clearInterval(slideTimer);
    slideTimer = setInterval(() => show(i + 1), 3500);
  };

  box.querySelector(".prev")?.addEventListener("click", () => { show(i - 1); auto(); });
  box.querySelector(".next")?.addEventListener("click", () => { show(i + 1); auto(); });
  dots.forEach((dt, k) => dt.addEventListener("click", () => { show(k); auto(); }));
  // Pause while the pointer is over the image, so it can be studied.
  box.addEventListener("mouseenter", () => clearInterval(slideTimer));
  box.addEventListener("mouseleave", auto);
  auto();
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
  el.themesBtn.classList.remove("active");
  showPlatforms();
});

el.themesBtn.addEventListener("click", () => {
  if (state.view === "themes") {
    el.themesBtn.classList.remove("active");
    showPlatforms();
  } else {
    showThemes();
  }
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
      `${human(s.disk_bytes)} on disk`,
    ].join(" · ");
    el.status.title =
      `Downloads:  ${s.roms_dir}\nArtwork:    ${s.media_dir}\n\n` +
      `Everything this app downloads lives there. Delete that folder to reclaim the space.`;
    window.__storage = s;
  } catch (e) {
    el.status.textContent = "backend error";
  }
  await showPlatforms();
})();
