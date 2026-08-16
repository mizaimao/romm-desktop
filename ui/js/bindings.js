// Keyboard bindings: defaults, persistence, and lookup.
//
// One binding per action. D/G/I/T are deliberately unbound out of the box —
// bare letters are easy to hit by accident, so those actions are opt-in via
// Settings.

const STORAGE_KEY = "keybindings";

/// Order here is the order shown in Settings.
export const ACTIONS = [
  { id: "left",    label: "Move left",            fallback: "ArrowLeft" },
  { id: "right",   label: "Move right",           fallback: "ArrowRight" },
  { id: "up",      label: "Move up",              fallback: "ArrowUp" },
  { id: "down",    label: "Move down",            fallback: "ArrowDown" },
  { id: "first",   label: "Jump to first",        fallback: "Home" },
  { id: "last",    label: "Jump to last",         fallback: "End" },
  { id: "pageUp",  label: "Page up",              fallback: "PageUp" },
  { id: "pageDown",label: "Page down",            fallback: "PageDown" },
  { id: "activate",label: "Open platform / play", fallback: "Enter" },
  { id: "back",    label: "Go back",              fallback: "Escape" },
  { id: "back2",   label: "Go back (alternate)",  fallback: "Backspace" },
  { id: "search",  label: "Focus search",         fallback: "/" },
  { id: "help",    label: "Shortcut list",        fallback: "?" },
  { id: "download",label: "Download without playing", fallback: null },
  { id: "layout",  label: "Toggle grid / list",   fallback: null },
  { id: "sidebar", label: "Toggle info pane",     fallback: null },
  { id: "settings",label: "Open settings",        fallback: null },
  { id: "prevSection", label: "Previous section",  fallback: "q" },
  { id: "nextSection", label: "Next section",      fallback: "e" },
  { id: "scrollUp",  label: "Scroll the list up",  fallback: null },
  { id: "scrollDown",label: "Scroll the list down",fallback: null },
  { id: "zoomIn",  label: "Bigger covers",         fallback: "+" },
  { id: "zoomOut", label: "Smaller covers",        fallback: "-" },
  { id: "video",   label: "Play gameplay video",   fallback: "v" },
  { id: "pictures",label: "Change the pictures",    fallback: null },
  { id: "sortCycle", label: "Next sort order",     fallback: null },
  { id: "sortMenu",  label: "Sort by…",            fallback: "s" },
  { id: "filterMenu",label: "Filter this list…",   fallback: "f" },
  { id: "random",    label: "Surprise me",         fallback: "r" },
];

/// Controller buttons, by W3C "standard mapping" index.
///
/// Separate from the keyboard map because the two are rebound independently —
/// and because an index means nothing without a name: 0 is the bottom face
/// button, which is A on Xbox, Cross on PlayStation and B on a Nintendo pad.
export const PAD_BUTTONS = [
  { index: 0,  name: "A / Cross (bottom face)" },
  { index: 1,  name: "B / Circle (right face)" },
  { index: 2,  name: "X / Square (left face)" },
  { index: 3,  name: "Y / Triangle (top face)" },
  { index: 4,  name: "L1 / LB" },
  { index: 5,  name: "R1 / RB" },
  { index: 6,  name: "L2 / LT" },
  { index: 7,  name: "R2 / RT" },
  { index: 8,  name: "Select / Share" },
  { index: 9,  name: "Start / Options" },
  { index: 10, name: "L3 (left stick)" },
  { index: 11, name: "R3 (right stick)" },
  { index: 12, name: "D-pad up" },
  { index: 13, name: "D-pad down" },
  { index: 14, name: "D-pad left" },
  { index: 15, name: "D-pad right" },
];

const PAD_KEY = "romm.pad";

/// Defaults, chosen by position rather than label so they read correctly on
/// every controller family.
const PAD_FALLBACK = {
  0: "activate",
  1: "back",
  // The shoulders move between sections — the navigation you use constantly,
  // and the one thing that should never need the cursor.
  4: "prevSection",
  5: "nextSection",
  // The triggers resize the covers. They are analog, so they are the closest
  // thing a pad has to the slider this replaces, and holding one sweeps the
  // whole range rather than stepping.
  // The triggers scroll the list, and how hard you pull decides how fast.
  // They were zoom, which is a thing you set once and then leave — a poor use
  // of the only two analogue controls on the pad, on a screen whose main job
  // is moving through two thousand games.
  6: "scrollUp",
  7: "scrollDown",
  // The top face button plays the gameplay video. It is the one thing ES-DE
  // has that is genuinely hard to find, so here it is on a button.
  3: "video",
  // Select cycles the pictures rather than opening settings. Settings is a
  // second window full of text fields and tables that a pad cannot navigate,
  // so the button opened something you then could not use and could only leave
  // again — whereas changing what the covers show is a thing you want to try
  // several times in a row while looking at them.
  8: "pictures",
  9: "help",
  // The left stick click steps through the sort orders, with the new one
  // named in a toast. The right one used to open the sort menu, which is a
  // list of items with no keyboard or pad navigation — so a controller could
  // open it and then only close it again. A button that opens something
  // unusable is worse than a button that does nothing, so it does nothing.
  10: "sortCycle",
  12: "up",
  13: "down",
  14: "left",
  15: "right",
};

