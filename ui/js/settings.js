// Settings pane: emulator location and keyboard bindings.

import {
  ACTIONS, keyFor, setKey, resetAll, keyLabel,
  padFor, setPad, resetPad, padLabel,
} from "./bindings.js";
import { toast } from "./util.js";
import { invoke } from "./state.js";

/// Set while waiting for a keypress to assign, so the global handler can get
/// out of the way.
let capturing = null;

/// Set while waiting for a controller button. The Gamepad API has no
/// button-down event, so binding one means polling until something is pressed.
let padCapture = null;

export function isCapturing() {
  return capturing !== null || padCapture !== null;
}

export function settingsOpen() {
  return !!document.getElementById("settings");
}

export function closeSettings() {
  stopPadCapture();
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
        <button class="set-ra-pick" title="Choose a folder">Browse…</button>
        <button class="set-ra-save">Save</button>
      </div>
      <p class="hint set-ra-status"></p>

      <h4>Controller</h4>
      <p class="hint">Click a button to rebind it. Press the new button on the
        pad, or Esc to leave it unset.</p>
      <p class="hint pad-live">No controller detected.</p>
      <div class="set-rows">${ACTIONS.map(
        (a) => `
        <div class="set-row pad-row" data-id="${a.id}">
          <span class="set-label">${a.label}</span>
          <button class="set-pad ${padFor(a.id) === null ? "unset" : ""}">${padLabel(padFor(a.id))}</button>
        </div>`
      ).join("")}</div>
      <footer><button class="set-pad-reset">Reset controller</button></footer>

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
  box.querySelector(".set-ra-pick").addEventListener("click", async () => {
    try {
      // Invoked directly rather than imported from @tauri-apps/plugin-dialog:
      // frontendDist is ui/, so node_modules is not in the bundle and the
      // import fails there — taking the whole module graph, and the page, with
      // it.
      const dir = await invoke("plugin:dialog|open", {
        options: { directory: true, multiple: false,
                   title: "Select the RetroArch folder" },
      });
      if (dir) raInput.value = dir;
    } catch (e) {
      raStatus.textContent = String(e);
    }
  });
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

  // Keyboard rows only — the controller rows below carry .pad-row and hold no
  // .set-key, so an unscoped selector would find a null button and throw,
  // taking the whole panel with it.
  box.querySelectorAll(".set-row:not(.pad-row)").forEach((row) => {
    const btn = row.querySelector(".set-key");
    btn.addEventListener("click", () => {
      if (capturing) capturing.btn.textContent = keyLabel(keyFor(capturing.id));
      capturing = { id: row.dataset.id, btn };
      btn.textContent = "press a key…";
      btn.classList.add("capturing");
    });
  });

  box.querySelector(".set-pad-reset").addEventListener("click", () => {
    resetPad();
    closeSettings();
    toggleSettings();
    toast("Controller bindings reset");
  });

  // Live readout of what the pad actually reports. The defaults assume the
  // W3C "standard" layout, but a pad that reports a different mapping puts the
  // face buttons at other indices — in which case the bindings look right and
  // nothing responds. This makes that visible instead of a guessing game.
  const live = box.querySelector(".pad-live");
  const tick = () => {
    if (!document.getElementById("settings")) return;
    const pad = (navigator.getGamepads?.() ?? []).find(Boolean);
    if (!pad) {
      live.textContent = "No controller detected — press a button to wake it.";
    } else {
      const down = pad.buttons
        .map((b, i) => (b.pressed ? i : null))
        .filter((i) => i !== null);
      live.textContent =
        `${pad.id} · mapping: ${pad.mapping || "(none reported)"} · ` +
        `${pad.buttons.length} buttons · pressed: ${down.length ? down.join(", ") : "none"}`;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  box.querySelectorAll(".pad-row").forEach((row) => {
    const btn = row.querySelector(".set-pad");
    btn.addEventListener("click", () => {
      stopPadCapture();
      btn.textContent = "press a button…";
      btn.classList.add("capturing");
      startPadCapture(row.dataset.id, btn);
    });
  });

  document.body.appendChild(box);
}

function stopPadCapture() {
  if (!padCapture) return;
  cancelAnimationFrame(padCapture.raf);
  padCapture.btn.classList.remove("capturing");
  padCapture.btn.textContent = padLabel(padFor(padCapture.id));
  padCapture = null;
}

function startPadCapture(id, btn) {
  // Ignore whatever is already held from the click that got us here, so the
  // action is not instantly bound to the button still under the user's thumb.
  const settled = new Set();
  const step = () => {
    const pads = navigator.getGamepads?.() ?? [];
    for (const pad of pads) {
      if (!pad) continue;
      for (let i = 0; i < pad.buttons.length; i++) {
        const down = pad.buttons[i]?.pressed;
        if (!down) {
          settled.add(i);
          continue;
        }
        if (!settled.has(i)) continue; // held since before we started
        setPad(id, i);
        padCapture = null;
        btn.classList.remove("capturing");
        redrawPadRows();
        return;
      }
    }
    if (padCapture) padCapture.raf = requestAnimationFrame(step);
  };
  padCapture = { id, btn, raf: requestAnimationFrame(step) };
}

function redrawPadRows() {
  document.querySelectorAll("#settings .pad-row").forEach((row) => {
    const b = row.querySelector(".set-pad");
    if (b.classList.contains("capturing")) return;
    const i = padFor(row.dataset.id);
    b.textContent = padLabel(i);
    b.classList.toggle("unset", i === null);
  });
}

/// Consume a keypress as a new binding. Returns true when handled.
export function captureKey(ev) {
  if (padCapture) {
    if (ev.key !== "Escape") return true;   // swallow keys while binding a pad
    ev.preventDefault();
    setPad(padCapture.id, null);
    const { btn } = padCapture;
    padCapture = null;
    btn.classList.remove("capturing");
    redrawPadRows();
    return true;
  }
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
  document.querySelectorAll("#settings .set-row:not(.pad-row)").forEach((row) => {
    const b = row.querySelector(".set-key");
    if (b.classList.contains("capturing")) return;
    const k = keyFor(row.dataset.id);
    b.textContent = keyLabel(k);
    b.classList.toggle("unset", !k);
  });
  return true;
}
