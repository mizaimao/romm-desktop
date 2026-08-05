// Keyboard navigation, driven by the configurable bindings in bindings.js.
//
// Works off whatever is currently rendered rather than a parallel model: the
// selectable elements are `.card` (platforms), `.gcard`/`.row` (games) and
// `.tcard` (themes), so one implementation serves every view.

import { el, state, trail, invoke } from "./state.js";
import { selectRom, setSidebar, play } from "./detail.js";
import { showPlatforms, showRoms, setLayout } from "./library.js";
import { showThemes } from "./themes.js";
import { ACTIONS, actionFor, keyFor, keyLabel } from "./bindings.js";
import { captureKey, isCapturing, settingsOpen, closeSettings, toggleSettings } from "./settings.js";

function items() {
  return [...el.list.querySelectorAll(".card, .gcard, .row, .tcard")];
}

// Rows are derived from where things actually landed, not from a column count.
//
// The previous version measured the first row, then moved by +/- that number
// with the result clamped into range. Three things fell out of that: Up on the
// top row clamped to index 0, so you jumped to the first card instead of
// staying put; Down on the last row clamped to the final card the same way;
// and Left/Right ran off the end of a row into the next one. Grouped search
// results made it worse, since each console section can have its own card
// shape and the single column count no longer described the page.
//
// `offsetTop`/`offsetLeft` rather than getBoundingClientRect: they are
// relative to the layout, so the map stays valid while scrolling and only
// needs rebuilding when the list itself changes.
let rowCache = null;

export function resetNav() {
  rowCache = null;
}
window.addEventListener("resize", resetNav);

function rows() {
  const nodes = items();
  if (rowCache && rowCache.count === nodes.length && rowCache.width === el.list.clientWidth) {
    return rowCache.rows;
  }
  const buckets = [];
  for (const n of nodes) {
    const top = n.offsetTop;
    // A few px of jitter is normal between cards of differing height.
    let b = buckets.find((x) => Math.abs(x.top - top) <= 6);
    if (!b) {
      b = { top, nodes: [] };
      buckets.push(b);
    }
    b.nodes.push(n);
  }
  buckets.sort((a, b) => a.top - b.top);
  const out = buckets.map((b) => b.nodes.sort((x, y) => x.offsetLeft - y.offsetLeft));
  rowCache = { rows: out, count: nodes.length, width: el.list.clientWidth };
  return out;
}

function locate() {
  const grid = rows();
  for (let r = 0; r < grid.length; r++) {
    const c = grid[r].findIndex((n) => n.classList.contains("sel"));
    if (c >= 0) return { grid, r, c };
  }
  return { grid, r: -1, c: -1 };
}

function focusNode(node) {
  if (!node) return;
  // Games own their selection (it drives the detail pane); other views just
  // need the highlight.
  if (node.dataset.id) selectRom(Number(node.dataset.id));
  else items().forEach((n) => n.classList.toggle("sel", n === node));
  node.scrollIntoView({ block: "nearest" });
}

/// Left/right within the current row. Stops at the ends rather than spilling
/// into the neighbouring row, which is what made this feel random.
function moveX(step) {
  const { grid, r, c } = locate();
  if (!grid.length) return;
  if (r < 0) return focusNode(grid[0][0]);
  const row = grid[r];
  focusNode(row[Math.max(0, Math.min(c + step, row.length - 1))]);
}

/// Up/down a row, keeping the column you were in.
///
/// Matched on horizontal centre rather than index, so a short last row, a
/// row of differently-shaped cards, or the next console's section all land
/// somewhere that looks directly above or below where you were.
function moveY(step) {
  const { grid, r, c } = locate();
  if (!grid.length) return;
  if (r < 0) return focusNode(grid[0][0]);
  const target = r + step;
  if (target < 0 || target >= grid.length) return; // stay put at the edges
  const from = grid[r][c];
  const x = from.offsetLeft + from.offsetWidth / 2;
  let best = grid[target][0];
  let bestDist = Infinity;
  for (const n of grid[target]) {
    const d = Math.abs(n.offsetLeft + n.offsetWidth / 2 - x);
    if (d < bestDist) {
      bestDist = d;
      best = n;
    }
  }
  focusNode(best);
}

function edge(last) {
  const nodes = items();
  focusNode(last ? nodes[nodes.length - 1] : nodes[0]);
}

