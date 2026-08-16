// Search inside the page you are looking at.
//
// Not the header's search box, which spans the library and takes you to a
// different screen. This one narrows what is already in front of you and
// leaves you where you are. It used to be drawn into the collections list —
// which is why it existed only there, and why it sat in the middle of a page
// of cards looking like something that had fallen out of a dialog.

import { test, describe, before, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

let dom, pf, el, tabs;

before(async () => {
  dom = new JSDOM(
    `<style>${readFileSync(join(uiDir, "style.css"), "utf8")}</style>` +
      readFileSync(join(uiDir, "index.html"), "utf8"),
    { url: "http://localhost/", pretendToBeVisual: true }
  );
  global.window = dom.window;
  global.document = dom.window.document;
  global.HTMLElement = dom.window.HTMLElement;
  global.localStorage = dom.window.localStorage;
  global.CSS = dom.window.CSS;
  global.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
  global.requestAnimationFrame = (f) => f();
  Object.defineProperty(global, "navigator", { value: dom.window.navigator, configurable: true });
  dom.window.__TAURI__ = {
    core: { invoke: async () => [], convertFileSrc: (p) => p },
    event: { listen: async () => () => {}, emit: () => {} },
  };
  pf = await import("../js/pagefilter.js");
  tabs = await import("../js/tabs.js");
  ({ el } = await import("../js/state.js"));
});

beforeEach(() => {
  el.list.innerHTML = `
    <div class="rows">
      <div class="row prow" data-slug="arcade"><span class="nm">Arcade</span></div>
      <div class="row prow" data-slug="gb"><span class="nm">Game Boy</span></div>
      <div class="row prow" data-slug="gba"><span class="nm">Game Boy Advance</span></div>
    </div>`;
  el.consoles.hidden = true;
  pf.clearPageFilter();
});

const visible = () =>
  [...el.list.querySelectorAll(".row")]
    .filter((n) => !n.classList.contains("filtered-out"))
    .map((n) => n.textContent.trim());

describe("filtering the page", () => {
  test("it keeps what matches and hides the rest", () => {
    pf.applyPageFilter("game boy");
    assert.deepEqual(visible(), ["Game Boy", "Game Boy Advance"]);
  });

  test("case does not matter, and neither does stray space", () => {
    pf.applyPageFilter("  ARCADE ");
    assert.deepEqual(visible(), ["Arcade"]);
  });

  test("emptying it puts everything back", () => {
    pf.applyPageFilter("arcade");
    pf.clearPageFilter();
    assert.equal(visible().length, 3);
  });

  /// A redraw builds fresh nodes that have never seen the filter. Without
  /// this, changing the order or coming back to a tab quietly undoes the
  /// search still sitting in the box.
  test("a redrawn list is filtered again", () => {
    pf.applyPageFilter("advance");
    assert.deepEqual(visible(), ["Game Boy Advance"]);
    el.list.innerHTML = `
      <div class="rows">
        <div class="row prow"><span class="nm">Arcade</span></div>
        <div class="row prow"><span class="nm">Game Boy Advance</span></div>
      </div>`;
    assert.equal(visible().length, 2, "the redraw should start unfiltered");
    pf.refreshPageFilter();
    assert.deepEqual(visible(), ["Game Boy Advance"]);
  });

  /// Search is per screen. Text left in the box on the way to another tab
  /// means arriving at a list that is missing things for no visible reason.
  test("changing view clears it", async () => {
    const shell = await import("../js/shell.js");
    pf.applyPageFilter("arcade");
    shell.enter({ title: "History" });
    assert.equal(pf.pageFilterText(), "");
    assert.equal(el.pageFilter.value, "");
  });

  test("it says what this page holds", () => {
    pf.setPageFilterLabel("27 collections");
    assert.match(el.pageFilter.placeholder, /27 collections/);
  });
});

describe("where the box sits", () => {
  /// The tab row runs the full width of the window and the preview is a column
  /// under its right-hand end, so left alone the box sits over the top of that
  /// column — text on one side of the seam, artwork on the other.
  test("it is in the tab row, before the two end buttons", () => {
    tabs.installTabs();
    const bar = dom.window.document.getElementById("page-filter");
    assert.equal(bar.parentElement?.id, "tabbar", "the box is not in the tab row");
    assert.ok(
      bar.nextElementSibling?.classList.contains("tabbar-end"),
      "Take offline and Hide info should be after it, at the very end"
    );
  });

  /// jsdom has no layout, so the widths are stubbed: what is under test is the
  /// arithmetic — the preview's width, less the two small buttons that stay at
  /// the very end.
  test("it stops short of the preview column", () => {
    tabs.installTabs();
    const bar = dom.window.document.getElementById("page-filter");
    const end = dom.window.document.querySelector(".tabbar-end");
    el.detail.hidden = false;
    el.detail.getBoundingClientRect = () => ({ width: 420 });
    end.getBoundingClientRect = () => ({ width: 120 });

    assert.equal(pf.layoutPageFilter(), 300, "the box is not held clear of the preview");
    assert.equal(bar.style.marginRight, "300px");

    // With the preview closed there is nothing to stay clear of.
    el.detail.hidden = true;
    assert.equal(pf.layoutPageFilter(), 0);
    assert.equal(bar.style.marginRight, "0px");
  });

  /// Never negative: a preview narrower than the buttons beside it would
  /// otherwise pull the box off the right-hand edge.
  test("a preview narrower than the buttons does not push it off the edge", () => {
    tabs.installTabs();
    const end = dom.window.document.querySelector(".tabbar-end");
    el.detail.hidden = false;
    el.detail.getBoundingClientRect = () => ({ width: 60 });
    end.getBoundingClientRect = () => ({ width: 120 });
    assert.equal(pf.layoutPageFilter(), 0);
  });
});
