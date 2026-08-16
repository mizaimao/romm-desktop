// The save-state shelf, and the history page.
//
// The shelf is worth testing because its failure mode is quiet and expensive:
// a state is the only record of where you are in a game, it cannot be
// downloaded again, and the difference between "resume slot 3" and "start from
// the title screen" is invisible until an hour of play has gone. So what is
// checked here is that the slot actually travels to the backend, and that the
// autosave — which has no slot number and cannot be entered — is not offered as
// though it could be.

import { test, describe, before, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

let dom, detail, history, invoked, states, historyData;

/// Enough of a detail for the pane to render without reaching for something
/// that is not there; the shelf is the part under test.
const DETAIL = {
  id: 7,
  name: "Chrono Trigger",
  fs_name: "ct.sfc",
  platform: "Super Nintendo",
  size_bytes: 4_194_304,
  downloaded: true,
  screenshots: [],
  genres: [],
  companies: [],
  franchises: [],
  game_modes: [],
  regions: [],
  alt_names: [],
  art: {},
};

before(async () => {
  dom = new JSDOM(readFileSync(join(uiDir, "index.html"), "utf8"), {
    url: "http://localhost/",
    pretendToBeVisual: true,
  });
  global.window = dom.window;
  global.document = dom.window.document;
  global.HTMLElement = dom.window.HTMLElement;
  global.localStorage = dom.window.localStorage;
  global.requestAnimationFrame = dom.window.requestAnimationFrame.bind(dom.window);
  Object.defineProperty(global, "navigator", {
    value: dom.window.navigator,
    configurable: true,
  });

  dom.window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        invoked.push({ cmd, args });
        if (cmd === "rom_detail") return DETAIL;
        if (cmd === "game_cores") return [];
        if (cmd === "game_states") return states;
        if (cmd === "play_history") return historyData;
        if (cmd === "launch_rom") return "played for 3 minutes";
        if (cmd === "game_video") return "/v/clip.mp4";
        if (cmd === "confirm_delete_state") return false;
        if (cmd === "delete_state") return "deleted Slot 3";
        if (cmd === "set_config_field") return "saved";
        return [];
      },
      convertFileSrc: (p) => `asset://${p}`,
    },
    event: { listen: async () => () => {} },
  };

  detail = await import("../js/detail.js");
  history = await import("../js/history.js");

  // The launch path measures the display over 24 animation frames before it
  // sends anything. Taken here, once, so a click in a test reaches the backend
  // in the same tick it would in an app that has been open for a second.
  const actions = await import("../js/actions.js");
  actions.warmRefresh();
  await frames(40);
});

/// Let `n` animation frames pass.
const frames = (n) =>
  new Promise((done) => {
    let left = n;
    const tick = () => (left-- > 0 ? dom.window.requestAnimationFrame(tick) : done());
    tick();
  });

beforeEach(() => {
  invoked = [];
  states = [];
  historyData = { total_seconds: 0, sessions: 0, games: 0, platforms: [], top: [], abandoned: [] };
});

const settle = () => new Promise((r) => dom.window.setTimeout(r, 0));

