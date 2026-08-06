// The controller path, exercised against the real index.html.
//
// This exists because "A and B do nothing" was reported five times and every
// diagnosis was made by reading the code, which was wrong every time. Reading
// cannot see an exception thrown inside a requestAnimationFrame callback: the
// next frame is already scheduled, so the loop keeps running and the failure is
// silent. These tests run the actual modules and assert on what happens.
//
//   npm test
//
// One page and one module graph for the whole file, because that is how the app
// runs: state.js caches element handles at import time, so the DOM has to exist
// first and cannot be swapped afterwards. Tests reset the list and the view
// instead of rebuilding the world.

import { test, describe, before, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

/// Selection class used across the UI. `sel`, not `selected` — worth naming
/// once here so a test never disagrees with library.js by a typo.
const SEL = "sel";

let dom, invoked, pads, ui;

/// Minimal stand-ins for the backend. Shapes matter: the render path reads
/// `.length` off list responses and `.downloaded` off a detail, so returning a
/// bare null here surfaces as an unhandled rejection a frame later rather than
/// as a failed assertion.
function reply(cmd, args) {
  if (cmd === "rom_detail") return { id: args.id, downloaded: false, files: [] };
  if (cmd === "status") return { configured: true };
  return [];
}

/// Let the promise chain a UI action kicked off run to completion, so its
/// failures land inside the test that caused them.
const settle = () => new Promise((r) => dom.window.setTimeout(r, 0));

before(async () => {
  dom = new JSDOM(readFileSync(join(uiDir, "index.html"), "utf8"), {
    url: "http://localhost/",
    pretendToBeVisual: true,
  });

  invoked = [];
  dom.window.__TAURI__ = {
    core: {
      invoke: (cmd, args) => {
        invoked.push({ cmd, args });
        return Promise.resolve(reply(cmd, args));
      },
      convertFileSrc: (p) => p,
    },
    event: { listen: () => Promise.resolve(() => {}) },
  };

  pads = [];
  dom.window.navigator.getGamepads = () => pads;

  // defineProperty, not assignment: `navigator` is a getter-only global in
  // Node. `performance` is deliberately left alone — shadowing it makes
  // jsdom's own now() recurse until the stack blows.
  for (const k of ["window", "document", "navigator", "localStorage", "CSS"]) {
    Object.defineProperty(globalThis, k, { value: dom.window[k], configurable: true });
  }
  globalThis.requestAnimationFrame = (fn) => dom.window.setTimeout(fn, 0);

  const load = (m) => import(join(uiDir, "js", m));
  ui = {
    ...(await load("state.js")),
    ...(await load("keys.js")),
    ...(await load("bindings.js")),
    ...(await load("gamepad.js")),
  };
});

beforeEach(() => {
  invoked.length = 0;
  pads = [];
  document.getElementById("list").innerHTML = "";
  ui.state.view = "platforms";
  ui.state.platform = null;
  ui.trail.length = 0;
  ui.resetPad();
});

/// A gamepad with `down` pressed, shaped like the real thing.
function pad(down = [], { mapping = "standard", axes = [0, 0] } = {}) {
  return {
    id: "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)",
    mapping,
    connected: true,
    axes,
    buttons: Array.from({ length: 17 }, (_, i) => ({
      pressed: down.includes(i),
      value: down.includes(i) ? 1 : 0,
    })),
  };
}

function cards(html) {
  document.getElementById("list").innerHTML = html;
  return [...document.querySelectorAll(".card")];
}

/// The slug of the console the UI last asked the backend for.
const openedPlatform = () =>
  [...invoked].reverse().find((c) => c.cmd === "roms")?.args.platform ?? null;

describe("pad bindings", () => {
  test("the default map sends the face buttons to open and back", () => {
    const map = ui.padMap();
    assert.equal(map[0], "activate", "bottom face button opens");
    assert.equal(map[1], "back", "right face button goes back");
    assert.equal(map[8], "settings", "Back/Select opens settings");
  });

  test("every default binding names a handler that exists", () => {
    // The failure this guards against is invisible at runtime: runAction looks
    // the id up, finds nothing, and returns without a sound. A binding pointing
    // at a renamed handler is exactly "the button does nothing".
    for (const [index, action] of Object.entries(ui.padMap())) {
      assert.ok(
        Object.hasOwn(ui.HANDLERS, action),
        `button ${index} is bound to "${action}", which is not in HANDLERS`
      );
    }
  });

  test("a rebind moves the action and frees the old button", () => {
    ui.setPad("activate", 3);
    assert.equal(ui.padMap()[3], "activate");
    assert.equal(ui.padMap()[0], null, "the old button is cleared, not left dangling");
    ui.resetPad();
    assert.equal(ui.padMap()[0], "activate", "reset restores the defaults");
  });

  test("padFor reports where an action currently lives", () => {
    assert.equal(ui.padFor("activate"), 0);
    ui.setPad("activate", 2);
    assert.equal(ui.padFor("activate"), 2);
  });
});

describe("activate — the A button", () => {
  test("opens the focused console", async () => {
    cards(`<div class="card" data-slug="snes"></div><div class="card" data-slug="genesis"></div>`)[1]
      .classList.add(SEL);
    ui.runAction("activate");
    await settle();
    assert.equal(openedPlatform(), "genesis");
  });

  test("opens the first console when nothing is focused yet", async () => {
    cards(`<div class="card" data-slug="snes"></div>`);
    ui.runAction("activate");
    await settle();
    assert.equal(openedPlatform(), "snes", "a fresh grid must still respond to A");
  });

  test("opens the focused game", async () => {
    cards(`<div class="card" data-id="42"></div>`);
    ui.runAction("activate");
    await settle();
    assert.deepEqual(
      invoked[0],
      { cmd: "rom_detail", args: { id: 42 } },
      "A on a game asks the backend for its detail first"
    );
    assert.ok(
      invoked.slice(1).some((c) => c.cmd === "launch_rom" || c.cmd === "download"),
      `and then acts on it; got ${invoked.map((c) => c.cmd).join(" -> ")}`
    );
  });

  test("clicks a collection card, reusing the handler that knows its group", () => {
    let clicked = 0;
    cards(`<div class="card" data-cid="7"></div>`)[0].addEventListener("click", () => clicked++);
    ui.runAction("activate");
    assert.equal(clicked, 1);
  });

  test("does nothing, quietly, on an empty grid", () => {
    assert.doesNotThrow(() => ui.runAction("activate"));
    assert.equal(invoked.length, 0);
  });
});

describe("back — the B button", () => {
  test("is a no-op on the top-level grid", () => {
    ui.state.view = "platforms";
    assert.doesNotThrow(() => ui.runAction("back"));
    assert.equal(ui.state.view, "platforms");
  });

  test("returns to the grid from a console", async () => {
    cards(`<div class="card" data-slug="snes"></div>`);
    ui.runAction("activate");
    await settle();
    assert.equal(ui.state.view, "roms");
    ui.runAction("back");
    await settle();
    assert.equal(ui.state.view, "platforms", "B must climb out of a console");
  });

  test("unwinds one level at a time inside collections", () => {
    ui.state.view = "collection-roms";
    let popped = 0;
    ui.trail.push(() => popped++);
    ui.runAction("back");
    assert.equal(popped, 1, "the trail is walked, not skipped past");
    assert.equal(ui.trail.length, 0);
  });
});

describe("the poll loop", () => {
  // These call the poll loop's own translation of pad state to actions,
  // rather than re-deriving it, so the test fails if that logic drifts.
  const pressedActions = (...a) => ui.pressedActions(...a);

  test("a pressed A resolves to activate", () => {
    assert.deepEqual([...pressedActions([pad([0])], ui.padMap())], ["activate"]);
  });

  test("a pressed B resolves to back", () => {
    assert.deepEqual([...pressedActions([pad([1])], ui.padMap())], ["back"]);
  });

  test("an unbound button resolves to nothing", () => {
    assert.deepEqual([...pressedActions([pad([16])], ui.padMap())], []);
  });

  test("a button cleared by a rebind is skipped rather than dispatched as null", () => {
    const map = { ...ui.padMap(), 0: null };
    assert.deepEqual([...pressedActions([pad([0])], map)], []);
  });

  test("the stick moves along its dominant axis only", () => {
    // Pushed diagonally, reporting both would move the cursor twice in one
    // frame, which reads as it jumping around on its own.
    assert.deepEqual([...pressedActions([pad([], { axes: [-0.9, -0.7] })], ui.padMap())], ["left"]);
    assert.deepEqual([...pressedActions([pad([], { axes: [-0.7, -0.9] })], ui.padMap())], ["up"]);
  });

  test("a resting stick reports nothing", () => {
    assert.deepEqual([...pressedActions([pad([], { axes: [0.2, -0.3] })], ui.padMap())], []);
  });

  test("a disconnected slot is skipped", () => {
    assert.doesNotThrow(() => pressedActions([null, pad([0])], ui.padMap()));
    assert.deepEqual([...pressedActions([null, pad([0])], ui.padMap())], ["activate"]);
  });

  test("a pad reporting no standard mapping still works by index", () => {
    // WebKit does not always fill in `mapping`. The buttons are still in
    // standard order, so nothing should key off that field.
    assert.deepEqual([...pressedActions([pad([0], { mapping: "" })], ui.padMap())], ["activate"]);
  });
});
