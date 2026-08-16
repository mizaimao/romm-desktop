// The Appearance tab: the pictures the lists draw, the console pictures, the
// glass, and the shader backdrop.
import { invoke, listen } from "../state.js";
import { toast, escapeHtml, cssColour } from "../util.js";
import { padFor, padLabel, keyFor, keyLabel, ACTIONS } from "../bindings.js";
import {
  backdropSupported, backdropSettings, saveBackdropSettings,
  backdropWanted, setBackdropWanted, SCHEMES,
  glassTint, setGlassTint, glassStrength, setGlassStrength,
} from "../backdrop.js";

export const html = `      <h4>Layout</h4>
      <div class="srow">
        <label>Window<span class="padmark" data-action="layout"></span></label>
        <div class="ctl">
          <select class="shell-mode">
            <option value="single">One pane — a screen at a time</option>
            <option value="columns">Three columns — consoles, games, preview</option>
          </select>
        </div>
      </div>
      <p class="hint">One pane shows a screen at a time and Back returns to the
        one before. Three columns keeps the consoles on the left, the games in
        the middle and the preview on the right, so nothing is ever replaced
        and there is nothing to go back to.</p>

      <h4>Artwork</h4>
      <div class="srow">
        <label>Game list shows<span class="padmark" data-action="pictures"></span></label>
        <div class="ctl"><select class="list-art"></select></div>
      </div>
      <p class="hint">What each game shows in the list and grid. Cartridge or
        disc by default — it is what you recognise a game by, and within one
        console they are all the same shape, so the grid stays even. Anything a
        console has no version of falls back to the miximage. The info pane
        always shows the miximage.</p>

      <h4>Console pictures</h4>
      <div class="srow">
        <label>Show<span class="padmark" data-action="pictures"></span></label>
        <div class="ctl"><div class="icon-styles"></div></div>
      </div>
      <p class="hint">What the console grid draws for each system. Styles with
        no pictures yet are greyed out — fetching gets them from four ES-DE
        themes at once, keeps the artwork, and throws the themes away. A few
        hundred kilobytes; after that, switching is instant and needs nothing.</p>
      <div class="srow">
        <label></label>
        <div class="ctl"><button class="set-icons">Get console pictures</button>
          <span class="set-icons-note"></span></div>
      </div>

      <h4>Color</h4>
      <div class="srow">
        <label>Scheme</label>
        <div class="ctl"><select class="scheme-preset"></select></div>
      </div>
      <p class="hint">One palette for the whole window: the glass over the
        cards and controls, and the gradient the shader draws behind them. These
        used to be two dropdowns, and every combination worth having was a
        matching pair. Custom sets all three colors separately.</p>
      <div class="srow">
        <label>Tint strength</label>
        <div class="ctl"><input class="glass-strength" type="range" min="0" max="60" step="2" />
          <span class="glass-strength-val"></span></div>
      </div>
      <p class="hint">How much of that color the glass carries. At 0 the glass
        is clear and only the blur remains.</p>
      <div class="srow bd-custom">
        <label>Glass color</label>
        <div class="ctl"><input class="glass-custom" type="color" /></div>
      </div>

      <h4>Shader backdrop</h4>
      <div class="srow">
        <label>Enabled</label>
        <div class="ctl"><button class="set-backdrop">Shader backdrop: off</button></div>
      </div>
      <div class="srow">
        <label>Motion</label>
        <div class="ctl"><input class="bd-speed" type="range" min="300" max="700" step="25" />
          <span class="bd-speed-val"></span></div>
      </div>
      <div class="srow">
        <label>Brightness</label>
        <div class="ctl"><input class="bd-strength" type="range" min="10" max="200" step="5" />
          <span class="bd-strength-val"></span></div>
      </div>
      <div class="srow bd-custom">
        <label>Dark color</label>
        <div class="ctl"><input class="bd-low" type="color" />
          <button class="bd-low-reset">Use theme</button></div>
      </div>
      <div class="srow bd-custom">
        <label>Light color</label>
        <div class="ctl"><input class="bd-high" type="color" />
          <button class="bd-high-reset">Use theme</button></div>
      </div>
      <p class="hint">Drawn on the GPU behind the library — not behind this
        window. Motion at 0 holds it still.</p>
      <p class="hint set-backdrop-status"></p>`;

/// Say which settings the controller can change without opening this window.
///
/// Cycling the artwork from the sofa is the reason several of these exist and
/// nothing here said so — the button is bound on the Control tab, two tabs
/// away, under a name ("Change the pictures") that does not obviously mean this
/// row. Read live rather than baked into the markup, so rebinding one is
/// reflected the next time this tab is opened.
function markPadControls(box) {
  for (const mark of box.querySelectorAll(".padmark")) {
    const id = mark.dataset.action;
    const action = ACTIONS.find((a) => a.id === id);
    const pad = padFor(id);
    const key = keyFor(id);
    if (pad === null && !key) {
      // Nothing bound to it: a badge pointing at a button that does not exist
      // is worse than no badge.
      mark.remove();
      continue;
    }
    mark.innerHTML = `<span class="icon icon-pad"></span>${escapeHtml(
      pad === null ? keyLabel(key) : padLabel(pad).split(" / ")[0]
    )}`;
    mark.title =
      `"${action?.label ?? id}" — changeable without opening this window.\n` +
      `Controller: ${padLabel(pad)}\nKeyboard: ${key ? keyLabel(key) : "unset"}\n` +
      `Both are rebindable on the Control tab.`;
  }
}

