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

  /// "Systems" was a button in the top bar between Grid and Take offline —
  /// things you do to what is on screen — so it read as another view of the
  /// library rather than as configuration, and gave no clue what pressing it
  /// would do.
  test("per-system emulator and shader settings are a settings tab", async () => {
    const { TABS, paneHtml } = await import(join(uiDir, "js", "settings-panes.js"));
    assert.ok(TABS.some((t) => t.id === "systems"), "no systems tab");
    const pane = new JSDOM(`<body>${paneHtml("systems")}</body>`).window.document;
    assert.ok(pane.querySelector(".sys-table"), "nowhere for the table to go");
  });

  /// The themes panel is gone: the app never rendered a theme, it only read
  /// the per-system pictures out of one. What is left is the picker for those
  /// pictures, which is the only part that changed what you see.
  test("console pictures are chosen in Appearance, with no theme gallery", async () => {
    const { paneHtml } = await import(join(uiDir, "js", "settings-panes.js"));
    const pane = new JSDOM(`<body>${paneHtml("appearance")}</body>`).window.document;
    assert.ok(pane.querySelector(".icon-styles"), "no way to pick a picture style");
    assert.ok(pane.querySelector(".set-icons"), "no way to fetch any");
  });

  /// The tab was called "Systems", which said nothing: every tab in there is
  /// about systems of one kind or another, and the word gave no clue that this
  /// is where you choose which emulator runs a console.
  test("the emulator tab is named for what you change in it", async () => {
    const { TABS } = await import(join(uiDir, "js", "settings-panes.js"));
    const tab = TABS.find((t) => t.id === "systems");
    assert.ok(tab, "the tab is gone entirely");
    assert.notEqual(tab.label, "Systems", "still named after nothing in particular");
    assert.match(tab.label, /Emulator/i);
  });

  /// Four ports, and a way to say the other three pads are like the first.
  test("the control tab covers players beyond the first", async () => {
    const { paneHtml } = await import(join(uiDir, "js", "settings-panes.js"));
    const pane = new JSDOM(`<body>${paneHtml("control")}</body>`).window.document;
    assert.ok(pane.querySelector(".pad-list"), "no list of connected controllers");
    assert.ok(
      pane.querySelector('[data-field="mirror_player_one"]'),
      "no way to bind players 2-4 like player 1"
    );
  });

  /// Every tab the window offers has to render something. A tab whose id has
  /// no markup opens onto a blank pane, which is indistinguishable from a
  /// window that has broken.
  test("every tab renders a pane", async () => {
    const { TABS, paneHtml } = await import(join(uiDir, "js", "settings-panes.js"));
    for (const t of TABS) {
      assert.ok(
        (paneHtml(t.id) ?? "").trim().length > 0,
        `the ${t.id} tab renders nothing`
      );
    }
  });
});
