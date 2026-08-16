// Browsing RomM's collections.
//
// These are the server's groupings, not ours — genres, franchises, companies,
// series and any hand-made lists, mirrored by `sync`. Three levels: groups →
// collections in a group → the games. ES-DE shows collections beside systems,
// so this sits next to the platform grid rather than inside it.

import { el, state, trail, invoke, convertFileSrc } from "./state.js";
import { enter as enterView, region, resetGames } from "./shell.js";
import { escapeHtml } from "./util.js";
import { pickerBar, wirePickerBar, sortPicker } from "./picker-order.js";
import { renderRows } from "./library.js";
import { shellMode } from "./shell.js";

function enter(fn) {
  trail.push(fn);
}

/// Light up the collection whose games are in the middle.
///
/// The column stays on screen while the middle changes, so without this there
/// is nothing to say which of twenty-seven you are looking at.
function markOpen(grid, cid) {
  for (const card of grid.querySelectorAll(".card")) {
    card.classList.toggle("open", card.dataset.cid === cid);
  }
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
  topBar("RomM browse");
  resetGames("Pick a group on the left.");

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

  // Attached inside the region the groups were drawn into. They were attached
  // to cards in the middle, which has none of them — so clicking a group in
  // the left column did nothing at all.
  const picker = region("picker");
  picker.querySelectorAll(".card").forEach((c) =>
    c.addEventListener("click", () => {
      const g = groups.find((x) => x.group === c.dataset.group);
      state.lastGroup = g.group;
      markGroup(picker, g.group);
      enter(showCollectionGroups);
      showCollectionsIn(g.group, g.label, { into: intoFor("browse") });
    })
  );
  picker.querySelector(".card")?.classList.add("sel");

  // Browse is three levels deep — groups, then collections, then games — and
  // there are two columns for lists. So the groups keep the column and their
  // collections go to the middle, rather than replacing the groups and leaving
  // no way to see that a group list existed at all.
  if (shellMode() === "columns" && groups.length) {
    const want = groups.find((g) => g.group === state.lastGroup) ?? groups[0];
    state.lastGroup = want.group;
    markGroup(picker, want.group);
    await showCollectionsIn(want.group, want.label, { into: "games" });
  }
}

/// Which region a group's collections belong in.
///
/// In My collections they are the top level and take the column. Reached
/// through Browse they are one level down, and the column already holds the
/// groups they came from.
function intoFor(section) {
  if (shellMode() !== "columns") return "picker";
  return section === "browse" ? "games" : "picker";
}

function markGroup(picker, group) {
  for (const card of picker.querySelectorAll("[data-group]")) {
    card.classList.toggle("open", card.dataset.group === group);
  }
}

export async function showCollectionsIn(group, label, { into = "picker" } = {}) {
  topBar(label || group);
  if (into === "picker") resetGames("Pick a collection on the left.");
  const items = await invoke("collections_in", { group });

  // 1,040 companies is not a browsable list, so filter locally. Kept separate
  // from the header search, which searches games rather than collections.
  // The filter and the order button in one opaque bar. The filter box on its
  // own was sticky and see-through, so the names scrolled through the middle
  // of it.
  region(into).innerHTML =
    pickerBar({ kind: "collections", filter: `Filter ${items.length} collections…` }) +
    `<div class="grid" id="cgrid"></div>`;
  wirePickerBar(region(into), "collections", () => draw(shown()));

  const grid = document.getElementById("cgrid");
  const filter = document.getElementById("cfilter");

  /// What the filter box leaves, unordered — `draw` applies the order, so
  /// changing the order does not throw away what was typed.
  function shown() {
    const q = filter.value.trim().toLowerCase();
    return q ? items.filter((c) => c.name.toLowerCase().includes(q)) : items;
  }

  function draw(unordered) {
    const list = sortPicker("collections", unordered);
    grid.innerHTML = list
      .map(
        (c) => `
        <div class="card" data-cid="${escapeHtml(c.id)}">
          <div class="logo mosaic"><span class="ph">${escapeHtml(c.name.slice(0, 2))}</span></div>
          <div class="name">${c.is_favorite ? "★ " : ""}${escapeHtml(c.name)}</div>
          <div class="meta">${c.rom_count} games${
            c.local_count ? `<span class="here"> · ${c.local_count} here</span>` : ""
          }</div>
        </div>`
      )
      .join("");

    grid.querySelectorAll(".card").forEach((card) =>
      card.addEventListener("click", () => {
        const c = list.find((x) => String(x.id) === card.dataset.cid);
        state.lastCollection = { id: c.id, name: c.name };
        markOpen(grid, card.dataset.cid);
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
    timer = setTimeout(() => draw(shown()), 150);
  });

  draw(items);

  // The middle belongs to a collection's games, and one of them may as well be
  // showing: the one you were last in, or the first. The same thing Library
  // does with consoles, and what makes returning to a tab put you back where
  // you were rather than at a prompt.
  if (shellMode() === "columns" && into === "picker" && items.length) {
    const want =
      items.find((c) => String(c.id) === String(state.lastCollection?.id)) ?? items[0];
    state.lastCollection = { id: want.id, name: want.name };
    markOpen(grid, String(want.id));
    await showCollectionRoms(want.id, want.name);
  }

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
      // In the region the cards were drawn into. Looking in the middle meant
      // the artwork never arrived in the left column — the same mistake as the
      // click handlers, in the same file.
      const logo = region(into).querySelector(
        `.card[data-cid="${CSS.escape(String(c.id))}"] .logo`
      );
      if (logo) logo.innerHTML = `<img src="${convertFileSrc(cover)}" alt="" />`;
    }
  } catch {
    // Placeholders are fine; artwork is not worth interrupting browsing for.
  }
}
