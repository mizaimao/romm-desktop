// What this is, who wrote it, and where it lives.
//
// The version numbers were already at the foot of the rail, which answers
// "are these two machines running the same thing" and nothing else. Everything
// else about the app — that it is somebody's project rather than a product,
// that the source is readable, that there is somewhere to report the things
// that are wrong with it — was written down nowhere the app itself could show
// you.

import { invoke } from "../state.js";
import { toast } from "../util.js";

const REPO = "https://github.com/mizaimao/romm-desktop";

export const html = `      <h4>RomM Desktop</h4>
      <p class="hint about-what">A desktop client for a self-hosted
        <a class="link" data-href="https://romm.app">RomM</a> server: browse the
        library, download what you want to keep, and launch it in RetroArch with
        the right core, shader and controller layout already set.</p>

      <div class="srow">
        <label>Version</label>
        <div class="ctl"><span class="about-version">…</span></div>
      </div>
      <p class="hint">The server's own version is beside it when this machine
        has spoken to one. Both, because "which version am I on" is usually
        asked when two machines behave differently, and the answer is as often
        the server as the client.</p>

      <div class="srow">
        <label>Written by</label>
        <div class="ctl">
          <a class="link" data-href="https://github.com/mizaimao">mizaimao</a>
        </div>
      </div>

      <div class="srow">
        <label>Source</label>
        <div class="ctl">
          <a class="link" data-href="${REPO}">github.com/mizaimao/romm-desktop</a>
        </div>
      </div>
      <p class="hint">Bugs and requests go in
        <a class="link" data-href="${REPO}/issues">the issues</a>. The parked
        list — the things known to be missing and deliberately not built yet —
        is in <code>docs/parked.md</code> in the repository.</p>

      <div class="srow">
        <label>Built with</label>
        <div class="ctl"><span class="hint">Rust, Tauri and RetroArch</span></div>
      </div>
      <p class="hint">Cover art and metadata come from your own RomM server.
        Icons are <a class="link" data-href="https://lucide.dev">Lucide</a>
        (ISC), bundled rather than fetched — this window works with no network
        at all.</p>`;

export async function wire(box) {
  // Every link goes out to the browser. A webview that follows one in place
  // turns the settings window into a web browser with no way back: no address
  // bar, no back button, and the app gone from underneath it.
  for (const a of box.querySelectorAll(".link")) {
    a.setAttribute("role", "link");
    a.setAttribute("tabindex", "0");
    a.addEventListener("click", async (ev) => {
      ev.preventDefault();
      try {
        await invoke("open_link", { url: a.dataset.href });
      } catch (e) {
        toast(`Could not open the link — ${e}`);
      }
    });
  }

  const where = box.querySelector(".about-version");
  if (!where) return;
  try {
    const [client, server] = await invoke("versions");
    where.textContent = server ? `${client} · server ${server}` : client;
  } catch {
    // A wrong version number is worse than none, since the only reason to read
    // it is to compare two machines.
    where.textContent = "unknown";
  }
}
