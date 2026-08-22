// Covers are fetched near the viewport and let go well away from it.
//
// The letting go is the whole of the memory problem: a cover is a few tens of
// kilobytes as a PNG and about 786 KB once decoded, and the version this
// replaces unobserved a card the moment its cover arrived — so every image the
// list had ever drawn stayed decoded for as long as the list was on screen.
// Measured 2026-08-20 at 578 MB of a ~671 MB total.
//
// Invisible from the outside: a list that holds every cover looks exactly like
// one that does not. It only shows up in `footprint`, which no test can read.
// So the observers are driven by hand here.

import { test, describe, before, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";
import { fakeBackend } from "./backend.js";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

let dom, lib, state, observers;

/// A stand-in that hands back the entries it was given, so a test can say
/// "these cards just came into view" or "these just left".
class Watcher {
  constructor(fn, opts) {
    this.fn = fn;
    this.margin = opts?.rootMargin ?? "0px";
    this.watched = new Set();
    observers.push(this);
  }
  observe(node) { this.watched.add(node); }
  unobserve(node) { this.watched.delete(node); }
  disconnect() { this.watched.clear(); }
  /// Report every card it is watching as entering, or leaving, the margin.
  fire(isIntersecting) {
    this.fn([...this.watched].map((target) => ({ target, isIntersecting })));
  }
}

/// The two observers, told apart by the distance each one works at.
///
/// By which is nearer rather than by a fixed number: the margins are a
/// fraction of the list's own height now, so the pixel values move with the
/// window and a test that names one is a test that breaks on a resize.
const px = (o) => Number(String(o.margin).replace("px", ""));
const both = () => {
  const last = observers.slice(-2);
  return last.sort((a, b) => px(a) - px(b));
};
const near = () => both()[0];
const far = () => both()[1];

const ROWS = Array.from({ length: 6 }, (_, i) => ({
  id: i + 1,
  name: `Game ${i} & Sons`,
  fs_name: `g${i}.zip`,
  platform: "arcade",
  size_bytes: 1,
  downloaded: true,
  favourite: i === 0,
  rating: null, year: null, last_played: null, players: null,
}));

before(async () => {
  dom = new JSDOM(readFileSync(join(uiDir, "index.html"), "utf8"), {
    url: "http://localhost/", pretendToBeVisual: true,
  });
  for (const k of ["window", "document", "localStorage", "CSS"])
    Object.defineProperty(globalThis, k, { value: dom.window[k], configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: dom.window.navigator, configurable: true });
  globalThis.requestAnimationFrame = (f) => dom.window.setTimeout(f, 0);
  dom.window.Element.prototype.scrollIntoView = function () {};

  observers = [];
  dom.window.IntersectionObserver = Watcher;
  globalThis.IntersectionObserver = Watcher;

  const backend = fakeBackend((cmd, args) => {
    if (cmd === "rom_covers") return args.ids.map((id) => ({ id, cover: `/art/${id}.png` }));
    if (cmd === "rom_detail")
      return { id: 1, name: "x", fs_name: "g.zip", platform: "arcade", platform_slug: "arcade",
               size_bytes: 1, downloaded: true, screenshots: [], genres: [], companies: [],
               franchises: [], game_modes: [], regions: [], alt_names: [], art: {} };
    return [];
  });
  dom.window.__TAURI__ = {
    core: { invoke: backend, convertFileSrc: (p) => p },
    event: { listen: async () => () => {}, emit: () => {} },
  };
  backend.rows(ROWS);

  lib = await import("../js/library.js");
  ({ state } = await import("../js/state.js"));
  await (await import("../js/arrange.js")).arrangeCurrentList();
});

beforeEach(() => {
  observers.length = 0;
  state.view = "roms";
  state.platform = "arcade";
  state.layout = "grid";
  state.rows = ROWS;
  lib.renderRows(ROWS, false);
});

/// A turn of the loop: covers are fetched in a batch, 80ms behind the scroll.
const settle = () => new Promise((r) => dom.window.setTimeout(r, 120));

const art = (id) => document.querySelector(`.gcard[data-id="${id}"] .art`);
const hasImage = (id) => !!art(id)?.querySelector("img");

describe("covers near the viewport", () => {
  test("a card that comes into view gets its cover", async () => {
    assert.equal(hasImage(1), false, "drawn with an image before anything scrolled");
    near().fire(true);
    await settle();
    assert.equal(hasImage(1), true, "no cover arrived");
    assert.equal(art(1).querySelector("img").getAttribute("src"), "/art/1.png");
  });

  /// Replacing the whole of `.art` with the image took the star with it, so a
  /// starred game lost its star the moment its cover arrived.
  test("a starred game keeps its star once the cover lands", async () => {
    near().fire(true);
    await settle();
    assert.ok(art(1).querySelector(".star"), "the star went with the placeholder");
    assert.equal(art(2).querySelector(".star"), null, "an unstarred game grew one");
  });
});

describe("covers well away from it", () => {
  /// The fix for 578 MB. Nothing else in the app releases an image.
  test("a card that leaves the far margin gives its cover back", async () => {
    near().fire(true);
    await settle();
    assert.equal(hasImage(1), true);

    far().fire(false);
    assert.equal(hasImage(1), false, "the decoded image is still being held");
    assert.ok(art(1).querySelector(".ph"), "nothing was put back in its place");
  });

  test("what goes back is the placeholder it was drawn with", async () => {
    near().fire(true);
    await settle();
    far().fire(false);
    // Two letters, and the ampersand escaped exactly once.
    assert.equal(art(1).querySelector(".ph").textContent, "Ga");
    assert.ok(art(1).querySelector(".star"), "a starred game lost its star on the way back");
  });

  /// A card whose cover was released has to be able to ask for it again — the
  /// version this replaces unobserved on load, so it never could.
  test("scrolling back fetches it again", async () => {
    near().fire(true);
    await settle();
    far().fire(false);
    assert.equal(hasImage(1), false);

    near().fire(true);
    await settle();
    assert.equal(hasImage(1), true, "the cover never came back");
  });

  /// The two margins are different distances on purpose: a card one flick of
  /// the wheel off the top of the screen is about to be looked at again, and
  /// dropping its cover there would mean decoding it twice for nothing.
  test("the release margin is further out than the load margin", () => {
    assert.ok(px(far()) > px(near()) * 2, `${far().margin} is not far enough past ${near().margin}`);
  });

  /// The bug this replaced: a flat 1,600px release margin was two screens on
  /// a desktop window and eight on a 720-tall handheld — so the machine with
  /// the least memory held the most pictures. A decoded cover is four bytes a
  /// pixel whatever size it is drawn at, so the count of cards inside this
  /// margin *is* the memory. See `docs/memory.md`.
  test("the margins are a fraction of the screen, not a fixed distance", () => {
    const list = dom.window.document.getElementById("list");
    const height = (n) =>
      Object.defineProperty(list, "clientHeight", { value: n, configurable: true });
    // A desktop window, then a handheld's screen.
    height(900);
    lib.observeCovers();
    const tall = px(far());
    height(300);
    lib.observeCovers();
    assert.ok(
      px(far()) < tall,
      `a shorter list kept the same ${far().margin} margin, so it holds the same pile of pictures`
    );
  });

  /// This runs for every card leaving the margin, on a list of 2,506.
  test("a card with nothing to release is left alone", async () => {
    const before = art(3).innerHTML;
    far().fire(false);
    assert.equal(art(3).innerHTML, before, "the placeholder was rewritten over itself");
  });
});