describe("the save-state shelf", () => {
  test("a game with no states shows no shelf at all", async () => {
    await detail.selectRom(7);
    await settle();
    assert.equal(document.querySelector(".shelf"), null, "an empty shelf is clutter");
  });

  test("each state is on the shelf, with its picture and how long ago", async () => {
    states = [
      {
        slot: "auto",
        label: "Where you left off",
        thumb: "/s/ct.state.auto.png",
        when: "2 hours ago",
        size_bytes: 900,
        core: "snes9x",
        resumable: false,
      },
      {
        slot: "3",
        label: "Slot 3",
        thumb: "/s/ct.state3.png",
        when: "6 days ago",
        size_bytes: 900,
        core: "snes9x",
        resumable: true,
      },
    ];
    await detail.selectRom(7);
    await settle();

    const shelf = [...document.querySelectorAll(".state")];
    assert.equal(shelf.length, 2);
    assert.match(shelf[0].textContent, /Where you left off/);
    assert.match(shelf[0].textContent, /2 hours ago/);
    assert.match(shelf[1].querySelector("img").src, /ct\.state3\.png/);
  });

  /// The autosave is the newest thing on the shelf and the one people will
  /// reach for. It has no slot number, so RetroArch cannot be told to enter it
  /// — `--entryslot auto` is not a thing. Offering it as a button that quietly
  /// starts the game from the beginning would be the worst outcome here.
  test("the autosave is shown but cannot be started from", async () => {
    states = [
      {
        slot: "auto",
        label: "Where you left off",
        thumb: null,
        when: "just now",
        size_bytes: 900,
        core: "snes9x",
        resumable: false,
      },
    ];
    await detail.selectRom(7);
    await settle();

    const auto = document.querySelector(".state");
    assert.ok(auto, "the autosave should still be visible");
    // Greyed, but not `disabled`: a disabled button fires no mouse events at
    // all, so marking it disabled also made it impossible to right-click —
    // and right-click is the only way to delete a state.
    assert.equal(auto.disabled, false, "a disabled button cannot be right-clicked");
    assert.ok(auto.classList.contains("noresume"), "it must not look like it works");

    auto.click();
    await settle();
    assert.equal(
      invoked.filter((c) => c.cmd === "launch_rom").length,
      0,
      "clicking it started the game — from the title screen, silently"
    );
  });

  /// The one this keeps failing on. A state that cannot be resumed still has
  /// to be deletable, and the only route to that is the right-click menu.
  test("the autosave can be right-clicked even though it cannot be started", async () => {
    states = [
      {
        slot: "auto",
        label: "Where you left off",
        thumb: null,
        when: "just now",
        size_bytes: 900,
        core: "FinalBurn Neo",
        resumable: false,
      },
    ];
    await detail.selectRom(7);
    await settle();

    const auto = document.querySelector(".state");
    auto.dispatchEvent(
      new dom.window.MouseEvent("contextmenu", { bubbles: true, cancelable: true })
    );
    await settle();

    const menu = document.querySelector(".ctx-menu");
    assert.ok(menu, "no menu on the one state that can only be deleted");
    assert.match(menu.textContent, /Delete/);
    menu.querySelector("button").click();
    await settle();
    assert.equal(
      invoked.find((c) => c.cmd === "delete_state")?.args.slot,
      "auto",
      "the autosave was not the state that got deleted"
    );
  });

  test("starting from a state sends that slot and no other", async () => {
    states = [
      {
        slot: "2",
        label: "Slot 2",
        thumb: null,
        when: "yesterday",
        size_bytes: 900,
        core: "snes9x",
        resumable: true,
      },
    ];
    await detail.selectRom(7);
    await settle();

    document.querySelector(".state").click();
    await settle();

    const launch = invoked.find((c) => c.cmd === "launch_rom");
    assert.ok(launch, "the shelf did not launch anything");
    assert.equal(launch.args.entrySlot, 2);
    assert.equal(launch.args.id, 7);
  });

  /// A state whose picture was never saved still has to be pickable. States
  /// made before thumbnails were switched on have none, and no amount of
  /// asking will produce one — the frame is not in the file.
  test("a state with no picture is still on the shelf", async () => {
    states = [
      {
        slot: "5",
        label: "Slot 5",
        thumb: null,
        when: "a month ago",
        size_bytes: 900,
        core: "snes9x",
        resumable: true,
      },
    ];
    await detail.selectRom(7);
    await settle();

    const btn = document.querySelector(".state");
    assert.equal(btn.querySelector("img"), null);
    assert.match(btn.querySelector(".state-blank").textContent, /5/);
    assert.equal(btn.disabled, false);
  });
});

describe("the history page", () => {
  /// A fresh install has nothing recorded, and three empty headings read as a
  /// broken page rather than a new one.
  test("with nothing recorded it says so rather than showing empty lists", async () => {
    await history.showHistory();
    await settle();
    assert.match(document.getElementById("list").textContent, /Nothing recorded yet/);
    assert.equal(document.querySelector(".hist-bar"), null);
  });

  test("consoles are drawn in proportion to the longest one", async () => {
    historyData = {
      total_seconds: 36_000,
      sessions: 12,
      games: 4,
      platforms: [
        { slug: "snes", name: "Super Nintendo", seconds: 27_000, spelled: "7 h 30 m", sessions: 8, games: 3 },
        { slug: "psx", name: "PlayStation", seconds: 9_000, spelled: "2 h 30 m", sessions: 4, games: 1 },
      ],
      top: [
        { id: 1, name: "Chrono Trigger", platform: "Super Nintendo", seconds: 20_000, spelled: "5 h 33 m", sessions: 5, last: "2026-08-01T10:00:00" },
      ],
      abandoned: [],
    };
    await history.showHistory();
    await settle();

    const widths = [...document.querySelectorAll(".hist-bar span")].map((b) => b.style.width);
    // The longest console fills its bar; everything else is measured against
    // it. Scaling against an absolute maximum leaves every console after the
    // first as an unreadable stub.
    assert.equal(widths[0], "100%");
    assert.match(widths[1], /^33\.3/);
  });

  test("the picked-up-and-put-down list only appears when there is one", async () => {
    historyData = {
      total_seconds: 3600,
      sessions: 4,
      games: 2,
      platforms: [{ slug: "snes", name: "Super Nintendo", seconds: 3600, spelled: "1 h 00 m", sessions: 4, games: 2 }],
      top: [],
      abandoned: [],
    };
    await history.showHistory();
    await settle();
    assert.doesNotMatch(document.getElementById("list").textContent, /Picked up and put down/);

    historyData.abandoned = [
      { id: 9, name: "Bounced Off", platform: "Super Nintendo", seconds: 900, spelled: "15 minutes", sessions: 3, last: null },
    ];
    await history.showHistory();
    await settle();
    const text = document.getElementById("list").textContent;
    assert.match(text, /Picked up and put down/);
    assert.match(text, /3 goes/);
  });
});

