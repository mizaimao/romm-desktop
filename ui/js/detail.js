// The sidebar: artwork, metadata, and the play/download actions.

import { el, state, invoke, convertFileSrc, rememberRom } from "./state.js";
import { tintFor } from "./tint.js";
import { human, escapeHtml, row, starBar, toast } from "./util.js";
import { openLightbox, setOpenHook } from "./lightbox.js";
import { showMenu } from "./menu.js";
import { shellMode } from "./shell.js";
import { deleteState } from "./states.js";
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
  // In Desk the preview is a column of the layout, so "show it" means show it
  // — on the console list too, where it holds whatever was last selected.
  // This used to insist on a game being selected, which disagreed with the
  // console screen's own line and left the column shut with no way back:
  // pressing Show info there did nothing at all, because nothing was selected
  // *to* show.
  //
  // In Sofa it slides over the list, so it is only meaningful where there is
  // something under the cursor.
  const allowed =
    shellMode() === "columns" ||
    ((state.view === "roms" || state.view === "search" || state.view === "collection-roms") &&
      state.selected !== null);
  el.detail.hidden = !(on && allowed);
}

/// The game the pane is showing, kept so the video button can build the same
/// set of media the artwork clicks do. `playVideo` is reachable from a key and
/// a pad as well as the button, so it cannot be handed the detail as an
/// argument.
let currentDetail = null;

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

  const [d, cores, states] = await Promise.all([
    invoke("rom_detail", { id }),
    // Never fatal: a missing core list should not blank the whole pane.
    invoke("game_cores", { id }).catch(() => []),
    // Nor a missing shelf. Reading it means walking the state folders, which
    // is fine per game and would not be fine if it blanked the pane on a
    // machine with no RetroArch.
    invoke("game_states", { id }).catch(() => []),
  ]);
  // A later click may have superseded this one; do not paint a stale game.
  if (state.selected !== id) return;
  currentDetail = d;
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

  // What this game has, as three small tags rather than a button here and two
  // links at the very bottom of the pane.
  //
  // The video one is a label, not a player: it says a video exists and waits
  // to be asked, because ES-DE starting one by itself after a pause leaves you
  // muting the emulator to browse and then having no sound when you actually
  // want to watch something. Y opens it, and it is the first thing in the
  // artwork reel below — so this does not need to be the way in, only the way
  // you find out there is one.
  const badges = [
    d.has_video
      ? `<button class="badge" id="playvid" title="Play it (Y)">
           <span class="icon icon-play"></span><span>Video</span></button>`
      : "",
    d.manual
      ? `<button class="badge" id="manual" title="Read the manual">
           <span class="icon icon-book"></span><span>Manual</span></button>`
      : "",
    // Not a play glyph: this one is a YouTube link and opens the browser. A
    // play triangle beside the video tag promises the same thing happens, and
    // it does not — one plays here, the other leaves the app entirely.
    d.youtube_id
      ? `<button class="badge" id="trailer" title="Watch the trailer on YouTube — opens your browser"
           data-yt="${escapeHtml(d.youtube_id)}">
           <span class="icon icon-external"></span><span>Trailer</span></button>`
      : "",
  ].join("");
  const video = badges ? `<div class="badges">${badges}</div>` : "";

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
      ${stateShelf(states)}
      ${artStrip(d)}
    </div>
    <div class="pinned">
      ${autofireRow(d)}
      <div class="actions">
        <button class="primary" id="play">${d.downloaded ? "Play" : "Download & Play"}</button>
        <button class="ghost" id="dl" ${d.downloaded ? "disabled" : ""}>Download</button>
      </div>
      <progress id="prog" hidden></progress>
      <div id="prog-text" hidden></div>
    </div>`;
  });

  wireArtwork(d);
  wireShelf(d);
  wireAutofire(d);

  document.getElementById("play").addEventListener("click", () => play(d));
  document.getElementById("dl").addEventListener("click", () => download(d.id, false));
  wireCorePicker(d.id);
}

/// The save states this game has, as pictures you can start from.
///
/// A state is the only record of where you actually are in a game — it cannot
/// be downloaded again and nothing else in the app knows what is in one. They
/// were being synced with the server already, which meant the app knew about
/// them and never showed one: the only way back into a state was to launch the
/// game, open RetroArch's menu, and guess at a slot number.
///
/// The picture is the point. RetroArch saves the frame beside the state, and
/// "the cave with the two doors" is a thing you recognise instantly where
/// "slot 3" is a thing you have to remember.
function stateShelf(states) {
  if (!states.length) return "";
  return `
    <div class="shelf">
      <div class="shelf-head">Save states</div>
      <div class="shelf-strip">
        ${states
          .map(
            (s) => `
          <button class="state ${s.resumable ? "" : "noresume"}"
            data-slot="${escapeHtml(s.slot)}"
            title="${s.resumable ? `Start from ${escapeHtml(s.label)}` : "RetroArch loads this one itself when you next play — right-click to delete it"}">
            ${
              s.thumb
                ? `<img src="${convertFileSrc(s.thumb)}" alt="" loading="lazy" />`
                : `<span class="state-blank">${escapeHtml(s.slot === "auto" ? "auto" : s.slot)}</span>`
            }
            <span class="state-label">${escapeHtml(s.label)}</span>
            <span class="state-when">${escapeHtml(s.when ?? "")}</span>
          </button>`
          )
          .join("")}
      </div>
    </div>`;
}

function wireShelf(d) {
  for (const btn of document.querySelectorAll(".state")) {
    // `noresume` rather than `disabled`. A disabled button fires no mouse
    // events at all — not click, not contextmenu — so marking the autosave
    // disabled also made it impossible to right-click, which is the only way
    // to delete one. It is greyed the same way and simply does not launch.
    if (!btn.classList.contains("noresume")) {
      btn.addEventListener("click", () => {
        // Same launch path as the Play button, so a state that needs a core
        // fetched, a BIOS file, or a save sync gets all of it.
        launch(d.id, { entrySlot: Number(btn.dataset.slot) });
      });
    }
    // Right-click, including on the autosave: it cannot be started from, but
    // it is a file like any other and clearing it out is a reasonable thing to
    // want.
    btn.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      stateMenu(d, btn, e.clientX, e.clientY);
    });
  }
}

/// Where the rapid fire lives, for games that can have it.
///
/// In the pinned strip directly above Play, rather than in Settings: this is a
/// thing you change *about the run you are about to start* — set the rate,
/// play a level, decide it wants to be slower — and a control three windows
/// away turns that into a trip. It does not scroll with the artwork for the same
/// reason.
///
/// Absent entirely for games it does not apply to, which is most of them.
/// The three the backend knows. Anything else — a value from a newer build, a
/// hand-edited config, a field that arrived as something unexpected — draws no
/// row at all rather than an empty one, because a control with nothing
/// selected is worse than no control.
const AUTOFIRE_MODES = ["off", "lb", "rb"];

function autofireRow(d) {
  // Games this does not apply to get nothing: it is only the arcade and Neo
  // Geo shooters, which is 879 of 2,668 arcade games and none of the consoles.
  if (!AUTOFIRE_MODES.includes(d.autofire)) return "";
  const opt = (key, label, title) =>
    `<button class="af ${d.autofire === key ? "on" : ""}" data-af="${key}"
       title="${escapeHtml(title)}">${escapeHtml(label)}</button>`;
  // Always here, including next to "Off". Hiding it meant the row changed
  // width as you moved between the three, and the rate is a thing you may want
  // to set before switching on rather than after — turning it on and finding
  // it too fast, then turning it off to slow it down, is a loop with no exit.
  const rate = `<span class="af-rate">
      <button class="af-step" data-hz="-1" title="Slower">−</button>
      <span class="af-hz">${d.autofire_hz ?? 5} Hz</span>
      <button class="af-step" data-hz="1" title="Faster">+</button>
    </span>`;
  return `
    <div class="autofire-row" title="Applies to every arcade shooter, not only this game">
      <span class="af-label">Rapid fire</span>
      ${opt("off", "Off", "The buttons behave as the cabinet did")}
      ${opt("lb", "Hold LB", "While LB is held, fire repeats at this rate. Let go and it stops.")}
      ${opt("rb", "Hold RB", "The same on the other shoulder, for when your left hand is busy")}
      ${rate}
    </div>`;
}

/// Redraw just this row, in place.
///
/// Rebuilding the whole pane instead sent the reader back to the top of it,
/// which for a control that lives at the bottom means every press throws away
/// where you were. Nothing else on the pane depends on these two values, so
/// nothing else needs to be touched.
function repaintAutofire(d) {
  const row = document.querySelector(".autofire-row");
  if (!row) return;
  const fresh = document.createElement("div");
  fresh.innerHTML = autofireRow(d);
  const next = fresh.firstElementChild;
  if (!next) return;
  row.replaceWith(next);
  wireAutofire(d);
}

function wireAutofire(d) {
  for (const step of document.querySelectorAll(".autofire-row .af-step")) {
    step.addEventListener("click", async () => {
      // Clamped here as well as in the backend, so the number on screen never
      // shows something that was not stored.
      const want = Math.min(30, Math.max(1, (d.autofire_hz ?? 5) + Number(step.dataset.hz)));
      if (want === d.autofire_hz) return;
      try {
        await invoke("set_config_field", { field: "autofire_hz", value: String(want) });
      } catch (e) {
        return toast(`Could not save — ${e}`, 8000);
      }
      d.autofire_hz = want;
      repaintAutofire(d);
    });
  }

  for (const btn of document.querySelectorAll(".autofire-row .af")) {
    btn.addEventListener("click", async () => {
      const want = btn.dataset.af;
      try {
        await invoke("set_config_field", { field: "autofire", value: want });
      } catch (e) {
        return toast(`Could not save — ${e}`, 8000);
      }
      d.autofire = want;
      repaintAutofire(d);
      toast(want === "off" ? "Rapid fire off" : `Rapid fire on ${want.toUpperCase()}`);
    });
  }
}

/// The right-click menu on a save state.
function stateMenu(d, btn, x, y) {
  const slot = btn.dataset.slot;
  showMenu(
    [
      {
        label: "Delete this state",
        danger: true,
        run: async () => {
          const gone = await deleteState(d.id, {
            slot,
            label: btn.querySelector(".state-label")?.textContent?.trim(),
          });
          // Redrawn from the backend rather than by removing the button here:
          // the shelf is what the folder says it is, not what this page
          // remembers.
          if (gone) await selectRom(d.id);
        },
      },
    ],
    x,
    y
  );
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
  if (!items.length && !d.has_video) return "";
  // The video first, the way ES-DE lays a game out: it is the thing you look
  // at, and it used to be a button somewhere else entirely, which left the
  // arrows walking a reel that started at the box art and never reached it.
  // No thumbnail because the file is not here yet — it is fetched when asked
  // for, and a tile that has to download 30MB to draw itself is not a
  // thumbnail.
  const vid = d.has_video
    ? `<figure data-art="video" class="vidtile" title="Gameplay video">
         <div class="vidthumb"><span class="icon icon-play"></span></div>
         <figcaption>Video</figcaption>
       </figure>`
    : "";
  return `<div class="artstrip">${vid}${items
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
  // The game is the authority on whether there is a video, not the presence of
  // a button. Y has to work whether or not the tag is on screen — it is a tag
  // now, and the pane it lives in can be hidden.
  if (!currentDetail?.has_video) return;
  const btn = document.getElementById("playvid");
  if (btn?.dataset.busy) return;

  if (btn) btn.dataset.busy = "1";
  const label = btn?.querySelector("span:not(.icon)");
  const was = label?.textContent;
  if (label) label.textContent = "Fetching…";
  try {
    const path = await invoke("game_video", { id });
    // Still the same game? A slow fetch must not open a video over whatever
    // was scrolled to in the meantime.
    if (state.selected !== id) return;
    // The whole reel, not just the video: the arrows are meant to walk from a
    // video into the artwork and back, which they cannot do in a set of one.
    const media = mediaSet(currentDetail ?? {}, path);
    const at = media.findIndex((m) => m.id === "video");
    openLightbox(media, at < 0 ? 0 : at);
  } catch (e) {
    toast(String(e), 6000);
  } finally {
    if (btn) delete btn.dataset.busy;
    if (label && was) label.textContent = was;
  }
}

