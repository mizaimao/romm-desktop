// The panes inside the Settings window, one per tab.
//
// Split out of the old single scrolling panel. That panel put emulator paths,
// sync buttons and every keyboard and controller binding in one column, which
// meant the two binding tables — the longest thing in the app — sat between the
// user and everything else.
//
// Each pane is a plain string of markup plus a `wire` function. Nothing here
// touches the main window's DOM, because this now runs in a separate window
// with its own document.

import {
  ACTIONS, keyFor, setKey, resetAll, keyLabel,
  padFor, setPad, resetPad, padLabel,
} from "./bindings.js";
import { invoke, listen } from "./state.js";
import { toast, escapeHtml } from "./util.js";
import { editServer, editAchievements, editScraper } from "./credentials.js";
import {
  backdropSupported, backdropSettings, saveBackdropSettings,
  backdropWanted, setBackdropWanted, PRESETS,
  GLASS_PRESETS, glassTint, setGlassTint, glassStrength, setGlassStrength,
} from "./backdrop.js";

/// Set while waiting for a keypress to assign, so the window's own key handler
/// gets out of the way.
let capturing = null;

/// Set while waiting for a controller button. The Gamepad API has no
/// button-down event, so binding one means polling until something is pressed.
let padCapture = null;

export function isCapturing() {
  return capturing !== null || padCapture !== null;
}

export const TABS = [
  { id: "general", label: "General" },
  { id: "appearance", label: "Appearance" },
  { id: "control", label: "Control" },
  // Its own tab because these are the things that go and fetch something, and
  // at the bottom of General under six unrelated headings the BIOS control was
  // simply not found.
  { id: "library", label: "Library" },
  // Per-system emulator, shader and light gun. This was a button in the top
  // bar labelled "Systems", beside Grid and Take offline, which are things you
  // do to what is on screen — so it read as another view of the library rather
  // than as configuration, and nobody could tell what it would do.
  { id: "systems", label: "Systems" },
];

