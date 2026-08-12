// ES-DE theme browser: download themes and pick which per-system art to use.

import { el, state, invoke } from "./state.js";
import { human, escapeHtml, toast } from "./util.js";

export async function showThemes() {
  state.view = "themes";
  el.back.hidden = false;
  el.detail.hidden = true;
  el.layoutBtn.hidden = true;
  el.grabBtn.hidden = true;
  el.sidebarBtn.hidden = true;
  el.zoomWrap.hidden = true;
  el.themesBtn.classList.add("active");
  el.systemsBtn.classList.remove("active");
  el.title.textContent = "Console icon themes";
  el.list.innerHTML = `<div class="empty">Loading themes…</div>`;

  let themes;
  try {
    themes = await invoke("themes_available");
  } catch (e) {
    el.list.innerHTML =
      `<div class="empty">Could not load the themes list.<br>${escapeHtml(String(e))}</div>`;
    return;
  }

  let styles = [];
  try {
    styles = await invoke("icon_styles");
  } catch (e) {
    // Non-fatal: the style picker just will not render.
  }
  render(themes, styles);
}

function render(themes, styles) {
  const bar = styles.length
    ? `<div class="stylebar">
         <span class="lbl">Console icons:</span>
         ${styles
           .map(
             (s) =>
               `<button data-style="${s.key}" class="${s.selected ? "on" : ""}" ${
                 s.available ? "" : "disabled"
               }>${escapeHtml(s.label)} <span style="opacity:.6">${s.available}</span></button>`
           )
           .join("")}
       </div>`
    : "";

  el.list.innerHTML = bar + `<div class="themes">${themes
    .map(
      (t) => `
      <div class="tcard ${t.installed ? "on" : ""}" data-repo="${t.reponame}">
        <div class="shot">${
          t.screenshot ? `<img src="${t.screenshot}" alt="" loading="lazy" />` : ""
        }</div>
        <div class="body">
          <div class="tname">${escapeHtml(t.name)}
            ${t.installed ? `<span class="tbadge">· installed ${human(t.size_bytes)}</span>` : ""}
          </div>
          <div class="tby">${escapeHtml(t.author || "unknown")}${
            t.variants?.length ? ` · ${t.variants.length} variants` : ""
          }</div>
          <div class="tacts">
            <button class="go" data-act="icons">Use icons</button>
            <button data-act="${t.installed ? "remove" : "full"}">${
              t.installed ? "Remove" : "Full download"
            }</button>
          </div>
        </div>
      </div>`
    )
    .join("")}</div>`;

  el.list.querySelectorAll(".stylebar button").forEach((b) =>
    b.addEventListener("click", async () => {
      try {
        const label = await invoke("set_icon_style", { key: b.dataset.style });
        el.list.querySelectorAll(".stylebar button").forEach((x) =>
          x.classList.toggle("on", x === b)
        );
        toast(`Console icons: ${label}`);
      } catch (e) {
        toast(String(e), 6000);
      }
    })
  );

  el.list.querySelectorAll(".tcard button").forEach((b) =>
    b.addEventListener("click", (ev) => {
      ev.stopPropagation();
      themeAction(b.closest(".tcard").dataset.repo, b.dataset.act, b);
    })
  );
}

async function themeAction(reponame, act, btn) {
  const card = btn.closest(".tcard");
  card.querySelectorAll("button").forEach((b) => (b.disabled = true));
  const original = btn.textContent;
  btn.textContent = act === "remove" ? "Removing…" : "Working…";
  try {
    const msg =
      act === "remove"
        ? await invoke("theme_remove", { reponame })
        // "Use icons" clones, extracts the platform logos, then deletes the
        // checkout — themes run to hundreds of MB and we render a few hundred KB.
        : await invoke("theme_download", { reponame, logosOnly: act === "icons" });
    toast(msg, 6000);
    await showThemes();
  } catch (e) {
    toast(`${reponame} — ${e}`, 9000);
    btn.textContent = original;
    card.querySelectorAll("button").forEach((b) => (b.disabled = false));
  }
}