/// Everything this game has, as one set the arrows can walk through.
///
/// Built here rather than in the lightbox because this is where the list of
/// artwork kinds lives, and it has to stay in the order the strip draws them —
/// stepping right should land on the picture to the right.
///
/// Each thing used to open on its own. Clicking the cart art gave you the cart
/// art and nothing else, and pressing Y gave you the video and nothing else, so
/// the arrow keys had a set of one to walk through and appeared not to work.
/// ES-DE treats a game's media as one reel and so does this now.
///
/// `videoSrc` arrives separately: the video is fetched on demand, so its path
/// is not known until someone asks for it.
function mediaSet(d, videoSrc = null) {
  const items = [];
  // First, matching the strip below the details and matching ES-DE. It used to
  // be last, so "one more right" from the end of a dozen pictures was the
  // video and there was no way to reach it from the left at all.
  if (videoSrc) {
    items.push({ src: convertFileSrc(videoSrc), kind: "video", caption: "Gameplay", id: "video" });
  }
  for (const [kind, label] of ART_ORDER) {
    if (d.art?.[kind]) {
      items.push({ src: convertFileSrc(d.art[kind]), kind: "image", caption: label, id: kind });
    }
  }
  (d.screenshots || []).forEach((sh, i) =>
    items.push({
      src: convertFileSrc(sh),
      kind: "image",
      caption: `Screenshot ${i + 1}`,
      id: `screenshot-${i}`,
    })
  );
  if (d.cover) {
    items.push({ src: convertFileSrc(d.cover), kind: "image", caption: "Cover", id: "cover" });
  }
  return items;
}

