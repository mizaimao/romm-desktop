// Download and launch, kept separate from the pane that triggers them so the
// grid can call them on double-click without importing the whole sidebar.

import { state, invoke } from "./state.js";
import { toast } from "./util.js";
import { askConflicts, conflictsFrom, askOffline, offlineFrom } from "./conflicts.js";

/// Display refresh in Hz, measured rather than asked for: no web API reports
/// it, and Tauri's Monitor exposes size, position and scale factor but not
/// refresh rate. The median gap between animation frames is a good proxy —
/// median rather than mean so one hitched frame does not drag the estimate.
///
/// Cached after the first read: this costs ~24 frames, and a launch should not
/// wait on it twice.
let refreshHz = null;

function measureRefresh(frames = 24) {
  if (refreshHz !== null) return Promise.resolve(refreshHz);
  return new Promise((resolve) => {
    const times = [];
    let last = performance.now();
    const tick = (now) => {
      times.push(now - last);
      last = now;
      if (times.length < frames) return requestAnimationFrame(tick);
      times.sort((a, b) => a - b);
      const median = times[times.length >> 1];
      // Guard against a throttled or backgrounded window, which reports
      // frame gaps far outside any real refresh rate.
      refreshHz = median > 0.5 && median < 40 ? Math.round(1000 / median) : null;
      resolve(refreshHz);
    };
    requestAnimationFrame(tick);
  });
}

export async function launch(id, { resolving = false, skipSync = false } = {}) {
  try {
    toast("Launching…");
    // The connected pad's name picks which RetroArch autoconfig profile the
    // gamepad hotkeys are built from. Raw button indices differ per controller
    // and per OS, so guessing them is how "hold Select" ended up as "hold B".
    toast(
      await invoke("launch_rom", {
        id,
        pad: state.gamepad,
        refresh: await measureRefresh(),
        skipSync,
      })
    );
  } catch (e) {
    // A save that changed in two places stops the launch rather than picking a
    // winner. Ask, then start again — the second attempt syncs cleanly because
    // the conflict is gone by then.
    const conflicts = conflictsFrom(e);
    if (conflicts && !resolving) {
      const answered = await askConflicts(conflicts);
      if (!answered) return toast("Launch cancelled — saves left as they were");
      // `resolving` guards the retry: if it somehow conflicts again we report
      // it rather than reopening the dialog forever.
      return launch(id, { resolving: true });
    }
    // Saves could not be checked at all. Ask rather than deciding: starting
    // silently risks an hour on top of a stale save, and refusing would mean a
    // server being off stops you playing.
    const offline = offlineFrom(e);
    if (offline && !skipSync) {
      const go = await askOffline(offline);
      if (!go) return toast("Launch cancelled");
      return launch(id, { skipSync: true });
    }

    toast(`Launch failed — ${e}`, 8000);
  }
}

export async function download(id, thenPlay) {
  const prog = document.getElementById("prog");
  if (prog) prog.hidden = false;
  try {
    toast(await invoke("download_rom", { id }));
    // Refresh so "Local: yes" and the download marker update.
    const { selectRom } = await import("./detail.js");
    if (state.selected === id) await selectRom(id);
    if (thenPlay) await launch(id);
  } catch (e) {
    toast(`Download failed — ${e}`, 8000);
  } finally {
    if (prog) prog.hidden = true;
  }
}
