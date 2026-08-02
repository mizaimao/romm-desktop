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
  { id: "themes",  label: "Open themes",          fallback: null },
];

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
