// The seam a different layout is built through.
//
// The claim this file has to keep honest is that the views no longer know what
// the window looks like. That was not true before: every screen set the chrome
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
    event: { listen: async () => () => {} },
  };
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
  test("asking for chrome the layout does not have is not an error", () => {
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
