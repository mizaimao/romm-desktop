// The save-conflict dialog: which copy do you want to keep?
//
// A conflict means the same save changed here and on the server since they last
// agreed. Nothing has been written, and the launch is refused until it is
// answered — playing on top of a save whose ownership is unresolved is how the
// loser gets overwritten for good on the way back out.
//
// Keyboard and pad reach this the same way they reach the rest of the app, so a
// controller-only user is never stuck at a dialog they cannot answer.

import { invoke } from "./state.js";
import { human, toast } from "./util.js";

/// A conflict the backend refused to guess about.
///
/// Shown side by side rather than as a sentence: "yours is newer" is the whole
/// decision, and a date next to a date makes that obvious in a way prose does
/// not.
function card(c) {
  const when = (t) => (t ? String(t).replace("T", " ").replace("Z", " UTC") : "unknown");
  const el = document.createElement("div");
  el.className = "conflict";
  el.innerHTML = `
    <h3>${escape(c.file_name)}</h3>
    <p class="why">${escape(c.reason || "This save changed here and on the server.")}</p>
    <div class="sides">
      <button class="side" data-keep="local">
        <span class="who">This machine</span>
        <span class="when">${escape(when(c.local_updated))}</span>
        <span class="size">${c.local_bytes ? human(c.local_bytes) : ""}</span>
      </button>
      <button class="side" data-keep="server">
        <span class="who">Server</span>
        <span class="when">${escape(when(c.server_updated))}</span>
        <span class="size"></span>
      </button>
    </div>
    <p class="note">The copy you do not keep is saved to
      <code>library/saves-backup/</code> first, so this is reversible.</p>
  `;
  return el;
}

function escape(s) {
  const d = document.createElement("div");
  d.textContent = s == null ? "" : String(s);
  return d.innerHTML;
}

/// Ask about each conflict in turn. Resolves true when every one was answered,
/// false if the user backed out — in which case the caller must not launch.
export function askConflicts(conflicts) {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.id = "conflict-overlay";
    const box = document.createElement("div");
    box.className = "conflict-box";
    box.innerHTML = `<header>
        <span class="icon icon-info-on"></span>
        <h2>Save conflict</h2>
      </header>
      <p class="lead">This save changed in two places. Choose which to keep —
        the launch is on hold until you do.</p>`;

    const list = document.createElement("div");
    box.appendChild(list);

    const cancel = document.createElement("button");
    cancel.className = "conflict-cancel";
    cancel.textContent = "Cancel launch";
    box.appendChild(cancel);
    overlay.appendChild(box);

    let index = 0;
    let settled = false;

    const finish = (ok) => {
      if (settled) return;
      settled = true;
      overlay.remove();
      document.removeEventListener("keydown", onKey, true);
      resolve(ok);
    };

    const show = () => {
      list.replaceChildren();
      if (index >= conflicts.length) return finish(true);
      const c = conflicts[index];
      const el = card(c);
      el.querySelectorAll("[data-keep]").forEach((btn) => {
        btn.addEventListener("click", async () => {
          btn.disabled = true;
          try {
            toast(
              await invoke("resolve_save_conflict", {
                fileName: c.file_name,
                keep: btn.dataset.keep,
              })
            );
            index += 1;
            show();
          } catch (e) {
            btn.disabled = false;
            // Staying open matters: the save is still unresolved, and closing
            // would leave the launch blocked with nothing on screen saying why.
            toast(`Could not resolve — ${e}`, 8000);
          }
        });
      });
      list.appendChild(el);
      el.querySelector("[data-keep]").focus();
    };

    // Left/right move between the two choices, Enter takes one, Esc cancels —
    // the same grammar as the rest of the app, so a pad works here too.
    const onKey = (e) => {
      const sides = [...list.querySelectorAll("[data-keep]")];
      if (!sides.length) return;
      const at = sides.indexOf(document.activeElement);
      if (e.key === "Escape") {
        e.preventDefault();
        finish(false);
      } else if (e.key === "ArrowRight" || e.key === "ArrowDown") {
        e.preventDefault();
        sides[Math.min(at + 1, sides.length - 1)].focus();
      } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
        e.preventDefault();
        sides[Math.max(at - 1, 0)].focus();
      } else if (e.key === "Enter" && at >= 0) {
        e.preventDefault();
        sides[at].click();
      }
      // Swallow everything else so the grid behind does not also act on it.
      e.stopPropagation();
    };

    cancel.addEventListener("click", () => finish(false));
    document.addEventListener("keydown", onKey, true);
    document.body.appendChild(overlay);
    show();
  });
}

/// Pull the conflict list out of the error a refused launch throws.
///
/// The backend cannot open a dialog and wait, so it refuses with the conflicts
/// attached and the frontend asks. A plain marker prefix rather than a typed
/// error because a Tauri command error is a string either way.
export function conflictsFrom(err) {
  const text = typeof err === "string" ? err : String(err?.message ?? err ?? "");
  const at = text.indexOf("SAVE_CONFLICT:");
  if (at === -1) return null;
  try {
    const parsed = JSON.parse(text.slice(at + "SAVE_CONFLICT:".length));
    return Array.isArray(parsed) && parsed.length ? parsed : null;
  } catch {
    return null;
  }
}
