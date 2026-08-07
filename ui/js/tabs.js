// The top-level sections: Library, My collections, Browse.
//
// Three views that were previously reached through the same header button and a
// drill-down, which meant "my six hand-made collections" and "one thousand and
// forty companies" lived behind the same click and looked identical when you
// got there. They are different things and now sit side by side.
//
// Reachable with the shoulder buttons on a pad and Q/E on a keyboard, because
// this is the one piece of navigation you use constantly and it should not need
// the cursor.

import { el, state, trail } from "./state.js";
import { showPlatforms } from "./library.js";
import { showCollectionGroups, showCollectionsIn } from "./collections.js";

/// `user` and `smart` are collections someone made — by hand or as a saved
/// filter. Everything else RomM generates from metadata: genre, franchise,
/// company. Those are worth browsing but they are not *yours*, and the
/// distinction is the whole reason for two tabs rather than one.
export const SECTIONS = [
  {
    id: "library",
    label: "Library",
    open: () => showPlatforms(),
  },
  {
    id: "mine",
    label: "My collections",
    // Straight into the list rather than a group picker: there is only one
    // group behind this, so the picker was a screen with a single card on it.
    open: () => showCollectionsIn("user", "My collections"),
  },
  {
    id: "browse",
    label: "Browse",
    open: () => showCollectionGroups({ exclude: ["user"] }),
  },
];

let current = "library";

/// Where each section was when you left it: the crumb trail, the platform or
/// collection that was open, and how far down the list had scrolled.
///
/// Without this, switching tabs and switching back dropped you at the top of
/// the section's front page — so the shoulder buttons, which are meant to be
/// the cheapest navigation in the app, cost you your place every time.
const parked = new Map();

function park() {
  parked.set(current, {
    trail: [...trail],
    view: state.view,
    platform: state.platform,
    collection: state.collection,
    scroll: el.list.scrollTop,
    name: state.collectionName,
  });
}

/// Put a section back exactly as it was left. Returns false when there is
/// nothing parked, in which case the caller opens the section fresh.
async function unpark(id) {
  const was = parked.get(id);
  if (!was) return false;

  // Re-open whatever screen was showing rather than replaying the trail: the
  // data may have changed underneath, and a rebuild is cheap.
  const { showRoms, showPlatforms } = await import("./library.js");
  const { showCollectionRoms, showCollectionsIn, showCollectionGroups } =
    await import("./collections.js");

  if (was.view === "roms" && was.platform) await showRoms(was.platform);
  else if (was.view === "collection-roms" && was.collection)
    await showCollectionRoms(was.collection, was.name);
  else if (was.view === "collections" && id === "mine")
    await showCollectionsIn("user", "My collections");
  else if (was.view === "collections") await showCollectionGroups({ exclude: ["user"] });
  else if (was.view === "platforms") await showPlatforms();
  else return false;

  trail.length = 0;
  trail.push(...was.trail);
  // After the list has rendered, or the scroll target does not exist yet.
  requestAnimationFrame(() => {
    el.list.scrollTop = was.scroll;
  });
  return true;
}

export function activeSection() {
  return current;
}

/// Switch to a section by id. Safe to call with an unknown id.
export async function showSection(id, { force = false } = {}) {
  const section = SECTIONS.find((s) => s.id === id);
  if (!section) return;
  // Re-selecting the tab you are already on would park the section and then
  // immediately restore it, losing nothing but doing a pointless rebuild.
  // `force` is for the first call at startup, where there is nothing to park.
  if (id === current && !force) return;

  const from = SECTIONS.findIndex((s) => s.id === current);
  const to = SECTIONS.findIndex((s) => s.id === id);

  // Save where this section was before leaving it.
  park();
  current = id;
  paint();

  if (!(await unpark(id))) {
    // Nothing parked — first visit this session.
    state.collection = null;
    trail.length = 0;
    await section.open();
  }
  paint();
  if (!force) slide(to > from ? "right" : "left");
}

/// Play the page turn, in the direction the section moved.
///
/// Applied after the new content is in place rather than before: animating the
/// outgoing list would mean holding the old rows on screen while the new ones
/// load, and a bumper held down would queue transitions behind each other.
function slide(direction) {
  el.list.classList.remove("turn-left", "turn-right");
  // Forces the animation to restart when the same direction is played twice in
  // a row; without it the second press does nothing visible.
  void el.list.offsetWidth;
  el.list.classList.add(direction === "right" ? "turn-right" : "turn-left");
}

/// Move by `delta` through the sections, wrapping.
///
/// Wrapping because there are three of them and a pad user should not have to
/// remember which end they are at.
export function cycleSection(delta) {
  const at = SECTIONS.findIndex((s) => s.id === current);
  const next = (at + delta + SECTIONS.length) % SECTIONS.length;
  return showSection(SECTIONS[next].id);
}

function paint() {
  for (const btn of el.tabbar.querySelectorAll(".stab")) {
    const on = btn.dataset.id === current;
    btn.classList.toggle("active", on);
    btn.setAttribute("aria-selected", String(on));
  }
}

export function installTabs() {
  el.tabbar.replaceChildren();

  const lb = document.createElement("span");
  lb.className = "bumper";
  lb.textContent = "LB";
  el.tabbar.appendChild(lb);

  for (const s of SECTIONS) {
    const btn = document.createElement("button");
    btn.className = "stab";
    btn.dataset.id = s.id;
    btn.textContent = s.label;
    btn.setAttribute("role", "tab");
    btn.addEventListener("click", () => showSection(s.id));
    el.tabbar.appendChild(btn);
  }

  const rb = document.createElement("span");
  rb.className = "bumper";
  rb.textContent = "RB";
  el.tabbar.appendChild(rb);

  paint();
}
