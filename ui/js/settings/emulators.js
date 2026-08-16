// The Emulators tab: which emulator runs each console, which shader it gets,
// which screen the game opens on, and the shape of its window.
import { invoke } from "../state.js";
import { toast, escapeHtml } from "../util.js";
import { wireConfigFields } from "./fields.js";

export const html = `      <h4>Emulators</h4>
      <p class="hint">Which emulator runs each console, which shader it gets,
        and whether a light gun takes the second controller port. Changes are
        written to config.toml and apply to the next game you launch.</p>
      <h4>Game window</h4>
      <div class="srow">
        <label>Fit to the game</label>
        <div class="ctl"><button data-field="fit_window">…</button></div>
      </div>
      <p class="hint">RetroArch keeps a game's proportions inside whatever
        window it is given, so a window of the wrong shape is a window with
        black bars in it — and on a full-screen one those bars are wide. A
        window shaped like the game has nothing left over to put one in. Off
        fills the screen and lets the emulator letterbox inside it.</p>
      <div class="srow">
        <label>Title bar</label>
        <div class="ctl"><button data-field="window_decorations">…</button></div>
      </div>
      <p class="hint">Off gives a clean edge with no chrome at all. There is
        then nothing to drag and nothing to click to close, so the way out is
        the controller combination or Escape.</p>

      <div class="srow sys-screen" hidden>
        <label>Open games on</label>
        <div class="ctl"><select class="game-display"></select></div>
      </div>
      <p class="hint sys-screen-hint" hidden>Which screen a game opens on.
        Automatic prefers an external one: plugging a monitor into a laptop is a
        deliberate act, and rarely done wanting the game on the laptop panel.</p>
      <div class="sys-motion"></div>
      <div class="sys-table">Loading…</div>

`;

/// The per-system table: emulator, shader and light gun for each console.
///
/// Rendered after the pane is on screen rather than as part of its markup,
/// because it needs two round trips — the systems themselves and the list of
/// motion shaders — and a tab that waited on those before drawing anything
/// would look like a tab that does not open.
export async function wire(box) {
  const table = box.querySelector(".sys-table");
  let rows, motion;
  try {
    [rows, motion] = await Promise.all([invoke("systems"), invoke("motion_options")]);
  } catch (e) {
    table.textContent = String(e);
    return;
  }
  // A slow answer that arrives after the tab was left has nowhere to go, and
  // writing to a detached node would leave the next tab looking fine while its
  // controls belong to a pane that is gone.
  if (!box.isConnected) return;

  await wireGameDisplay(box);
  // The two window toggles are config.toml fields, like the ones in General.
  wireConfigFields(box);
  box.querySelector(".sys-motion").innerHTML = motionMarkup(motion);
  table.innerHTML = `
    <table class="systbl">
      <thead>
        <tr><th>System</th><th>Games</th><th>Display</th><th>Emulator</th><th>Shader</th>
            <th title="Aim with the mouse in light gun games">Light gun</th></tr>
      </thead>
      <tbody>${rows.map(systemRow).join("")}</tbody>
    </table>`;

  for (const gun of box.querySelectorAll('input[data-field="lightgun"]')) {
    gun.addEventListener("change", async () => {
      const { slug, field } = gun.dataset;
      try {
        toast(await invoke("set_system_choice", {
          slug,
          field,
          value: gun.checked ? "on" : "off",
        }));
      } catch (e) {
        toast(String(e), 8000);
      }
    });
  }

  for (const sel of box.querySelectorAll(".sys-table select, .sys-motion select")) {
    sel.addEventListener("change", async () => {
      const { slug, field } = sel.dataset;
      try {
        // The motion layer is global, so it has its own command rather than a
        // per-system one. Everything else is keyed by platform slug.
        toast(
          field === "motion"
            ? await invoke("set_motion_shader", { value: sel.value })
            : await invoke("set_system_choice", { slug, field, value: sel.value })
        );
      } catch (e) {
        toast(String(e), 8000);
      }
    });
  }
}

