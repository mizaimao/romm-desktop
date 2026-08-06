// Download and launch, kept separate from the pane that triggers them so the
// grid can call them on double-click without importing the whole sidebar.

import { state, invoke } from "./state.js";
import { toast } from "./util.js";

export async function launch(id) {
  try {
    toast("Launching…");
    // The connected pad's name picks which RetroArch autoconfig profile the
    // gamepad hotkeys are built from. Raw button indices differ per controller
    // and per OS, so guessing them is how "hold Select" ended up as "hold B".
    toast(await invoke("launch_rom", { id, pad: state.gamepad }));
  } catch (e) {
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
