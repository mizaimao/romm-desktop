// The order of the left column.
//
// The server hands consoles and collections back by size, so the column opened
// on "Arcade Fighting, 322 games" and buried "Best of nes" thirty rows down,
// with nothing on screen to say why or to change it.

import { test, describe, before, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

let dom, order;

before(async () => {
  dom = new JSDOM(`<style>${readFileSync(join(uiDir, "style.css"), "utf8")}</style><body></body>`, {
    url: "http://localhost/",
  });
  global.window = dom.window;
  global.document = dom.window.document;
  global.localStorage = dom.window.localStorage;
  global.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
  // state.js reads these at import time, and this module reaches it through
  // the shared helpers.
  dom.window.__TAURI__ = {
    core: { invoke: async () => [], convertFileSrc: (p) => p },
    event: { listen: async () => () => {}, emit: () => {} },
  };
  order = await import(join(uiDir, "js", "picker-order.js"));
});

beforeEach(() => dom.window.localStorage.clear());

const consoles = [
  { name: "Arcade", rom_count: 322, playable: false },
  { name: "Nintendo 64", rom_count: 12, playable: true },
  { name: "Game Boy", rom_count: 90, playable: true },
];

describe("the column is ordered by something someone chose", () => {
  /// By name, not by size. The size order is the server's, and it is the reason
  /// the same three arcade lists sat at the top of every screen.
  test("name is the order until told otherwise", () => {
    assert.equal(order.pickerOrder("platforms").id, "name");
    assert.deepEqual(
      order.sortPicker("platforms", consoles).map((p) => p.name),
      ["Arcade", "Game Boy", "Nintendo 64"]
    );
  });

  test("the chosen order is remembered", () => {
    order.setPickerOrder("platforms", "count");
    assert.equal(order.pickerOrder("platforms").id, "count");
    assert.deepEqual(
      order.sortPicker("platforms", consoles).map((p) => p.rom_count),
      [322, 90, 12]
    );
    // Unlike the game sort, which is deliberately forgotten: the order of the
    // column is the shape of the app rather than a question about one console.
    assert.equal(dom.window.localStorage.getItem("romm.order.platforms"), "count");
  });

  test("consoles you can actually play can come first", () => {
    order.setPickerOrder("platforms", "playable");
    const names = order.sortPicker("platforms", consoles).map((p) => p.name);
    assert.equal(names.at(-1), "Arcade", "the console with no emulator is not last");
  });

  /// A starred collection is one you said you wanted at hand, so it stays at
  /// the top whatever else is chosen.
  test("favourites stay on top of any order", () => {
    const cols = [
      { name: "Zebra", rom_count: 1, is_favorite: true },
      { name: "Alpha", rom_count: 500 },
    ];
    for (const id of ["name", "count", "here"]) {
      order.setPickerOrder("collections", id);
      assert.equal(
        order.sortPicker("collections", cols)[0].name,
        "Zebra",
        `the favourite is not first under ${id}`
      );
    }
  });

  test("sorting does not disturb what it was given", () => {
    const original = [...consoles];
    order.setPickerOrder("platforms", "count");
    order.sortPicker("platforms", consoles);
    assert.deepEqual(consoles, original, "the caller's array was reordered under it");
  });
});

describe("the bar above the column", () => {
  /// The filter box was the sticky element itself — an input with a
  /// see-through background — so collection names scrolled visibly through the
  /// middle of it.
  test("the filter sits inside an opaque strip, not on its own", () => {
    const doc = dom.window.document;
    doc.body.className = "columns";
    doc.body.innerHTML = `<nav id="consoles">${order.pickerBar({
      kind: "collections",
      filter: "Filter 27 collections…",
    })}</nav>`;
    const bar = doc.querySelector("#consoles .pickbar");
    assert.ok(bar, "there is no strip");
    assert.ok(bar.querySelector("#cfilter"), "the filter box is not in it");
    const style = dom.window.getComputedStyle(bar);
    assert.equal(style.position, "sticky", "the strip does not stay put");
    // jsdom does not resolve var(), so the colour is checked in the source.
    const css = readFileSync(join(uiDir, "style.css"), "utf8");
    const at = css.indexOf("\nbody.columns #consoles .pickbar {");
    const block = css.slice(at, css.indexOf("}", at));
    assert.match(block, /background:\s*var\(--bg\)/, "the strip is see-through");
  });

  test("the order button says which order it is in", () => {
    order.setPickerOrder("platforms", "count");
    const doc = dom.window.document;
    doc.body.innerHTML = order.pickerBar({ kind: "platforms" });
    assert.match(doc.querySelector(".pick-sort").textContent, /Most games/);
  });

  test("choosing an order redraws the list", () => {
    const doc = dom.window.document;
    doc.body.innerHTML = order.pickerBar({ kind: "platforms" });
    let redrawn = 0;
    order.wirePickerBar(doc.body, "platforms", () => redrawn++);
    doc.querySelector(".pick-sort").dispatchEvent(
      new dom.window.MouseEvent("click", { bubbles: true })
    );
    const item = [...doc.querySelectorAll(".ctx-menu button")].find((b) =>
      b.textContent.includes("Fewest games")
    );
    assert.ok(item, "the menu does not offer the orders");
    item.click();
    assert.equal(order.pickerOrder("platforms").id, "fewest");
    assert.equal(redrawn, 1, "the list was not drawn again");
  });
});
