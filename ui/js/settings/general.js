// The General tab: where RetroArch is, who the server is, and the accounts.
import { invoke } from "../state.js";
import { toast } from "../util.js";
import { wireConfigFields } from "./fields.js";

export const html = `      <h4>RetroArch</h4>
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
        <label>Login</label>
        <div class="ctl">
          <button class="ra-check">Check login</button>
          <span class="ra-state dim">not checked</span>
        </div>
      </div>
      <p class="hint">Checks the username and token above against
        RetroAchievements, through the same login RetroArch uses — so a tick
        means the login the emulator will attempt, not merely that the account
        exists.</p>
      <div class="srow">
        <label>Hardcore mode</label>
        <div class="ctl"><button data-field="achievements_hardcore">…</button></div>
      </div>
      <p class="hint">Hardcore disables save states, fast-forward and rewind —
        four of the hotkeys this app binds.</p>

      <h4>Save states</h4>
      <div class="srow">
        <label>Ask before deleting</label>
        <div class="ctl"><button data-field="confirm_delete_state">…</button></div>
      </div>
      <p class="hint">Off by default — clearing old states is done several at a time. A copy
      always goes to the backups folder, so deleting one is undoable by hand.</p>

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

export function wire(box) {
  wireAchievementCheck(box);
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


/// Ask RetroAchievements whether the login works, and say so plainly.
///
/// Three states rather than two. "Not checked" is honest — the app has not
/// asked — and matters because the other two both make a claim: a tick says
/// the emulator's own login will succeed, and a cross names the reason the
/// server gave. Blurring "unknown" into "bad" is how an indicator becomes
/// something people learn to ignore.
export function wireAchievementCheck(box) {
  const btn = box.querySelector(".ra-check");
  const out = box.querySelector(".ra-state");
  if (!btn || !out) return;

  const paint = (cls, text) => {
    out.className = `ra-state ${cls}`;
    out.textContent = text;
  };

  btn.addEventListener("click", async () => {
    btn.disabled = true;
    paint("dim", "checking…");
    try {
      const v = await invoke("verify_achievements");
      if (v.ok) {
        paint("ok", `token accepted for ${v.user}`);
        toast("RetroAchievements login confirmed");
      } else {
        paint("bad", v.error || "rejected");
        toast(`RetroAchievements: ${v.error || "login rejected"}`, 7000);
      }
    } catch (e) {
      paint("bad", String(e));
      toast(`Could not check: ${e}`, 7000);
    } finally {
      btn.disabled = false;
    }
  });
}
