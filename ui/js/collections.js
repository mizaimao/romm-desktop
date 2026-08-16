// Browsing RomM's collections.
//
// These are the server's groupings, not ours — genres, franchises, companies,
// series and any hand-made lists, mirrored by `sync`. Three levels: groups →
// collections in a group → the games. ES-DE shows collections beside systems,
// so this sits next to the platform grid rather than inside it.

import { el, state, trail, invoke, convertFileSrc } from "./state.js";
import { enter as enterView , region } from "./shell.js";
import { escapeHtml } from "./util.js";
import { renderRows } from "./library.js";

function enter(fn) {
  trail.push(fn);
}

/// The top bar both collection-browsing levels want.
function topBar(title) {
  state.view = "collections";
  state.platform = null;
  state.selected = null;
  el.detail.hidden = true;
  el.search.value = "";
  // The zoom slider stays: these are cards too, and sizing them is the same
  // want as sizing covers.
  enterView({ title, back: true, zoom: true });
}

export async function showCollectionGroups({ exclude = [] } = {}) {
  trail.length = 0;
  topBar("Browse");

  // `user` lives in its own tab now, so showing it here too would be the same
  // collections in two places with different names.
  const groups = (await invoke("collection_groups")).filter(
    (g) => !exclude.includes(g.group)
  );
  if (!groups.length) {
    region("picker").innerHTML = `<div class="empty">No collections on the server. Run a sync, or make one in RomM.</div>`;
    return;
  }

  region("picker").innerHTML = `<div class="grid">${groups
    .map(
      (g) => `
      <div class="card" data-group="${escapeHtml(g.group)}">
        <div class="logo"><span class="ph">${escapeHtml(g.label.slice(0, 2))}</span></div>
        <div class="name">${escapeHtml(g.label)}</div>
        <div class="meta">${g.count} collections</div>
      </div>`
    )
    .join("")}</div>`;

  el.list.querySelectorAll(".card").forEach((c) =>
    c.addEventListener("click", () => {
      const g = groups.find((x) => x.group === c.dataset.group);
      enter(showCollectionGroups);
      showCollectionsIn(g.group, g.label);
    })
  );
  el.list.querySelector(".card")?.classList.add("sel");
}

export async function showCollectionsIn(group, label) {
  topBar(label || group);
  const items = await invoke("collections_in", { group });

  // 1,040 companies is not a browsable list, so filter locally. Kept separate
  // from the header search, which searches games rather than collections.
  region("picker").innerHTML = `<input id="cfilter" class="filter" type="search" placeholder="Filter ${items.length} collections…" />` +
    `<div class="grid" id="cgrid"></div>`;

  const grid = document.getElementById("cgrid");
  const filter = document.getElementById("cfilter");

  function draw(list) {
    grid.innerHTML = list
      .map(
        (c) => `
        <div class="card" data-cid="${escapeHtml(c.id)}">
          <div class="logo mosaic"><span class="ph">${escapeHtml(c.name.slice(0, 2))}</span></div>
          <div class="name">${c.is_favorite ? "★ " : ""}${escapeHtml(c.name)}</div>
          <div class="meta">${c.rom_count} games${
            c.local_count ? ` · <span class="here">${c.local_count} here</span>` : ""
          }</div>
        </div>`
      )
      .join("");

    grid.querySelectorAll(".card").forEach((card) =>
      card.addEventListener("click", () => {
        const c = list.find((x) => String(x.id) === card.dataset.cid);
        enter(() => showCollectionsIn(group, label));
        showCollectionRoms(c.id, c.name);
      })
    );
    // Leave a selection so the controller can act immediately, as the
    // platform grid does.
    grid.querySelector(".card")?.classList.add("sel");
    loadMosaics(list.slice(0, 60));
  }

  let timer;
  filter.addEventListener("input", () => {
    clearTimeout(timer);
    const q = filter.value.trim().toLowerCase();
    timer = setTimeout(
      () => draw(q ? items.filter((c) => c.name.toLowerCase().includes(q)) : items),
      150
    );
  });

  draw(items);
  // Deliberately not focused: arrow keys and the controller should navigate
  // the grid straight away. Click the box, or press the search key, to filter.
}

export async function showCollectionRoms(id, name) {
  state.view = "collection-roms";
  state.collection = id;
  // The collection's own name, kept apart from the rendered title. Restoring a
  // parked section used to hand the title back as the name, so "Arcade Sports —
  // 256 games" became the name and gained a second "— 256 games" every time.
  state.collectionName = name;
  el.search.value = "";

  state.rows = await invoke("collection_roms", { id: String(id) });
  enterView({
    title: `${name} — ${state.rows.length} games`,
    back: true,
    layout: true,
    sidebar: true,
    grab: true,
    sort: true,
    zoom: "grid",
    gridLayout: state.layout === "grid",
  });
  // Collections mix platforms, so the list view needs the platform column.
  renderRows(state.rows, true);
}

/// Fill collection cards with a member's cover, through the same local cache
/// the game grids use — so this works offline and needs no server request.
async function loadMosaics(list) {
  const ids = list.flatMap((c) => c.sample_ids.slice(0, 1));
  if (!ids.length) return;
  try {
    const covers = await invoke("rom_covers", { ids });
    const byId = new Map(covers.map((c) => [c.id, c.cover]));
    for (const c of list) {
      const cover = byId.get(c.sample_ids[0]);
      if (!cover) continue;
      const logo = el.list.querySelector(
        `.card[data-cid="${CSS.escape(String(c.id))}"] .logo`
      );
      if (logo) logo.innerHTML = `<img src="${convertFileSrc(cover)}" alt="" />`;
    }
  } catch {
    // Placeholders are fine; artwork is not worth interrupting browsing for.
  }
}
