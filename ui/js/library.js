// Platform grid, game grid/list, and lazy cover loading.

import { el, state, trail, invoke, convertFileSrc, rememberedRom } from "./state.js";
import { resetNav } from "./keys.js";
import { sorted, refreshSortButton } from "./sort.js";
import { showMenu } from "./menu.js";
import { deleteState } from "./states.js";
import { human, escapeHtml, toast } from "./util.js";
import { selectRom, play, withTransition } from "./detail.js";
import { download } from "./actions.js";

export async function showPlatforms() {
  state.view = "platforms";
  trail.length = 0;
  state.platform = null;
  state.selected = null;
  el.back.hidden = true;
  el.detail.hidden = true;
  // Grid or list works here too. It was hidden on this screen, which is why
  // there was no way to see thirty-five consoles without scrolling past
  // thirty-five pictures of them.
  el.layoutBtn.hidden = false;
  el.sidebarBtn.hidden = true;
  // Offered here as well as inside a console: burying it one level down means
  // it is only found by someone who already knows it exists.
  el.grabBtn.hidden = false;
  el.zoomWrap.hidden = state.layout !== "grid";
  coverObserver?.disconnect();
  el.title.textContent = "Platforms";

  const items = await invoke("platforms");
  for (const p of items) if (p.cover_aspect) state.aspects[p.slug] = p.cover_aspect;
  // Kept so switching layout can redraw without asking the backend again.
  state.platforms = items;

  renderPlatforms(items);
  await showRecent();

  restorePlatformCursor();
}

/// A row of the games you played most recently, above the consoles.
///
/// The timestamps are the server's, so this is the same list on every machine —
/// which is the point: the thing you were in the middle of is rarely on the
/// computer you are now sitting at. Absent entirely when nothing has been
/// played, rather than an empty row explaining itself.
/// How many recent games the strip will hold before it stops growing.
///
/// One row that scrolls sideways, up to about two screens' worth. Past that the
/// strip stops being a shortcut and becomes a second library above the real
/// one — so the rest go behind a button rather than off the side of a scroll
/// nobody would reach the end of.
const RECENT_IN_STRIP = 20;

async function showRecent() {
  let rows = [];
  try {
    // One more than fits, so the strip can tell whether there is a "more" to
    // show without a second call.
    rows = await invoke("recent_games", { limit: RECENT_IN_STRIP + 1 });
  } catch {
    return;
  }
  if (!rows.length) return;

  const overflow = rows.length > RECENT_IN_STRIP;
  const shown = rows.slice(0, RECENT_IN_STRIP);

  const strip = document.createElement("section");
  strip.className = "recent";
  strip.innerHTML =
    `<h2 class="ghead"><span class="gslug">Continue playing</span>` +
    (overflow ? `<button class="link recent-more">More…</button>` : "") +
    `</h2>` +
    `<div class="gcards">${shown.map((r) => `
       <div class="gcard" data-id="${r.id}">
         <div class="art"><span class="ph">${escapeHtml(r.name.slice(0, 2))}</span></div>
         <div class="gname">${escapeHtml(r.name)}</div>
         <div class="gmeta">${r.downloaded ? "▣ " : ""}${r.platform}</div>
       </div>`).join("")}</div>`;
  el.list.prepend(strip);
  strip.querySelectorAll(".gcard").forEach((c) => wireGame(c, Number(c.dataset.id)));
  strip.querySelector(".recent-more")?.addEventListener("click", showAllRecent);
  // Re-observe after prepending, so these cards get covers like any others.
  // The observer was set up before this row existed.
  observeCovers();
}

/// Everything you have played, as a page rather than a strip.
export async function showAllRecent() {
  state.view = "search";
  state.platform = null;
  trail.length = 0;
  trail.push(() => showPlatforms());
  el.back.hidden = false;
  el.detail.hidden = !state.sidebar;
  el.layoutBtn.hidden = false;
  el.sidebarBtn.hidden = false;
  el.grabBtn.hidden = true;
  el.zoomWrap.hidden = state.layout !== "grid";
  el.title.textContent = "Continue playing";

  let rows = [];
  try {
    rows = await invoke("recent_games", { limit: 500 });
  } catch (e) {
    el.list.innerHTML = `<div class="empty">${escapeHtml(String(e))}</div>`;
    return;
  }
  // Grouped by console, like a search: these come from everywhere.
  state.rows = rows;
  renderRows(rows, true);
}

