// The sidebar: artwork, metadata, and the play/download actions.

import { el, state, invoke, convertFileSrc } from "./state.js";
import { human, escapeHtml, row, starBar, toast } from "./util.js";
import { openLightbox, detailMedia, setOpenHook } from "./lightbox.js";
import { launch, download } from "./actions.js";

let slideTimer;
setOpenHook(() => clearInterval(slideTimer));

/// Show/hide the pane, honouring both the toggle and the current view.
export function setSidebar(on) {
  state.sidebar = on;
  localStorage.setItem("sidebar", on ? "on" : "off");
  el.sidebarBtn.textContent = on ? "Hide info" : "Show info";
  // Never show the pane on the platform or collection-list screens — nothing
  // is selected there. Games reached through a collection do get it.
  const allowed =
    state.view === "roms" || state.view === "search" || state.view === "collection-roms";
  el.detail.hidden = !(on && allowed && state.selected !== null);
}

export async function selectRom(id) {
  state.selected = id;
  el.list.querySelectorAll(".row, .gcard").forEach((r) =>
    r.classList.toggle("sel", Number(r.dataset.id) === id)
  );

  const [d, cores] = await Promise.all([
    invoke("rom_detail", { id }),
    // Never fatal: a missing core list should not blank the whole pane.
    invoke("game_cores", { id }).catch(() => []),
  ]);
  // A later click may have superseded this one; do not paint a stale game.
  if (state.selected !== id) return;
  const shots = d.screenshots || [];

  // Screenshots on top (cycled when there are several), cover below.
  const top = shots.length
    ? `<div class="shots" id="shots">
         ${shots
           .map((s, i) => `<img src="${convertFileSrc(s)}" class="${i === 0 ? "on" : ""}" alt="" />`)
           .join("")}
         ${
           shots.length > 1
             ? `<div class="dots">${shots
                 .map((_, i) => `<span class="${i === 0 ? "on" : ""}"></span>`)
                 .join("")}</div>
                <button class="nav prev">‹</button><button class="nav next">›</button>`
             : ""
         }
       </div>`
    : d.video
      ? `<video src="${convertFileSrc(d.video)}" controls muted loop></video>`
      : "";

  const cover = d.cover ? `<img class="cover" src="${convertFileSrc(d.cover)}" alt="" />` : "";
  const video =
    shots.length && d.video
      ? `<video src="${convertFileSrc(d.video)}" controls muted loop></video>`
      : "";

  el.detail.hidden = !state.sidebar;
  el.detail.innerHTML = `
    <div class="scroll">
      <h2>${escapeHtml(d.name)}</h2>
      <div class="sub">${escapeHtml(d.fs_name)}</div>
      ${top}
      ${cover}
      ${video}
      ${d.rating ? starBar(d.rating) : ""}
      ${d.summary ? `<p class="summary">${escapeHtml(d.summary)}</p>` : ""}
      <dl>
        ${row("Released", d.release_year)}
        ${row("Genre", d.genres.join(", "))}
        ${row("Developer", d.companies.join(", "))}
        ${row("Series", d.franchises.join(", "))}
        ${row("Players", d.player_count)}
        ${row("Modes", d.game_modes.join(", "))}
        ${row("Region", d.regions.join(", "))}
        ${row("Also known as", d.alt_names.join(" · "))}
        <dt>Platform</dt><dd>${d.platform}</dd>
        <dt>Size</dt><dd>${human(d.size_bytes)}</dd>
        <dt>Core</dt><dd>${corePicker(cores, d)}</dd>
        <dt>Local</dt><dd>${d.downloaded ? "yes" : "no"}</dd>
      </dl>
      ${artStrip(d)}
      ${
        d.manual || d.youtube_id
          ? `<div class="extras">
               ${d.manual ? `<button class="link" id="manual">📖 Manual</button>` : ""}
               ${d.youtube_id ? `<a class="link" target="_blank"
                   href="https://www.youtube.com/watch?v=${encodeURIComponent(d.youtube_id)}">▶ Trailer</a>` : ""}
             </div>`
          : ""
      }
    </div>
    <div class="pinned">
      <div class="actions">
        <button class="primary" id="play">${d.downloaded ? "Play" : "Download & Play"}</button>
        <button class="ghost" id="dl" ${d.downloaded ? "disabled" : ""}>Download</button>
      </div>
      <progress id="prog" hidden></progress>
    </div>`;

  if (shots.length > 1) startSlideshow(shots.length);
  wireArtwork(d);

  document.getElementById("play").addEventListener("click", () => play(d));
  document.getElementById("dl").addEventListener("click", () => download(d.id, false));
  wireCorePicker(d.id);
}

