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
  // Enough of a detail for the info pane to render: it joins several of these
  // arrays, so a thinner stub throws inside the pane rather than failing an
  // assertion here.
  if (cmd === "rom_detail")
    return {
      id: args.id,
      name: `Game ${args.id}`,
      fs_name: `game${args.id}.rom`,
      platform: "Super Nintendo",
      platform_slug: "snes",
      size_bytes: 1024,
      downloaded: false,
      files: [],
      screenshots: [],
      genres: [],
      companies: [],
      franchises: [],
      game_modes: [],
      regions: [],
      alt_names: [],
      art: {},
    };
  if (cmd === "status") return { configured: true };
  if (cmd === "recent_games") return recentGames;
  if (cmd === "download_estimate") return [estimateSummary, estimateFits, "note"];
  if (cmd === "game_states")
    return [{ slot: "1", label: "Slot 1", when: "yesterday", resumable: true }];
  if (cmd === "platforms")
    return [
      { slug: "snes", name: "Super Nintendo", rom_count: 12 },
      { slug: "psx", name: "PlayStation", rom_count: 40 },
      { slug: "megadrive", name: "Mega Drive", rom_count: 9 },
    ];
  return [];
}

/// What `recent_games` should answer with, per test.
let recentGames = [];
let estimateSummary = "10 game(s), about 1.0 GB";
let estimateFits = true;

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
  // jsdom has no layout, so it does not implement this at all. The app calls it
  // whenever it moves the cursor; without a stand-in every such path throws.
  dom.window.Element.prototype.scrollIntoView = function () {};
  // jsdom has no IntersectionObserver, and the cover loader builds one on every
  // render. A stand-in that observes nothing is right for these tests: they are
  // about what is drawn, not about lazy loading.
  class FakeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  dom.window.IntersectionObserver = FakeObserver;
  globalThis.IntersectionObserver = FakeObserver;

  const load = (m) => import(join(uiDir, "js", m));
  ui = {
    ...(await load("state.js")),
    ...(await load("keys.js")),
    ...(await load("bindings.js")),
    ...(await load("gamepad.js")),
    ...(await load("tabs.js")),
    ...(await load("library.js")),
    ...(await load("bulk.js")),
    ...(await load("lightbox.js")),
  };
});