export function wire(box) {
  markPadControls(box);
  wireShellMode(box);
  wireIconStyles(box);

  // What the game lists draw. Populated from the backend rather than listed
  // here, so the names cannot drift from the directories they map to.
  const artSel = box.querySelector(".list-art");
  if (artSel) {
    invoke("list_art_options")
      .then(([choices, current]) => {
        artSel.innerHTML = choices
          .map(([k, label]) =>
            `<option value="${k}" ${k === current ? "selected" : ""}>${escapeHtml(label)}</option>`
          )
          .join("");
      })
      .catch(() => (artSel.disabled = true));
    // Same again for the game-list pictures, which Select cycles from inside a
    // console.
    listen("art-changed", () => {
      invoke("list_art_options")
        .then(([, current]) => {
          if (artSel.isConnected) artSel.value = current;
        })
        .catch(() => {});
    });
    artSel.addEventListener("change", async () => {
      try {
        toast(await invoke("set_list_art", { value: artSel.value }));
        // The library window redraws itself; this one cannot reach its DOM.
        window.__TAURI__?.event?.emit?.("art-changed");
      } catch (e) {
        toast(String(e), 8000);
      }
    });
  }

  // Shader backdrop. A switch rather than always-on: it runs a GPU loop for as
  // long as the app is open, which is not a cost to impose on someone who did
  // not ask for it.
  const bdBtn = box.querySelector(".set-backdrop");
  const bdStatus = box.querySelector(".set-backdrop-status");
  const paintBackdropButton = () => {
    const on = backdropWanted();
    bdBtn.textContent = `Shader backdrop: ${on ? "on" : "off"}`;
    bdBtn.classList.toggle("active", on);
  };
  if (!backdropSupported()) {
    bdBtn.disabled = true;
    bdBtn.textContent = "Shader backdrop: unavailable";
    bdStatus.textContent =
      "This machine's graphics driver does not offer WebGL2 to the app window.";
  } else {
    paintBackdropButton();
    bdBtn.addEventListener("click", () => {
      // Toggles the library window, not this one. This window has no library
      // behind it to put a backdrop on.
      const on = setBackdropWanted(!backdropWanted());
      bdStatus.textContent = on ? "On in the library window." : "";
      paintBackdropButton();
    });
  }

  // Live controls. Every change is applied to the running shader immediately —
  // a colour picker whose result you cannot see is not usable.
  const cfg = backdropSettings();
  const speed = box.querySelector(".bd-speed");
  const speedVal = box.querySelector(".bd-speed-val");
  const strength = box.querySelector(".bd-strength");
  const strengthVal = box.querySelector(".bd-strength-val");
  const low = box.querySelector(".bd-low");
  const high = box.querySelector(".bd-high");

  // One scheme for both surfaces. The glass tint applies to this window
  // immediately as well as being announced, so the effect is visible on the
  // control that changed it.
  const schemeSel = box.querySelector(".scheme-preset");
  const glassCustom = box.querySelector(".glass-custom");
  const strengthEl = box.querySelector(".glass-strength");
  const strengthOut = box.querySelector(".glass-strength-val");

  if (strengthEl) {
    strengthEl.value = String(glassStrength());
    strengthOut.textContent = `${glassStrength()}%`;
    strengthEl.addEventListener("input", () => {
      strengthOut.textContent = `${setGlassStrength(strengthEl.value)}%`;
    });
  }

  // The individual pickers only mean anything on "custom"; beside a preset
  // they invite changing one and watching the preset put it back.
  const showCustom = () => {
    const on = schemeSel.value === "custom";
    box.querySelectorAll(".bd-custom").forEach((r) => (r.hidden = !on));
  };

  schemeSel.innerHTML = SCHEMES.map(
    (c) => `<option value="${c.id}">${c.label}</option>`
  ).join("");
  schemeSel.value = SCHEMES.some((c) => c.id === cfg.preset) ? cfg.preset : "midnight";
  glassCustom.value = glassTint();
  showCustom();

  schemeSel.addEventListener("change", () => {
    const chosen = SCHEMES.find((c) => c.id === schemeSel.value);
    showCustom();
    if (!chosen || chosen.id === "custom") {
      // Keep whatever the three pickers already hold rather than blanking
      // them: "custom" means "leave this to me", not "start again".
      saveBackdropSettings({ preset: "custom" });
      return;
    }
    setGlassTint(chosen.glass);
    glassCustom.value = chosen.glass;
    low.value = chosen.low;
    high.value = chosen.high;
    saveBackdropSettings({ preset: chosen.id });
  });
  glassCustom.addEventListener("input", () => setGlassTint(glassCustom.value));

  const showValues = (c) => {
    speedVal.textContent = `${Math.round(c.speed * 100)}%`;
    strengthVal.textContent = `${Math.round(c.strength * 100)}%`;
  };
  speed.value = String(Math.round(cfg.speed * 100));
  strength.value = String(Math.round(cfg.strength * 100));
  // A colour input cannot show "unset", so an empty value reads back as the
  // theme's own colour and the reset button is what clears it again.
  low.value = cfg.low || cssColour("--bg", "#0d0d12");
  high.value = cfg.high || cssColour("--accent", "#2e3358");
  showValues(cfg);

  speed.addEventListener("input", () =>
    showValues(saveBackdropSettings({ speed: Number(speed.value) / 100 }))
  );
  strength.addEventListener("input", () =>
    showValues(saveBackdropSettings({ strength: Number(strength.value) / 100 }))
  );
  low.addEventListener("input", () => saveBackdropSettings({ low: low.value }));
  high.addEventListener("input", () => saveBackdropSettings({ high: high.value }));
  box.querySelector(".bd-low-reset").addEventListener("click", () => {
    saveBackdropSettings({ low: "" });
    low.value = cssColour("--bg", "#0d0d12");
  });
  box.querySelector(".bd-high-reset").addEventListener("click", () => {
    saveBackdropSettings({ high: "" });
    high.value = cssColour("--accent", "#2e3358");
  });
}

