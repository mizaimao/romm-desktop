// Browsing RomM's collections.
//
// These are the server's groupings, not ours — genres, franchises, companies,
// series and any hand-made lists, mirrored by `sync`. Three levels: groups →
// collections in a group → the games. ES-DE shows collections beside systems,
// so this sits next to the platform grid rather than inside it.

import { el, state, trail, invoke, convertFileSrc } from "./state.js";
import { escapeHtml } from "./util.js";
import { renderRows } from "./library.js";

function enter(fn) {
  trail.push(fn);
}

/// Common chrome for the two collection-browsing levels.
function chrome(title) {
  state.view = "collections";
  state.platform = null;
  state.selected = null;
  el.back.hidden = false;
  el.detail.hidden = true;
  el.layoutBtn.hidden = true;
  el.sidebarBtn.hidden = true;
  el.zoomWrap.hidden = false;
  el.search.value = "";
  el.title.textContent = title;
  el.collectionsBtn.classList.add("active");
  el.themesBtn.classList.remove("active");
  el.systemsBtn.classList.remove("active");
}

export async function showCollectionGroups() {
  trail.length = 0;
  chrome("Collections");

  const groups = await invoke("collection_groups");
  if (!groups.length) {
    el.list.innerHTML = `<div class="empty">No collections on the server. Run a sync, or make one in RomM.</div>`;
    return;
  }

  el.list.innerHTML = `<div class="grid">${groups
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
}

export async function showCollectionsIn(group, label) {
  chrome(label || group);
  const items = await invoke("collections_in", { group });

  // 1,040 companies is not a browsable list, so filter locally. Kept separate
  // from the header search, which searches games rather than collections.
  el.list.innerHTML =
    `<input id="cfilter" class="filter" type="search" placeholder="Filter ${items.length} collections…" />` +
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
          <div class="meta">${c.rom_count} games</div>
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
  el.back.hidden = false;
  el.layoutBtn.hidden = false;
  el.sidebarBtn.hidden = false;
  el.zoomWrap.hidden = state.layout !== "grid";
  el.search.value = "";

  state.rows = await invoke("collection_roms", { id: String(id) });
  el.title.textContent = `${name} — ${state.rows.length} games`;
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
