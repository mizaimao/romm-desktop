// Narrowing a list down to the part you meant.
//
// The arcade console holds 2,506 games. Sorting them is not the same as
// finding one: "by rating" still leaves 2,506 rows, and the eleven you have
// actually played are somewhere in it. Every frontend has this and this one
// did not.
//
// Deliberately built out of what a row already carries — downloaded, starred,
// rated, its year, whether it has ever been played — so nothing has to be
// fetched and the filtering is instant on a list of any size. Genre is missing
// on purpose: the RomM browse tab is genres, done properly, as collections
// from the server.

import { el, state } from "./state.js";
import { showMenu } from "./menu.js";

/// The filters, in the order the menu offers them.
///
/// `keep` answers "does this row survive". Several can be on at once and they
/// all have to pass — "downloaded" and "never played" together is the list of
/// things taking up disk that you have not touched, which is a question worth
/// asking and cannot be asked any other way.
export const FILTERS = [
  { id: "local", label: "On this machine", keep: (r) => r.downloaded },
  { id: "missing", label: "Not downloaded", keep: (r) => !r.downloaded },
  { id: "fav", label: "Starred", keep: (r) => r.favourite },
  { id: "unplayed", label: "Never played", keep: (r) => !r.last_played },
  { id: "played", label: "Played before", keep: (r) => !!r.last_played },
  // 8/10 on RomM's scale. A "good games" filter with no number attached is a
  // filter nobody can predict the results of.
  { id: "great", label: "Rated 8 or better", keep: (r) => (r.rating ?? -1) >= 8 },
  // The first filter here about the *game* rather than about your relationship
  // to it. "Somebody is coming over" is a real question and the answer was
  // buried in a metadata blob.
  //
  // Unknown is excluded, not assumed: two thirds of this library has no player
  // count, and treating those as two-player would make the filter meaningless.
  { id: "twoplayer", label: "Two players or more", keep: (r) => (r.players ?? 0) >= 2 },
];

/// Pairs that cannot both be true. Choosing one clears the other rather than
/// leaving an empty list and no clue why — "on this machine" plus "not
/// downloaded" matches nothing, ever.
const OPPOSITE = { local: "missing", missing: "local", unplayed: "played", played: "unplayed" };

/// Chosen filters per view, for this run only.
///
/// The same reasoning as the game sort: "show me what I have not played on
/// this console" is a question about this console, asked now. Finding every
/// list still filtered a week later, with the reason forgotten, is a library
/// that looks like it has lost half its games.
const chosen = new Map();

function scope() {
  return `${state.view}:${state.platform ?? ""}:${state.collection ?? ""}`;
}

function active() {
  return chosen.get(scope()) ?? new Set();
}

export function activeFilters() {
  return [...active()];
}

export function toggleFilter(id) {
  const on = new Set(active());
  if (on.has(id)) on.delete(id);
  else {
    on.add(id);
    if (OPPOSITE[id]) on.delete(OPPOSITE[id]);
  }
  chosen.set(scope(), on);
  return on;
}

export function clearFilters() {
  chosen.set(scope(), new Set());
}

/// Apply them. Every filter has to pass; no filters means the list untouched.
export function filtered(rows) {
  const on = active();
  if (!on.size) return rows;
  const tests = FILTERS.filter((f) => on.has(f.id));
  return rows.filter((r) => tests.every((f) => f.keep(r)));
}

/// Whether this view has a list worth narrowing.
export function filterable() {
  return state.view !== "platforms" && state.view !== "systems" && state.view !== "history";
}

/// The button says how many are on, because a filtered list looks exactly like
/// a short one and there is nothing else on screen to say why.
export function refreshFilterButton() {
  if (!el.filterBtn) return;
  el.filterBtn.hidden = !filterable();
  const on = active().size;
  el.filterBtn.classList.toggle("on", on > 0);
  el.filterBtn
    .querySelector("span:not(.icon)")
    ?.replaceChildren(document.createTextNode(on ? `Filters · ${on}` : "Filter"));
}

/// The menu. Items are sticky: a filter is something you build up out of two
/// or three choices, and a menu that shuts on each one makes that four trips.
export function openFilterMenu(anchor) {
  if (!filterable()) return;
  const items = FILTERS.map((f) => ({
    label: `${active().has(f.id) ? "✓" : "  "} ${f.label}`,
    sticky: true,
    run: () => {
      toggleFilter(f.id);
      redraw();
      return true;
    },
  }));
  if (active().size) {
    items.push(null, {
      label: "Clear filters",
      run: () => {
        clearFilters();
        redraw();
      },
    });
  }
  const at = (anchor ?? el.filterBtn)?.getBoundingClientRect();
  showMenu(items, at?.left ?? 40, (at?.bottom ?? 40) + 4);
}

/// Redraw the current list. Imported lazily for the same reason the sort does:
/// library.js draws the rows and this has to ask it to draw them again.
async function redraw() {
  const { renderRows } = await import("./library.js");
  if (state.rows.length) renderRows(state.rows, state.view === "search");
  refreshFilterButton();
  // Rebuild the menu in place so the ticks move as they are pressed.
  if (document.querySelector(".ctx-menu")) openFilterMenu();
}
