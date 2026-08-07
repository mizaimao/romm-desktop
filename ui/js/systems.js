// Per-system configuration, ES-DE style: which emulator core runs each
// platform, and which video shader it gets.

import { el, state, invoke } from "./state.js";
import { escapeHtml, toast } from "./util.js";

export async function showSystems() {
  state.view = "systems";
  el.back.hidden = false;
  el.detail.hidden = true;
  el.layoutBtn.hidden = true;
  el.sidebarBtn.hidden = true;
  el.zoomWrap.hidden = true;
  el.themesBtn.classList.remove("active");
  el.systemsBtn.classList.add("active");
  el.title.textContent = "Systems";
  el.list.innerHTML = `<div class="empty">Loading…</div>`;

  let rows;
  let motion = { current: null, options: [] };
  try {
    [rows, motion] = await Promise.all([invoke("systems"), invoke("motion_options")]);
  } catch (e) {
    el.list.innerHTML = `<div class="empty">${escapeHtml(String(e))}</div>`;
    return;
  }
  render(rows, motion);
}

function render(rows, motion) {
  el.list.innerHTML = `
    ${motionMarkup(motion)}
    <table class="systbl">
      <thead>
        <tr><th>System</th><th>Games</th><th>Display</th><th>Emulator</th><th>Shader</th>
            <th title="Aim with the mouse in light gun games">Light gun</th></tr>
      </thead>
      <tbody>${rows.map(rowMarkup).join("")}</tbody>
    </table>
    <p class="syshint">Changes are written to config.toml and apply to the next game you launch.
      Emulators not installed are marked — use <code>cores --install</code> to fetch them.</p>`;

  el.list.querySelectorAll('input[data-field="lightgun"]').forEach((box) =>
    box.addEventListener("change", async () => {
      const { slug, field } = box.dataset;
      try {
        toast(await invoke("set_system_choice", {
          slug,
          field,
          value: box.checked ? "on" : "off",
        }));
      } catch (e) {
        toast(String(e), 8000);
      }
    })
  );

  el.list.querySelectorAll("select").forEach((sel) =>
    sel.addEventListener("change", async () => {
      const { slug, field } = sel.dataset;
      try {
        // The motion layer is global, so it has its own command rather than a
        // per-system one. Everything else is keyed by platform slug.
        const msg = field === "motion"
          ? await invoke("set_motion_shader", { value: sel.value })
          : await invoke("set_system_choice", { slug, field, value: sel.value });
        toast(msg);
      } catch (e) {
        toast(String(e), 8000);
      }
    })
  );
}

/// The strobe/BFI layer. Deliberately above the table and separate from it: it
/// chains onto whatever shader each system already uses rather than replacing
/// one, so it is not a per-system choice and should not look like a column.
function motionMarkup(motion) {
  if (!motion?.options?.length) return "";
  const options = motion.options
    .map(
      (o) =>
        `<option value="${o.path}" ${o.path === motion.current ? "selected" : ""} title="${escapeHtml(
          o.note
        )}">${escapeHtml(o.label)} — ${escapeHtml(o.note)}</option>`
    )
    .join("");
  return `
    <div class="sysmotion">
      <label for="motion-sel"><strong>Motion layer</strong></label>
      <select id="motion-sel" data-field="motion">
        <option value="none" ${!motion.current ? "selected" : ""}>Off</option>
        ${options}
      </select>
      <p>Reduces the smearing an LCD gives 60fps content, by strobing across
         sub-frames. Chained on top of each system's own shader — it does not
         replace it. Best on a 120Hz+ display; CRT systems only.</p>
    </div>`;
}

function rowMarkup(s) {
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
               `<option value="${o.path}" ${o.path === s.shader ? "selected" : ""} title="${escapeHtml(
                 o.note
               )}">${escapeHtml(o.label)}</option>`
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
    <td class="sysname">${escapeHtml(s.name)}<div class="dim">${s.slug}</div></td>
    <td class="num">${s.rom_count}</td>
    <td><span class="badge ${s.display === "Handheld" ? "hh" : "crt"}">${s.display}</span></td>
    <td>${cores}</td>
    <td>${shaders}</td>
    <td>${gun}</td>
  </tr>`;
}
