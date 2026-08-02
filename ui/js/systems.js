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
  try {
    rows = await invoke("systems");
  } catch (e) {
    el.list.innerHTML = `<div class="empty">${escapeHtml(String(e))}</div>`;
    return;
  }
  render(rows);
}

function render(rows) {
  el.list.innerHTML = `
    <table class="systbl">
      <thead>
        <tr><th>System</th><th>Games</th><th>Display</th><th>Emulator</th><th>Shader</th></tr>
      </thead>
      <tbody>${rows.map(rowMarkup).join("")}</tbody>
    </table>
    <p class="syshint">Changes are written to config.toml and apply to the next game you launch.
      Emulators not installed are marked — use <code>cores --install</code> to fetch them.</p>`;

  el.list.querySelectorAll("select").forEach((sel) =>
    sel.addEventListener("change", async () => {
      const { slug, field } = sel.dataset;
      try {
        toast(await invoke("set_system_choice", { slug, field, value: sel.value }));
      } catch (e) {
        toast(String(e), 8000);
      }
    })
  );
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

  return `<tr>
    <td class="sysname">${escapeHtml(s.name)}<div class="dim">${s.slug}</div></td>
    <td class="num">${s.rom_count}</td>
    <td><span class="badge ${s.display === "Handheld" ? "hh" : "crt"}">${s.display}</span></td>
    <td>${cores}</td>
    <td>${shaders}</td>
  </tr>`;
}
