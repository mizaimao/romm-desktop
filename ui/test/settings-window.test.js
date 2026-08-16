// Every settings tab, rendered and wired the way the window does it.
//
// The other settings tests check that the markup contains a control. That is
// not the same as the tab working, and the difference was three live bugs that
// had been shipped for weeks:
//
//   * General wired a "reset keyboard" button that lives on the Control tab.
//     The lookup returned null, `.addEventListener` threw, and everything after
//     it in that pane — the wiring for every text field and toggle on the tab —
//     never ran. So editing the server URL or the library folder did nothing.
//   * Appearance called `cssColour()`, which was defined nowhere at all, so
//     wiring that tab threw partway through and the backdrop's own controls
//     were never connected.
//   * Both reset buttons called `closeSettings()` and `toggleSettings()`, which
//     were never imported into that module — a ReferenceError on click.
//
// None of them failed a test, because a pane that throws while being wired
// still contains all of its markup. This file wires the panes.

import { test, describe, before } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

let dom, panes, asked;

before(async () => {
  dom = new JSDOM(readFileSync(join(uiDir, "settings.html"), "utf8"), {
    url: "http://localhost/",
    pretendToBeVisual: true,
  });
  global.window = dom.window;
  global.document = dom.window.document;
  global.HTMLElement = dom.window.HTMLElement;
  global.localStorage = dom.window.localStorage;
  global.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
  global.requestAnimationFrame = dom.window.requestAnimationFrame.bind(dom.window);
  Object.defineProperty(global, "navigator", {
    value: dom.window.navigator,
    configurable: true,
  });

  asked = [];
  dom.window.__TAURI__ = {
    core: {
      invoke: async (cmd) => {
        asked.push(cmd);
        // Shapes the panes actually read. Anything else is a list.
        if (cmd === "config_fields") return { config_exists: true, library_root: "./library" };
        if (cmd === "motion_options") return { current: null, options: [] };
        if (cmd === "bios_status") return [0, 0, 0];
        return [];
      },
      convertFileSrc: (p) => p,
    },
    event: { listen: async () => () => {}, emit: () => {} },
  };

  panes = await import(join(uiDir, "js", "settings-panes.js"));
});

describe("every tab renders and wires", () => {
  for (const id of ["general", "appearance", "control", "library", "systems"]) {
    test(`${id} wires without throwing`, async () => {
      const box = dom.window.document.createElement("div");
      box.innerHTML = panes.paneHtml(id);
      dom.window.document.body.appendChild(box);
      // The failure this is here to catch is a throw, so the assertion is that
      // there is not one. `wirePane` is async for the tabs that fetch first.
      await assert.doesNotReject(async () => panes.wirePane(id, box));
      box.remove();
    });
  }

  /// A tab that reads config.toml has to actually ask for it. General's call
  /// sat after the line that threw, so it never happened — the tab looked
  /// right and wrote nothing.
  test("the tabs with settings in config.toml read them", async () => {
    asked.length = 0;
    for (const id of ["general", "control", "systems"]) {
      const box = dom.window.document.createElement("div");
      box.innerHTML = panes.paneHtml(id);
      dom.window.document.body.appendChild(box);
      await panes.wirePane(id, box);
      box.remove();
    }
    const reads = asked.filter((c) => c === "config_fields").length;
    assert.equal(reads, 3, `only ${reads} of the three tabs read their settings`);
  });

  /// Each tab exports the same two things, which is what lets the dispatcher be
  /// a table rather than a chain of ifs.
  test("every tab in the table has markup and a wire function", () => {
    for (const t of panes.TABS) {
      assert.ok(t.pane, `${t.id} has no pane module`);
      assert.equal(typeof t.pane.html, "string", `${t.id} exports no markup`);
      assert.ok(t.pane.html.trim().length > 0, `${t.id}'s markup is empty`);
      assert.equal(typeof t.pane.wire, "function", `${t.id} exports no wire function`);
    }
  });

  /// The settings window asks this before deciding what a keypress means, and
  /// the capture state moved out from under it when the panes were split.
  test("the window can still ask whether a key is being captured", () => {
    assert.equal(typeof panes.isCapturing, "function");
    assert.equal(typeof panes.captureKey, "function");
    assert.equal(panes.isCapturing(), false);
  });
});