/// Draw the consoles, in whichever layout is selected.
///
/// Both shapes carry the same three facts — name, how many games, whether a
/// core is installed — so switching layout changes the density and nothing
/// else. The list is for finding a console you can name; the grid is for
/// recognising one you cannot.
function renderPlatforms(items) {
  el.list.innerHTML =
    state.layout === "grid"
      ? `<div class="grid">${items.map(platformCard).join("")}</div>`
      : `<div class="rows">${items.map(platformRow).join("")}</div>`;

  el.list.querySelectorAll(".card, .prow").forEach((c) =>
    c.addEventListener("click", () => openPlatform(c.dataset.slug, c))
  );
  resetNav();
}

/// Open a console, carrying its name up into the title bar.
///
/// The two are the same words in two places, so the browser is told they are
/// one element and moves it, rather than fading a grid out and a list in with
/// no thread between them. That thread is the whole point: it says where the
/// screen you are now looking at came from.
///
/// Falls back to a plain navigation wherever view transitions are missing or
/// the user has asked for less motion — `withTransition` handles both — so the
/// tags are cleaned up in a `finally` and never left on an element.
export async function openPlatform(slug, card) {
  const label = card?.querySelector(".name, .nm");
  if (label) label.style.viewTransitionName = "heading";
  try {
    await withTransition(async () => {
      await showRoms(slug);
      // Tagged inside the callback: the new snapshot is taken after this runs,
      // and the title only holds the console's name by then.
      el.title.style.viewTransitionName = "heading";
    });
  } finally {
    el.title.style.viewTransitionName = "";
    if (label) label.style.viewTransitionName = "";
  }
}

/// The same move in reverse, coming back out to the consoles.
///
/// Worth the symmetry: a transition that plays going in and not coming out
/// reads as a glitch rather than as a deliberate direction.
export async function backToPlatforms() {
  el.title.style.viewTransitionName = "heading";
  let label = null;
  try {
    await withTransition(async () => {
      await showPlatforms();
      label = el.list.querySelector(
        `[data-slug="${CSS.escape(state.lastPlatform ?? "")}"] .name, ` +
          `[data-slug="${CSS.escape(state.lastPlatform ?? "")}"] .nm`
      );
      if (label) label.style.viewTransitionName = "heading";
    });
  } finally {
    el.title.style.viewTransitionName = "";
    if (label) label.style.viewTransitionName = "";
  }
}

function platformCard(p) {
  return `
      <div class="card" data-slug="${p.slug}">
        <div class="logo">${
          p.logo
            ? `<img class="${p.logo_wordmark ? "wordmark" : "art"}" src="${convertFileSrc(p.logo)}" alt="" />`
            : `<span class="wordtype">${escapeHtml(p.slug)}</span>`
        }</div>
        <div class="name">${escapeHtml(p.name)}</div>
        <div class="meta">
          <span class="dot ${p.playable ? "on" : ""}"></span>
          ${p.rom_count} games${p.playable ? "" : " · no core"}
        </div>
      </div>`;
}

function platformRow(p) {
  // `row` as well as `prow` so keyboard and pad navigation pick it up: the
  // movement code selects `.card, .gcard, .row, .tcard`, and a row that is not
  // one of those is unreachable without a mouse.
  return `
      <div class="row prow" data-slug="${p.slug}">
        <span class="have"><span class="dot ${p.playable ? "on" : ""}"></span></span>
        <span class="nm">${escapeHtml(p.name)}</span>
        <span class="pf">${escapeHtml(p.slug)}</span>
        <span class="sz">${p.rom_count} games${p.playable ? "" : " · no core"}</span>
      </div>`;
}

