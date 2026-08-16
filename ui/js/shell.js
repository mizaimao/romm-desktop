// Where a view is drawn, and what the window looks like while it is.
//
// This exists so the layout is one decision rather than forty-three. Every
// screen used to set the chrome itself — hide Back, show Grid, show the zoom
// slider unless we are in list mode, set the title — six or seven imperative
// lines repeated in each of six functions. That is not merely repetitive: each
// copy states, in code, that this is a single-pane app with a back button.
// Changing the shape of the window meant editing every view that had an
// opinion about it, and they all did.
//
// A view now *describes* what it needs and hands over its content. This module
// decides what that means for the window that is actually on screen. A
// different arrangement — consoles down the left, games in the middle, the
// preview on the right, nothing hidden and no Back at all — is a different
// implementation of `enter` and `paint`, not a change to any view.
//
// Nothing in here knows what a console or a game is, and no view knows what a
// column is. That is the whole point of the split.

import { el } from "./state.js";

/// Where content goes, by role rather than by element.
///
/// `primary` is whatever holds the main list. `aside` is the detail beside it.
/// A three-column shell adds `nav` for the left column; a view that has
/// nothing to put there simply never asks for it, and asking for a region the
/// shell does not have is not an error — it is a region that is not shown.
const REGIONS = {
  primary: () => el.list,
  aside: () => el.detail,
};

const custom = new Map();

/// Point a role at a different element.
///
/// The seam a new layout is built through: a three-column shell registers its
/// own three, and every view carries on calling `paint("primary", …)`.
export function setRegion(role, node) {
  if (node) custom.set(role, node);
  else custom.delete(role);
}

export function region(role) {
  return custom.get(role) ?? REGIONS[role]?.() ?? null;
}

/// Draw `html` into a region. Missing regions are ignored rather than thrown
/// at: a layout without an aside is a layout, not a bug.
export function paint(role, html) {
  const node = region(role);
  if (!node) return null;
  node.innerHTML = html;
  return node;
}

/// The chrome a view wants.
///
/// Everything is optional and everything defaults to hidden, so a view lists
/// what it needs rather than what it does not. `zoom: "grid"` means "only when
/// the covers are on screen", which is the one conditional every view repeated
/// and got to decide for itself.
const CHROME = ["back", "layout", "sidebar", "grab", "sort"];

const HANDLES = {
  back: () => el.back,
  layout: () => el.layoutBtn,
  sidebar: () => el.sidebarBtn,
  grab: () => el.grabBtn,
  sort: () => el.sortBtn,
};

/// Put the window into the state this view wants.
///
/// A shell with no Back button, or no zoom slider, simply has nothing to set —
/// which is why every lookup tolerates a missing element instead of assuming
/// the skeleton this app happens to ship with.
export function enter({ title = "", zoom = false, gridLayout = true, ...wants } = {}) {
  for (const name of CHROME) {
    const node = HANDLES[name]?.();
    if (node) node.hidden = !wants[name];
  }
  if (el.zoomWrap) {
    // The one conditional worth keeping: the slider sizes covers, so it means
    // nothing in a list.
    el.zoomWrap.hidden = zoom === "grid" ? !gridLayout : !zoom;
  }
  if (el.title) el.title.textContent = title;
}

/// Show or hide the zoom slider on its own.
///
/// Switching between grid and list is a live change rather than entering a
/// view, so it does not go through `enter` — but it is the same question about
/// the same control, and a layout without a slider still has to survive being
/// asked.
export function showZoom(on) {
  if (el.zoomWrap) el.zoomWrap.hidden = !on;
}
