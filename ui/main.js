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
  layoutBtn: document.getElementById("layout-btn"),
  sidebarBtn: document.getElementById("sidebar-btn"),
  lb: document.getElementById("lightbox"),
};

let state = {
  view: "platforms", // platforms | roms | search | themes
  platform: null,
  rows: [],
  selected: null,
  downloading: new Map(), // rom id -> {done, total}
  // Grid shows box art, list is denser. Remembered across launches.
  layout: localStorage.getItem("layout") || "grid",
  aspects: {}, // platform slug -> cover w/h

  // Detail pane visibility, remembered. Shown by default.
  sidebar: localStorage.getItem("sidebar") !== "off",
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
  el.layoutBtn.hidden = true;
  el.sidebarBtn.hidden = true;
  coverObserver?.disconnect();
  state.platform = null;
  state.selected = null;
  el.back.hidden = true;
  el.detail.hidden = true;
  el.title.textContent = "Platforms";

  const items = await invoke("platforms");
  for (const p of items) if (p.cover_aspect) state.aspects[p.slug] = p.cover_aspect;
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
  el.layoutBtn.hidden = false;
  el.sidebarBtn.hidden = false;
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
  el.layoutBtn.hidden = false;
  el.sidebarBtn.hidden = false;
  el.title.textContent = `Search “${term}” — ${rows.length}`;
  renderRows(rows, true);
}

function renderRows(rows, showPlatform) {
  if (!rows.length) {
    el.list.innerHTML = `<div class="empty">Nothing here.</div>`;
    return;
  }
  el.list.innerHTML =
    state.layout === "grid" ? gridMarkup(rows) : listMarkup(rows, showPlatform);

  el.list.querySelectorAll("[data-id]").forEach((n) => {
    const id = Number(n.dataset.id);
    n.addEventListener("click", () => selectRom(id));
    // Double-click is the shortcut for "just play it" — downloads first if
    // needed, exactly like the primary button.
    n.addEventListener("dblclick", async (ev) => {
      ev.preventDefault();
      const d = await invoke("rom_detail", { id });
      play(d);
    });
  });

  if (state.layout === "grid") observeCovers();

  // Opening straight into a selected game means the artwork pane is populated
  // instead of the user staring at an empty sidebar.
  if (rows.length) selectRom(rows[0].id);
}