describe("browsing a game's media", () => {
  /// Each thing used to open on its own. Clicking the cart art gave you the
  /// cart art and nothing else; pressing Y gave you the video and nothing else.
  /// So the arrow keys had a set of one to walk through and looked broken —
  /// where ES-DE treats a game's media as one reel.
  test("opening one picture opens the whole reel, positioned on it", async () => {
    Object.assign(DETAIL, {
      art: { miximages: "/a/mix.png", "3dboxes": "/a/box.png", physicalmedia: "/a/cart.png" },
      cover: "/a/cover.png",
      screenshots: ["/a/s1.png"],
      has_video: true,
    });
    await detail.selectRom(7);
    await settle();

    const cart = document.querySelector('.artstrip figure[data-art="physicalmedia"]');
    assert.ok(cart, "the cart art is not in the strip");
    cart.click();
    await settle();

    const caption = document.querySelector("#lightbox figcaption").textContent;
    // "n of m" only appears when there is more than one thing to walk to.
    assert.match(caption, /Cart\/disc/);
    assert.match(caption, /of \d+/, `a set of one: ${caption}`);
  });

  /// The order is the strip's order, so stepping right lands on the picture to
  /// the right rather than somewhere the layout does not explain.
  test("the arrows walk the reel in the order it is drawn", async () => {
    await detail.selectRom(7);
    await settle();
    document.querySelector('.artstrip figure[data-art="miximages"]').click();
    await settle();

    const captionNow = () =>
      document.querySelector("#lightbox figcaption").textContent.split(" — ")[0];
    assert.equal(captionNow(), "Mix");

    const lb = await import("../js/lightbox.js");
    lb.stepLightbox(1);
    assert.equal(captionNow(), "3D box", "stepping right left the strip's order");
    lb.stepLightbox(-1);
    assert.equal(captionNow(), "Mix");
    // And it wraps, so holding a direction never dead-ends.
    lb.stepLightbox(-1);
    assert.notEqual(captionNow(), "Mix");
    lb.closeLightbox();
  });
});

describe("the video button", () => {
  test("pressing it opens the player", async () => {
    Object.assign(DETAIL, { has_video: true, art: { miximages: "/a/mix.png" } });
    await detail.selectRom(7);
    await settle();

    const btn = document.getElementById("playvid");
    assert.ok(btn, "no video button for a game that has one");

    await detail.playVideo();
    await settle();

    const lb = await import("../js/lightbox.js");
    assert.equal(lb.isLightboxOpen(), true, "the player did not open");
    assert.ok(
      document.querySelector("#lightbox video"),
      "the player opened on something that is not the video"
    );
    lb.closeLightbox();
  });
});

describe("the right-click menu", () => {
  /// It shipped with no stylesheet, so it was laid out in the normal flow and
  /// appeared past the end of the page — a menu that existed in the DOM and
  /// could not be seen or reached. Nothing about the JavaScript said so.
  test("right-clicking a save state opens a menu that is actually placed", async () => {
    states = [
      {
        slot: "2",
        label: "Slot 2",
        thumb: null,
        when: "yesterday",
        size_bytes: 900,
        core: "snes9x",
        resumable: true,
      },
    ];
    await detail.selectRom(7);
    await settle();

    const btn = document.querySelector(".state");
    const ev = new dom.window.MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 120,
      clientY: 90,
    });
    btn.dispatchEvent(ev);
    await settle();

    assert.equal(ev.defaultPrevented, true, "the browser's own menu was left to appear");
    const menu = document.querySelector(".ctx-menu");
    assert.ok(menu, "no menu opened");
    // Placed where the click was. That the rule making the placement mean
    // anything exists at all is checked against the stylesheet, in
    // chrome.test.js — that is the half that was missing.
    assert.equal(menu.style.left, "120px");
    assert.equal(menu.style.top, "90px");
    assert.match(menu.textContent, /Delete/);
  });

  test("choosing delete asks the backend for that slot", async () => {
    states = [
      {
        slot: "3",
        label: "Slot 3",
        thumb: null,
        when: "yesterday",
        size_bytes: 900,
        core: "snes9x",
        resumable: true,
      },
    ];
    await detail.selectRom(7);
    await settle();

    document.querySelector(".state").dispatchEvent(
      new dom.window.MouseEvent("contextmenu", { bubbles: true, cancelable: true })
    );
    await settle();
    document.querySelector(".ctx-menu button").click();
    await settle();

    const call = invoked.find((c) => c.cmd === "delete_state");
    assert.ok(call, "nothing was deleted");
    assert.equal(call.args.slot, "3");
    assert.equal(call.args.id, 7);
  });
});