/// Markup for one tab. Unknown ids return nothing rather than throwing, so a
/// stale saved tab cannot leave the window blank.
export function paneHtml(id) {
  if (id === "general") return `      <h4>RetroArch</h4>
      <div class="srow">
        <label>Location</label>
        <div class="ctl">
          <input class="set-ra" type="text" spellcheck="false"
                 placeholder="search the usual places" />
          <button class="set-ra-pick" title="Choose a folder">Browse…</button>
          <button class="set-ra-save">Save</button>
        </div>
      </div>
      <p class="hint">Empty searches the usual locations. Set it when the install
        lives elsewhere, such as <code>E:\\Emulators\\RetroArch</code>.</p>
      <p class="hint set-ra-status"></p>

      <h4>Saves</h4>
      <div class="srow">
        <label>Sync now</label>
        <div class="ctl"><button class="set-savesync">Sync saves</button></div>
      </div>
      <p class="hint">Compares your saves and save states with the server and
        transfers whatever differs. Anything changed on both sides is reported,
        not overwritten.</p>
      <p class="hint set-savesync-status"></p>

      <h4>Server</h4>
      <div class="srow">
        <label>RomM server</label>
        <div class="ctl">
          <button class="cred-open" data-cred="server">Edit…</button>
          <span class="cred-summary" data-cred-summary="server"></span>
        </div>
      </div>
      <p class="hint">Address and credentials, with a connection check before
        anything is written.</p>

      <h4>Achievements</h4>
      <div class="srow">
        <label>RetroAchievements</label>
        <div class="ctl"><button data-field="achievements_enabled">…</button></div>
      </div>
      <div class="srow">
        <label>Account</label>
        <div class="ctl">
          <button class="cred-open" data-cred="achievements">Edit…</button>
          <span class="cred-summary" data-cred-summary="achievements"></span>
        </div>
      </div>
      <div class="srow">
        <label>Hardcore mode</label>
        <div class="ctl"><button data-field="achievements_hardcore">…</button></div>
      </div>
      <p class="hint">Hardcore disables save states, fast-forward and rewind —
        four of the hotkeys this app binds.</p>

      <h4>ScreenScraper</h4>
      <div class="srow">
        <label>Account</label>
        <div class="ctl">
          <button class="cred-open" data-cred="scraper">Edit…</button>
          <span class="cred-summary" data-cred-summary="scraper"></span>
        </div>
      </div>
      <p class="hint">Stored but not used yet — kept with the rest of the
        configuration rather than in someone's notes.</p>

`;

  if (id === "systems") return `      <h4>Systems</h4>
      <p class="hint">Which emulator runs each console, which shader it gets,
        and whether a light gun takes the second controller port. Changes are
        written to config.toml and apply to the next game you launch.</p>
      <div class="sys-motion"></div>
      <div class="sys-table">Loading…</div>

`;

  if (id === "library") return `      <h4>Library</h4>
      <div class="srow">
        <label>Folder</label>
        <div class="ctl"><input class="cf-text" data-field="library_root"
          type="text" spellcheck="false" placeholder="./library" /></div>
      </div>
      <p class="hint">Everything downloaded lives here — games, artwork, save
        backups. Deleting this folder reclaims all of it.</p>
      <div class="srow">
        <label>Fetch game list</label>
        <div class="ctl">
          <button class="set-libsync">Sync library</button>
          <button class="set-libsync-full">Full resync</button>
        </div>
      </div>
      <p class="hint">The index the grid is built from. Nothing is downloaded —
        but a fresh install shows nothing until this has run once.</p>
      <p class="hint set-libsync-status"></p>

      <div class="srow">
        <label>BIOS files</label>
        <div class="ctl">
          <button class="set-bios">Check BIOS</button>
          <progress class="set-bios-bar" hidden max="1" value="0"></progress>
        </div>
      </div>
      <p class="hint">Neo Geo, PlayStation and the MAME family will not start
        without these. Optional and only when you ask — it is a few hundred MB.
        Needs <code>firmware.read</code> on the token.</p>
      <p class="hint set-bios-status"></p>

      <div class="srow">
        <label>Missing artwork</label>
        <div class="ctl"><button class="set-scrape">Find missing artwork</button></div>
      </div>
      <p class="hint">For games ES-DE never scraped. Asks your RomM server to
        identify each one and fetches the box art ScreenScraper has for it —
        your server already holds the ScreenScraper account, so this needs no
        login and no SD card. Slow on purpose: it is one game at a time so the
        server's allowance is not spent in a burst.</p>
      <p class="hint set-scrape-status"></p>`;

  if (id === "appearance") return `      <h4>Artwork</h4>
      <div class="srow">
        <label>Game list shows</label>
        <div class="ctl"><select class="list-art"></select></div>
      </div>
      <p class="hint">What each game shows in the list and grid. Cartridge or
        disc by default — it is what you recognise a game by, and within one
        console they are all the same shape, so the grid stays even. Anything a
        console has no version of falls back to the miximage. The info pane
        always shows the miximage.</p>

      <h4>Console pictures</h4>
      <div class="srow">
        <label>Show</label>
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

      <h4>Glass</h4>
      <div class="srow">
        <label>Window colour</label>
        <div class="ctl">
          <select class="glass-preset"></select>
          <input class="glass-custom" type="color" />
        </div>
      </div>
      <div class="srow">
        <label>Tint strength</label>
        <div class="ctl"><input class="glass-strength" type="range" min="0" max="60" step="2" />
          <span class="glass-strength-val"></span></div>
      </div>
      <p class="hint">Tints the cards, the selection glow and the controls —
        one colour for all of it. At 0 the glass is clear and only the blur
        remains.</p>

      <h4>Shader backdrop</h4>
      <div class="srow">
        <label>Enabled</label>
        <div class="ctl"><button class="set-backdrop">Shader backdrop: off</button></div>
      </div>
      <div class="srow">
        <label>Colour scheme</label>
        <div class="ctl"><select class="bd-preset"></select></div>
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
        <label>Dark colour</label>
        <div class="ctl"><input class="bd-low" type="color" />
          <button class="bd-low-reset">Use theme</button></div>
      </div>
      <div class="srow bd-custom">
        <label>Light colour</label>
        <div class="ctl"><input class="bd-high" type="color" />
          <button class="bd-high-reset">Use theme</button></div>
      </div>
      <p class="hint">Drawn on the GPU behind the library — not behind this
        window. Motion at 0 holds it still. Colours left on "theme" follow
        whatever palette is in force.</p>
      <p class="hint set-backdrop-status"></p>`;
  if (id === "control") return `      <h4>Bindings</h4>
      <p class="hint">Every action with its key and its controller button side
        by side. Click either to rebind — press the new key or button, or Esc to
        leave it unset.</p>
      <p class="hint pad-live">No controller detected.</p>
      <table class="bindtbl">
        <thead><tr><th>Action</th><th>Keyboard</th><th>Controller</th></tr></thead>
        <tbody>
        ${ACTIONS.map(
          (a) => `
          <tr data-id="${a.id}">
            <td class="bindname">${a.label}</td>
            <td class="key-cell"><button class="set-key ${keyFor(a.id) ? "" : "unset"}">${keyLabel(keyFor(a.id))}</button></td>
            <td class="pad-cell"><button class="set-pad ${padFor(a.id) === null ? "unset" : ""}">${padLabel(padFor(a.id))}</button></td>
          </tr>`
        ).join("")}
        </tbody>
      </table>
      <footer>
        <button class="set-reset">Reset keyboard</button>
        <button class="set-pad-reset">Reset controller</button>
      </footer>`;
  return "";
}