function listMarkup(rows, showPlatform) {
  return `<div class="rows">${rows
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
}

function gridMarkup(rows) {
  // One shared ratio per view when the platform is uniform; mixed results
  // (search across platforms) fall back to per-card ratios.
  const uniform = state.view !== "search" ? state.aspects[state.platform] : null;
  const style = uniform ? ` style="--ar:${uniform.toFixed(3)}"` : "";
  return `<div class="gcards"${style}>${rows
    .map(
      (r) => `
      <div class="gcard" data-id="${r.id}"${
        state.view === "search" && state.aspects[r.platform]
          ? ` style="--ar:${state.aspects[r.platform].toFixed(3)}"`
          : ""
      }>
        <div class="art"><span class="ph">${escapeHtml(r.name.slice(0, 2))}</span></div>
        <div class="gname">${escapeHtml(r.name)}</div>
        <div class="gmeta">${r.downloaded ? "▣ " : ""}${human(r.size_bytes)}</div>
      </div>`
    )
    .join("")}</div>`;
}

// Covers are fetched only for cards that scroll into view, in batches, so a
// 2,400-game platform does not trigger 2,400 requests on open.
let coverObserver;
let coverQueue = [];
let coverTimer;

function observeCovers() {
  coverObserver?.disconnect();
  coverQueue = [];
  coverObserver = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        coverObserver.unobserve(e.target);
        coverQueue.push(Number(e.target.dataset.id));
      }
      clearTimeout(coverTimer);
      coverTimer = setTimeout(flushCovers, 80);
    },
    { root: el.list, rootMargin: "300px" }
  );
  el.list.querySelectorAll(".gcard").forEach((c) => coverObserver.observe(c));
}

async function flushCovers() {
  const ids = coverQueue.splice(0, 40);
  if (!ids.length) return;
  try {
    const covers = await invoke("rom_covers", { ids });
    for (const { id, cover } of covers) {
      if (!cover) continue;
      const art = el.list.querySelector(`.gcard[data-id="${id}"] .art`);
      if (art) art.innerHTML = `<img src="${convertFileSrc(cover)}" alt="" />`;
    }
  } catch (e) {
    /* leave placeholders in place */
  }
  if (coverQueue.length) setTimeout(flushCovers, 30);
}

function setSidebar(on) {
  state.sidebar = on;
  localStorage.setItem("sidebar", on ? "on" : "off");
  el.sidebarBtn.textContent = on ? "Hide info" : "Show info";
  // Never show the pane on the platform screen — there is no game selected.
  const allowed = state.view === "roms" || state.view === "search";
  el.detail.hidden = !(on && allowed && state.selected !== null);
}

function setLayout(next) {
  state.layout = next;
  localStorage.setItem("layout", next);
  el.layoutBtn.textContent = next === "grid" ? "List" : "Grid";
  el.layoutBtn.title = next === "grid" ? "Switch to list view" : "Switch to grid view";
  if (state.rows.length) renderRows(state.rows, state.view === "search");
}

// --- lightbox ------------------------------------------------------------
//
// Full-size artwork over a dimmed backdrop, inside the main window rather than
// a second OS window: no dock icon, no window management, and Esc closes it.

const lb = {
  items: [],   // [{src, kind: "image"|"video", caption}]
  index: 0,
  open: false,
};

function openLightbox(items, index = 0) {
  if (!items.length) return;
  lb.items = items;
  lb.index = index;
  lb.open = true;
  el.lb.hidden = false;
  // Stop the slideshow so it cannot swap the image out from under the viewer.
  clearInterval(slideTimer);
  renderLightbox();
}

function closeLightbox() {
  lb.open = false;
  el.lb.hidden = true;
  // Release the video so audio does not keep playing behind the closed view.
  el.lb.querySelector(".lb-stage").innerHTML = "";
}

function stepLightbox(delta) {
  if (!lb.open || lb.items.length < 2) return;
  lb.index = (lb.index + delta + lb.items.length) % lb.items.length;
  renderLightbox();
}

function renderLightbox() {
  const it = lb.items[lb.index];
  const stage = el.lb.querySelector(".lb-stage");
  stage.innerHTML =
    it.kind === "video"
      ? `<video src="${it.src}" controls autoplay loop></video>`
      : it.kind === "pdf"
        // WKWebView renders PDFs natively, so an iframe is the whole viewer.
        ? `<iframe src="${it.src}" title="Manual"></iframe>`
        : `<img src="${it.src}" alt="" />`;
  el.lb.querySelector("figcaption").textContent =
    lb.items.length > 1
      ? `${it.caption} — ${lb.index + 1} of ${lb.items.length}`
      : it.caption;
  const multi = lb.items.length > 1;
  el.lb.querySelector(".lb-prev").disabled = !multi;
  el.lb.querySelector(".lb-next").disabled = !multi;
}

/// Everything in the detail pane, as one navigable set.
function detailMedia(d) {
  const items = (d.screenshots || []).map((s, i) => ({
    src: convertFileSrc(s),
    kind: "image",
    caption: `Screenshot ${i + 1}`,
  }));
  if (d.cover) items.push({ src: convertFileSrc(d.cover), kind: "image", caption: "Cover" });
  if (d.video) items.push({ src: convertFileSrc(d.video), kind: "video", caption: "Video" });
  return items;
}

el.lb.querySelector(".lb-close").addEventListener("click", closeLightbox);
el.lb.querySelector(".lb-prev").addEventListener("click", () => stepLightbox(-1));
el.lb.querySelector(".lb-next").addEventListener("click", () => stepLightbox(1));
// Clicking the backdrop closes; clicking the artwork itself does not.
el.lb.addEventListener("click", (ev) => {
  if (ev.target === el.lb || ev.target.tagName === "FIGURE") closeLightbox();
});

window.addEventListener("keydown", (ev) => {
  if (!lb.open) return;
  if (ev.key === "Escape") closeLightbox();
  else if (ev.key === "ArrowLeft") stepLightbox(-1);
  else if (ev.key === "ArrowRight") stepLightbox(1);
});

// --- theme picker --------------------------------------------------------

async function showThemes() {
  state.view = "themes";
  el.back.hidden = false;
  el.detail.hidden = true;
  el.themesBtn.classList.add("active");
  el.layoutBtn.hidden = true;
  el.sidebarBtn.hidden = true;
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
  el.list.querySelectorAll(".row, .gcard").forEach((r) =>
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

  el.detail.hidden = !state.sidebar;
  el.detail.innerHTML = `
    <div class="scroll">
      <h2>${escapeHtml(d.name)}</h2>
      <div class="sub">${escapeHtml(d.fs_name)}</div>
      ${top}
      ${bottom}
      ${vid}
      ${d.rating ? starBar(d.rating) : ""}
      ${d.summary ? `<p class="summary">${escapeHtml(d.summary)}</p>` : ""}
      <dl>
        ${row("Released", d.release_year)}
        ${row("Genre", d.genres.join(", "))}
        ${row("Developer", d.companies.join(", "))}
        ${row("Series", d.franchises.join(", "))}
        ${row("Players", d.player_count)}
        ${row("Modes", d.game_modes.join(", "))}
        ${row("Region", d.regions.join(", "))}
        ${row("Also known as", d.alt_names.join(" · "))}
        <dt>Platform</dt><dd>${d.platform}</dd>
        <dt>Size</dt><dd>${human(d.size_bytes)}</dd>
        <dt>Core</dt><dd>${d.core_label ? escapeHtml(d.core_label) : "<em>none installed</em>"}</dd>
        <dt>Local</dt><dd>${d.downloaded ? "yes" : "no"}</dd>
      </dl>
      ${
        d.manual || d.youtube_id
          ? `<div class="extras">
               ${d.manual ? `<button class="link" id="manual">📖 Manual</button>` : ""}
               ${d.youtube_id ? `<a class="link" target="_blank"
                   href="https://www.youtube.com/watch?v=${encodeURIComponent(d.youtube_id)}">▶ Trailer</a>` : ""}
             </div>`
          : ""
      }
    </div>
    <div class="pinned">
      <div class="actions">
        <button class="primary" id="play">${d.downloaded ? "Play" : "Download & Play"}</button>
        <button class="ghost" id="dl" ${d.downloaded ? "disabled" : ""}>Download</button>
      </div>
      <progress id="prog" hidden></progress>
    </div>`;

  if (shots.length > 1) startSlideshow(shots.length);

  // Clicking any artwork opens it full size, positioned at what was clicked.
  const media = detailMedia(d);
  const openAt = (pred) => () => {
    const i = media.findIndex(pred);
    openLightbox(media, i < 0 ? 0 : i);
  };
  el.detail
    .querySelector(".shots")
    ?.addEventListener("click", (ev) => {
      // Leave the slideshow's own arrows and dots alone.
      if (ev.target.closest(".nav, .dots")) return;
      const shown = el.detail.querySelector(".shots img.on");
      const idx = [...el.detail.querySelectorAll(".shots img")].indexOf(shown);
      openLightbox(media, Math.max(idx, 0));
    });
  el.detail
    .querySelector("img.cover")
    ?.addEventListener("click", openAt((m) => m.caption === "Cover"));
  el.detail.querySelectorAll("video").forEach((v) =>
    v.addEventListener("dblclick", openAt((m) => m.kind === "video"))
  );
  document.getElementById("manual")?.addEventListener("click", () =>
    openLightbox([{ src: convertFileSrc(d.manual), kind: "pdf", caption: "Manual" }], 0)
  );

  document.getElementById("play").addEventListener("click", () => play(d));
  document.getElementById("dl").addEventListener("click", () => download(d.id, false));
}

// Omit a row entirely when the field is empty, rather than showing a blank.
function row(label, value) {
  if (value === null || value === undefined || value === "") return "";
  return `<dt>${label}</dt><dd>${escapeHtml(String(value))}</dd>`;
}

/// RomM stores ratings 0-100; show five stars plus the raw number.
function starBar(rating) {
  const n = Math.round((rating / 100) * 5 * 2) / 2;
  const full = Math.floor(n);
  const half = n - full >= 0.5;
  const stars = "★".repeat(full) + (half ? "⯨" : "") + "☆".repeat(5 - full - (half ? 1 : 0));
  return `<div class="rating"><span class="stars">${stars}</span>
          <span class="num">${Math.round(rating)}/100</span></div>`;
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

el.sidebarBtn.addEventListener("click", () => setSidebar(!state.sidebar));

el.layoutBtn.addEventListener("click", () =>
  setLayout(state.layout === "grid" ? "list" : "grid")
);

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
  setLayout(state.layout);
  setSidebar(state.sidebar);
  el.layoutBtn.hidden = true;
  el.sidebarBtn.hidden = true;
  await showPlatforms();
})();
