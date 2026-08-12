// "Take this with me" — download a whole platform or collection.
//
// The dialog exists because the choice is not obvious and the cost is not
// visible. Artwork is tens of kilobytes a game; a video is tens of megabytes.
// Ticking "videos" on a 2,400-game collection is the difference between a
// download that finishes before a flight and one that does not, and nothing on
// screen says so until the estimate moves. So the estimate updates on every
// change, and it is the thing the buttons are sized around.

import { invoke, listen } from "./state.js";
import { toast, escapeHtml } from "./util.js";

let open = null;

/// Ask what to take, then take it.
///
/// `what` is `{platform}` or `{collection, name}`.
export function askDownload(what) {
  // `isConnected`, not just a non-null reference: if the overlay is removed by
  // anything other than its own close button, a bare null check would leave
  // this permanently convinced a dialog is up and refuse to open another.
  if (open?.isConnected) return;
  const title = what.name ?? what.platform;

  const box = document.createElement("div");
  box.id = "bulk-overlay";
  box.innerHTML = `
    <div class="bulk-panel" role="dialog" aria-label="Download ${escapeHtml(title)}">
      <h3>Take ${escapeHtml(title)} with you</h3>
      <p class="bulk-est">Working out the size…</p>

      <label class="bulk-row">
        <span>Artwork</span>
        <select class="bulk-art">
          <option value="minimal" selected>Just what's shown (recommended)</option>
          <option value="full">Every picture</option>
          <option value="none">None</option>
        </select>
      </label>
      <p class="hint">Minimal is the two images the app draws: the game list's
        picture and the info pane's. Every picture adds box art, cartridge, 3D
        box, screenshots, title screens and marquees.</p>

      <label class="bulk-row bulk-check">
        <input type="checkbox" class="bulk-videos" />
        <span>Gameplay videos <em>— roughly 8 MB each</em></span>
      </label>
      <label class="bulk-row bulk-check">
        <input type="checkbox" class="bulk-manuals" />
        <span>Manuals</span>
      </label>

      <p class="bulk-space"></p>
      <div class="bulk-buttons">
        <button class="ghost bulk-cancel">Cancel</button>
        <button class="primary bulk-go">Download</button>
      </div>
    </div>`;
  document.body.appendChild(box);
  open = box;

  const q = (s) => box.querySelector(s);
  const close = () => {
    box.remove();
    open = null;
  };

  async function refresh() {
    const est = q(".bulk-est");
    est.textContent = "Working out the size…";
    try {
      const [summary, fits, note] = await invoke("download_estimate", {
        platform: what.platform ?? null,
        collection: what.collection ?? null,
        art: q(".bulk-art").value,
        videos: q(".bulk-videos").checked,
        manuals: q(".bulk-manuals").checked,
      });
      est.textContent = summary;
      q(".bulk-space").textContent = note;
      q(".bulk-space").classList.toggle("bad", !fits);
      // Refusing here rather than after an hour of transfer is the entire
      // point of asking the disk first.
      q(".bulk-go").disabled = !fits;
    } catch (e) {
      est.textContent = String(e);
      q(".bulk-go").disabled = true;
    }
  }

  for (const sel of [".bulk-art", ".bulk-videos", ".bulk-manuals"]) {
    q(sel).addEventListener("change", refresh);
  }
  q(".bulk-cancel").addEventListener("click", close);
  box.addEventListener("keydown", (e) => e.key === "Escape" && close());

  q(".bulk-go").addEventListener("click", async () => {
    const go = q(".bulk-go");
    go.disabled = true;
    const est = q(".bulk-est");
    const stop = await listen("bulk-progress", ({ payload }) => {
      est.textContent = String(payload);
    });
    try {
      toast(await invoke("download_set", {
        platform: what.platform ?? null,
        collection: what.collection ?? null,
        art: q(".bulk-art").value,
        videos: q(".bulk-videos").checked,
        manuals: q(".bulk-manuals").checked,
      }), 8000);
      close();
    } catch (e) {
      est.textContent = String(e);
      go.disabled = false;
    } finally {
      stop?.();
    }
  });

  refresh();
}