/// Clicking artwork opens it full size, starting at what was clicked.
function wireArtwork(d) {
  const media = mediaSet(d);
  const openAt = (pred) => () => {
    const i = media.findIndex(pred);
    openLightbox(media, i < 0 ? 0 : i);
  };

  el.detail.querySelector(".shots")?.addEventListener("click", (ev) => {
    // Leave the slideshow's own arrows and dots alone.
    if (ev.target.closest(".nav, .dots")) return;
    const shown = el.detail.querySelector(".shots img.on");
    const idx = [...el.detail.querySelectorAll(".shots img")].indexOf(shown);
    openLightbox(media, Math.max(media.findIndex((m) => m.id === `screenshot-${idx}`), 0));
  });
  el.detail.querySelector("img.cover")?.addEventListener("click", openAt((m) => m.id === "cover"));
  document.getElementById("playvid")?.addEventListener("click", playVideo);
  el.detail.querySelectorAll(".artstrip figure").forEach((fig) =>
    // The video tile has no picture behind it — the file is fetched when it is
    // asked for — so it goes the same way Y does rather than through the reel,
    // which has no video in it until that fetch has happened.
    fig.addEventListener(
      "click",
      fig.dataset.art === "video" ? playVideo : openAt((m) => m.id === fig.dataset.art)
    )
  );
  document.getElementById("manual")?.addEventListener("click", () =>
    openLightbox([{ src: convertFileSrc(d.manual), kind: "pdf", caption: "Manual" }], 0)
  );
  // Out to the browser rather than into this window. It was an <a target=
  // "_blank">, which in a webview is a navigation: YouTube would have replaced
  // the library, with no address bar and no way back.
  document.getElementById("trailer")?.addEventListener("click", (ev) => {
    const yt = ev.currentTarget.dataset.yt;
    invoke("open_link", { url: `https://www.youtube.com/watch?v=${encodeURIComponent(yt)}` }).catch(
      (e) => toast(`Could not open the trailer — ${e}`)
    );
  });
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
