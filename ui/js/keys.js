// Keyboard navigation, driven by the configurable bindings in bindings.js.
//
// Works off whatever is currently rendered rather than a parallel model: the
// selectable elements are `.card` (platforms), `.gcard`/`.row` (games) and
// `.tcard` (themes), so one implementation serves every view.

import { el, state, trail, invoke } from "./state.js";
import { selectRom, setSidebar, play, playVideo, showPlatformInfo } from "./detail.js";
import {
  showPlatforms, setLayout, setZoom, openPlatform, scrollList, randomGame,
} from "./library.js";
import { escapeHtml, toast } from "./util.js";
import {
  closeLightbox, zoomLightbox, stepLightbox, isLightboxOpen, togglePlayback,
} from "./lightbox.js";
import { cyclePictures } from "./pictures.js";
import { openSortMenu, cycleOrder } from "./sort.js";
import { openFilterMenu } from "./filter.js";
import { actions, actionFor, keyLabelFor, padMap, padLabelFor, padLabel } from "./bindings.js";
import { captureKey, isCapturing, settingsOpen, closeSettings, toggleSettings } from "./settings.js";
import { cycleSection, resetSection } from "./tabs.js";

function items() {
  // Not the ones the page filter has hidden: the cursor stepping onto a card
  // that is not on screen looks like the cursor disappearing.
  return [...el.list.querySelectorAll(".card, .gcard, .row, .tcard")].filter(
    (n) => !n.classList.contains("filtered-out")
  );
}

// Where the cursor goes next is worked out in `src/gridnav.rs`, from where the
// cards actually landed rather than from a column count. The version that
// replaced measured the first row and moved by plus or minus that number with
// the result clamped into range, and three things fell out of it: Up on the
// top row clamped to index 0, so you jumped to the first card instead of
// staying put; Down on the last row clamped to the final card the same way;
// and Left/Right ran off the end of a row into the next one. Grouped search
// results made it worse, since each console section can have its own card
// shape and one column count no longer described the page.
//
// The geometry goes over once per rebuild, not once per keypress. Reading
// `offsetTop` out of the page forces a layout, and a held direction repeats
// nine times a second across 2,506 arcade cards.
//
// `offsetTop`/`offsetLeft` rather than getBoundingClientRect: they are
// relative to the layout, so the map stays valid while scrolling and only
// needs rebuilding when the list itself changes.
let sent = null;

export function resetNav() {
  sent = null;
}
window.addEventListener("resize", resetNav);

async function syncGeometry(nodes) {
  const shape = `${nodes.length}:${el.list.clientWidth}`;
  if (sent === shape) return;
  await invoke("set_grid", {
    cards: nodes.map((n) => [n.offsetTop, n.offsetLeft, n.offsetWidth]),
  });
  sent = shape;
}

/// Move the cursor, and put it on whatever the backend chose.
///
/// A null answer means stay put, which is the whole point of the geometry:
/// running off the top of a grid leaves you where you were.
async function step(axis, delta) {
  const nodes = items();
  if (!nodes.length) return;
  await syncGeometry(nodes);
  const at = nodes.findIndex((n) => n.classList.contains("sel"));
  const to = await invoke("grid_move", {
    selected: at < 0 ? null : at,
    axis,
    step: delta,
  });
  if (to !== null && to !== undefined) focusNode(nodes[to]);
}

function focusNode(node) {
  if (!node) return;
  // Games own their selection (it drives the preview); a console highlights
  // itself and puts its own facts in the pane, which is what that pane shows
  // on the Library screen.
  if (node.dataset.id) selectRom(Number(node.dataset.id));
  else {
    items().forEach((n) => n.classList.toggle("sel", n === node));
    if (node.dataset.slug) showPlatformInfo(node.dataset.slug);
  }
  node.scrollIntoView({ block: "nearest" });
}

/// Left/right within the current row. Stops at the ends rather than spilling
/// into the neighbouring row, which is what made this feel random.
const moveX = (delta) => step("x", delta);

/// Up/down a row, keeping the column you were in.
///
/// Matched on horizontal centre rather than index, so a short last row, a row
/// of differently-shaped cards, or the next console's section all land
/// somewhere that looks directly above or below where you were.
const moveY = (delta) => step("y", delta);

const edge = (last) => step("edge", last ? 1 : -1);

/// Index of the selected card, or -1. Lost when linear navigation was replaced
/// with the grid-aware version below, while `activate` kept calling it — so
/// every press of Enter or A threw a ReferenceError, inside a rAF callback
/// where nothing surfaces it.
function focusedIndex(nodes) {
  return nodes.findIndex((n) => n.classList.contains("sel"));
}

