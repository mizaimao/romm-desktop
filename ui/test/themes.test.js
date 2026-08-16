// Themes.
//
// The claim this file has to keep honest is that a theme is *only* data. If the
// stylesheet stops reading a token, or a theme forgets one, the failure is a
// window that half-changes — the cards in the new palette and the header in the
// old one — and nothing reports it.

import { test, describe, before, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const css = readFileSync(join(uiDir, "style.css"), "utf8");

let dom, themes, emitted;

before(async () => {
  dom = new JSDOM(readFileSync(join(uiDir, "index.html"), "utf8"), {
    url: "http://localhost/",
    pretendToBeVisual: true,
  });
  global.window = dom.window;
  global.document = dom.window.document;
  global.localStorage = dom.window.localStorage;
  dom.window.__TAURI__ = { event: { emit: (name, payload) => emitted.push([name, payload]) } };
  themes = await import("../js/themes.js");
});

beforeEach(() => {
  emitted = [];
  dom.window.localStorage.clear();
  dom.window.document.documentElement.removeAttribute("style");
});

const tokenOnRoot = (name) =>
  dom.window.document.documentElement.style.getPropertyValue(`--${name}`);

describe("a theme is a set of tokens", () => {
  /// The original look has to survive being made themeable. If the default
  /// theme is not identical to what the stylesheet already had, then every
  /// existing install silently gets a slightly different app.
  test("the first theme reproduces the stylesheet's own defaults", () => {
    const root = css.slice(css.indexOf(":root {"), css.indexOf("}", css.indexOf(":root {")));
    const declared = Object.fromEntries(
      [...root.matchAll(/--([a-z0-9-]+): *([^;]+);/g)].map((m) => [m[1], m[2].trim()])
    );
    themes.applyTheme("aero", { announce: false });
    for (const [name, value] of Object.entries(declared)) {
      // Only the tokens a theme owns; the stylesheet has others.
      const applied = tokenOnRoot(name);
      if (!applied) continue;
      assert.equal(applied, value, `--${name} differs from the stylesheet default`);
    }
  });

  /// Setting only what a theme names leaves the previous theme's values on
  /// everything else — so going from a light theme to a dark one keeps
  /// whichever colours the second happened not to mention, and the window ends
  /// up half of each.
  test("switching writes every token, not only the ones the theme names", () => {
    themes.applyTheme("paper", { announce: false });
    assert.equal(tokenOnRoot("bg"), "#f4f2ee");

    // Aero names almost nothing — it is the defaults.
    themes.applyTheme("aero", { announce: false });
    assert.equal(tokenOnRoot("bg"), "#14161a", "the light background survived the switch");
    assert.equal(tokenOnRoot("round"), "1");
    assert.equal(tokenOnRoot("density"), "1");
  });

  /// Roundness and density are multipliers, so a theme that leaves them alone
  /// keeps every corner in the proportions it was drawn at.
  test("every theme's numbers are multipliers a stylesheet can use", () => {
    for (const t of themes.THEMES) {
      const round = Number(themes.tokenOf("round", t));
      const density = Number(themes.tokenOf("density", t));
      assert.ok(Number.isFinite(round) && round >= 0 && round <= 3, `${t.id}: round ${round}`);
      assert.ok(
        Number.isFinite(density) && density >= 0.5 && density <= 2,
        `${t.id}: density ${density}`
      );
    }
  });

  test("every theme carries a backdrop gradient and a readable name", () => {
    for (const t of themes.THEMES) {
      assert.match(t.low, /^#[0-9a-f]{6}$/i, `${t.id} has no dark end`);
      assert.match(t.high, /^#[0-9a-f]{6}$/i, `${t.id} has no light end`);
      assert.ok(t.label && t.note, `${t.id} has no label or note`);
    }
    const ids = themes.THEMES.map((t) => t.id);
    assert.equal(new Set(ids).size, ids.length, "two themes share an id");
  });

  /// A stored theme that has since been renamed or dropped must not leave the
  /// window with no palette at all.
  test("a theme that no longer exists falls back to the first", () => {
    dom.window.localStorage.setItem("romm.theme", "a-theme-that-was-deleted");
    assert.equal(themes.currentThemeId(), themes.THEMES[0].id);
    assert.equal(themes.currentTheme().id, themes.THEMES[0].id);
  });

  test("choosing one is remembered and announced to the other window", () => {
    themes.applyTheme("amber");
    assert.equal(themes.currentThemeId(), "amber");
    assert.deepEqual(emitted, [["theme-changed", "amber"]]);

    // Applying without announcing is for the window receiving that event; it
    // must not store or re-emit, or the two windows would talk in a circle.
    emitted.length = 0;
    themes.applyTheme("paper", { announce: false });
    assert.deepEqual(emitted, []);
    assert.equal(themes.currentThemeId(), "amber");
  });
});

describe("the stylesheet reads the tokens", () => {
  /// A theme is only data because the stylesheet goes through tokens. These
  /// check the other half of that bargain — that the corners and the typeface
  /// really are wired up, and that nothing has crept back in hardcoded.
  test("corners are drawn through the roundness multiplier", () => {
    const literal = css.match(/border-radius: *\d+px/g) ?? [];
    assert.deepEqual(
      literal,
      [],
      `${literal.length} corner(s) ignore the theme: ${literal.slice(0, 3).join(", ")}`
    );
    assert.ok(css.includes("var(--round)"), "nothing uses the roundness multiplier");
  });

  test("the typeface comes from the theme", () => {
    assert.match(css, /font: *[\d.]+px\/[\d.]+ var\(--font\)/, "the body font is not a token");
  });

  test("the library's spacing comes from the density multiplier", () => {
    const uses = css.match(/var\(--density\)/g) ?? [];
    assert.ok(uses.length >= 3, `only ${uses.length} place(s) respond to density`);
  });
});
