// Controller support via the Gamepad API.
//
// WKWebView exposes the same Gamepad API Safari does, and macOS presents Xbox,
// DualShock/DualSense, Switch Pro and most 8BitDo pads as standard-mapping
// gamepads over Bluetooth or USB — so one mapping covers all of them without
// any per-vendor code or a native dependency.
//
// Buttons follow the W3C "standard" layout, which macOS reports for those
// controllers. Face-button *positions* are used, not labels: index 0 is the
// bottom face button (A on Xbox, Cross on PlayStation, B on a Nintendo
// layout), which is "confirm" on every platform.

import { el, state } from "./state.js";
import { runAction } from "./keys.js";
import { padMap } from "./bindings.js";
import { toast } from "./util.js";
import { scrollDetail } from "./detail.js";
import { closeLightbox, stepLightbox } from "./lightbox.js";

// Rebindable in Settings; padMap() layers the user's choices over the
// position-based defaults in bindings.js.

// Held directions repeat; one-shot buttons do not.
const REPEATABLE = new Set([
  "up", "down", "left", "right", "pageUp", "pageDown",
  // Holding a trigger sweeps the size rather than stepping once per press.
  "zoomIn", "zoomOut",
]);
const FIRST_REPEAT_MS = 380;
const REPEAT_MS = 110;
const STICK_DEADZONE = 0.55;

const held = new Map(); // action -> next fire time
let running = false;

/// Set while the pad must be ignored until every button is released.
///
/// The quit hotkey is Select + A, and both are bound here too — Select opens
/// Settings, A launches whatever is selected. So the moment RetroArch exited
/// and this window took focus again, the still-held buttons read as fresh
/// presses: the game relaunched and Settings opened behind it.
///
/// Waiting for release rather than a fixed timeout, because how long someone
/// holds a quit combo is not something to guess at.
///
/// Release alone turned out not to be enough. Coming back from RetroArch the
/// pad can report *nothing* held for a frame or two before the real state
/// arrives — the window regains focus first and the Gamepad API catches up
/// after — so the still-held quit combo read as released, the lock lifted, and
/// then the same buttons arrived as a fresh press. A short floor under the
/// lock covers that gap: input is ignored until the pad is at rest **and** the
/// floor has passed.
const SETTLE_FLOOR_MS = 200;

let settling = false;
let settleUntil = 0;

export function ignorePadUntilReleased() {
  settling = true;
  settleUntil = performance.now() + SETTLE_FLOOR_MS;
  held.clear();
}

/// Set for as long as a game is running.
///
/// Waiting for the pad to be released is not enough while the emulator is up.
/// The pad goes to rest a second after launch, the lock lifts, and this window
/// carries on polling behind the game for the whole session — so the buttons
/// pressed to quit are read here as they are pressed, and the app relaunches
/// the game it was told to leave.
///
/// It never showed on macOS, where the webview stops receiving gamepad events
/// once it loses focus. WebView2 on Windows keeps delivering them, so there the
/// entire session is one long stream of input nobody meant for this window.
let suspended = false;

export function suspendPad() {
  suspended = true;
  held.clear();
}

export function resumePad() {
  suspended = false;
  ignorePadUntilReleased();
}

/// Whether the lock may lift: the pad is at rest *and* the floor has passed.
///
/// Split out so the rule can be tested. The bug it encodes is invisible from
/// the outside — a lock that lifts one frame too early looks exactly like a
/// game relaunching itself.
export function settleLifted(pressed, now) {
  return pressed.size === 0 && now >= settleUntil;
}

function fire(action, now) {
  const due = held.get(action);
  if (due === undefined) {
    // Newly pressed: act immediately, then wait out the initial delay.
    held.set(action, now + FIRST_REPEAT_MS);
    runAction(action);
    return;
  }
  if (now < due) return;
  if (!REPEATABLE.has(action)) return;
  held.set(action, now + REPEAT_MS);
  runAction(action);
}

/// Translate raw pad state into the set of actions held this frame.
///
/// Split out of `poll` so it can be tested without a browser: this is where
/// "the A button does nothing" would live, and a bug here is otherwise
/// invisible, since poll runs inside requestAnimationFrame where a throw is
/// swallowed and the next frame is already queued.
/// The pad that drives the interface: the first one connected, and only that
/// one.
///
/// With four controllers plugged in for a four-player game, every one of them
/// used to move the cursor in the library — so three other people fidgeting
/// with sticks while player one tries to pick a game made the menu unusable.
/// In the emulator all four are players; out here, player one is in charge.
export function primaryPad(pads) {
  for (const pad of pads) {
    if (pad?.connected !== false && pad) return pad;
  }
  return null;
}