function activate() {
  const nodes = items();
  if (!nodes.length) return;
  // With nothing selected, focusedIndex is -1 and `nodes[-1]` is undefined, so
  // this used to return silently — pressing A on a freshly opened grid did
  // nothing at all until you nudged the stick first. Fall back to the first
  // item so the button always does something.
  const node = nodes[focusedIndex(nodes)] ?? nodes[0];
  // Through openPlatform, not showRoms: the controller should get the same
  // transition the mouse does, and a click handler is not the place to keep it.
  if (node.dataset.slug) openPlatform(node.dataset.slug, node);
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
  // The section you are in, not always the platform grid — Back from inside a
  // collection belongs in My collections, not the library.
  resetSection();
}

function toggleHelp() {
  const existing = document.getElementById("shortcuts");
  if (existing) return existing.remove();

  // Both columns, side by side. This listed the keyboard and then said
  // "Controller" over a live readout of button indices — which answers "is this
  // pad working" and not "what does this button do", the question somebody
  // opens a help page to ask. An action with a button and no key belongs here
  // too, so the filter is either-or rather than keyboard-only.
  const bound = actions()
    .map((a) => ({
      ...a,
      key: keyLabelFor(a.id),
      pad: padLabelFor(a.id),
    }))
    // "—" and "unset" are what the two labels say for an action nobody bound.
    .filter((a) => a.key !== "—" || a.pad !== "unset");

  const box = document.createElement("div");
  box.id = "shortcuts";
  box.innerHTML = `<div class="sc-panel">
    <h3>Controls</h3>
    <table class="sc-table">
      <thead><tr><th>Action</th><th>Keyboard</th><th>Controller</th></tr></thead>
      <tbody>${bound
        .map(
          (a) => `<tr>
            <td>${escapeHtml(a.label)}</td>
            <td>${a.key === "—" ? "<span class=\"dim\">—</span>" : escapeHtml(a.key)}</td>
            <td>${a.pad === "unset" ? "<span class=\"dim\">—</span>" : escapeHtml(a.pad)}</td>
          </tr>`
        )
        .join("")}</tbody>
    </table>
    <p class="pad-readout">No controller detected.</p>

    <h3>What the marks mean</h3>
    <table class="sc-table sc-marks">
      <tbody>
        <tr><td><span class="mark here"><span class="icon icon-disk"></span></span></td>
            <td>On this machine — ready to play with the server off</td></tr>
        <tr><td><span class="mark away"><span class="icon icon-cloud"></span></span></td>
            <td>On the server — downloads when you play it</td></tr>
        <tr><td><span class="star">★</span></td>
            <td>Starred, in one of your starred collections</td></tr>
        <tr><td><span class="dot on"></span></td>
            <td>An emulator for this console is installed</td></tr>
        <tr><td><span class="dot"></span></td>
            <td>No emulator — games on this console will not start</td></tr>
        <tr><td><span class="bumper">LB</span></td>
            <td>The shoulder buttons move between these tabs</td></tr>
      </tbody>
    </table>

    <p>Rebind these in Settings · Esc to close</p></div>`;
  box.addEventListener("click", () => box.remove());
  document.body.appendChild(box);

  // A live readout of what the pad actually reports, here rather than only in
  // Settings. "Button X does nothing" is unanswerable without it: the bindings
  // are by index, and an index that is not the one you think it is looks
  // exactly like an app that ignores the button.
  const out = box.querySelector(".pad-readout");
  // Every 60ms rather than every animation frame. The body of this rebuilds a
  // paragraph of markup, and doing that at the display's refresh rate is text
  // layout 120 times a second for something a thumb changes a few times.
  let last = "";
  const tick = () => {
    if (!box.isConnected) return;
    setTimeout(tick, 60);
    const pad = (navigator.getGamepads?.() ?? []).find(Boolean);
    if (!pad) return;
    // The index alone is not the answer. A rebind clears whatever else held
    // that button by writing null, so a button can be reported as pressed and
    // still be bound to nothing — which looks exactly like the app ignoring it.
    const map = padMap();
    const down = pad.buttons
      .map((b, i) => (b.pressed ? i : null))
      .filter((i) => i !== null)
      .map((i) => {
        const action = map[i];
        const label = action
          ? actions().find((a) => a.id === action)?.label || action
          : "<em>not bound</em>";
        return `${i} → ${label}`;
      });
    const axes = pad.axes
      .map((v, i) => (Math.abs(v) > 0.35 ? `axis${i}: ${v.toFixed(2)}` : null))
      .filter(Boolean);
    const html =
      `<strong>${escapeHtml(pad.id)}</strong><br>` +
      `mapping: ${pad.mapping || "(none reported)"} · ${pad.buttons.length} buttons<br>` +
      `pressed: <strong>${down.length ? down.join(" · ") : "nothing"}</strong>` +
      (axes.length ? `<br>${axes.join(" · ")}` : "") +
      `<br><span class="hint">A button that says "not bound" is why it does ` +
      `nothing — Settings · Control · Reset controller puts the defaults back.</span>`;
    if (html !== last) {
      last = html;
      out.innerHTML = html;
    }
  };
  tick();
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
  settings: toggleSettings,
  prevSection: () => cycleSection(-1),
  nextSection: () => cycleSection(1),
  download: () => {
    if (state.selected) import("./actions.js").then((m) => m.download(state.selected, false));
  },
  layout: () => {
    // Wherever the button is offered, the binding works too.
    if (el.layoutBtn.hidden) return;
    setLayout(state.layout === "grid" ? "list" : "grid");
  },
  sidebar: () => {
    if (state.view === "roms" || state.view === "search") setSidebar(!state.sidebar);
  },
  video: playVideo,
  // A press worth of scrolling, for a digital trigger or a key. A pad with
  // analogue triggers never comes through here — see gamepad.js, which reads
  // how far they are pulled and scrolls by that instead.
  scrollUp: () => scrollList(-120),
  scrollDown: () => scrollList(120),
  pictures: cyclePictures,
  sortMenu: () => openSortMenu(),
  filterMenu: () => openFilterMenu(),
  random: () => randomGame(),
  sortCycle: () => {
    const now = cycleOrder(1);
    if (now) toast(`Sorted by ${now}`);
  },
  // The triggers zoom whatever is in front of you: the picture on the stage
  // when the lightbox is open, the covers in the grid when it is not.
  zoomIn: () => (isLightboxOpen() ? zoomLightbox(1) : nudgeZoom(1)),
  zoomOut: () => (isLightboxOpen() ? zoomLightbox(-1) : nudgeZoom(-1)),
};

