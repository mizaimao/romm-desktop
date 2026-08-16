// Narrowing a list, and picking out of it at random.
//
// The arcade console holds 2,506 games. Sorting them is not finding one: "by
// rating" still leaves 2,506 rows with the eleven you have played somewhere
// inside. Both of these are about the same problem and both are things every
// other frontend has.

import { test, describe, before, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

let dom, filter, library, state, el;

const ROWS = [
  { id: 1, name: "Alpha", downloaded: true, favourite: false, last_played: null, rating: 9 },
  { id: 2, name: "Beta", downloaded: false, favourite: true, last_played: "2026-01-01", rating: 4 },
  { id: 3, name: "Gamma", downloaded: true, favourite: true, last_played: "2026-02-02", rating: 8 },
  { id: 4, name: "Delta", downloaded: false, favourite: false, last_played: null, rating: null },
];

before(async () => {
  dom = new JSDOM(readFileSync(join(uiDir, "index.html"), "utf8"), {
    url: "http://localhost/",
    pretendToBeVisual: true,
  });
  global.window = dom.window;
  global.document = dom.window.document;
  global.HTMLElement = dom.window.HTMLElement;
  global.localStorage = dom.window.localStorage;
  global.CSS = dom.window.CSS;
  global.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
  global.requestAnimationFrame = (f) => f();
  Object.defineProperty(global, "navigator", { value: dom.window.navigator, configurable: true });
  dom.window.Element.prototype.scrollIntoView = function () {};
  class FakeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  dom.window.IntersectionObserver = FakeObserver;
  global.IntersectionObserver = FakeObserver;
  dom.window.__TAURI__ = {
    core: {
      // Selecting a row draws the preview, which joins several of these
      // arrays. A thinner stub throws after the test has finished, as an
      // unhandled rejection rather than a failure.
      invoke: async (cmd, args) =>
        cmd === "rom_detail"
          ? {
              id: args.id,
              name: `Game ${args.id}`,
              fs_name: "g.zip",
              platform: "arcade",
              platform_slug: "arcade",
              size_bytes: 1,
              downloaded: true,
              screenshots: [],
              genres: [],
              companies: [],
              franchises: [],
              game_modes: [],
              regions: [],
              alt_names: [],
              art: {},
            }
          : [],
      convertFileSrc: (p) => p,
    },
    event: { listen: async () => () => {}, emit: () => {} },
  };
  filter = await import("../js/filter.js");
  library = await import("../js/library.js");
  ({ state, el } = await import("../js/state.js"));
});

beforeEach(() => {
  state.view = "roms";
  state.platform = "arcade";
  state.collection = null;
  state.rows = ROWS;
  state.layout = "list";
  filter.clearFilters();
  document.querySelector(".ctx-menu")?.remove();
});

const names = (rows) => filter.filtered(rows).map((r) => r.name);

describe("filters", () => {
  test("nothing on leaves the list alone", () => {
    assert.equal(filter.filtered(ROWS), ROWS, "an untouched list was copied anyway");
  });

  test("each one keeps what it says", () => {
    filter.toggleFilter("local");
    assert.deepEqual(names(ROWS), ["Alpha", "Gamma"]);
    filter.clearFilters();
    filter.toggleFilter("fav");
    assert.deepEqual(names(ROWS), ["Beta", "Gamma"]);
    filter.clearFilters();
    filter.toggleFilter("unplayed");
    assert.deepEqual(names(ROWS), ["Alpha", "Delta"]);
    filter.clearFilters();
    filter.toggleFilter("great");
    assert.deepEqual(names(ROWS), ["Alpha", "Gamma"], "an unrated game counted as good");
  });

  /// The reason several can be on at once: "downloaded and never played" is
  /// the list of things taking up disk you have not touched, and there is no
  /// other way to ask it.
  test("two of them both have to pass", () => {
    filter.toggleFilter("local");
    filter.toggleFilter("unplayed");
    assert.deepEqual(names(ROWS), ["Alpha"]);
  });

  /// Choosing one clears its opposite rather than leaving a list that can
  /// never match anything and no clue why.
  test("opposites cancel rather than emptying the list", () => {
    filter.toggleFilter("local");
    filter.toggleFilter("missing");
    assert.deepEqual(filter.activeFilters(), ["missing"]);
    assert.deepEqual(names(ROWS), ["Beta", "Delta"]);
  });

  /// The same reasoning as the game sort: "what have I not played on this
  /// console" is a question about this console, asked now. A library still
  /// filtered a week later, for a forgotten reason, looks like one that has
  /// lost half its games.
  test("they belong to the list they were set on", () => {
    filter.toggleFilter("fav");
    state.platform = "gb";
    assert.deepEqual(filter.activeFilters(), [], "the next console inherited them");
    state.platform = "arcade";
    assert.deepEqual(filter.activeFilters(), ["fav"], "and lost them coming back");
  });

  test("the console grid has nothing to filter", () => {
    state.view = "platforms";
    assert.equal(filter.filterable(), false);
    state.view = "roms";
    assert.equal(filter.filterable(), true);
  });
});

describe("the filter menu and button", () => {
  /// A filtered list looks exactly like a short one, and the filter itself is
  /// off screen in a menu — so the button has to say it is doing something.
  test("the button counts what is on", () => {
    filter.refreshFilterButton();
    assert.match(el.filterBtn.textContent, /Filter/);
    assert.equal(el.filterBtn.classList.contains("on"), false);

    filter.toggleFilter("fav");
    filter.refreshFilterButton();
    assert.match(el.filterBtn.textContent, /Filters · 1/);
    assert.equal(el.filterBtn.classList.contains("on"), true, "a filtered list looks unfiltered");
  });

  /// A filter is built out of two or three choices. A menu that shuts on each
  /// one turns that into four trips to the same button.
  test("choosing one leaves the menu open", () => {
    filter.openFilterMenu();
    const item = [...document.querySelectorAll(".ctx-menu button")].find((b) =>
      b.textContent.includes("Starred")
    );
    assert.ok(item, "the menu does not offer the filters");
    item.click();
    assert.deepEqual(filter.activeFilters(), ["fav"]);
    assert.ok(document.querySelector(".ctx-menu"), "the menu shut after one choice");
  });

  /// An empty result is indistinguishable from an empty console, and the
  /// reason for it is hidden in a menu.
  test("an empty result says why, and offers the way out", () => {
    filter.toggleFilter("local");
    filter.toggleFilter("great");
    library.renderRows([{ ...ROWS[3] }], false);
    const empty = document.getElementById("list").textContent;
    assert.match(empty, /matches the filters/, `unexplained empty list: ${empty}`);

    document.querySelector(".clear-filters").click();
    assert.deepEqual(filter.activeFilters(), [], "the way out did not clear them");
  });
});

describe("surprise me", () => {
  /// Nobody knows 2,506 arcade games, and scrolling until something looks
  /// familiar always lands in the same three letters of the alphabet.
  test("it picks something and puts the cursor on it", () => {
    library.renderRows(ROWS, false);
    const pick = library.randomGame();
    assert.ok(pick, "nothing was picked");
    assert.equal(state.selected, pick.id, "the cursor is somewhere else");
  });

  /// From what is shown, not from everything — so "surprise me out of the ones
  /// I have not played" is a question you can actually ask.
  test("it picks from what the filters left", () => {
    filter.toggleFilter("unplayed");
    library.renderRows(ROWS, false);
    for (let i = 0; i < 20; i++) {
      const pick = library.randomGame();
      assert.ok(
        ["Alpha", "Delta"].includes(pick.name),
        `${pick.name} is filtered out but was offered`
      );
    }
  });

  test("an empty list is not an error", () => {
    state.rows = [];
    assert.equal(library.randomGame(), null);
  });
});