beforeEach(async () => {
  invoked.length = 0;
  pads = [];
  recentGames = [];
  estimateFits = true;
  document.getElementById("bulk-overlay")?.remove();
  // A test that fails before its cleanup leaves its dialog in the document,
  // and the next test would then drive that one instead of its own.
  document.getElementById("conflict-overlay")?.remove();
  document.querySelector(".ctx-menu")?.remove();
  ui.closeLightbox();
  document.getElementById("list").innerHTML = "";
  ui.state.view = "platforms";
  ui.state.platform = null;
  ui.trail.length = 0;
  ui.resetPad();
  // Flush anything the previous test left held. The loop only forgets a button
  // on a poll where it is not pressed, and a test does not poll on the way out.
  pads = [];
  ui.stepForTest();
  // The section is module state and survives between tests. A test that ends
  // in My collections would otherwise change what "back" means for the next
  // one, which is the kind of coupling that makes a suite lie.
  await ui.showSection("library");
  invoked.length = 0;
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
    // Select cycles the pictures. It used to open settings — a second window
    // of text fields and tables that a pad cannot navigate, so the button
    // opened something you could then only leave again.
    assert.equal(map[8], "pictures", "Select should change the pictures");
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
    // Not yet on disk, so it downloads first; a downloaded game launches
    // straight away. Either is "A did something", which is what is being
    // asserted -- the bug this guards against is A doing nothing at all.
    assert.ok(
      invoked.slice(1).some((c) => c.cmd === "download_rom" || c.cmd === "launch_rom"),
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

describe("the triggers resize the covers", () => {
  test("LT and RT are bound to zoom, not to paging", () => {
    const map = ui.padMap();
    // LT out, RT in: the left trigger takes you back and the right one takes
    // you further, which is how every other pair of triggers here works.
    assert.equal(map[6], "zoomOut", "LT shrinks the covers");
    assert.equal(map[7], "zoomIn", "RT grows them");
    // The stick clicks sort the list now. They were paging, which the d-pad
    // and the sticks already do a screen at a time, so pressing them looked
    // like nothing happening.
    assert.equal(map[10], "sortCycle");
    // Nothing on the right stick click. It opened the sort menu, which a pad
    // cannot navigate — so the button opened something you could then only
    // close again.
    assert.equal(map[11], undefined, "the right stick click should be unbound");
    // The menu is still reachable by mouse and by key.
    assert.equal(ui.keyFor("sortMenu"), "s");
    // Paging is not gone, it is on the keyboard, where it has a key that says
    // what it does.
    assert.equal(ui.keyFor("pageUp"), "PageUp");
    assert.equal(ui.keyFor("pageDown"), "PageDown");
  });

  test("zoom stops at the slider's own limits", () => {
    const zoom = document.getElementById("zoom");
    document.getElementById("zoom-wrap").hidden = false;
    const max = Number(zoom.max);
    const min = Number(zoom.min);

    ui.state.zoom = max;
    ui.runAction("zoomIn");
    assert.equal(ui.state.zoom, max, "cannot grow past the top of the range");

    ui.state.zoom = min;
    ui.runAction("zoomOut");
    assert.equal(ui.state.zoom, min, "cannot shrink past the bottom");
  });

  test("nothing happens where there are no covers to resize", () => {
    document.getElementById("zoom-wrap").hidden = true;
    ui.state.zoom = 150;
    ui.runAction("zoomIn");
    assert.equal(ui.state.zoom, 150);
  });
});

describe("the lock after the emulator exits", () => {
  /// The quit combo is Select + A, and both are bound in here too. Coming back
  /// from RetroArch the pad can report nothing held for a frame or two before
  /// its real state arrives, so "wait for release" alone lifted the lock while
  /// the combo was still physically down — and the game relaunched itself.
  test("does not lift on the empty frame right after the emulator exits", () => {
    ui.ignorePadUntilReleased();
    const now = performance.now();
    assert.equal(
      ui.settleLifted(new Set(), now),
      false,
      "an empty frame immediately after the exit is the pad catching up, not a release"
    );
  });

  test("lifts once the pad is at rest and the floor has passed", () => {
    ui.ignorePadUntilReleased();
    const later = performance.now() + 500;
    assert.equal(ui.settleLifted(new Set(), later), true);
  });

  test("stays locked past the floor while a button is still held", () => {
    ui.ignorePadUntilReleased();
    const later = performance.now() + 500;
    assert.equal(
      ui.settleLifted(new Set(["activate"]), later),
      false,
      "the floor is a minimum, not a replacement for waiting"
    );
  });
});

describe("back at the top of a section", () => {
  /// Back did nothing at all from inside a console: with the crumb trail empty
  /// it asked for the section it was already in, and switching to the section
  /// you are already on is correctly a no-op. It has to reopen instead.
  test("reopens the section rather than asking to switch to it", async () => {
    cards(`<div class="card" data-slug="snes"></div>`);
    ui.runAction("activate");
    await settle();
    assert.equal(ui.state.view, "roms");

    await ui.resetSection();
    await settle();
    assert.equal(ui.state.view, "platforms", "Back must climb out of a console");
  });

  /// Back has to land on the front of the section you are in, not always on
  /// the platform grid. The tab bar says where you are and it has to stay true.
  test("lands on the front of the section you are actually in", async () => {
    await ui.showSection("mine");
    await settle();
    ui.state.view = "collection-roms";

    await ui.resetSection();
    await settle();
    assert.equal(ui.activeSection(), "mine", "Back must not tip you into the library");
    assert.notEqual(ui.state.view, "platforms");
  });
});

describe("grid / list on the consoles screen", () => {
  /// Pressing it there used to redraw `state.rows`, which still held the last
  /// console you opened — so the button appeared to open a console by itself.
  test("switching layout on the consoles screen does not open a console", async () => {
    cards(`<div class="card" data-slug="snes"></div>`);
    ui.runAction("activate");
    await settle();
    assert.equal(ui.state.view, "roms", "we opened one, so state.rows is populated");

    await ui.resetSection();
    await settle();
    assert.equal(ui.state.view, "platforms");

    const before = invoked.length;
    ui.setLayout("list");
    await settle();
    assert.equal(ui.state.view, "platforms", "the layout button must not open anything");
    assert.equal(
      invoked.slice(before).filter((c) => c.cmd === "roms").length,
      0,
      "no console was asked for"
    );
  });

  test("consoles render in both layouts, and stay reachable by keyboard", async () => {
    ui.state.platforms = [
      { slug: "snes", name: "Super Nintendo", rom_count: 50, playable: true },
      { slug: "gb", name: "Game Boy", rom_count: 32, playable: false },
    ];
    ui.state.view = "platforms";

    ui.setLayout("list");
    assert.equal(document.querySelectorAll(".prow").length, 2);
    // The navigation code selects `.card, .gcard, .row, .tcard`; a console row
    // that is not one of those cannot be reached without a mouse.
    assert.equal(document.querySelectorAll(".row").length, 2, "rows must be navigable");

    ui.setLayout("grid");
    assert.equal(document.querySelectorAll(".card").length, 2);
    assert.equal(document.querySelectorAll(".prow").length, 0);
  });

  test("opening a console from a row works, not just from a card", async () => {
    ui.state.platforms = [{ slug: "gb", name: "Game Boy", rom_count: 32, playable: true }];
    ui.state.view = "platforms";
    ui.setLayout("list");

    document.querySelector(".prow").click();
    await settle();
    assert.equal(openedPlatform(), "gb");
  });
});

describe("the pad while a game is running", () => {
  // The obvious test — suspend, press the quit combo, assert nothing opened —
  // could not be made to fail with the guard removed. `step` returns early on
  // several checks before it reaches the dispatcher, and every attempt ended up
  // asserting on a poll that never ran. A test that passes either way is worse
  // than none, so this is left to the machine it actually affects.
  // Same story for "resuming re-arms the lock": it passed with the re-arm
  // removed, because settleLifted reads module state the previous test had
  // already left in the right shape. Removed rather than kept as decoration.
});

describe("continue playing", () => {
  test("the row is absent entirely when nothing has been played", async () => {
    recentGames = [];
    await ui.showPlatforms();
    await settle();
    assert.equal(document.querySelector(".recent"), null,
      "an empty row explaining itself is worse than no row");
  });

  test("recent games appear above the consoles, in the order given", async () => {
    recentGames = [
      { id: 7, name: "Chrono Trigger", platform: "snes", downloaded: true },
      { id: 8, name: "Gunstar Heroes", platform: "megadrive", downloaded: false },
    ];
    await ui.showPlatforms();
    await settle();
    const strip = document.querySelector(".recent");
    assert.ok(strip, "the row should be drawn");
    const names = [...strip.querySelectorAll(".gname")].map((n) => n.textContent);
    assert.deepEqual(names, ["Chrono Trigger", "Gunstar Heroes"]);
    // Above the consoles, not after them: it is the first thing you look at.
    assert.equal(document.getElementById("list").firstElementChild, strip);
  });
});

describe("take offline", () => {
  /// The size is asked for, not assumed. Counting what is already on disk is
  /// one filesystem call per game, and on a 2,500-game collection doing that on
  /// every checkbox froze the window for seconds.
  test("opening the dialog does not count anything", async () => {
    ui.askDownload({ platform: "snes" });
    await settle();
    assert.equal(
      invoked.filter((c) => c.cmd === "download_estimate").length,
      0,
      "nothing should be counted until asked"
    );
  });

  test("checking the size asks once, with the cheap defaults", async () => {
    ui.askDownload({ platform: "snes" });
    await settle();
    document.querySelector(".bulk-size").click();
    await settle();
    const asked = invoked.filter((c) => c.cmd === "download_estimate");
    assert.equal(asked.length, 1);
    assert.deepEqual(asked[0].args.choice.platforms, ["snes"], "the console on screen starts ticked");
    assert.equal(asked[0].args.choice.art, "minimal");
    assert.equal(asked[0].args.choice.videos, false, "videos must never be on by default");
    assert.equal(asked[0].args.choice.bios, false, "300 MB must never arrive unasked");
  });

  /// Nobody travels with one console, and choosing them one at a time meant
  /// running the whole dialog once per system — each size check against a disk
  /// the previous run had already eaten into, so the last always claimed to
  /// fit and did not.
  test("several systems go in one download", async () => {
    ui.askDownload({ platform: "snes" });
    await settle();
    const psx = [...document.querySelectorAll(".bulk-plat")].find((c) => c.value === "psx");
    psx.checked = true;
    document.querySelector(".bulk-size").click();
    await settle();
    const asked = invoked.filter((c) => c.cmd === "download_estimate").at(-1);
    assert.deepEqual(asked.args.choice.platforms.sort(), ["psx", "snes"]);
  });

  test("All and None move every tick at once", async () => {
    ui.askDownload({ platform: "snes" });
    await settle();
    document.querySelector(".bulk-all").click();
    assert.equal([...document.querySelectorAll(".bulk-plat:checked")].length, 3);
    document.querySelector(".bulk-none").click();
    assert.equal([...document.querySelectorAll(".bulk-plat:checked")].length, 0);
  });

  /// Somewhere with no server is exactly where a missing BIOS becomes a console
  /// that will not boot and cannot be fixed. It belongs in the pane people open
  /// before going there, not three tabs deep in settings.
  test("BIOS can be taken along with the games", async () => {
    ui.askDownload({ platform: "psx" });
    await settle();
    const bios = document.querySelector(".bulk-bios");
    assert.ok(bios, "no way to take BIOS files from the offline pane");
    bios.checked = true;
    document.querySelector(".bulk-size").click();
    await settle();
    assert.equal(
      invoked.filter((c) => c.cmd === "download_estimate").at(-1).args.choice.bios,
      true
    );
  });

  /// With nothing ticked the backend cannot tell "everything" from "nothing",
  /// so the dialog refuses rather than guessing.
  test("downloading nothing is refused before it is sent", async () => {
    ui.askDownload({});
    await settle();
    document.querySelector(".bulk-none").click();
    document.querySelector(".bulk-go").click();
    await settle();
    assert.equal(
      invoked.filter((c) => c.cmd === "download_set").length,
      0,
      "an empty download reached the backend"
    );
  });

  /// A figure computed before a checkbox moved is a wrong figure. It is cleared
  /// rather than silently left on screen next to different options.
  test("changing an option clears the figure instead of recomputing", async () => {
    ui.askDownload({ platform: "snes" });
    await settle();
    document.querySelector(".bulk-size").click();
    await settle();
    const before = invoked.filter((c) => c.cmd === "download_estimate").length;

    const box = document.querySelector(".bulk-videos");
    box.checked = true;
    // Bubbling, as a real change event does: the dialog listens once on the
    // panel rather than on each control.
    box.dispatchEvent(new window.Event("change", { bubbles: true }));
    await settle();

    assert.equal(
      invoked.filter((c) => c.cmd === "download_estimate").length,
      before,
      "a checkbox must not trigger the expensive count"
    );
    assert.ok(
      !document.querySelector(".bulk-est").textContent.includes("GB"),
      "the stale figure must not sit next to the new options"
    );
  });

  /// The whole point of asking the disk: refusing here costs nothing, refusing
  /// an hour in costs a half-written game and a full disk.
  test("a download that will not fit cannot be started", async () => {
    estimateFits = false;
    ui.askDownload({ platform: "psx" });
    await settle();
    document.querySelector(".bulk-size").click();
    await settle();
    assert.equal(document.querySelector(".bulk-go").disabled, true);
    assert.ok(document.querySelector(".bulk-space").classList.contains("bad"));
  });
});

describe("the lightbox and the controller", () => {
  /// The player used to swallow the pad completely. So the button that started
  /// a video could not stop it, and the only way out of a full-screen video was
  /// to find the mouse — on a machine being used from a sofa with a controller.
  test("the button that opened a video closes it again", () => {
    const y = Number(
      Object.entries(ui.padMap()).find(([, a]) => a === "video")?.[0]
    );
    assert.ok(Number.isInteger(y), "nothing is bound to the video action");

    ui.openLightbox([{ src: "x.mp4", kind: "video", caption: "Gameplay" }], 0);
    assert.equal(ui.isLightboxOpen(), true);

    // One poll with nothing held, as happens between letting go of whatever
    // opened the player and reaching for the next button.
    pads = [pad([])];
    ui.stepForTest();

    pads = [pad([y])];
    ui.stepForTest();
    assert.equal(ui.isLightboxOpen(), false, "the pad cannot close the player");
  });

  test("back closes it too", () => {
    const b = Number(Object.entries(ui.padMap()).find(([, a]) => a === "back")?.[0]);
    ui.openLightbox([{ src: "x.png", kind: "image", caption: "Box" }], 0);
    pads = [pad([])];
    ui.stepForTest();
    pads = [pad([b])];
    ui.stepForTest();
    assert.equal(ui.isLightboxOpen(), false);
  });

  /// The triggers zoom whatever is in front of you. With the player open that
  /// is the video, not the covers behind it — which is what they used to
  /// resize, invisibly, while a video played over the top.
  test("the triggers zoom the video, not the grid behind it", () => {
    const zin = Number(Object.entries(ui.padMap()).find(([, a]) => a === "zoomIn")?.[0]);
    assert.ok(Number.isInteger(zin), "nothing is bound to zoom in");

    const before = ui.state.zoom;
    ui.openLightbox([{ src: "x.mp4", kind: "video", caption: "Gameplay" }], 0);
    pads = [pad([])];
    ui.stepForTest();
    pads = [pad([zin])];
    ui.stepForTest();

    assert.equal(ui.isLightboxOpen(), true, "zooming must not close it");
    assert.equal(ui.state.zoom, before, "the grid behind was resized instead");
    const scale = Number(document.getElementById("lightbox").style.getPropertyValue("--lb-zoom"));
    assert.ok(scale > 1, `the stage did not zoom (--lb-zoom = ${scale})`);
  });

  /// Coming back to a picture at whatever scale the last one was left at is
  /// disorienting, and nothing on screen explains it.
  test("zoom resets each time it opens", () => {
    ui.openLightbox([{ src: "x.mp4", kind: "video", caption: "Gameplay" }], 0);
    ui.zoomLightbox(1);
    ui.zoomLightbox(1);
    ui.closeLightbox();
    ui.openLightbox([{ src: "y.png", kind: "image", caption: "Box" }], 0);
    assert.equal(document.getElementById("lightbox").style.getPropertyValue("--lb-zoom"), "1");
  });
});

describe("four controllers", () => {
  /// With four pads plugged in for a four-player game, every one of them moved
  /// the cursor in the library — so three people fidgeting with sticks made the
  /// menu unusable while player one tried to pick a game. In the emulator all
  /// four are players; out here, player one is in charge.
  test("only the first controller drives the menus", () => {
    const map = ui.padMap();
    const right = Number(Object.entries(map).find(([, a]) => a === "right")?.[0]);
    const down = Number(Object.entries(map).find(([, a]) => a === "down")?.[0]);

    // Player two pressing right, player one pressing nothing.
    const acts = ui.pressedActions([pad([]), pad([right]), pad([down])], map);
    assert.equal(acts.size, 0, `player two moved the cursor: ${[...acts]}`);

    // And player one still works with the others connected.
    assert.ok(ui.pressedActions([pad([right]), pad([down])], map).has("right"));
  });

  /// A disconnected pad leaves a null hole in the list rather than shrinking
  /// it, so "the first one" is not always index zero.
  test("a gap where a controller was does not stop the next one", () => {
    const map = ui.padMap();
    const right = Number(Object.entries(map).find(([, a]) => a === "right")?.[0]);
    assert.ok(ui.pressedActions([null, pad([right])], map).has("right"));
  });

  /// The stick is read from the same pad as the buttons. Reading it from all
  /// of them put the second player's resting drift into player one's cursor.
  test("the stick is only read from the first controller", () => {
    const map = ui.padMap();
    const acts = ui.pressedActions([pad([]), pad([], { axes: [1, 0] })], map);
    assert.equal(acts.has("right"), false, "another pad's stick moved the cursor");
  });
});

describe("walking the reel with a pad", () => {
  test("the d-pad steps through the artwork while a video plays", () => {
    const map = ui.padMap();
    const right = Number(Object.entries(map).find(([, a]) => a === "right")?.[0]);
    ui.openLightbox(
      [
        { src: "a.png", kind: "image", caption: "Mix" },
        { src: "b.mp4", kind: "video", caption: "Gameplay" },
      ],
      1
    );
    const shown = () =>
      document.querySelector("#lightbox figcaption").textContent.split(" — ")[0];
    assert.equal(shown(), "Gameplay");
    pads = [pad([])];
    ui.stepForTest();
    pads = [pad([right])];
    ui.stepForTest();
    assert.equal(shown(), "Mix", "the d-pad did not change the picture");
    ui.closeLightbox();
  });

  test("the left stick does the same", () => {
    ui.openLightbox(
      [
        { src: "a.png", kind: "image", caption: "Mix" },
        { src: "b.mp4", kind: "video", caption: "Gameplay" },
      ],
      1
    );
    pads = [pad([])];
    ui.stepForTest();
    pads = [pad([], { axes: [1, 0] })];
    ui.stepForTest();
    assert.equal(
      document.querySelector("#lightbox figcaption").textContent.split(" — ")[0],
      "Mix",
      "the stick did not change the picture"
    );
    ui.closeLightbox();
  });
});

describe("sorting a list", () => {
  const games = [
    { id: 1, name: "Zed Blade", rating: 6.1, year: 1994, size_bytes: 900, favourite: false },
    { id: 2, name: "Alpha Mission", rating: null, year: 1985, size_bytes: 100, favourite: false },
    { id: 3, name: "Metal Slug", rating: 9.2, year: 1996, size_bytes: 500, favourite: false },
    { id: 4, name: "Zzz Last", rating: 8.0, year: 1999, size_bytes: 50, favourite: true },
  ];
  const names = (rows) => rows.map((g) => g.name);

  test("alphabetical by default", async () => {
    const sort = await import("../js/sort.js");
    ui.state.view = "roms";
    ui.state.platform = "neogeo";
    assert.equal(sort.currentOrder().id, "name");
    // The favourite is first whatever the order — that is what a favourite is.
    assert.deepEqual(names(sort.sorted(games)), [
      "Zzz Last", "Alpha Mission", "Metal Slug", "Zed Blade",
    ]);
  });

  /// An unrated game is not a bad game, and a list that opens on the unknowns
  /// answers nothing.
  test("rating sorts high to low, with the unrated last", async () => {
    const sort = await import("../js/sort.js");
    ui.state.view = "roms";
    ui.state.platform = "neogeo";
    sort.setOrder("rating");
    assert.deepEqual(names(sort.sorted(games)), [
      "Zzz Last", "Metal Slug", "Zed Blade", "Alpha Mission",
    ]);
    sort.setOrder("name");
  });

  /// "Sort this console by rating" is a statement about that console. Carrying
  /// it to the next one silently reorders a screen nobody asked about.
  test("each console keeps its own order", async () => {
    const sort = await import("../js/sort.js");
    ui.state.view = "roms";
    ui.state.platform = "neogeo";
    sort.setOrder("year");
    assert.equal(sort.currentOrder().id, "year");

    ui.state.platform = "snes";
    assert.equal(sort.currentOrder().id, "name", "the order followed to another console");

    ui.state.platform = "neogeo";
    assert.equal(sort.currentOrder().id, "year", "and was forgotten on the way back");
    sort.setOrder("name");
  });

  /// The console grid is a couple of dozen tiles in an order people learn the
  /// shape of. Shuffling it costs more than it gives.
  test("the console grid has no sort", async () => {
    const sort = await import("../js/sort.js");
    ui.state.view = "platforms";
    assert.equal(sort.sortable(), false);
    assert.equal(sort.cycleOrder(1), null, "the stick click sorted the consoles");
    ui.state.view = "roms";
  });

  test("sorting never reorders the caller's array", async () => {
    const sort = await import("../js/sort.js");
    ui.state.view = "roms";
    ui.state.platform = "neogeo";
    sort.setOrder("size");
    const before = names(games);
    sort.sorted(games);
    assert.deepEqual(names(games), before, "state.rows was sorted in place");
    sort.setOrder("name");
  });
});

describe("opening the player with a held button", () => {
  /// Y opens the player and Y closes it, and a button is held down across
  /// several polls. So the poll that first sees the player open must not treat
  /// the button that opened it as a fresh press — or the player opens and shuts
  /// in the same breath and the button looks dead.
  test("the button that opened the player does not immediately close it", () => {
    const y = Number(Object.entries(ui.padMap()).find(([, a]) => a === "video")?.[0]);
    pads = [pad([y])];
    // The press that opens it. The real path goes through playVideo, which is
    // async; the poll below is what would happen while the button is still down.
    ui.openLightbox([{ src: "x.mp4", kind: "video", caption: "Gameplay" }], 0);
    ui.stepForTest();
    assert.equal(ui.isLightboxOpen(), true, "the player shut as soon as it opened");

    // Released, then pressed again: now it should close.
    pads = [pad([])];
    ui.stepForTest();
    pads = [pad([y])];
    ui.stepForTest();
    assert.equal(ui.isLightboxOpen(), false, "a second press should close it");
  });
});

describe("the menu on a game", () => {
  /// It offered "take this console offline", which on a game in Continue
  /// playing reads as an offer to download a whole platform — the opposite of
  /// the small local thing a right-click on one game should do. What it offers
  /// now is that game's own save states.
  test("offers this game's save states, not a platform download", async () => {
    document.getElementById("list").innerHTML =
      `<div class="gcards"><div class="gcard" data-id="7"></div></div>`;
    const card = document.querySelector(".gcard");
    ui.wireGame(card, 7);

    card.dispatchEvent(
      new window.MouseEvent("contextmenu", { bubbles: true, cancelable: true })
    );
    await settle();
    await settle();

    const menu = document.querySelector(".ctx-menu");
    assert.ok(menu, "no menu opened");
    const text = menu.textContent;
    assert.doesNotMatch(text, /offline/i, "still offering to download the console");
    assert.match(text, /play/i, "no way to start the game");
    assert.match(text, /Delete Slot 1/, "the game's own states are not offered");
    document.querySelector(".ctx-menu")?.remove();
  });
});

describe("dialogs answer the controller", () => {
  /// A launch can stop dead on "your saves could not be checked — play
  /// anyway?", and that question was mouse-only. On a machine being used from
  /// a sofa that is a dead end, not a dialog.
  const openDialog = () => {
    const d = document.createElement("div");
    d.id = "conflict-overlay";
    d.innerHTML = `<button class="a" data-name="a">Play anyway</button>
                   <button class="b" data-name="b">Keep the server's</button>
                   <button data-go="no" class="cancel" data-name="cancel">Cancel</button>`;
    document.body.appendChild(d);
    return d;
  };

  test("the d-pad moves through the buttons and A presses one", () => {
    const map = ui.padMap();
    const down = Number(Object.entries(map).find(([, a]) => a === "down")?.[0]);
    const go = Number(Object.entries(map).find(([, a]) => a === "activate")?.[0]);
    const d = openDialog();
    let clicked = null;
    for (const b of d.querySelectorAll("button")) {
      b.addEventListener("click", () => (clicked = b.dataset.name));
    }

    pads = [pad([down])];
    ui.stepForTest();
    assert.ok(
      document.activeElement.classList.contains("pad-focus"),
      "nothing shows where the controller is"
    );
    pads = [pad([])];
    ui.stepForTest();
    pads = [pad([down])];
    ui.stepForTest();

    pads = [pad([])];
    ui.stepForTest();
    pads = [pad([go])];
    ui.stepForTest();
    assert.equal(clicked, "b", `pressed ${clicked} instead of the second button`);
    d.remove();
  });

  test("B backs out through whatever the dialog calls cancelling", () => {
    const map = ui.padMap();
    const back = Number(Object.entries(map).find(([, a]) => a === "back")?.[0]);
    const d = openDialog();
    let cancelled = false;
    d.querySelector(".cancel").addEventListener("click", () => (cancelled = true));

    pads = [pad([back])];
    ui.stepForTest();
    assert.equal(cancelled, true, "the pad could not back out of the dialog");
    d.remove();
  });

  /// While a dialog is up it owns the pad: moving the cursor in the library
  /// behind it is how someone answers a question about a game they are no
  /// longer looking at.
  test("the library behind it does not move", () => {
    const map = ui.padMap();
    const down = Number(Object.entries(map).find(([, a]) => a === "down")?.[0]);
    document.getElementById("list").innerHTML =
      `<div class="gcards"><div class="gcard sel" data-id="1"></div><div class="gcard" data-id="2"></div></div>`;
    const d = openDialog();
    pads = [pad([down])];
    ui.stepForTest();
    assert.ok(
      document.querySelector('.gcard[data-id="1"]').classList.contains("sel"),
      "the selection moved in the library while a dialog was open"
    );
    d.remove();
  });
});
