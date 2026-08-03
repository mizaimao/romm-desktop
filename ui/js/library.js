// Platform grid, game grid/list, and lazy cover loading.

import { el, state, trail, invoke, convertFileSrc } from "./state.js";
import { human, escapeHtml } from "./util.js";
import { selectRom, play } from "./detail.js";

export async function showPlatforms() {
  state.view = "platforms";
  trail.length = 0;
  state.platform = null;
  state.selected = null;
  el.back.hidden = true;
  el.detail.hidden = true;
  el.layoutBtn.hidden = true;
  el.sidebarBtn.hidden = true;
  el.zoomWrap.hidden = false; // the platform grid scales too
  el.themesBtn.classList.remove("active");
  el.systemsBtn.classList.remove("active");
  el.collectionsBtn.classList.remove("active");
  coverObserver?.disconnect();
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

export async function showRoms(slug) {
  state.view = "roms";
  state.platform = slug;
  el.back.hidden = false;
  el.layoutBtn.hidden = false;
  el.sidebarBtn.hidden = false;
  el.zoomWrap.hidden = state.layout !== "grid";
  el.search.value = "";
  state.rows = await invoke("roms", { platform: slug });
  el.title.textContent = `${slug} — ${state.rows.length} games`;
  renderRows(state.rows, false);
}

export async function runSearch(term) {
  if (!term.trim()) {
    return state.platform ? showRoms(state.platform) : showPlatforms();
  }
  state.view = "search";
  el.back.hidden = false;
  el.layoutBtn.hidden = false;
  el.sidebarBtn.hidden = false;
  el.zoomWrap.hidden = state.layout !== "grid";
  state.rows = await invoke("search", { term });
  const consoles = new Set(state.rows.map((r) => r.platform)).size;
  el.title.textContent =
    `Search “${term}” — ${state.rows.length}` +
    (consoles > 1 ? ` across ${consoles} consoles` : "");
  renderRows(state.rows, true);
}

export function renderRows(rows, showPlatform) {
  if (!rows.length) {
    el.list.innerHTML = `<div class="empty">Nothing here.</div>`;
    return;
  }
  // Search spans every console, so a flat list of 200 hits buries the ones you
  // meant. Grouping also lets each console keep its own cover shape, which a
  // single mixed grid cannot.
  el.list.innerHTML = showPlatform
    ? groupedMarkup(rows)
    : state.layout === "grid"
      ? gridMarkup(rows)
      : listMarkup(rows, showPlatform);

  el.list.querySelectorAll("[data-id]").forEach((n) => {
    const id = Number(n.dataset.id);
    n.addEventListener("click", () => selectRom(id));
    // Double-click is the shortcut for "just play it".
    n.addEventListener("dblclick", async (ev) => {
      ev.preventDefault();
      play(await invoke("rom_detail", { id }));
    });
  });

  if (state.layout === "grid") observeCovers();
  // Open with something selected so the artwork pane is not blank.
  if (rows.length) selectRom(rows[0].id);
}

/// Search results, split into one section per console, biggest group first.
function groupedMarkup(rows) {
  const groups = new Map();
  for (const r of rows) {
    if (!groups.has(r.platform)) groups.set(r.platform, []);
    groups.get(r.platform).push(r);
  }
  const ordered = [...groups.entries()].sort(
    (a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0])
  );

  return ordered
    .map(
      ([platform, items]) => `
      <section class="pgroup">
        <h2 class="ghead">
          <span class="gslug">${escapeHtml(platform)}</span>
          <span class="gcount">${items.length}</span>
        </h2>
        ${state.layout === "grid" ? gridMarkup(items, platform) : listMarkup(items, false)}
      </section>`
    )
    .join("");
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

function gridMarkup(rows, platform) {
  // One ratio per grid. Grouped search passes its console in, so each section
  // is uniform even though the results as a whole are not.
  const slug = platform ?? (state.view !== "search" ? state.platform : null);
  const uniform = slug ? state.aspects[slug] : null;
  const style = uniform ? ` style="--ar:${uniform.toFixed(3)}"` : "";
  return `<div class="gcards"${style}>${rows
    .map(
      (r) => `
      <div class="gcard" data-id="${r.id}"${
        !uniform && state.aspects[r.platform]
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

/// Card size, applied as a CSS variable so both grids scale from one number.
export function setZoom(px) {
  state.zoom = px;
  localStorage.setItem("zoom", String(px));
  el.zoom.value = String(px);
  document.documentElement.style.setProperty("--card", `${px}px`);
}

export function setLayout(next) {
  state.layout = next;
  localStorage.setItem("layout", next);
  el.layoutBtn.textContent = next === "grid" ? "List" : "Grid";
  el.layoutBtn.title = next === "grid" ? "Switch to list view" : "Switch to grid view";
  el.zoomWrap.hidden = next !== "grid" && state.view !== "platforms";
  if (state.rows.length) renderRows(state.rows, state.view === "search");
}

// Covers load only for cards near the viewport, batched — opening a
// 2,400-game platform must not fire 2,400 requests.
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
    for (const { id, cover } of await invoke("rom_covers", { ids })) {
      if (!cover) continue;
      const art = el.list.querySelector(`.gcard[data-id="${id}"] .art`);
      if (art) art.innerHTML = `<img src="${convertFileSrc(cover)}" alt="" />`;
    }
  } catch (e) {
    // Leave placeholders; a failed batch is not worth interrupting browsing.
  }
  if (coverQueue.length) setTimeout(flushCovers, 30);
}
