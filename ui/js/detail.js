// The sidebar: artwork, metadata, and the play/download actions.

import { el, state, invoke, convertFileSrc, rememberRom } from "./state.js";
import { tintFor } from "./tint.js";
import { human, escapeHtml, row, starBar, toast } from "./util.js";
import { openLightbox, detailMedia, setOpenHook } from "./lightbox.js";
import { launch, download } from "./actions.js";

let slideTimer;
setOpenHook(() => clearInterval(slideTimer));

/// Show/hide the pane, honouring both the toggle and the current view.
export function setSidebar(on) {
  state.sidebar = on;
  localStorage.setItem("sidebar", on ? "on" : "off");
  // The button now holds an icon plus a label span; writing textContent on
  // the button itself would wipe the icon out.
  el.sidebarBtn.querySelector("span:not(.icon)").textContent = on ? "Hide info" : "Show info";
  el.sidebarBtn.querySelector(".icon").className = `icon icon-info-${on ? "on" : "off"}`;
  // Never show the pane on the platform or collection-list screens — nothing
  // is selected there. Games reached through a collection do get it.
  const allowed =
    state.view === "roms" || state.view === "search" || state.view === "collection-roms";
  el.detail.hidden = !(on && allowed && state.selected !== null);
}

export async function selectRom(id) {
  // The card the cursor is moving to, tagged so the browser can carry it into
  // the detail pane's cover rather than cross-fading two unrelated images.
  // Tagged before the class changes, since the old tag has to be cleared in the
  // same frame or two elements claim the same name and the transition is
  // skipped entirely.
  const card = el.list.querySelector(`[data-id="${id}"] .art img`);
  const previous = document.querySelector('[style*="view-transition-name: cover"]');
  if (previous) previous.style.viewTransitionName = "";
  if (card) card.style.viewTransitionName = "cover";

  state.selected = id;
  rememberRom(id);
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

  // No slideshow and no player at the top any more: the miximage below already
  // carries a screenshot, the box and the logo in one picture, so cycling
  // through screenshots above it was the same information twice.
  const top = "";

  // The other half of the morph: same name as the tagged card art, so the
  // browser treats them as one element moving rather than two fading.
  const cover = d.cover
    ? `<img class="cover" style="view-transition-name: cover" src="${convertFileSrc(d.cover)}" alt="" />`
    : "";

  // An indicator, not a player. ES-DE starts the video by itself after a pause
  // and plays its audio, which leaves you muting the emulator to browse and
  // then having no sound when you actually want to watch something. So this
  // says a video exists and waits to be asked — and because being asked is
  // explicit, it plays with sound.
  const video = d.has_video
    ? `<button id="playvid" class="vidbtn" title="Play the gameplay video (Y)">
         <span class="icon icon-play"></span><span>Gameplay video</span>
       </button>`
    : "";

  el.detail.hidden = !state.sidebar;
  // The glow around the selection takes the cover's own colour. Started here
  // rather than awaited: the pane must not wait on a canvas read, so the
  // colour arrives a frame or two later and simply transitions in.
  applyTint(id, d.cover ? convertFileSrc(d.cover) : null);
  // Wrapped so the browser can match the tagged card art to the pane's cover
  // and move one into the other, instead of replacing the pane wholesale.
  await withTransition(() => {
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
      <div id="prog-text" hidden></div>
    </div>`;
  });

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

/// Play the selected game's video, fetching it first if it is not here yet.
///
/// Exported because the controller reaches it too — this is the button ES-DE
/// buries in a sidebar, and burying it again behind a mouse would miss the
/// point.
export async function playVideo() {
  const id = state.selected;
  if (id === null) return;
  const btn = document.getElementById("playvid");
  if (!btn) return; // no video for this game
  if (btn.dataset.busy) return;

  btn.dataset.busy = "1";
  const label = btn.querySelector("span:not(.icon)");
  const was = label?.textContent;
  if (label) label.textContent = "Fetching…";
  try {
    const path = await invoke("game_video", { id });
    // Still the same game? A slow fetch must not open a video over whatever
    // was scrolled to in the meantime.
    if (state.selected !== id) return;
    openLightbox([{ src: convertFileSrc(path), kind: "video", caption: "Gameplay" }], 0);
  } catch (e) {
    toast(String(e), 6000);
  } finally {
    delete btn.dataset.busy;
    if (label && was) label.textContent = was;
  }
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
  document.getElementById("playvid")?.addEventListener("click", playVideo);
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

/// A drag handle between the list and the detail pane.
///
/// The pane was a fixed 360px, which cropped cover art and wrapped most
/// metadata lines. It is wider by default now and the width is remembered, so
/// this is set once rather than every session.
export function installDetailResizer() {
  const grip = document.createElement("div");
  grip.id = "detail-grip";
  grip.setAttribute("role", "separator");
  grip.setAttribute("aria-orientation", "vertical");
  grip.title = "Drag to resize";
  el.detail.parentNode.insertBefore(grip, el.detail);

  const saved = Number(localStorage.getItem("detailWidth"));
  if (saved) applyWidth(saved);

  let startX = 0;
  let startW = 0;

  const onMove = (ev) => {
    // Dragging left widens the pane: it grows from its left edge.
    applyWidth(startW + (startX - ev.clientX));
  };
  const onUp = () => {
    document.removeEventListener("pointermove", onMove);
    document.removeEventListener("pointerup", onUp);
    grip.classList.remove("dragging");
    document.body.classList.remove("resizing-detail");
    localStorage.setItem("detailWidth", String(el.detail.getBoundingClientRect().width | 0));
  };

  grip.addEventListener("pointerdown", (ev) => {
    ev.preventDefault();
    startX = ev.clientX;
    startW = el.detail.getBoundingClientRect().width;
    grip.classList.add("dragging");
    document.body.classList.add("resizing-detail");
    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
  });

  // Double-click resets, so a pane dragged off screen is recoverable without
  // clearing storage by hand.
  grip.addEventListener("dblclick", () => {
    document.documentElement.style.removeProperty("--detail-w");
    localStorage.removeItem("detailWidth");
  });
}

function applyWidth(px) {
  // Clamped in code as well as in CSS: the CSS bound stops it rendering wrong,
  // this stops a nonsense value being written to storage.
  const max = Math.min(window.innerWidth * 0.7, 900);
  const clamped = Math.max(300, Math.min(px, max));
  document.documentElement.style.setProperty("--detail-w", `${clamped}px`);
}

/// Paint the selection in the colours of the game's own box art.
///
/// Set on the two elements that show a selection — the pane and the selected
/// card or row — rather than on the document, so nothing else in the window
/// shifts colour and there is nothing to unset when the selection moves.
///
/// Guarded by the id: a fast scroll starts one of these per game and they can
/// finish out of order, so a slow read for a game you have already scrolled
/// past must not repaint the one you are now on.
async function applyTint(id, url) {
  const colour = await tintFor(url);
  if (state.selected !== id) return;

  const targets = [el.detail, el.list.querySelector(`[data-id="${id}"]`)];
  for (const node of targets) {
    if (!node) continue;
    if (colour) node.style.setProperty("--pick", colour);
    else node.style.removeProperty("--pick");
  }
}

/// Scroll the detail pane, for the controller's right stick.
///
/// The left stick moves the cursor through the list; the right one reads the
/// pane next to it, which is otherwise unreachable without a mouse.
/// Run `fn` inside a view transition when the browser has one.
///
/// The API is Chromium-and-newer-WebKit; where it is missing this is a plain
/// call, so the app behaves identically and simply does not animate. Nothing
/// downstream may depend on the transition having happened.
export function withTransition(fn) {
  const start = document.startViewTransition?.bind(document);
  if (!start || window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    return Promise.resolve(fn());
  }
  return start(fn).finished.catch(() => {});
}

export function scrollDetail(amount) {
  const pane = el.detail?.querySelector(".scroll");
  if (!pane || el.detail.hidden) return false;
  pane.scrollTop += amount;
  return true;
}