function activate() {
  const nodes = items();
  if (!nodes.length) return;
  // With nothing selected, focusedIndex is -1 and `nodes[-1]` is undefined, so
  // this used to return silently — pressing A on a freshly opened grid did
  // nothing at all until you nudged the stick first. Fall back to the first
  // item so the button always does something.
  const node = nodes[focusedIndex(nodes)] ?? nodes[0];
  if (node.dataset.slug) showRoms(node.dataset.slug);
  else if (node.dataset.id) invoke("rom_detail", { id: Number(node.dataset.id) }).then(play);
  // Collection cards already carry the navigation in their click handler,
  // which knows the group it came from; reuse it rather than duplicating.
  else if (node.dataset.cid || node.dataset.group) node.click();
  else if (node.dataset.repo) node.querySelector('button[data-act="icons"]')?.click();
}

function goBack() {
  if (state.view === "platforms") return;
  el.search.value = "";
  // Same one-level-at-a-time walk the Back button does, so a controller
  // behaves identically inside collections.
  const up = trail.pop();
  if (up) return up();
  el.themesBtn.classList.remove("active");
  el.collectionsBtn.classList.remove("active");
  showPlatforms();
}

function toggleHelp() {
  const existing = document.getElementById("shortcuts");
  if (existing) return existing.remove();

  const bound = ACTIONS.filter((a) => keyFor(a.id));
  const box = document.createElement("div");
  box.id = "shortcuts";
  box.innerHTML = `<div class="sc-panel"><h3>Keyboard</h3><dl>${bound
    .map((a) => `<dt>${keyLabel(keyFor(a.id))}</dt><dd>${a.label}</dd>`)
    .join("")}</dl><p>Rebind these in Settings · Esc to close</p></div>`;
  box.addEventListener("click", () => box.remove());
  document.body.appendChild(box);
}

export const HANDLERS = {
  // Movement is grid-aware now, so none of these need a column count: a list
  // is simply a grid one card wide and behaves correctly without a special case.
  left: () => moveX(-1),
  right: () => moveX(1),
  up: () => moveY(-1),
  down: () => moveY(1),
  pageUp: () => moveY(-3),
  pageDown: () => moveY(3),
  first: () => edge(false),
  last: () => edge(true),
  activate,
  back: goBack,
  back2: goBack,
  search: () => el.search.focus(),
  help: toggleHelp,
  download: () => {
    if (state.selected) import("./actions.js").then((m) => m.download(state.selected, false));
  },
  layout: () => {
    if (state.view === "roms" || state.view === "search") {
      setLayout(state.layout === "grid" ? "list" : "grid");
    }
  },
  sidebar: () => {
    if (state.view === "roms" || state.view === "search") setSidebar(!state.sidebar);
  },
  themes: () => (state.view === "themes" ? showPlatforms() : showThemes()),
};

/// Run an action by id, as the keyboard would. Shared with the gamepad.
export function runAction(id) {
  const handler = HANDLERS[id];
  if (handler) handler();
}

export function installKeys() {
  el.settingsBtn?.addEventListener("click", toggleSettings);

  window.addEventListener("keydown", (ev) => {
    // Rebinding swallows everything, so any key can be assigned.
    if (isCapturing() && captureKey(ev)) return;

    // The lightbox owns the keyboard while it is open.
    if (!el.lb.hidden) return;

    if (settingsOpen()) {
      if (ev.key === "Escape") {
        ev.preventDefault();
        closeSettings();
      }
      return;
    }

    const help = document.getElementById("shortcuts");
    if (help) {
      ev.preventDefault();
      return help.remove();
    }

    // Any other text field (the collection filter) owns the keyboard outright,
    // otherwise typing a name would fire single-key shortcuts.
    const focused = document.activeElement;
    if (focused !== el.search && focused?.tagName === "INPUT" && focused.type !== "range") {
      if (ev.key === "Escape") focused.blur();
      return;
    }

    // Typing in the search box: only Esc and Enter mean anything.
    if (focused === el.search) {
      if (ev.key === "Escape") {
        el.search.value = "";
        el.search.blur();
        goBack();
      } else if (ev.key === "Enter") {
        el.search.blur();
      }
      return;
    }

    // Leave platform and browser shortcuts alone, except ⌘F for search.
    if (ev.metaKey || ev.ctrlKey) {
      if (ev.key === "f") {
        ev.preventDefault();
        el.search.focus();
      }
      return;
    }

    const action = actionFor(ev.key);
    const handler = action && HANDLERS[action];
    if (!handler) return;
    ev.preventDefault();
    handler();
  });
}
