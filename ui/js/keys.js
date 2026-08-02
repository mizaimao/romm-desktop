// Keyboard navigation, driven by the configurable bindings in bindings.js.
//
// Works off whatever is currently rendered rather than a parallel model: the
// selectable elements are `.card` (platforms), `.gcard`/`.row` (games) and
// `.tcard` (themes), so one implementation serves every view.

import { el, state, invoke } from "./state.js";
import { selectRom, setSidebar, play } from "./detail.js";
import { showPlatforms, showRoms, setLayout } from "./library.js";
import { showThemes } from "./themes.js";
import { ACTIONS, actionFor, keyFor, keyLabel } from "./bindings.js";
import { captureKey, isCapturing, settingsOpen, closeSettings, toggleSettings } from "./settings.js";

function items() {
  return [...el.list.querySelectorAll(".card, .gcard, .row, .tcard")];
}

/// Columns in the current grid, so arrows mean what they look like. Derived
/// from rendered positions rather than CSS, which uses auto-fill and varies
/// with window width.
function columns(nodes) {
  if (nodes.length < 2) return 1;
  const top = nodes[0].getBoundingClientRect().top;
  const n = nodes.findIndex((el) => el.getBoundingClientRect().top > top + 1);
  return n <= 0 ? nodes.length : n;
}

function focusedIndex(nodes) {
  const i = nodes.findIndex((n) => n.classList.contains("sel"));
  return i < 0 ? -1 : i;
}

function focus(nodes, index) {
  const node = nodes[Math.max(0, Math.min(index, nodes.length - 1))];
  if (!node) return;
  // Games own their selection (it drives the detail pane); other views just
  // need the highlight.
  if (node.dataset.id) selectRom(Number(node.dataset.id));
  else nodes.forEach((n) => n.classList.toggle("sel", n === node));
  node.scrollIntoView({ block: "nearest" });
}

function move(delta) {
  const nodes = items();
  if (!nodes.length) return;
  const cur = focusedIndex(nodes);
  focus(nodes, cur < 0 ? 0 : cur + delta);
}

function activate() {
  const nodes = items();
  const node = nodes[focusedIndex(nodes)];
  if (!node) return;
  if (node.dataset.slug) showRoms(node.dataset.slug);
  else if (node.dataset.id) invoke("rom_detail", { id: Number(node.dataset.id) }).then(play);
  else if (node.dataset.repo) node.querySelector('button[data-act="icons"]')?.click();
}

function goBack() {
  if (state.view === "platforms") return;
  el.search.value = "";
  el.themesBtn.classList.remove("active");
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

const HANDLERS = {
  left: () => move(-1),
  right: () => move(1),
  // A list is one column, so vertical movement steps one row there and a full
  // row in a grid.
  up: (cols) => move(-cols),
  down: (cols) => move(cols),
  pageUp: (cols) => move(-cols * 3),
  pageDown: (cols) => move(cols * 3),
  first: () => focus(items(), 0),
  last: () => { const n = items(); focus(n, n.length - 1); },
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

    // Typing in the search box: only Esc and Enter mean anything.
    if (document.activeElement === el.search) {
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
    handler(columns(items()));
  });
}
