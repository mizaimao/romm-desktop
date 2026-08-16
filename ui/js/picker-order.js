// The order of the left column, and the little bar above it.
//
// Separate from sort.js, which orders games. The two answer different
// questions and want different answers: a game list sorted by rating is a
// question you asked about one console and stop caring about when you leave
// it, so that one is deliberately forgotten. The order of the column is the
// shape of the whole app — you learn where things are in it — so this one is
// remembered.
//
// It also had no order anybody chose. The server hands consoles and
// collections back by size, so the column opened on "Arcade Fighting, 322" and
// buried "Best of nes" thirty rows down, and there was no way to say otherwise.

import { escapeHtml } from "./util.js";
import { showMenu } from "./menu.js";

/// Orders per kind of list, in the sequence the menu offers them.
///
/// `key` returns something comparable and `dir` is 1 for ascending. The two
/// kinds share the idea but not the fields: a console knows whether an
/// emulator is installed, a collection knows how much of it is downloaded, and
/// neither knows the other's.
export const PICKER_ORDERS = {
  collections: [
    { id: "name", label: "Name", key: (c) => c.name.toLowerCase(), dir: 1 },
    { id: "count", label: "Most games", key: (c) => c.rom_count ?? 0, dir: -1 },
    { id: "fewest", label: "Fewest games", key: (c) => c.rom_count ?? 0, dir: 1 },
    { id: "here", label: "Most downloaded", key: (c) => c.local_count ?? 0, dir: -1 },
  ],
};

/// Consoles, alphabetically, with no button to say otherwise.
///
/// There are thirty-five of them and they do not change, so the column is
/// something you learn the shape of — which a button that reshuffles it works
/// against. Collections are different: there are twenty-seven, they arrive from
/// the server in size order, and which of them you want at the top depends on
/// what you are doing.
export function byName(items) {
  return [...items].sort((a, b) => a.name.localeCompare(b.name));
}

const KEY = (kind) => `romm.order.${kind}`;

/// Name, not size. The server's own order is by count, which is why every list
/// in this app opened on whichever console happens to have the most ROMs in
/// it.
export function pickerOrder(kind) {
  const orders = PICKER_ORDERS[kind] ?? [];
  const saved = localStorage.getItem(KEY(kind));
  return orders.find((o) => o.id === saved) ?? orders[0];
}

export function setPickerOrder(kind, id) {
  localStorage.setItem(KEY(kind), id);
}

/// Keep the button's own label in step. The list is redrawn by the caller, but
/// the bar above it is not part of that redraw — so without this the button
/// went on saying "Name" over a list sorted by size.
function relabel(root, kind) {
  const btn = root?.querySelector(".pick-sort span");
  if (btn) btn.textContent = pickerOrder(kind)?.label ?? "Name";
}

/// Sort a copy. The caller's array is what it was handed and re-sorting it in
/// place would compound across redraws.
export function sortPicker(kind, items) {
  const order = pickerOrder(kind);
  if (!order) return [...items];
  return [...items].sort((a, b) => {
    // Favourites first whatever else is chosen — a starred collection is one
    // you said you wanted at hand.
    if (!!a.is_favorite !== !!b.is_favorite) return a.is_favorite ? -1 : 1;
    const ka = order.key(a);
    const kb = order.key(b);
    if (ka < kb) return -order.dir;
    if (ka > kb) return order.dir;
    // A stable tie-break, or two consoles with the same count swap places
    // between one redraw and the next.
    return (a.name ?? "").localeCompare(b.name ?? "");
  });
}

/// The bar that sits above the column: a filter box when there is something
/// worth filtering, and the order button.
///
/// It is one element rather than a loose input because it has to be opaque and
/// stick to the top of a list that scrolls under it. The filter box alone was
/// sticky and see-through, so console names slid through the middle of it.
export function pickerBar({ kind, filter = null }) {
  const box = filter
    ? `<input id="cfilter" class="filter" type="search" placeholder="${escapeHtml(filter)}" />`
    : "";
  return `<div class="pickbar">${box}
    <button class="pick-sort" type="button" title="Change the order of this list">
      <span>${escapeHtml(pickerOrder(kind)?.label ?? "Name")}</span>
    </button>
  </div>`;
}

/// Wire the order button inside `root`. `redraw` is called after a change.
export function wirePickerBar(root, kind, redraw) {
  const btn = root?.querySelector(".pick-sort");
  if (!btn) return;
  btn.addEventListener("click", (ev) => {
    ev.stopPropagation();
    const now = pickerOrder(kind)?.id;
    const at = btn.getBoundingClientRect();
    showMenu(
      (PICKER_ORDERS[kind] ?? []).map((o) => ({
        label: o.id === now ? `✓ ${o.label}` : `   ${o.label}`,
        run: () => {
          setPickerOrder(kind, o.id);
          relabel(root, kind);
          redraw();
        },
      })),
      at.left,
      at.bottom + 4
    );
  });
}