/// Which screen a game opens on.
///
/// Hidden entirely with one display attached: a dropdown with a single entry is
/// a question that has one answer, and this is the settings pane rather than an
/// inventory of the machine.
async function wireGameDisplay(box) {
  const row = box.querySelector(".sys-screen");
  const hint = box.querySelector(".sys-screen-hint");
  const sel = box.querySelector(".game-display");
  if (!sel) return;

  let screens = [];
  try {
    screens = await invoke("game_displays");
  } catch {
    return;
  }
  if (!screens.length || !box.isConnected) return;

  row.hidden = false;
  hint.hidden = false;
  sel.innerHTML = screens
    .map(
      (d) =>
        `<option value="${escapeHtml(d.key)}" ${d.selected ? "selected" : ""}>
           ${escapeHtml(d.label)}</option>`
    )
    .join("");
  sel.addEventListener("change", async () => {
    try {
      toast(await invoke("set_config_field", { field: "game_display", value: sel.value }));
    } catch (e) {
      toast(`Could not save — ${e}`, 8000);
    }
  });
}

/// The strobe/BFI layer. Above the table and separate from it: it chains onto
/// whatever shader each system already uses rather than replacing one, so it
/// is not a per-system choice and should not look like a column.
function motionMarkup(motion) {
  if (!motion?.options?.length) return "";
  const options = motion.options
    .map(
      (o) =>
        `<option value="${o.path}" ${o.path === motion.current ? "selected" : ""}>${escapeHtml(
          o.label
        )} — ${escapeHtml(o.note)}</option>`
    )
    .join("");
  return `
    <div class="srow">
      <label>Motion layer</label>
      <div class="ctl">
        <select data-field="motion">
          <option value="none" ${!motion.current ? "selected" : ""}>Off</option>
          ${options}
        </select>
      </div>
    </div>
    <p class="hint">Reduces the smearing an LCD gives 60fps content by blanking
      the screen between frames. That is flicker by construction, and it only
      reads as sharper motion on a display locked to a fixed refresh at an exact
      multiple of 60 — on a variable-refresh panel such as ProMotion the black
      frames land unevenly and it simply flickers. Chained on top of each
      system's own shader rather than replacing it. CRT systems only.</p>`;
}

function systemRow(s) {
  const cores = s.emulators.length
    ? `<select data-slug="${s.slug}" data-field="core">${s.emulators
        .map(
          (e) =>
            `<option value="${e.core}" ${e.core === s.core ? "selected" : ""}>${escapeHtml(
              e.label
            )}${e.installed ? "" : " — not installed"}${e.is_default ? " (default)" : ""}</option>`
        )
        .join("")}</select>`
    : `<span class="dim">none known</span>`;

  const shaders = s.shaders.length
    ? `<select data-slug="${s.slug}" data-field="shader">
         <option value="none" ${!s.shader ? "selected" : ""}>None</option>
         ${s.shaders
           .map(
             (o) =>
               `<option value="${o.path}" ${o.path === s.shader ? "selected" : ""}>${escapeHtml(
                 o.label
               )}</option>`
           )
           .join("")}
       </select>`
    : `<span class="dim">no RetroArch</span>`;

  // Only for consoles that had a gun, and off by default: on most of them the
  // gun goes in the port a second pad would use, so leaving it on everywhere
  // would quietly break two-player games.
  const gun = s.gun
    ? `<label class="gun" title="${escapeHtml(s.gun)} in place of a pad — aim with the mouse, left button fires">
         <input type="checkbox" data-slug="${s.slug}" data-field="lightgun" ${s.gun_on ? "checked" : ""} />
         <span>${escapeHtml(s.gun)}</span>
       </label>`
    : `<span class="dim">—</span>`;

  return `<tr>
    <td class="sysname">${escapeHtml(s.name)}<div class="dim">${escapeHtml(s.slug)}</div></td>
    <td class="num">${s.rom_count}</td>
    <td><span class="badge ${s.display === "Handheld" ? "hh" : "crt"}">${escapeHtml(s.display)}</span></td>
    <td>${cores}</td>
    <td>${shaders}</td>
    <td>${gun}</td>
  </tr>`;
}