/// Which pictures the console grid uses, and how to get more.
///
/// This was a whole top-level panel with a gallery of ES-DE themes,
/// screenshots, and a full-download button. The app never rendered a theme —
/// it only ever read the per-system artwork out of one — so all that offered
/// was a choice that changed nothing and a download measured in hundreds of
/// megabytes. What is left is the part that does change the screen.
/// One pane or three columns.
///
/// Applied to the library window rather than this one — the setting is about
/// where things are drawn over there, and this window has no list, no consoles
/// and no preview to rearrange.
function wireShellMode(box) {
  const sel = box.querySelector(".shell-mode");
  if (!sel) return;
  sel.value = localStorage.getItem("romm.shell") === "columns" ? "columns" : "single";
  sel.addEventListener("change", () => {
    localStorage.setItem("romm.shell", sel.value);
    window.__TAURI__?.event?.emit?.("shell-mode", sel.value);
    toast(sel.value === "columns" ? "Three columns" : "One pane");
  });
}

async function wireIconStyles(box) {
  const holder = box.querySelector(".icon-styles");
  const note = box.querySelector(".set-icons-note");
  if (!holder) return;

  const draw = async () => {
    let styles = [];
    try {
      styles = await invoke("icon_styles");
    } catch {
      holder.textContent = "Could not read the installed pictures";
      return;
    }
    if (!box.isConnected) return;
    holder.innerHTML = styles
      .map(
        (s) =>
          `<button class="icon-style ${s.selected ? "on" : ""}" data-style="${escapeHtml(s.key)}"
             ${s.available ? "" : "disabled"}>${escapeHtml(s.label)}
             <em>${s.available}</em></button>`
      )
      .join("");
    for (const b of holder.querySelectorAll(".icon-style:not([disabled])")) {
      b.addEventListener("click", async () => {
        try {
          const label = await invoke("set_icon_style", { key: b.dataset.style });
          holder
            .querySelectorAll(".icon-style")
            .forEach((x) => x.classList.toggle("on", x === b));
          // The console grid is in the other window and cannot be reached from
          // here. Without this the choice only appeared after leaving the
          // console page and coming back, which reads as the setting not
          // having taken.
          window.__TAURI__?.event?.emit?.("icons-changed");
          toast(`Console pictures: ${label}`);
        } catch (e) {
          toast(String(e), 6000);
        }
      });
    }
  };
  await draw();

  // The pad changes this from the library window, which cannot reach this one.
  // Without the redraw the panel would sit there showing the style that was
  // selected when it opened, disagreeing with the screen behind it.
  listen("icons-changed", draw);

  box.querySelector(".set-icons")?.addEventListener("click", async (e) => {
    const btn = e.currentTarget;
    btn.disabled = true;
    // Four themes cloned one after another is a minute or so of nothing. It
    // says which one it is on, because a button that goes quiet for a minute
    // is a button people press again.
    const stop = await listen("icons-progress", ({ payload }) => {
      note.textContent = String(payload);
    });
    try {
      const summary = await invoke("fetch_icons");
      note.textContent = summary.split("\n")[0];
      window.__TAURI__?.event?.emit?.("icons-changed");
      toast(summary, 9000);
      await draw();
    } catch (err) {
      note.textContent = "";
      toast(`Could not fetch pictures — ${err}`, 9000);
    } finally {
      stop?.();
      btn.disabled = false;
    }
  });
}