/// Attach behaviour to a rendered pane.
///
/// Every lookup is scoped to `box`, so a pane only ever wires its own controls
/// and switching tabs cannot leave a listener pointing at a removed element.
export function wirePane(id, box) {
  stopPadCapture();
  if (id === "general") return wireGeneral(box);
  if (id === "library") return wireLibrary(box);
  if (id === "appearance") return wireAppearance(box);
  if (id === "control") return wireControl(box);
  if (id === "systems") return wireSystems(box);
}

/// The per-system table: emulator, shader and light gun for each console.
///
/// Rendered after the pane is on screen rather than as part of its markup,
/// because it needs two round trips — the systems themselves and the list of
/// motion shaders — and a tab that waited on those before drawing anything
/// would look like a tab that does not open.
async function wireSystems(box) {
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
    <p class="hint">Reduces the smearing an LCD gives 60fps content, by strobing
      across sub-frames. Chained on top of each system's own shader — it does
      not replace it. Best on a 120Hz+ display; CRT systems only.</p>`;
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

function wireGeneral(box) {
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

  // Saves. The button disables itself while running: the scan plus a round
  // trip per file takes a few seconds, and a second click would start a
  // concurrent sync over the same files.
  const syncBtn = box.querySelector(".set-savesync");
  const syncStatus = box.querySelector(".set-savesync-status");
  syncBtn?.addEventListener("click", async () => {
    syncBtn.disabled = true;
    syncStatus.textContent = "Scanning saves…";
    try {
      syncStatus.textContent = await invoke("sync_saves");
    } catch (e) {
      syncStatus.textContent = `Sync failed — ${e}`;
    } finally {
      syncBtn.disabled = false;
    }
  });

  // config.toml fields. Loaded once and written back on change, through a
  // targeted TOML edit so the hand-written comments in that file survive.
  wireConfigFields(box);
}

/// The tab that fetches things: the game index, BIOS, and artwork the
/// scrapers missed. Each reports before it works and while it works, because a
/// button that goes quiet for a minute reads as one that does nothing.
function wireLibrary(box) {
  // Missing artwork.
  const scrapeBtn = box.querySelector(".set-scrape");
  const scrapeStatus = box.querySelector(".set-scrape-status");
  scrapeBtn?.addEventListener("click", async () => {
    scrapeBtn.disabled = true;
    scrapeStatus.textContent = "Counting…";
    const stop = await listen("scrape-progress", ({ payload }) => {
      scrapeStatus.textContent = String(payload);
    });
    try {
      scrapeStatus.textContent = await invoke("scrape_missing", { platform: null });
      // The grid is showing blanks for exactly these games.
      window.__TAURI__?.event?.emit?.("art-changed");
    } catch (e) {
      scrapeStatus.textContent = `Failed — ${e}`;
    } finally {
      stop?.();
      scrapeBtn.disabled = false;
    }
  });

  // BIOS. Progress by name rather than a spinner: it is 67 files here, and a
  // spinner says nothing about whether it is nearly done or barely started.
  const biosBtn = box.querySelector(".set-bios");
  const biosStatus = box.querySelector(".set-bios-status");
  const biosBar = box.querySelector(".set-bios-bar");

  // Two presses rather than one. The old button started a sync and said nothing
  // until the listing came back — indistinguishable from a control that does
  // nothing — and where the files were already present it did all that work to
  // report "already complete". Now the first press asks, and downloading is
  // only offered when there is something to download.
  let biosPlan = null;

  async function startBios() {
    biosBtn.disabled = true;
    biosBar.hidden = false;
    biosBar.max = biosPlan.total;
    biosBar.value = 0;
    // A count alone does not show progress at a glance; the bar does, and the
    // filename says which one a stall is sitting on.
    const stop = await listen("bios-progress", ({ payload }) => {
      const [done, total, name] = payload;
      biosBar.max = total;
      biosBar.value = done;
      biosStatus.textContent = `${done}/${total}  ${name}`;
    });
    try {
      biosStatus.textContent = await invoke("sync_bios");
    } catch (e) {
      biosStatus.textContent = `Failed — ${e}`;
    } finally {
      stop?.();
      biosBar.hidden = true;
      biosBtn.disabled = false;
      biosBtn.textContent = "Check BIOS";
      biosPlan = null;
    }
  }

  biosBtn?.addEventListener("click", async () => {
    if (biosPlan) return startBios();
    biosBtn.disabled = true;
    biosStatus.textContent = "Asking the server…";
    try {
      const [total, have, bytes] = await invoke("bios_status");
      if (have >= total) {
        biosStatus.textContent = `All ${total} BIOS files are already here.`;
        biosPlan = null;
      } else {
        biosPlan = { total, have, bytes };
        biosStatus.textContent =
          `${total - have} of ${total} missing, about ${(bytes / 1e6).toFixed(0)} MB.`;
        biosBtn.textContent = `Download ${total - have} files`;
      }
    } catch (e) {
      biosStatus.textContent = `Failed — ${e}`;
    } finally {
      biosBtn.disabled = false;
    }
  });

  // Library. This is the one the Windows build had no way to run: the release
  // ships only the GUI, so a fresh install had an empty cache, an empty grid,
  // and nothing anywhere to fill it.
  const libStatus = box.querySelector(".set-libsync-status");
  const runLibSync = async (btn, full) => {
    const buttons = [
      box.querySelector(".set-libsync"),
      box.querySelector(".set-libsync-full"),
    ];
    buttons.forEach((b) => b && (b.disabled = true));
    libStatus.textContent = full ? "Re-fetching everything…" : "Syncing…";
    // A full pull of ~9,000 games takes several seconds, so the backend says
    // which stage it is on rather than leaving the panel looking hung.
    const stop = await listen("sync-progress", ({ payload }) => {
      libStatus.textContent = String(payload);
    });
    try {
      libStatus.textContent = await invoke("sync_library", { full });
      // The grid is built from the cache, so it has to be rebuilt to show what
      // just arrived.
      const { showPlatforms } = await import("./library.js");
      await showPlatforms();
    } catch (e) {
      libStatus.textContent = `Sync failed — ${e}`;
    } finally {
      stop?.();
      buttons.forEach((b) => b && (b.disabled = false));
    }
  };
  box.querySelector(".set-libsync")?.addEventListener("click", (e) =>
    runLibSync(e.currentTarget, false)
  );
  box.querySelector(".set-libsync-full")?.addEventListener("click", (e) =>
    runLibSync(e.currentTarget, true)
  );
}


/// Which pictures the console grid uses, and how to get more.
///
/// This was a whole top-level panel with a gallery of ES-DE themes,
/// screenshots, and a full-download button. The app never rendered a theme —
/// it only ever read the per-system artwork out of one — so all that offered
/// was a choice that changed nothing and a download measured in hundreds of
/// megabytes. What is left is the part that does change the screen.
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
          toast(`Console pictures: ${label}`);
        } catch (e) {
          toast(String(e), 6000);
        }
      });
    }
  };
  await draw();

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

function wireAppearance(box) {
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

  // Glass tint. Applied to this window immediately as well as announced, so
  // the effect is visible on the control that changed it.
  const glassSel = box.querySelector(".glass-preset");
  const glassCustom = box.querySelector(".glass-custom");
  if (glassSel) {
    glassSel.innerHTML =
      GLASS_PRESETS.map((g) => `<option value="${g.colour}">${g.label}</option>`).join("") +
      `<option value="custom">Custom…</option>`;
    const current = glassTint();
    const known = GLASS_PRESETS.find((g) => g.colour.toLowerCase() === current.toLowerCase());
    glassSel.value = known ? known.colour : "custom";
    glassCustom.value = current;
    glassCustom.hidden = !!known;

    glassSel.addEventListener("change", () => {
      if (glassSel.value === "custom") {
        glassCustom.hidden = false;
        setGlassTint(glassCustom.value);
      } else {
        glassCustom.hidden = true;
        glassCustom.value = glassSel.value;
        setGlassTint(glassSel.value);
      }
    });
    glassCustom.addEventListener("input", () => setGlassTint(glassCustom.value));

    const strengthEl = box.querySelector(".glass-strength");
    const strengthOut = box.querySelector(".glass-strength-val");
    strengthEl.value = String(glassStrength());
    strengthOut.textContent = `${glassStrength()}%`;
    strengthEl.addEventListener("input", () => {
      strengthOut.textContent = `${setGlassStrength(strengthEl.value)}%`;
    });
  }

  // The custom pickers only mean anything on the "custom" scheme; showing them
  // beside a preset invites changing one and watching nothing happen.
  const preset = box.querySelector(".bd-preset");
  preset.innerHTML = PRESETS.map(
    (p) => `<option value="${p.id}">${p.label}</option>`
  ).join("");
  preset.value = cfg.preset || "midnight";
  const showCustom = () => {
    const on = preset.value === "custom";
    box.querySelectorAll(".bd-custom").forEach((r) => (r.hidden = !on));
  };
  showCustom();
  preset.addEventListener("change", () => {
    saveBackdropSettings({ preset: preset.value });
    showCustom();
  });

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

function wireControl(box) {
  box.querySelectorAll("tr[data-id]").forEach((row) => {
    const btn = row.querySelector(".key-cell .set-key");
    if (!btn) return;
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

  box.querySelectorAll("tr[data-id]").forEach((row) => {
    const btn = row.querySelector(".pad-cell .set-pad");
    if (!btn) return;
    btn.addEventListener("click", () => {
      stopPadCapture();
      btn.textContent = "press a button…";
      btn.classList.add("capturing");
      startPadCapture(row.dataset.id, btn);
    });
  });

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
  document.querySelectorAll("#settings tr[data-id]").forEach((row) => {
    const b = row.querySelector(".pad-cell .set-pad");
    if (!b || b.classList.contains("capturing")) return;
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
  document.querySelectorAll("#settings tr[data-id]").forEach((row) => {
    const b = row.querySelector(".key-cell .set-key");
    if (!b || b.classList.contains("capturing")) return;
    const k = keyFor(row.dataset.id);
    b.textContent = keyLabel(k);
    b.classList.toggle("unset", !k);
  });
  return true;
}

/// A theme colour as a `#rrggbb` string, for a colour input's initial value.
function cssColour(name, fallback) {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return /^#[0-9a-f]{6}$/i.test(raw) ? raw : fallback;
}

/// Bind every `data-field` control in a pane to config.toml.
///
/// Text fields save on blur rather than on every keystroke: writing the file on
/// each character would rewrite it thirty times while someone types a path, and
/// a half-typed path saved and reloaded is worse than no path.
async function wireConfigFields(box) {
  const fields = box.querySelectorAll("[data-field]");
  if (!fields.length) return;

  let current;
  try {
    current = await invoke("config_fields");
  } catch (e) {
    box.querySelectorAll(".cf-text").forEach((i) => {
      i.disabled = true;
      i.placeholder = `unavailable — ${e}`;
    });
    return;
  }

  if (!current.config_exists) {
    // Writing settings into a file that does not exist creates one with only
    // those settings in it, which is a worse starting point than the template.
    const warn = document.createElement("p");
    warn.className = "hint";
    warn.textContent = `No config.toml at ${current.config_path} — copy config.example.toml there first.`;
    box.prepend(warn);
  }

  const save = async (field, value) => {
    try {
      toast(await invoke("set_config_field", { field, value: String(value) }));
    } catch (e) {
      toast(`Could not save — ${e}`, 8000);
    }
  };

  // Credentials live behind a button and inside a dialog: nothing is written
  // until Save, and a stored secret is never handed back to be displayed.
  const summarise = () => {
    const set = (name, text) => {
      const el = box.querySelector(`[data-cred-summary="${name}"]`);
      if (el) el.textContent = text;
    };
    set("server", current.server_url ? `${current.server_url}${current.server_token_set ? " · token set" : ""}` : "not configured");
    set("achievements", current.achievements_username
      ? `${current.achievements_username}${current.achievements_token_set ? " · token set" : ""}`
      : "not configured");
    set("scraper", current.scraper_ssid ? current.scraper_ssid : "not configured");
  };
  summarise();

  for (const btn of box.querySelectorAll(".cred-open")) {
    btn.addEventListener("click", async () => {
      const which = btn.dataset.cred;
      const editor =
        which === "server" ? editServer : which === "achievements" ? editAchievements : editScraper;
      const out = await editor(current);
      if (!out) return;
      for (const [field, value] of Object.entries(out)) {
        // A blank secret means "keep what is stored", not "clear it" — the
        // dialog never shows the existing value, so blank is the normal state
        // for a field nobody touched.
        const secret = field.includes("token") || field.includes("password");
        if (secret && !value) continue;
        await save(field, value);
        current[field] = value;
        if (secret) current[`${field}_set`] = true;
      }
      summarise();
    });
  }

  for (const node of fields) {
    const field = node.dataset.field;
    const value = current[field];

    if (node.tagName === "INPUT") {
      node.value = value ?? "";
      // Blur and Enter, not input: see above.
      node.addEventListener("change", () => save(field, node.value));
      continue;
    }

    // Everything else is a toggle rendered as a button, so it works under a
    // controller the same way every other control here does.
    let on = !!value;
    const paint = () => {
      node.textContent = on ? "On" : "Off";
      node.classList.toggle("active", on);
    };
    paint();
    node.addEventListener("click", () => {
      on = !on;
      paint();
      save(field, on);
    });
  }
}
