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

import { el, state } from "./state.js";
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

export function activeSection() {
  return current;
}

/// Switch to a section by id. Safe to call with an unknown id.
export async function showSection(id) {
  const section = SECTIONS.find((s) => s.id === id);
  if (!section) return;
  current = id;
  paint();
  // Leaving a section clears the crumb trail: the back button walks *within* a
  // section, and carrying a trail across a tab switch sends you somewhere you
  // were never looking at.
  state.collection = null;
  await section.open();
  paint();
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