function loadPad() {
  try {
    return JSON.parse(localStorage.getItem(PAD_KEY)) || {};
  } catch {
    return {};
  }
}

/// index -> action, user overrides layered over the defaults.
/// Actions the app is unusable without.
///
/// Anything else can be left unbound on purpose — plenty of people never want a
/// themes button on their pad. These four are different: with a direction
/// missing you cannot reach half the grid, and with Confirm missing you cannot
/// open anything at all.
const ESSENTIAL = ["up", "down", "left", "right", "activate"];

/// The resolved map, held between calls.
///
/// `padMap` is called from the controller poll, which runs on every animation
/// frame — 120 times a second on this display. Rebuilding it there meant a
/// synchronous localStorage read, a JSON parse, an object spread and a scan of
/// the result, at 120Hz, forever, for a value that changes only when somebody
/// rebinds a button. It was the app's largest idle cost.
let cached = null;

/// Forget the cached map. Called by everything that writes the bindings, and by
/// a storage event — the settings window is a separate document, so rebinding
/// there changes nothing this one can see until the browser tells it.
function forgetPadMap() {
  cached = null;
}
if (typeof window !== "undefined") {
  window.addEventListener("storage", (ev) => {
    if (!ev.key || ev.key === PAD_KEY) forgetPadMap();
  });
}

export function padMap() {
  if (cached) return cached;
  const map = { ...PAD_FALLBACK, ...loadPad() };

  // Rebinding clears whichever button previously held that action by writing
  // null over it. If that leaves an essential action with no button at all, the
  // pad is broken rather than customised — a direction that does nothing looks
  // exactly like an app ignoring the button, and there is nothing on screen to
  // say otherwise. Put the default back.
  for (const action of ESSENTIAL) {
    if (Object.values(map).includes(action)) continue;
    const home = Object.entries(PAD_FALLBACK).find(([, a]) => a === action)?.[0];
    // Only if its own default button is free, so healing one binding never
    // steals a button the user deliberately assigned to something else.
    if (home !== undefined && !map[home]) map[home] = action;
  }
  cached = map;
  return map;
}

/// Which button currently triggers `action`, or null.
export function padFor(action) {
  const entry = Object.entries(padMap()).find(([, a]) => a === action);
  return entry ? Number(entry[0]) : null;
}

/// Bind `action` to `index`, clearing whatever else held that button. A null
/// index unbinds.
export function setPad(action, index) {
  const custom = loadPad();
  for (const [i, a] of Object.entries(padMap())) {
    if (a === action) custom[i] = null;
  }
  if (index !== null) custom[index] = action;
  localStorage.setItem(PAD_KEY, JSON.stringify(custom));
  forgetPadMap();
}

export function resetPad() {
  localStorage.removeItem(PAD_KEY);
  forgetPadMap();
}

export function padLabel(index) {
  if (index === null || index === undefined) return "unset";
  return PAD_BUTTONS.find((b) => b.index === index)?.name ?? `button ${index}`;
}

function load() {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY)) || {};
  } catch {
    return {};
  }
}

let overrides = load();

/// Current key for an action, or null when unbound.
export function keyFor(id) {
  if (Object.prototype.hasOwnProperty.call(overrides, id)) return overrides[id];
  return ACTIONS.find((a) => a.id === id)?.fallback ?? null;
}

/// Action bound to a pressed key, or null. Case-insensitive for letters so a
/// binding works whether or not Shift is held.
export function actionFor(key) {
  const k = key.length === 1 ? key.toLowerCase() : key;
  for (const a of ACTIONS) {
    const bound = keyFor(a.id);
    if (!bound) continue;
    if ((bound.length === 1 ? bound.toLowerCase() : bound) === k) return a.id;
  }
  return null;
}

export function setKey(id, key) {
  // A key can only drive one action; clear whoever held it.
  if (key) {
    for (const a of ACTIONS) {
      if (a.id !== id && keyFor(a.id) === key) overrides[a.id] = null;
    }
  }
  overrides[id] = key;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides));
}

export function resetAll() {
  overrides = {};
  localStorage.removeItem(STORAGE_KEY);
}

/// Human label for a key, e.g. `ArrowLeft` -> `←`.
export function keyLabel(key) {
  if (!key) return "—";
  return (
    {
      ArrowLeft: "←", ArrowRight: "→", ArrowUp: "↑", ArrowDown: "↓",
      Escape: "Esc", Backspace: "⌫", Enter: "⏎", " ": "Space",
      PageUp: "PgUp", PageDown: "PgDn",
    }[key] || key.toUpperCase()
  );
}
