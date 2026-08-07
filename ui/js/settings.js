// Opening the Settings window from the main one.
//
// Everything that used to be here — the panel markup, the binding tables, the
// capture logic — moved to settings-panes.js and now runs inside its own
// window. What is left is the door.

import { invoke } from "./state.js";
import { toast } from "./util.js";

/// Nothing in the main window captures keys any more; that happens in the
/// settings window, against its own document. Kept so keys.js has one less
/// thing to know about which window it is in.
export function isCapturing() {
  return false;
}

export function captureKey() {
  return false;
}

/// There is no in-window panel to be open, so the main window never has to
/// change its key handling for one.
export function settingsOpen() {
  return false;
}

export function closeSettings() {}

export async function toggleSettings() {
  try {
    await invoke("open_settings");
  } catch (e) {
    toast(`Could not open Settings — ${e}`, 6000);
  }
}
