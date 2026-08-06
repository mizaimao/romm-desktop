// Settings pane: emulator location and keyboard bindings.

import { ACTIONS, keyFor, setKey, resetAll, keyLabel } from "./bindings.js";
import { toast } from "./util.js";
import { invoke } from "./state.js";

/// Set while waiting for a keypress to assign, so the global handler can get
/// out of the way.
let capturing = null;

export function isCapturing() {
  return capturing !== null;
}

export function settingsOpen() {
  return !!document.getElementById("settings");
}

export function closeSettings() {
  document.getElementById("settings")?.remove();
  capturing = null;
}

export function toggleSettings() {
  if (settingsOpen()) return closeSettings();

  const box = document.createElement("div");
  box.id = "settings";
  box.innerHTML = `
    <div class="set-panel" role="dialog" aria-label="Settings">
      <header>
        <h3>Settings</h3>
        <button class="set-close" title="Close">×</button>
      </header>

      <h4>RetroArch</h4>
      <p class="hint">Leave empty to search the usual locations. Set it when the
        install lives elsewhere, such as <code>E:\\Emulators\\RetroArch</code>.</p>
      <div class="set-path">
        <input class="set-ra" type="text" spellcheck="false"
               placeholder="path to the RetroArch folder" />
        <button class="set-ra-save">Save</button>
      </div>
      <p class="hint set-ra-status"></p>

      <h4>Keyboard</h4>
      <p class="hint">Click a key to rebind it. Press Esc while rebinding to leave it unset.</p>
      <div class="set-rows">${ACTIONS.map(
        (a) => `
        <div class="set-row" data-id="${a.id}">
          <span class="set-label">${a.label}</span>
          <button class="set-key ${keyFor(a.id) ? "" : "unset"}">${keyLabel(keyFor(a.id))}</button>
        </div>`
      ).join("")}</div>
      <footer>
        <button class="set-reset">Reset to defaults</button>
      </footer>
    </div>`;

  // Clicking outside the panel closes.
  box.addEventListener("click", (ev) => {
    if (ev.target === box) closeSettings();
  });
  box.querySelector(".set-close").addEventListener("click", closeSettings);

  // RetroArch location. The backend verifies the path before writing it to
  // config.toml, so an invalid one is reported here rather than failing later
  // at launch time.
  const raInput = box.querySelector(".set-ra");
  const raStatus = box.querySelector(".set-ra-status");
  invoke("status")
    .then((s) => {
      if (s?.retroarch) raInput.placeholder = s.retroarch;
      raStatus.textContent = s?.retroarch
        ? `Currently using ${s.retroarch} (${s.cores_installed} cores)`
        : "Not found. Set a path, or install RetroArch.";
    })
    .catch(() => {});
  box.querySelector(".set-ra-save").addEventListener("click", async () => {
    raStatus.textContent = "Checking…";
    try {
      raStatus.textContent = await invoke("set_retroarch_root", { path: raInput.value });
      toast("RetroArch path saved");
    } catch (e) {
      raStatus.textContent = String(e);
    }
  });
  box.querySelector(".set-reset").addEventListener("click", () => {
    resetAll();
    closeSettings();
    toggleSettings();
    toast("Keyboard bindings reset");
  });

  box.querySelectorAll(".set-row").forEach((row) => {
    const btn = row.querySelector(".set-key");
    btn.addEventListener("click", () => {
      if (capturing) capturing.btn.textContent = keyLabel(keyFor(capturing.id));
      capturing = { id: row.dataset.id, btn };
      btn.textContent = "press a key…";
      btn.classList.add("capturing");
    });
  });

  document.body.appendChild(box);
}

/// Consume a keypress as a new binding. Returns true when handled.
export function captureKey(ev) {
  if (!capturing) return false;
  ev.preventDefault();

  // Modifiers alone are not bindings.
  if (["Shift", "Control", "Alt", "Meta"].includes(ev.key)) return true;

  const key = ev.key === "Escape" ? null : ev.key;
  setKey(capturing.id, key);

  const { btn } = capturing;
  btn.classList.remove("capturing");
  btn.classList.toggle("unset", !key);
  btn.textContent = keyLabel(key);
  capturing = null;

  // Another row may have lost its key to this one; redraw them all.
  document.querySelectorAll("#settings .set-row").forEach((row) => {
    const b = row.querySelector(".set-key");
    if (b.classList.contains("capturing")) return;
    const k = keyFor(row.dataset.id);
    b.textContent = keyLabel(k);
    b.classList.toggle("unset", !k);
  });
  return true;
}
