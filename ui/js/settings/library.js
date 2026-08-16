// The Library tab: the things that go and fetch something.
//
// Its own tab because these are the controls that take a minute and need to
// report while they work — at the bottom of General, under six unrelated
// headings, the BIOS control was simply not found.
import { invoke, listen } from "../state.js";
import { toast } from "../util.js";
import { wireConfigFields } from "./fields.js";

export const html = `      <h4>Library</h4>
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

/// The tab that fetches things: the game index, BIOS, and artwork the
/// scrapers missed. Each reports before it works and while it works, because a
/// button that goes quiet for a minute reads as one that does nothing.
export function wire(box) {
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