/// Step the cover size, staying inside the slider's own range.
///
/// Reads the bounds off the slider rather than repeating them, so the pad and
/// the slider can never disagree about how big "biggest" is.
function nudgeZoom(direction) {
  if (el.zoomWrap.hidden) return;
  const step = Number(el.zoom.step) || 10;
  const min = Number(el.zoom.min) || 90;
  const max = Number(el.zoom.max) || 300;
  setZoom(Math.min(max, Math.max(min, (state.zoom || min) + direction * step)));
}

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

    // Settings, the way every desktop opens settings: Cmd+, on macOS and
    // Ctrl+, everywhere else. Checked before anything else, including the
    // lightbox and the text fields, because it is the one shortcut people
    // expect to work from wherever they happen to be.
    if (ev.key === "," && (ev.metaKey || ev.ctrlKey) && !ev.altKey) {
      ev.preventDefault();
      toggleSettings();
      return;
    }

    // The lightbox owns the keyboard while it is open — but not entirely. The
    // button that opened it has to close it again: pressing Y to start a video
    // and then having to reach for the mouse to stop it is the worst kind of
    // half-finished. Zoom works in there too, on the same keys.
    if (!el.lb.hidden) {
      // Space, on anything with a video on it. It is the one key every video
      // player on every platform agrees about, and the alternative was aiming
      // at a control bar that fades out. Not bindable, for the same reason:
      // this is muscle memory rather than a preference.
      if (ev.key === " " && togglePlayback()) {
        ev.preventDefault();
        return;
      }
      const action = actionFor(ev.key);
      if (action === "video" || action === "back") {
        ev.preventDefault();
        closeLightbox();
      } else if (action === "zoomIn" || action === "zoomOut") {
        ev.preventDefault();
        zoomLightbox(action === "zoomIn" ? 1 : -1);
      } else if (action === "left" || action === "right") {
        // Walk the reel: artwork, screenshots, cover, video. Owned here rather
        // than in the lightbox because these are bindable, and a second handler
        // reading the raw arrow keys would step twice for anyone who left them
        // as they are and not at all for anyone who moved them.
        ev.preventDefault();
        stepLightbox(action === "right" ? 1 : -1);
      }
      return;
    }

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