/// Put the cursor back on the console you were just in. Coming out of a game
/// list to find the selection reset to the top-left corner means re-finding
/// your place every single time.
function restorePlatformCursor() {
  const back = state.lastPlatform
    ? el.list.querySelector(`[data-slug="${CSS.escape(state.lastPlatform)}"]`)
    : null;
  if (back) {
    back.classList.add("sel");
    back.scrollIntoView({ block: "center" });
  } else {
    // Always leave something selected, so the controller's A button has a
    // target the moment the grid appears.
    el.list.querySelector(".card, .prow")?.classList.add("sel");
  }
}

export async function showRoms(slug) {
  state.view = "roms";
  state.platform = slug;
  state.lastPlatform = slug;
  localStorage.setItem("lastPlatform", slug);
  el.back.hidden = false;
  el.layoutBtn.hidden = false;
  el.sidebarBtn.hidden = false;
  el.grabBtn.hidden = false;
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

/// Click, double-click and right-click on one game.
///
/// Shared, because there are two places that draw games — the list, and the
/// "continue playing" strip above it — and they had drifted: the strip could be
/// selected and nothing else. Right-click there did nothing at all, and neither
/// did double-click, on cards that look exactly like the ones below where both
/// work.
export function wireGame(node, id) {
  node.addEventListener("click", () => selectRom(id));
  // Double-click is the shortcut for "just play it".
  node.addEventListener("dblclick", async (ev) => {
    ev.preventDefault();
    play(await invoke("rom_detail", { id }));
  });
  node.addEventListener("contextmenu", async (ev) => {
    ev.preventDefault();
    selectRom(id);
    let d;
    try {
      d = await invoke("rom_detail", { id });
    } catch {
      return;
    }
    // The game's save states, if it has any. This menu used to offer "take
    // this console offline" instead, which on a game in Continue playing reads
    // as an offer to download a whole platform — the opposite of the small,
    // local thing a right-click on one game should do. Taking a console
    // offline is on the toolbar, where it belongs.
    let states = [];
    try {
      states = await invoke("game_states", { id });
    } catch {
      states = [];
    }
    showMenu(
      [
        { label: d.downloaded ? "Play" : "Download and play", run: () => play(d) },
        d.downloaded ? null : { label: "Download", run: () => download(id, false) },
        // A rule between starting the game and destroying part of it.
        states.length ? null : undefined,
        ...states.map((st) => ({
          label: `Delete ${st.label}${st.when ? ` — ${st.when}` : ""}`,
          danger: true,
          run: () => deleteState(id, st),
        })),
      ],
      ev.clientX,
      ev.clientY
    );
  });
}

export function renderRows(unsorted, showPlatform) {
  // Ordered here rather than by whoever supplied the rows, so every path that
  // draws a list — a console, a collection, a search, a redraw after the order
  // changed — goes through the same comparison.
  const rows = sorted(unsorted);
  refreshSortButton();
  if (!rows.length) {
    el.list.innerHTML = `<div class="empty">Nothing here.</div>`;
    return;
  }
  // Search spans every console, so a flat list of 200 hits buries the ones you
  // meant. Grouping also lets each console keep its own cover shape, which a
  // single mixed grid cannot.
  resetNav();
  el.list.innerHTML = showPlatform
    ? groupedMarkup(rows)
    : state.layout === "grid"
      ? gridMarkup(rows)
      : listMarkup(rows, showPlatform);

  el.list.querySelectorAll("[data-id]").forEach((n) => wireGame(n, Number(n.dataset.id)));

  if (state.layout === "grid") observeCovers();
  // Put the cursor back where it was in this list, falling back to the top.
  // Without this, every trip out to the platform grid and back means scrolling
  // to find your place again — which on a 2,500-game arcade list is the whole
  // journey a second time.
  if (rows.length) {
    const want = rememberedRom(rows) ?? rows[0].id;
    selectRom(want);
    const node = el.list.querySelector(`[data-id="${want}"]`);
    // `nearest` rather than `center`: if the remembered row is already on
    // screen, scrolling it to the middle moves the list for no reason.
    node?.scrollIntoView({ block: "nearest" });
  }
}

/// Search results, split into one section per console, biggest group first.
/// Consoles that are the same machine under two names, in the order they
/// should appear.
///
/// A search for "Zelda" turns up the NES release and the Famicom one, and they
/// belong beside each other — they are the same game on the same hardware,
/// sold in two places. Still separate headings, because the cartridges, the
/// regions and the dumps genuinely differ, but never with eight other consoles
/// between them.
const FAMILIES = [
  ["nes", "famicom"],
  ["snes", "sfc"],
];

/// The family a console belongs to, and where it sits within it. A console in
/// no family is a family of one.
function family(slug) {
  const at = FAMILIES.findIndex((f) => f.includes(slug));
  return at < 0 ? [slug, 0] : [FAMILIES[at][0], FAMILIES[at].indexOf(slug)];
}

function groupedMarkup(rows) {
  const groups = new Map();
  for (const r of rows) {
    if (!groups.has(r.platform)) groups.set(r.platform, []);
    groups.get(r.platform).push(r);
  }

  // Families are ordered by their combined weight, so a console with three
  // hits and its twin with two outrank one with four — they are one machine as
  // far as "how much of this search is about it" goes.
  const weight = new Map();
  for (const [slug, items] of groups) {
    const [key] = family(slug);
    weight.set(key, (weight.get(key) ?? 0) + items.length);
  }

  const ordered = [...groups.entries()].sort((a, b) => {
    const [ka, ia] = family(a[0]);
    const [kb, ib] = family(b[0]);
    if (ka !== kb) return weight.get(kb) - weight.get(ka) || ka.localeCompare(kb);
    // Inside a family, the order the family declares — the home-market name
    // first, which is the one most people search for.
    return ia - ib;
  });

  // One group is not a grouping. A collection that happens to hold games from a
  // single console got a heading repeating what the title already said, sitting
  // over the first row and cropping it.
  const single = ordered.length === 1;

  return ordered
    .map(
      ([platform, items]) => `
      <section class="pgroup">
        ${single ? "" : `<h2 class="ghead">
          <span class="gslug">${escapeHtml(platform)}</span>
          <span class="gcount">${items.length}</span>
        </h2>`}
        ${state.layout === "grid" ? gridMarkup(items, platform) : listMarkup(items, false)}
      </section>`
    )
    .join("");
}

function listMarkup(rows, showPlatform) {
  return `<div class="rows">${rows
    .map(
      (r) => `
      <div class="row${r.favourite ? " fav" : ""}" data-id="${r.id}">
        <span class="have">${r.downloaded ? "▣" : ""}</span>
        <span class="nm">${r.favourite ? `<span class="star">★</span>` : ""}${escapeHtml(r.name)}</span>
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
        <div class="art"><span class="ph">${escapeHtml(r.name.slice(0, 2))}</span>${
          r.favourite ? `<span class="star">★</span>` : ""
        }</div>
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
  el.layoutBtn.querySelector("span:not(.icon)").textContent = next === "grid" ? "List" : "Grid";
  el.layoutBtn.querySelector(".icon").className = `icon icon-${next === "grid" ? "list" : "grid"}`;
  el.layoutBtn.title = next === "grid" ? "Switch to list view" : "Switch to grid view";
  // Only a grid has anything to resize, on the consoles screen as much as
  // anywhere else.
  el.zoomWrap.hidden = next !== "grid";

  // Redraw whatever is actually on screen.
  //
  // This used to redraw `state.rows` regardless of the current view, and
  // `state.rows` still holds the last console you opened. So pressing the
  // button on the consoles screen replaced it with that console's games —
  // which looks exactly like the button opened a console by itself.
  if (state.view === "platforms") {
    renderPlatforms(state.platforms);
    restorePlatformCursor();
  } else if (state.rows.length) {
    renderRows(state.rows, state.view === "search");
  }
}

// Covers load only for cards near the viewport, batched — opening a
// 2,400-game platform must not fire 2,400 requests.
let coverObserver;
let coverQueue = [];
let coverTimer;

let coverErrorShown = false;

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
    // Placeholders stay — a failed batch is not worth interrupting browsing —
    // but it is said once. Swallowing this entirely meant a grid that fetched
    // no artwork at all looked exactly like a server with no artwork on it.
    if (!coverErrorShown) {
      coverErrorShown = true;
      toast(`Cover art is not loading — ${e}`, 8000);
    }
  }
  if (coverQueue.length) setTimeout(flushCovers, 30);
}