describe("rapid fire, above the Play button", () => {
  /// It is a thing you change about the run you are about to start — try Y,
  /// play a level, decide it should have been A — so it sits with Play rather
  /// than three windows away in Settings, and does not scroll off with the
  /// artwork.
  test("games that can have it get the control, pinned with Play", async () => {
    Object.assign(DETAIL, { autofire: "y" });
    await detail.selectRom(7);
    await settle();

    const row = document.querySelector(".autofire-row");
    assert.ok(row, "no rapid-fire control");
    assert.ok(
      row.closest(".pinned"),
      "it is in the scrolling part, so it disappears as you read the game"
    );
    assert.ok(
      row.compareDocumentPosition(document.getElementById("play")) &
        dom.window.Node.DOCUMENT_POSITION_FOLLOWING,
      "it should sit above Play, not below it"
    );
    assert.equal(row.querySelectorAll(".af").length, 3, "three choices");
    assert.equal(row.querySelector(".af.on").dataset.af, "y");
  });

  test("games it does not apply to get nothing at all", async () => {
    // Every shape "not applicable" can arrive in. A console game, a game with
    // no metadata, an older backend that does not send the field: all of them
    // must draw nothing rather than a row with no selection.
    for (const value of [null, undefined, "", "maybe", 0]) {
      Object.assign(DETAIL, { autofire: value });
      await detail.selectRom(7);
      await settle();
      assert.equal(
        document.querySelector(".autofire-row"),
        null,
        `a rapid-fire row appeared for autofire=${JSON.stringify(value)}`
      );
    }
  });

  /// "Off" and "not applicable" are different answers: one shows a control
  /// with Off selected, the other shows no control.
  test("off still shows the control, with off selected", async () => {
    Object.assign(DETAIL, { autofire: "off" });
    await detail.selectRom(7);
    await settle();
    assert.ok(document.querySelector(".autofire-row"), "the control vanished when turned off");
    assert.equal(document.querySelector(".af.on").dataset.af, "off");
  });

  test("choosing one saves it", async () => {
    Object.assign(DETAIL, { autofire: "off" });
    await detail.selectRom(7);
    await settle();
    document.querySelector('.af[data-af="a"]').click();
    await settle();
    const saved = invoked.find((c) => c.cmd === "set_config_field");
    assert.ok(saved, "nothing was saved");
    assert.equal(saved.args.field, "autofire");
    assert.equal(saved.args.value, "a");
    Object.assign(DETAIL, { autofire: null });
  });
});

describe("how fast the rapid fire goes", () => {
  test("the rate shows and steps by one", async () => {
    Object.assign(DETAIL, { autofire: "y", autofire_hz: 5 });
    await detail.selectRom(7);
    await settle();

    assert.match(document.querySelector(".af-hz").textContent, /5 Hz/);
    document.querySelector('.af-step[data-hz="1"]').click();
    await settle();

    const saved = invoked.filter((c) => c.cmd === "set_config_field").at(-1);
    assert.equal(saved.args.field, "autofire_hz");
    assert.equal(saved.args.value, "6");
  });

  /// A stepper beside "Off" is a control for nothing.
  test("there is no rate to set when it is off", async () => {
    Object.assign(DETAIL, { autofire: "off", autofire_hz: 5 });
    await detail.selectRom(7);
    await settle();
    assert.ok(document.querySelector(".autofire-row"), "the row should still be there");
    assert.equal(document.querySelector(".af-rate"), null, "a rate with nothing to apply to");
  });

  /// Below one it would divide by zero inside the emulator, and above thirty
  /// it is faster than the game can read.
  test("it does not step past its limits", async () => {
    Object.assign(DETAIL, { autofire: "a", autofire_hz: 1 });
    await detail.selectRom(7);
    await settle();
    invoked.length = 0;
    document.querySelector('.af-step[data-hz="-1"]').click();
    await settle();
    assert.equal(
      invoked.some((c) => c.cmd === "set_config_field"),
      false,
      "it saved a rate below one"
    );
    Object.assign(DETAIL, { autofire: null, autofire_hz: 5 });
  });
});
