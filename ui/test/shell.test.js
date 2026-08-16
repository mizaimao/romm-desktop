// The seam a different layout is built through.
//
// The claim this file has to keep honest is that the views no longer know what
// the window looks like. That was not true before: every screen set the top bar
// itself — hide Back, show Grid, show the zoom slider unless we are in list
// mode — six or seven imperative lines repeated in each of six functions, each
// copy stating in code that this is a single-pane app with a back button.
// Forty-three of those lines, and every one of them would have had to change
// for a three-column layout.
//
// So the test builds a three-column skeleton — consoles down the left, games
// in the middle, preview on the right, no Back button at all because nothing
// is ever replaced — points the shell at it, and drives the ordinary view code
// against it. Nothing in ui/js knows this file exists.

import { test, describe, before, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

let dom, shell, el;

/// A layout this app does not have. Deliberately missing things the current
/// one takes for granted: there is no #back, because a column that is always
/// on screen is never navigated away from.
const THREE_COLUMNS = `
  <main class="cols">
    <nav id="col-consoles"></nav>
    <section id="col-games"></section>
    <aside id="col-preview"></aside>
  </main>`;

before(async () => {
  dom = new JSDOM(readFileSync(join(uiDir, "index.html"), "utf8"), {
    url: "http://localhost/",
    pretendToBeVisual: true,
  });
  global.window = dom.window;
  global.document = dom.window.document;
  global.localStorage = dom.window.localStorage;
  dom.window.__TAURI__ = {
    core: {
      // Selecting a row draws the info pane, which joins several of these
      // arrays — a thinner stub throws inside the pane, after the test has
      // finished, as an unhandled rejection rather than a failure.
      invoke: async (cmd, args) =>
        cmd === "platforms"
          ? [
              { slug: "arcade", name: "Arcade", rom_count: 9 },
              { slug: "gb", name: "Game Boy", rom_count: 4 },
            ]
          : cmd === "rom_detail"
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
    event: { listen: async () => () => {} },
  };
  // jsdom implements no layout, so it has no scrollIntoView — and the list
  // calls it to keep the cursor visible. A no-op is the honest stand-in: there
  // is nothing to scroll into view in a document with no viewport.
  dom.window.Element.prototype.scrollIntoView = function () {};

  shell = await import("../js/shell.js");
  ({ el } = await import("../js/state.js"));
});

beforeEach(() => {
  for (const role of ["primary", "aside", "nav"]) shell.setRegion(role, null);
  document.querySelector(".cols")?.remove();
});

describe("what a view asks for", () => {
  /// A view lists what it needs; everything else is hidden. Before, each view
  /// had to remember to turn off what the previous one turned on, and getting
  /// that wrong left a button on screen that acted on a game that was no
  /// longer there.
  test("anything not asked for is hidden", () => {
    for (const node of [el.back, el.layoutBtn, el.sidebarBtn, el.grabBtn, el.sortBtn]) {
      node.hidden = false;
    }
    shell.enter({ title: "Platforms", layout: true, grab: true });

    assert.equal(el.layoutBtn.hidden, false, "asked for and missing");
    assert.equal(el.grabBtn.hidden, false, "asked for and missing");
    assert.equal(el.back.hidden, true, "not asked for and still there");
    assert.equal(el.sidebarBtn.hidden, true);
    assert.equal(el.sortBtn.hidden, true);
    assert.equal(el.title.textContent, "Platforms");
  });

  /// The one conditional every view repeated and each got to decide for
  /// itself: the slider sizes covers, so it means nothing in a list.
  test("the zoom slider follows the covers, not the view", () => {
    shell.enter({ zoom: "grid", gridLayout: true });
    assert.equal(el.zoomWrap.hidden, false);
    shell.enter({ zoom: "grid", gridLayout: false });
    assert.equal(el.zoomWrap.hidden, true, "a zoom slider over a list of names");
    // A view that wants it regardless — the collection cards are cards too.
    shell.enter({ zoom: true, gridLayout: false });
    assert.equal(el.zoomWrap.hidden, false);
  });
});

describe("a layout this app does not have", () => {
  const threeColumns = () => {
    document.body.insertAdjacentHTML("beforeend", THREE_COLUMNS);
    shell.setRegion("nav", document.getElementById("col-consoles"));
    shell.setRegion("primary", document.getElementById("col-games"));
    shell.setRegion("games", document.getElementById("col-games"));
    shell.setRegion("aside", document.getElementById("col-preview"));
  };

  /// The point of the whole exercise: content goes where the shell says, and
  /// the view that produced it never learns where that was.
  test("views draw into whichever columns the shell has been given", () => {
    threeColumns();
    shell.paint("primary", "<div class='game'>Metal Slug</div>");
    shell.paint("nav", "<div class='console'>Arcade</div>");
    shell.paint("aside", "<div class='shot'>art</div>");

    assert.match(document.getElementById("col-games").innerHTML, /Metal Slug/);
    assert.match(document.getElementById("col-consoles").innerHTML, /Arcade/);
    assert.match(document.getElementById("col-preview").innerHTML, /art/);
    // And not into the one-column skeleton's list, which is still in the page.
    assert.equal(el.list.innerHTML, "");
  });

  /// A three-column layout has no Back button, because a column that is always
  /// on screen is never navigated away from. Every view still asks for one.
  test("asking for a button the layout does not have is not an error", () => {
    threeColumns();
    // The handles, not the nodes. `el` is built from getElementById once at
    // import, so a skeleton without these ids holds null — removing the
    // element from the page leaves the handle pointing at a detached node,
    // which still answers `.hidden` and proves nothing.
    const kept = { back: el.back, zoomWrap: el.zoomWrap, sortBtn: el.sortBtn };
    el.back = null;
    el.zoomWrap = null;
    el.sortBtn = null;
    try {
      assert.doesNotThrow(() =>
        shell.enter({ title: "Arcade", back: true, layout: true, zoom: "grid", gridLayout: true })
      );
      assert.doesNotThrow(() => shell.showZoom(true));
    } finally {
      Object.assign(el, kept);
    }
  });

  /// A region the layout does not offer is a region that is not shown, not a
  /// crash: a two-column arrangement with no preview should drop the preview,
  /// not take the window down with it.
  test("painting a region that does not exist is a no-op", () => {
    threeColumns();
    shell.setRegion("aside", null);
    // With no override it falls back to this skeleton's #detail; remove that
    // too, so the role genuinely resolves to nothing.
    const detail = el.detail;
    detail.remove();
    assert.doesNotThrow(() => shell.paint("aside", "<p>preview</p>"));
    document.body.appendChild(detail);
  });

  /// The end of the argument: a real view, unmodified, drawing into the middle
  /// column of a layout it has never heard of. If this needs the view to change
  /// then the seam is in the wrong place.
  test("an actual view renders into the middle column", async () => {
    threeColumns();
    const { renderRows } = await import("../js/library.js");
    const { state } = await import("../js/state.js");
    state.view = "roms";
    state.platform = "arcade";
    state.layout = "list";

    renderRows(
      [
        {
          id: 1,
          name: "Metal Slug",
          fs_name: "mslug.zip",
          platform: "arcade",
          size_bytes: 1,
          downloaded: true,
          favourite: false,
        },
      ],
      false
    );

    assert.match(
      document.getElementById("col-games").innerHTML,
      /Metal Slug/,
      "the game list did not reach the middle column"
    );
    assert.equal(el.list.innerHTML, "", "it went to the one-column skeleton instead");
  });

  test("a role with no mapping anywhere resolves to nothing", () => {
    assert.equal(shell.region("sidebar-that-does-not-exist"), null);
    assert.equal(shell.paint("sidebar-that-does-not-exist", "<p>x</p>"), null);
  });
});

describe("three columns, in the real page", () => {
  /// Not a made-up skeleton this time: the app's own index.html, switched to
  /// the other arrangement. The one element three columns adds is the left
  /// one; the middle and right are the same #list and #detail the single pane
  /// uses, which is why this is a handful of rules rather than a second UI.
  let library, state;

  before(async () => {
    library = await import("../js/library.js");
    ({ state } = await import("../js/state.js"));
  });

  beforeEach(() => {
    // The suite above points the roles at its own made-up columns, and those
    // overrides outlive it — this one is about the real page.
    for (const role of ["primary", "aside", "nav", "games", "picker"]) {
      shell.setRegion(role, null);
    }
    shell.setMode("single");
    el.consoles.innerHTML = "";
    el.list.innerHTML = "";
    state.platform = null;
  });

  test("the consoles get their own column, and the games keep the middle", () => {
    shell.setMode("columns");
    assert.equal(shell.region("picker"), el.consoles, "consoles did not move left");
    assert.equal(shell.region("games"), el.list, "the games left the middle");
    assert.equal(el.consoles.hidden, false, "the left column is still hidden");
    assert.ok(document.body.classList.contains("columns"));
  });

  test("in one pane the consoles and the games share the same element", () => {
    shell.setMode("single");
    assert.equal(shell.region("picker"), el.list);
    assert.equal(shell.region("games"), el.list);
    assert.equal(el.consoles.hidden, true, "the left column is showing in one pane");
  });

  /// The point of the arrangement: opening a console fills the middle and
  /// leaves the console list where it is. In one pane the same call replaces
  /// the screen.
  test("opening a console does not take the console list away", () => {
    shell.setMode("columns");
    el.consoles.innerHTML = `<div class="rows"><div class="prow" data-slug="arcade"></div></div>`;

    library.renderRows(
      [
        {
          id: 1,
          name: "Metal Slug",
          fs_name: "mslug.zip",
          platform: "arcade",
          size_bytes: 1,
          downloaded: true,
          favourite: false,
        },
      ],
      false
    );

    assert.match(el.list.innerHTML, /Metal Slug/, "the games did not reach the middle");
    assert.ok(
      el.consoles.querySelector('[data-slug="arcade"]'),
      "the console list was wiped by opening a console"
    );
  });

  /// Nothing is ever replaced, so there is nothing to go back to and the
  /// preview is a column rather than something to slide over the list.
  test("Back and the preview toggle are not offered", () => {
    shell.setMode("columns");
    shell.enter({ title: "Arcade", back: true, sidebar: true, layout: true });
    assert.equal(el.back.hidden, true, "a Back button with nowhere to go");
    assert.equal(el.sidebarBtn.hidden, true, "a toggle for a column");
    assert.equal(el.layoutBtn.hidden, false, "grid/list still applies to the games");

    // And both come back in one pane.
    shell.setMode("single");
    shell.enter({ title: "Arcade", back: true, sidebar: true, layout: true });
    assert.equal(el.back.hidden, false);
    assert.equal(el.sidebarBtn.hidden, false);
  });

  test("the choice is remembered", () => {
    shell.chooseMode("columns");
    assert.equal(shell.storedMode(), "columns");
    shell.chooseMode("single");
    assert.equal(shell.storedMode(), "single");
    // Anything unrecognised is the one being worked on, which is the default
    // while three columns is being built.
    shell.chooseMode("nonsense");
    assert.equal(shell.shellMode(), "columns");
    shell.chooseMode("single");
  });
});

describe("each tab fills the left column with its own list", () => {
  /// The left column is not "the consoles" — it is whatever this tab is a list
  /// of. Library gives consoles, My collections gives collections, Browse
  /// gives the groups. It was drawing consoles in every tab because the role
  /// was named after the first thing that used it.
  let collections, history, state;

  before(async () => {
    collections = await import("../js/collections.js");
    history = await import("../js/history.js");
    ({ state } = await import("../js/state.js"));
  });

  beforeEach(() => {
    for (const role of ["primary", "aside", "nav", "games", "picker"]) {
      shell.setRegion(role, null);
    }
    shell.setMode("columns");
    el.consoles.innerHTML = "";
    el.list.innerHTML = "";
  });

  test("My collections puts the collections there, not the consoles", async () => {
    await collections.showCollectionsIn("user", "My collections");
    assert.equal(
      shell.region("picker"),
      el.consoles,
      "the picker is not the left column in three columns"
    );
    assert.match(el.consoles.innerHTML, /Filter/, "the collections did not reach the column");
  });

  test("Browse puts the groups there", async () => {
    await collections.showCollectionGroups({ exclude: ["user"] });
    // The stub answers with nothing, which is still an answer drawn into the
    // column rather than into the middle.
    assert.ok(el.consoles.innerHTML.length > 0, "the groups did not reach the column");
  });

  /// History is a page rather than a list with a detail beside it, so it owns
  /// no left column — and leaving the last tab's list there would be a list
  /// that acts on a screen that is gone.
  test("History clears the column and draws into the middle", async () => {
    el.consoles.innerHTML = "<div class='prow'>Arcade</div>";
    await history.showHistory();
    assert.equal(el.consoles.innerHTML, "", "the previous tab's list is still there");
    assert.match(el.list.innerHTML, /Nothing recorded yet|hist/, "History did not draw");
  });

  /// It set no view name at all, so the section machinery parked History under
  /// whatever was showing before — usually the console grid — and restored
  /// that instead every time the tab was opened. The tab simply never showed.
  test("History says which view it is", async () => {
    state.view = "platforms";
    await history.showHistory();
    assert.equal(state.view, "history");
  });
});

describe("three columns opens with all three filled", () => {
  /// A window that opens with two thirds of itself empty is asking to be told
  /// what it already knows: which console you were last in, or failing that
  /// the first one.
  let library, state, asked;

  before(async () => {
    library = await import("../js/library.js");
    ({ state } = await import("../js/state.js"));
  });

  beforeEach(() => {
    for (const role of ["primary", "aside", "nav", "games", "picker"]) {
      shell.setRegion(role, null);
    }
    shell.setMode("columns");
    el.consoles.innerHTML = "";
    el.list.innerHTML = "";
    state.platform = null;
    state.lastPlatform = null;
  });

  test("a console is opened without being clicked", async () => {
    await library.showPlatforms();
    assert.ok(state.platform, "no console was opened, so the middle is empty");
    assert.ok(el.consoles.innerHTML.length > 0, "the left column is empty");
  });

  /// The one you were last in, not merely the first alphabetically — coming
  /// back to where you were is the point of remembering it at all.
  test("it picks up where you left off", async () => {
    state.lastPlatform = "gb";
    await library.showPlatforms();
    assert.equal(state.platform, "gb");
  });

  /// The column stays on screen while the middle changes, so without a mark
  /// there is nothing to say which of thirty-five consoles you are looking at.
  test("the open console is marked in the column", async () => {
    await library.showPlatforms();
    const lit = el.consoles.querySelectorAll(".open");
    assert.equal(lit.length, 1, `${lit.length} consoles look open`);
    assert.equal(lit[0].dataset.slug, state.platform);
  });
});
