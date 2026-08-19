// How a list of games is ordered, and the menu that changes it.
//
// Per view, and deliberately not saved. Sorting by rating to see what a console
// is famous for, and then finding every console still sorted that way a week
// later, is a setting that has outlived its question. It lasts as long as the
// app is open, which is how long the reason for it usually lasts.
//
// The console grid has no sort of its own: it is one screen of a couple of
// dozen tiles in a fixed order that people learn the shape of, and shuffling it
// would cost more than it gives.

import { el, state } from "./state.js";
import { escapeHtml } from "./util.js";

/// The orders, in the sequence the menu lists them.
///
/// `key` returns something comparable; `missing` is where games with nothing to
/// compare go. A game with no rating sorts last under "rating" rather than
/// first, because an unrated game is not a bad one and a screen that opens on
/// the unknowns is a screen that answers nothing.
export const ORDERS = [
  { id: "name", label: "Name", key: (g) => g.name.toLowerCase(), dir: 1 },
  { id: "rating", label: "Rating", key: (g) => g.rating ?? -1, dir: -1 },
  { id: "year", label: "Release year", key: (g) => g.year ?? -1, dir: -1 },
  { id: "played", label: "Recently played", key: (g) => g.last_played ?? "", dir: -1 },
  { id: "size", label: "Size", key: (g) => g.size_bytes ?? 0, dir: -1 },
  // Only meaningful where the rows come from more than one console — a
  // search, or Continue playing. Inside a console every row has the same key
  // and this degrades to the name tie-break, which is harmless.
  { id: "platform", label: "Console", key: (g) => (g.platform ?? "").toLowerCase(), dir: 1 },
];

/// Chosen order per view, for this run of the app only.
///
/// Keyed by what is on screen rather than globally: "sort this console by
/// rating" is a statement about that console. A Map, not localStorage — the
/// forgetting is the feature.
const chosen = new Map();

function scope() {
  return `${state.view}:${state.platform ?? ""}:${state.collection ?? ""}`;
}

export function currentOrder() {
  return ORDERS.find((o) => o.id === chosen.get(scope())) ?? ORDERS[0];
}

export function setOrder(id) {
  chosen.set(scope(), id);
}

/// The order a view starts in, if the user has not picked one for it yet.
///
/// Continue playing is ordered by *when you played it* or it is not a
/// continue-playing list — it arrived grouped by console, which threw that
/// away. Set rather than forced, so choosing something else still sticks.
export function defaultOrder(id) {
  if (!chosen.has(scope())) chosen.set(scope(), id);
}

/// Sort a copy of `rows`. Never in place: `state.rows` is what the page was
/// given and re-sorting it repeatedly would compound rather than replace.
export function sorted(rows) {
  const order = currentOrder();
  const copy = [...rows];
  copy.sort((a, b) => {
    // Favourites stay on top whatever the order, which is what they are for.
    if (a.favourite !== b.favourite) return a.favourite ? -1 : 1;
    const ka = order.key(a);
    const kb = order.key(b);
    if (ka < kb) return -order.dir;
    if (ka > kb) return order.dir;
    // Name as the tie-break, so two games with the same rating do not swap
    // places between one redraw and the next.
    return a.name.localeCompare(b.name);
  });
  return copy;
}

/// Whether this view has anything worth sorting.
export function sortable() {
  return state.view !== "platforms" && state.view !== "systems";
}

let open = null;

/// The menu. Opened by the button in the header or by the right stick click.
export function openSortMenu(anchor) {
  if (open?.isConnected) {
    open.remove();
    open = null;
    return;
  }
  if (!sortable()) return;

  const menu = document.createElement("div");
  menu.id = "sort-menu";
  const now = currentOrder().id;
  menu.innerHTML = ORDERS.map(
    (o) =>
      `<button data-order="${o.id}" class="${o.id === now ? "on" : ""}">
         ${escapeHtml(o.label)}</button>`
  ).join("");

  const at = (anchor ?? el.sortBtn)?.getBoundingClientRect();
  menu.style.top = `${(at?.bottom ?? 40) + 4}px`;
  menu.style.right = `${Math.max(8, window.innerWidth - (at?.right ?? window.innerWidth))}px`;
  document.body.appendChild(menu);
  open = menu;

  const close = () => {
    menu.remove();
    open = null;
  };
  for (const b of menu.querySelectorAll("button")) {
    b.addEventListener("click", () => {
      setOrder(b.dataset.order);
      close();
      redraw();
    });
  }
  // Next frame, or the click that opened this closes it again.
  setTimeout(() => {
    window.addEventListener("pointerdown", close, { once: true });
  }, 0);
}

/// Step to the next order without opening anything, for the stick click.
export function cycleOrder(delta = 1) {
  if (!sortable()) return null;
  const at = ORDERS.findIndex((o) => o.id === currentOrder().id);
  const want = ORDERS[(at + delta + ORDERS.length) % ORDERS.length];
  setOrder(want.id);
  redraw();
  return want.label;
}

/// Redraw the current list in the new order.
///
/// Imported lazily to keep this module out of a cycle: library.js draws the
/// rows and needs the sort, and the sort needs to ask it to draw again.
async function redraw() {
  const { renderRows } = await import("./library.js");
  if (state.rows.length) renderRows(state.rows, state.view === "search");
  el.sortBtn?.querySelector("span:not(.icon)")?.replaceChildren(
    document.createTextNode(currentOrder().label)
  );
}

/// Keep the header button's label and visibility in step with the view.
export function refreshSortButton() {
  if (!el.sortBtn) return;
  el.sortBtn.hidden = !sortable();
  el.sortBtn.querySelector("span:not(.icon)")?.replaceChildren(
    document.createTextNode(currentOrder().label)
  );
}
