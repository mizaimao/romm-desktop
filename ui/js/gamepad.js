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

// Rebindable in Settings; padMap() layers the user's choices over the
// position-based defaults in bindings.js.

// Held directions repeat; one-shot buttons do not.
const REPEATABLE = new Set(["up", "down", "left", "right", "pageUp", "pageDown"]);
const FIRST_REPEAT_MS = 380;
const REPEAT_MS = 110;
const STICK_DEADZONE = 0.55;

const held = new Map(); // action -> next fire time
let running = false;

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

function poll() {
  if (!running) return;
  requestAnimationFrame(poll);

  // The lightbox and settings own input while open, as with the keyboard.
  if (!el.lb.hidden || document.getElementById("settings")) return;

  const pads = navigator.getGamepads?.() ?? [];
  const now = performance.now();
  const pressed = new Set();

  for (const pad of pads) {
    if (!pad) continue;

    for (const [index, action] of Object.entries(BUTTONS)) {
      if (pad.buttons[index]?.pressed) pressed.add(action);
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
  }

  for (const action of pressed) fire(action, now);
  // Release anything no longer held so the next press fires immediately.
  for (const action of [...held.keys()]) {
    if (!pressed.has(action)) held.delete(action);
  }
}

export function installGamepad() {
  if (!navigator.getGamepads) return;

  window.addEventListener("gamepadconnected", (ev) => {
    // id is a vendor string like "Xbox Wireless Controller (STANDARD GAMEPAD)".
    const name = ev.gamepad.id.replace(/\s*\(.*\)\s*$/, "").trim();
    toast(`Controller connected — ${name || "gamepad"}`);
    state.gamepad = name;
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
      toast("Controller disconnected");
    }
  });

  // A pad paired before the window opened only appears once it is polled.
  if ((navigator.getGamepads() ?? []).some(Boolean)) {
    running = true;
    requestAnimationFrame(poll);
  }
}