/// A dropdown of the cores this game can run under.
///
/// Per-game rather than per-platform because arcade romsets are mixed: the
/// platform default is a best guess, and individual games need to escape it.
function corePicker(cores, d) {
  if (!cores.length) {
    return d.core_label ? escapeHtml(d.core_label) : "<em>none installed</em>";
  }
  const pinned = cores.some((c) => c.pinned);
  return `<select id="core-pick" title="Core used to launch this game">
      <option value=""${pinned ? "" : " selected"}>Platform default</option>
      ${cores
        .map(
          (c) => `<option value="${escapeHtml(c.core)}"${c.pinned ? " selected" : ""}>
            ${escapeHtml(c.label)}${c.installed ? "" : " (not installed)"}${
              c.current && !c.pinned ? " — default" : ""
            }</option>`
        )
        .join("")}
    </select>`;
}

function wireCorePicker(id) {
  const pick = document.getElementById("core-pick");
  if (!pick) return;
  pick.addEventListener("change", async () => {
    try {
      const msg = await invoke("set_game_core", { id, core: pick.value });
      toast(msg);
    } catch (e) {
      toast(String(e));
    }
  });
}

/// Human labels for ES-DE media types, in the order worth showing.
const ART_ORDER = [
  ["miximages", "Mix"],
  ["3dboxes", "3D box"],
  ["backcovers", "Back"],
  ["titlescreens", "Title"],
  ["marquees", "Marquee"],
  ["physicalmedia", "Cart/disc"],
  ["fanart", "Fan art"],
];

/// Thumbnail row of everything ES-DE has for this game beyond the cover.
function artStrip(d) {
  const items = ART_ORDER.filter(([k]) => d.art && d.art[k]);
  if (!items.length) return "";
  return `<div class="artstrip">${items
    .map(
      ([k, label]) =>
        `<figure data-art="${k}" title="${label}">
           <img src="${convertFileSrc(d.art[k])}" alt="${label}" loading="lazy" />
           <figcaption>${label}</figcaption>
         </figure>`
    )
    .join("")}</div>`;
}

/// Clicking artwork opens it full size, starting at what was clicked.
function wireArtwork(d) {
  const media = detailMedia(d);
  const openAt = (pred) => () => {
    const i = media.findIndex(pred);
    openLightbox(media, i < 0 ? 0 : i);
  };

  el.detail.querySelector(".shots")?.addEventListener("click", (ev) => {
    // Leave the slideshow's own arrows and dots alone.
    if (ev.target.closest(".nav, .dots")) return;
    const shown = el.detail.querySelector(".shots img.on");
    const idx = [...el.detail.querySelectorAll(".shots img")].indexOf(shown);
    openLightbox(media, Math.max(idx, 0));
  });
  el.detail.querySelector("img.cover")?.addEventListener("click", openAt((m) => m.caption === "Cover"));
  el.detail.querySelectorAll("video").forEach((v) =>
    v.addEventListener("dblclick", openAt((m) => m.kind === "video"))
  );
  el.detail.querySelectorAll(".artstrip figure").forEach((fig) =>
    fig.addEventListener("click", () => {
      const kind = fig.dataset.art;
      const label = ART_ORDER.find(([k]) => k === kind)?.[1] ?? kind;
      openLightbox([{ src: convertFileSrc(d.art[kind]), kind: "image", caption: label }], 0);
    })
  );
  document.getElementById("manual")?.addEventListener("click", () =>
    openLightbox([{ src: convertFileSrc(d.manual), kind: "pdf", caption: "Manual" }], 0)
  );
}

function startSlideshow(count) {
  clearInterval(slideTimer);
  const box = document.getElementById("shots");
  if (!box) return;
  const imgs = [...box.querySelectorAll("img")];
  const dots = [...box.querySelectorAll(".dots span")];
  let i = 0;

  const show = (n) => {
    i = (n + count) % count;
    imgs.forEach((im, k) => im.classList.toggle("on", k === i));
    dots.forEach((dt, k) => dt.classList.toggle("on", k === i));
  };
  const auto = () => {
    clearInterval(slideTimer);
    slideTimer = setInterval(() => show(i + 1), 3500);
  };

  box.querySelector(".prev")?.addEventListener("click", () => { show(i - 1); auto(); });
  box.querySelector(".next")?.addEventListener("click", () => { show(i + 1); auto(); });
  dots.forEach((dt, k) => dt.addEventListener("click", () => { show(k); auto(); }));
  // Pause while the pointer is over the image, so it can be studied.
  box.addEventListener("mouseenter", () => clearInterval(slideTimer));
  box.addEventListener("mouseleave", auto);
  auto();
}

/// Enter / primary button: play, downloading first if needed.
export async function play(d) {
  if (d.downloaded) return launch(d.id);
  return download(d.id, true);
}
