// The window chrome, checked against the real stylesheet.
//
// This file exists because of one bug that looked like three. `hidden` is
// implemented by the browser's own stylesheet as `[hidden] { display: none }`,
// and a user-agent rule loses to *any* author rule that sets `display` — no
// matter how weak. style.css sets one on every button that holds an icon
// beside a word, so Back, Grid and Hide info stayed on screen after the code
// had hidden them. The Grid button then redrew the last console you had open,
// which reads as a button that opens consoles by itself.
//
// None of that is visible from reading the JavaScript: the code sets `hidden`
// correctly and the element is genuinely hidden as far as the DOM is
// concerned. It has to be asked of the cascade.
//
// jsdom evaluates plain selectors but not `:has()`, so the assertions below
// cover the ID rules. That is enough to catch the regression: the fix is one
// declaration and either it is there or it is not.

import { test, describe, before } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

let dom;

/// Buttons the app hides depending on which screen is showing. Every one of
/// them carries an icon, a word, or both, which is what put them in the way of
/// the author `display` rules in the first place.
const TOGGLED = ["back", "layout-btn", "sidebar-btn", "zoom-wrap"];

before(() => {
  dom = new JSDOM(readFileSync(join(uiDir, "index.html"), "utf8"), {
    pretendToBeVisual: true,
  });
  const style = dom.window.document.createElement("style");
  style.textContent = readFileSync(join(uiDir, "style.css"), "utf8");
  dom.window.document.head.appendChild(style);
});

const displayOf = (id) =>
  dom.window.getComputedStyle(dom.window.document.getElementById(id)).display;

describe("hidden actually hides", () => {
  for (const id of TOGGLED) {
    test(`${id} disappears when hidden`, () => {
      const node = dom.window.document.getElementById(id);
      node.hidden = true;
      assert.equal(
        displayOf(id),
        "none",
        `#${id} is still laid out while hidden — an author rule setting ` +
          `display is beating the browser's [hidden], so the screen shows a ` +
          `control that the code believes is gone`
      );
    });

    test(`${id} comes back when shown`, () => {
      const node = dom.window.document.getElementById(id);
      node.hidden = true;
      node.hidden = false;
      assert.notEqual(
        displayOf(id),
        "none",
        `#${id} stays invisible after being shown — a blanket display:none ` +
          `would hide these buttons for good`
      );
    });
  }
});

describe("the right-click menu is positioned by the stylesheet", () => {
  /// The menu shipped with `left` and `top` set inline and no `position` rule
  /// anywhere, so the browser laid it out in the normal document flow: it
  /// appeared past the end of the page, with the coordinates ignored. Nothing
  /// in the JavaScript was wrong and nothing reported anything — it simply
  /// looked like a right-click that did nothing.
  test("a menu given coordinates is taken out of the flow", () => {
    const menu = dom.window.document.createElement("div");
    menu.className = "ctx-menu";
    menu.style.left = "120px";
    menu.style.top = "90px";
    dom.window.document.body.appendChild(menu);

    const style = dom.window.getComputedStyle(menu);
    assert.equal(
      style.position,
      "fixed",
      "left and top mean nothing on a statically positioned element, so the " +
        "menu lands wherever the document flow puts it"
    );
    menu.remove();
  });
});
