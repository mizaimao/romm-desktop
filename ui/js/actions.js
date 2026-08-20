// Download and launch, kept separate from the pane that triggers them so the
// grid can call them on double-click without importing the whole sidebar.

import { state, invoke, listen } from "./state.js";
import { toast } from "./util.js";
import { askConflicts, conflictsFrom, askOffline, offlineFrom, askBios, biosFrom, noteLightGun } from "./conflicts.js";
import { suspendPad, resumePad } from "./gamepad.js";

/// Display refresh in Hz, measured rather than asked for: no web API reports
/// it, and Tauri's Monitor exposes size, position and scale factor but not
/// refresh rate. The median gap between animation frames is a good proxy —
/// median rather than mean so one hitched frame does not drag the estimate.
///
/// Cached after the first read: this costs ~24 frames, and a launch should not
/// wait on it twice. It is also taken at idle rather than on demand, because
/// the first launch of a session otherwise waited out those frames — most of a
/// second on a 30Hz-throttled window — before the request to start the game had
/// even been sent.
let refreshHz = null;

/// Take the measurement now, while nobody is waiting for it.
export function warmRefresh() {
  measureRefresh();
}

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

/// True from the moment a launch starts until the emulator has exited and the
/// pad has settled.
///
/// `launch_rom` does not return until the game quits, so a second press in the
/// meantime used to start a second copy — two RetroArch windows over each
/// other, both holding the same save. A held A repeats slowly enough that it
/// took a deliberate double press, which is exactly what a controller invites.
let launching = false;

export function launchInFlight() {
  return launching;
}

/// Launch, optionally picking up where the game was left.
///
/// `resume: true` means "carry on if there is something to carry on from" —
/// the newest resumable state is used, and a game with none simply starts. It
/// is not the same as `entrySlot`, which names one exact state the user
/// clicked; resume is a decision this function makes.
export async function launch(
  id,
  { resolving = false, skipSync = false, entrySlot = null, resume = false } = {}
) {
  if (resume && entrySlot === null) {
    try {
      const states = await invoke("game_states", { id });
      // Newest first is not guaranteed by the backend, so sort rather than
      // assume — and only slots that can actually be started from. RetroArch's
      // auto-save slot reports `resumable: false` because loading it is what
      // RetroArch does by itself, and starting *into* it double-loads.
      const best = states
        .filter((st) => st.resumable)
        .sort((a, b) => (b.when_epoch ?? 0) - (a.when_epoch ?? 0))[0];
      if (best) entrySlot = Number(best.slot);
    } catch {
      // No states, no RetroArch, no answer — start the game. A resume that
      // cannot be worked out is not a reason to refuse to play.
    }
  }
  // Say once, on the first gun console launched, that the mouse is the gun.
  // Before the guard below and before the pad goes quiet, because it is a
  // dialog the user answers rather than anything the launch depends on.
  if (!resolving && !skipSync) {
    try {
      // Shape-checked, not just truthy. `[]` is truthy in JavaScript, so a
      // backend — or a stub — answering with an empty array opened a dialog
      // with no text and no way to know what it was about, and the launch
      // waited on a promise nobody could resolve.
      const gun = await invoke("game_lightgun", { id });
      if (Array.isArray(gun) && gun.length === 2 && gun[1]) {
        await noteLightGun(gun[0], gun[1]);
      }
    } catch {
      // A console with no gun, or a backend that does not know: launch.
    }
  }
  // The retry paths below call back into this function on purpose, so they
  // pass through the guard rather than being stopped by it.
  if (launching && !resolving && !skipSync) return;
  launching = true;
  // And the pad goes quiet immediately, before anything is awaited: the press
  // that started this is still down, and the poll runs sixty times a second.
  suspendPad();
  // Declared out here so the error paths can drop the listener too: `const`
  // inside the try block is not in scope in the catch.
  let stop;
  try {
    toast("Launching…");
    // The connected pad's name picks which RetroArch autoconfig profile the
    // gamepad hotkeys are built from. Raw button indices differ per controller
    // and per OS, so guessing them is how "hold Select" ended up as "hold B".
    // The pad is very likely still held: this call does not return until the
    // emulator exits, and the way out of a game is a button combination.
    // Whatever is down belongs to that, not to us. (Suspended above as well,
    // before the first await — this one is the one that survives a retry.)
    suspendPad();
    stop = await listen("launch-progress", ({ payload }) => {
      toast(String(payload), 30_000);
    });
    const result = await invoke("launch_rom", {
      id,
      pad: state.gamepad,
      refresh: await measureRefresh(),
      skipSync,
      entrySlot,
    });
    stop?.();
    toast(result);
  } catch (e) {
    stop?.();
    // A save that changed in two places stops the launch rather than picking a
    // winner. Ask, then start again — the second attempt syncs cleanly because
    // the conflict is gone by then.
    const conflicts = conflictsFrom(e);
    if (conflicts && !resolving) {
      const answered = await askConflicts(conflicts);
      if (!answered) return toast("Launch cancelled — saves left as they were");
      // `resolving` guards the retry: if it somehow conflicts again we report
      // it rather than reopening the dialog forever.
      return launch(id, { resolving: true, entrySlot });
    }
    // A BIOS this machine has not got and the server cannot supply. The
    // automatic fetch on the way in handles every other case, so reaching here
    // means there is nothing left to download — ask, because a core that
    // declares a BIOS does not always need one.
    //
    // `entrySlot` is carried through: it is which save state the launch was
    // resuming from, and dropping it would restart the game from the
    // beginning after the user said "play anyway".
    const bios = biosFrom(e);
    if (bios && !skipSync) {
      const go = await askBios(bios);
      if (!go) return toast("Launch cancelled");
      return launch(id, { skipSync: true, entrySlot });
    }
    // Saves could not be checked at all. Ask rather than deciding: starting
    // silently risks an hour on top of a stale save, and refusing would mean a
    // server being off stops you playing.
    const offline = offlineFrom(e);
    if (offline && !skipSync) {
      const go = await askOffline(offline);
      if (!go) return toast("Launch cancelled");
      return launch(id, { skipSync: true, entrySlot });
    }

    toast(`Launch failed — ${e}`, 8000);
  } finally {
    // Whatever happened — played, cancelled, failed — the pad is locked for a
    // moment on the way out. It is the same lock the emulator's own exit
    // combination needs: the buttons that quit the game are still down when
    // this window gets them back.
    launching = false;
    // In `finally`, not after the await: a launch that throws — a save
    // conflict, a missing core, an unreachable server — would otherwise leave
    // the controller ignored for good, with no way back but restarting.
    resumePad();
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