export function pressedActions(pads, map) {
  const pressed = new Set();
  const pad = primaryPad(pads);
  if (!pad) return pressed;

  for (const [index, action] of Object.entries(map)) {
    // A rebind clears the old slot by writing null, so skip those rather
    // than dispatching an action of null.
    if (action && pad.buttons[index]?.pressed) pressed.add(action);
  }
  // Right stick scrolls the detail pane. Axes 2 and 3 in the W3C standard
  // layout. Done here rather than as an action because it is continuous
  // rather than a repeat-fire keypress: how far you push decides how fast it
  // moves, which a discrete action cannot express.
  const ry = pad.axes[3] ?? 0;
  if (Math.abs(ry) > STICK_DEADZONE) {
    // Squared so a small push creeps and a full push moves properly.
    const speed = Math.sign(ry) * (Math.abs(ry) ** 2) * 26;
    scrollDetail(speed);
  }

  // Left stick doubles as a d-pad, but only along its dominant axis. Pushed
  // diagonally it would otherwise report left+up in the same frame and move
  // twice, which reads as the cursor jumping around on its own.
  const [x = 0, y = 0] = pad.axes;
  if (Math.abs(x) > Math.abs(y)) {
    if (x < -STICK_DEADZONE) pressed.add("left");
    if (x > STICK_DEADZONE) pressed.add("right");
  } else {
    if (y < -STICK_DEADZONE) pressed.add("up");
    if (y > STICK_DEADZONE) pressed.add("down");
  }

  return pressed;
}

/// Reported at most once. A throw inside a rAF callback does not stop the loop
/// — the next frame was already scheduled — so without this the controller just
/// silently stops responding, which is precisely how a broken poll survived
/// several rounds of being reported and "fixed".
let complained = false;

function poll() {
  if (!running) return;
  requestAnimationFrame(poll);
  // Nothing to read while the window is not on screen: a button pressed behind
  // another application is not aimed at this one, and the emulator owns the pad
  // anyway during the one case where this window is hidden and a pad is in use.
  if (document.hidden) return;
  try {
    step();
  } catch (e) {
    console.error("gamepad poll failed", e);
    if (!complained) {
      complained = true;
      toast(`Controller input error: ${e.message}`);
    }
  }
}

/// Whether the lightbox was open on the previous poll.
let wasLightboxOpen = false;

function step() {

  const map = padMap();

  // Opening or closing the lightbox is a change of context, and anything the
  // pad was holding belonged to the old one: a direction held while the
  // lightbox opens would otherwise arrive inside it with its repeat timer
  // already running, so the first real press does nothing.
  //
  // But the button that caused the change is usually still down — Y opens the
  // player and Y also closes it — so the held set is rebuilt from what is
  // pressed rather than emptied. Emptying it made the poll immediately after
  // opening treat that same held Y as a new press, and the player opened and
  // shut in the same breath.
  const lightboxOpen = !el.lb.hidden;
  if (lightboxOpen !== wasLightboxOpen) {
    wasLightboxOpen = lightboxOpen;
    const now = performance.now();
    held.clear();
    for (const action of pressedActions(navigator.getGamepads?.() ?? [], map)) {
      held.set(action, now + FIRST_REPEAT_MS);
    }
  }

  // The lightbox used to swallow the pad completely, which meant the button
  // that opened a video could not close it — the only way out was the mouse.
  // Four actions get through: the one that opened it, Back, and the triggers,
  // which zoom what is on the stage rather than the covers behind it.
  if (!el.lb.hidden) {
    const now = performance.now();
    const pressed = pressedActions(navigator.getGamepads?.() ?? [], map);
    for (const action of pressed) {
      if (action === "video" || action === "back") {
        // Edge-triggered through the same `held` map as everything else, or a
        // held button would close the lightbox and then act on the library
        // underneath it in the same press.
        if (!held.has(action)) {
          held.set(action, now);
          closeLightbox();
          // Settle, so lifting the button does not register anywhere else.
          settling = true;
        }
      } else if (action === "zoomIn" || action === "zoomOut") {
        fire(action, now);
      } else if (action === "left" || action === "right") {
        // Through `fire` so holding a direction repeats, the same as it does
        // in the grid. Walking a dozen pictures one press at a time is the
        // kind of thing that makes people reach for the mouse again.
        if (!held.has(action)) {
          held.set(action, now + FIRST_REPEAT_MS);
          stepLightbox(action === "right" ? 1 : -1);
        } else if (now >= held.get(action)) {
          held.set(action, now + REPEAT_MS);
          stepLightbox(action === "right" ? 1 : -1);
        }
      }
    }
    for (const action of [...held.keys()]) {
      if (!pressed.has(action)) held.delete(action);
    }
    return;
  }

  // A dialog has taken over the window: the save-conflict question, the
  // "saves are not syncing" warning, the take-offline pane, a right-click
  // menu. All of them were mouse-only, which on a machine being used from a
  // sofa means a launch can stop dead on a question that cannot be answered.
  //
  // The same four actions serve every one of them, because a dialog is a list
  // of buttons: move through them, press one, or back out.
  const dialog = openDialog();
  if (dialog) {
    const now = performance.now();
    const pressed = pressedActions(navigator.getGamepads?.() ?? [], map);
    const items = focusables(dialog);
    for (const action of pressed) {
      if (action === "activate") {
        if (!held.has(action)) {
          held.set(action, now);
          (focused(items) ?? items[0])?.click();
          settling = true;
        }
      } else if (action === "back" || action === "back2") {
        if (!held.has(action)) {
          held.set(action, now);
          dismiss(dialog, items);
          settling = true;
        }
      } else if (["up", "left", "down", "right"].includes(action)) {
        const step = action === "up" || action === "left" ? -1 : 1;
        if (!held.has(action)) {
          held.set(action, now + FIRST_REPEAT_MS);
          move(items, step);
        } else if (now >= held.get(action)) {
          held.set(action, now + REPEAT_MS);
          move(items, step);
        }
      }
    }
    for (const action of [...held.keys()]) {
      if (!pressed.has(action)) held.delete(action);
    }
    return;
  }

  // Settings is different: the pad must still be able to close it, or a
  // controller-only user is trapped the moment they open it. Everything else
  // in there is bound by pressing buttons, which reads the pad directly.
  if (document.getElementById("settings")) {
    const backIndex = Object.entries(map).find(([, a]) => a === "back")?.[0];
    const down = (navigator.getGamepads?.() ?? []).some(
      (p) => p && backIndex !== undefined && p.buttons[backIndex]?.pressed
    );
    if (down) {
      if (!held.has("closeSettings")) {
        held.set("closeSettings", performance.now());
        runAction("settings");
      }
    } else {
      held.delete("closeSettings");
    }
    return;
  }

  // Nothing here is ours while the emulator owns the screen.
  if (suspended) return;

  const now = performance.now();
  const pressed = pressedActions(navigator.getGamepads?.() ?? [], map);

  if (settling) {
    if (settleLifted(pressed, now)) settling = false;
    return;
  }

  for (const action of pressed) fire(action, now);
  // Release anything no longer held so the next press fires immediately.
  for (const action of [...held.keys()]) {
    if (!pressed.has(action)) held.delete(action);
  }
}

