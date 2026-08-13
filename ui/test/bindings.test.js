// The bindings table.
//
// Two stacked lists — every action once for the keyboard, then every action
// again for the controller — meant scrolling past twenty rows to compare a key
// with its button, and reading each action's name twice to do it. One row per
// action makes the comparison the layout. A regression to two lists has to fail
// here rather than be noticed by eye.

import { test, describe, before } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

let paneHtml, doc;

before(async () => {
  // settings-panes.js reaches state.js, which reads localStorage at import
  // time, so the page has to exist before the module does.
  const dom = new JSDOM(readFileSync(join(uiDir, "settings.html"), "utf8"), {
    url: "http://localhost/",
  });
  dom.window.__TAURI__ = {
    core: { invoke: () => Promise.resolve([]), convertFileSrc: (p) => p },
    event: { listen: () => Promise.resolve(() => {}) },
  };
  for (const k of ["window", "document", "navigator", "localStorage", "CSS"]) {
    Object.defineProperty(globalThis, k, { value: dom.window[k], configurable: true });
  }
  ({ paneHtml } = await import(join(uiDir, "js", "settings-panes.js")));
  doc = new JSDOM(`<body>${paneHtml("control")}</body>`).window.document;
});

describe("keyboard and controller side by side", () => {
  test("every action is one row carrying both bindings", () => {
    const rows = [...doc.querySelectorAll("tr[data-id]")];
    assert.ok(rows.length > 10, `expected a row per action, got ${rows.length}`);
    for (const row of rows) {
      assert.ok(row.querySelector(".key-cell .set-key"), `${row.dataset.id}: no key button`);
      assert.ok(row.querySelector(".pad-cell .set-pad"), `${row.dataset.id}: no pad button`);
    }
  });

  test("the columns say what they are", () => {
    const heads = [...doc.querySelectorAll("th")].map((h) => h.textContent.trim());
    assert.deepEqual(heads, ["Action", "Keyboard", "Controller"]);
  });

  /// Both buttons now live in one row, so anything scoped to the row rather
  /// than the cell would hand the keyboard handler the controller's button and
  /// silently rebind the wrong one.
  test("the two buttons in a row are distinguishable by cell", () => {
    const row = doc.querySelector("tr[data-id]");
    const key = row.querySelector(".key-cell .set-key");
    const pad = row.querySelector(".pad-cell .set-pad");
    assert.ok(key && pad);
    assert.notEqual(key, pad);
    assert.equal(row.querySelectorAll(".set-key").length, 1, "one key button per row");
    assert.equal(row.querySelectorAll(".set-pad").length, 1, "one pad button per row");
  });

  /// Each action appears once. The old layout listed all of them twice, which
  /// is the thing being fixed.
  test("no action is listed twice", () => {
    const ids = [...doc.querySelectorAll("tr[data-id]")].map((r) => r.dataset.id);
    assert.equal(new Set(ids).size, ids.length, `duplicated rows: ${ids}`);
  });
});

describe("the settings tabs", () => {
  /// The BIOS control sat at the bottom of General under six unrelated
  /// headings — RetroArch, saves, server, achievements, ScreenScraper — and was
  /// simply not found. The things that go and fetch something now have their
  /// own tab.
  test("fetching things has its own tab, and it holds the BIOS control", async () => {
    const { TABS, paneHtml } = await import(join(uiDir, "js", "settings-panes.js"));
    assert.ok(TABS.some((t) => t.id === "library"), "no library tab");

    const lib = new JSDOM(`<body>${paneHtml("library")}</body>`).window.document;
    assert.ok(lib.querySelector(".set-bios"), "BIOS control is not on the library tab");
    assert.ok(lib.querySelector(".set-bios-bar"), "and it needs a progress bar");
    assert.ok(lib.querySelector(".set-libsync"), "library sync belongs here too");
    assert.ok(lib.querySelector(".set-scrape"), "so does missing artwork");

    // And it is no longer buried in General.
    const gen = new JSDOM(`<body>${paneHtml("general")}</body>`).window.document;
    assert.equal(gen.querySelector(".set-bios"), null, "still duplicated in General");
  });
});
