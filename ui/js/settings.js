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

/// Android has no second window, so settings is an overlay there.
///
/// Tauri will happily *build* a second webview window on Android and then has
/// no way to get rid of it: `close()` hangs without settling, and `destroy()`
/// and `hide()` return quietly and change nothing. All three measured on the
/// device. That left settings as a room with no door — the one thing a
/// handheld user cannot work around, because there is no title bar to click.
///
/// So on Android the same settings.html is loaded into a full-screen iframe in
/// this document instead. Same page, same code, same origin; the difference is
/// that removing an element is something the platform can actually do.
///
/// Desktop is untouched and still gets a real window: it is resizable, it can
/// sit beside the library, and none of the above applies there.
const MOBILE = /\bAndroid\b/.test(navigator.userAgent);
const OVERLAY_ID = "settings-overlay";

/// Whether a settings overlay is up in *this* document.
///
/// Always false on desktop, where settings is a separate window and this one
/// has nothing to close.
export function settingsOpen() {
  return document.getElementById(OVERLAY_ID) !== null;
}

export function closeSettings() {
  document.getElementById(OVERLAY_ID)?.remove();
}

export async function toggleSettings() {
  if (MOBILE) {
    if (settingsOpen()) return closeSettings();
    const frame = document.createElement("iframe");
    frame.id = OVERLAY_ID;
    frame.src = "settings.html";
    // Covering the whole viewport, above everything. Inline rather than a
    // stylesheet rule because this element exists on one platform and a rule
    // in style.css would be dead weight in every other build.
    frame.style.cssText =
      "position:fixed;inset:0;width:100%;height:100%;border:0;z-index:9999;";
    document.body.appendChild(frame);
    // The page inside asks to be closed rather than closing itself, because
    // from in there `window.close()` is the Tauri call that does nothing.
    frame.contentWindow?.focus?.();
    return;
  }
  try {
    await invoke("open_settings");
  } catch (e) {
    toast(`Could not open Settings — ${e}`, 6000);
  }
}

// The overlay asking to be taken down. Same origin, so the check is a
// formality, but an unchecked message handler on the main document is not a
// habit worth having.
window.addEventListener("message", (ev) => {
  if (ev.origin !== location.origin) return;
  if (ev.data === "close-settings") closeSettings();
});