/// The modal currently over the window, if any.
///
/// Listed rather than detected, because "is this element a dialog" has no
/// answer a page can give: a right-click menu and a save-conflict question are
/// both just divs.
function openDialog() {
  return document.querySelector("#conflict-overlay, #bulk-overlay, .ctx-menu");
}

/// The things inside it a person could press, in the order they are drawn.
function focusables(dialog) {
  return [
    ...dialog.querySelectorAll(
      "button:not([disabled]), select:not([disabled]), input:not([disabled]):not([type=hidden])"
    ),
  ];
}

function focused(items) {
  return items.find((n) => n === document.activeElement);
}

/// Move the highlight, wrapping. A dialog is a short list and running off the
/// end of it is more annoying than coming round again.
function move(items, delta) {
  if (!items.length) return;
  const at = items.indexOf(document.activeElement);
  const next = items[(at + delta + items.length) % items.length] ?? items[0];
  next.focus();
  for (const n of items) n.classList.toggle("pad-focus", n === next);
}

/// Back out: whatever the dialog calls cancelling, or simply closing it.
function dismiss(dialog, items) {
  const cancel =
    dialog.querySelector(".bulk-cancel, [data-go=\"no\"], .cancel") ??
    // A right-click menu has no cancel; going back means it goes away.
    null;
  if (cancel) {
    cancel.click();
    return;
  }
  dialog.remove();
  void items;
}

/// Run one poll, for tests. The loop itself is driven by rAF, which a test
/// cannot wait on deterministically.
export function stepForTest() {
  step();
}

export function installGamepad() {
  if (!navigator.getGamepads) return;

  window.addEventListener("gamepadconnected", (ev) => {
    // id is a vendor string like "Xbox Wireless Controller (STANDARD GAMEPAD)".
    const name = ev.gamepad.id.replace(/\s*\(.*\)\s*$/, "").trim();
    toast(`Controller connected — ${name || "gamepad"}`);
    state.gamepad = name;
    // Reveals the shoulder-button hints in the tab bar. Hidden without a pad,
    // where they would be advice for hardware that is not there.
    document.body.classList.add("has-pad");
    if (!running) {
      running = true;
      requestAnimationFrame(poll);
    }
  });

  window.addEventListener("gamepaddisconnected", () => {
    if (!(navigator.getGamepads() ?? []).some(Boolean)) {
      running = false;
      held.clear();
      state.gamepad = null;
      document.body.classList.remove("has-pad");
      toast("Controller disconnected");
    }
  });

  // A pad paired before the window opened only appears once it is polled.
  if ((navigator.getGamepads() ?? []).some(Boolean)) {
    running = true;
    requestAnimationFrame(poll);
  }
}
