// The shader backdrop, with a stand-in for WebGL.
//
// jsdom has no WebGL at all, so `getContext("webgl2")` returns null and the
// whole of this module's real work was never once executed by a test. That is
// how a reference error in it — `apply` calling `resize` a dozen lines before
// `resize` exists — shipped, threw out of startup, and took the tab row, the
// settings button and the page background with it. A backdrop is decoration
// and it managed to be fatal.
//
// The stand-in answers every call this module makes and records nothing about
// how they are drawn. What is under test is that the code runs at all: builds,
// switches style, resizes, and animates without throwing.

import { test, describe, before, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");

let dom, backdrop, calls;

/// Enough of WebGL2 to get through: every entry point this module touches,
/// returning something of the right shape.
function fakeGl() {
  const handle = (name) => ({ name });
  return {
    FRAGMENT_SHADER: 1,
    VERTEX_SHADER: 2,
    COMPILE_STATUS: 3,
    LINK_STATUS: 4,
    ARRAY_BUFFER: 5,
    STATIC_DRAW: 6,
    FLOAT: 7,
    TRIANGLES: 8,
    createShader: () => handle("shader"),
    shaderSource: () => {},
    compileShader: () => {},
    getShaderParameter: () => true,
    getShaderInfoLog: () => "",
    deleteShader: () => {},
    createProgram: () => handle("program"),
    attachShader: () => {},
    linkProgram: () => {},
    getProgramParameter: () => true,
    getProgramInfoLog: () => "",
    useProgram: (p) => calls.push(["useProgram", p?.name]),
    createBuffer: () => handle("buffer"),
    bindBuffer: () => {},
    bufferData: () => {},
    getAttribLocation: () => 0,
    enableVertexAttribArray: () => {},
    vertexAttribPointer: () => {},
    getUniformLocation: (_p, n) => handle(n),
    uniform1f: (loc, v) => calls.push(["uniform1f", loc?.name, v]),
    uniform2f: () => {},
    uniform3fv: () => {},
    viewport: () => {},
    clearColor: () => {},
    clear: () => {},
    drawArrays: () => calls.push(["draw"]),
  };
}

before(async () => {
  dom = new JSDOM(readFileSync(join(uiDir, "index.html"), "utf8"), {
    url: "http://localhost/",
    pretendToBeVisual: true,
  });
  global.window = dom.window;
  global.document = dom.window.document;
  global.localStorage = dom.window.localStorage;
  global.HTMLElement = dom.window.HTMLElement;
  global.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
  Object.defineProperty(global, "navigator", { value: dom.window.navigator, configurable: true });
  // The animation loop reaches for the bare global, as it does in a browser.
  // It hands back an id and never calls back: the loop reschedules itself
  // every frame, so a stub that actually fires would keep this process alive
  // for as long as node is willing to wait.
  global.requestAnimationFrame = () => 1;
  global.cancelAnimationFrame = () => {};
  dom.window.HTMLCanvasElement.prototype.getContext = function (kind) {
    return kind === "webgl2" ? fakeGl() : null;
  };
  dom.window.__TAURI__ = {
    core: { invoke: async () => [], convertFileSrc: (p) => p },
    event: { listen: async () => () => {}, emit: () => {} },
  };
  backdrop = await import("../js/backdrop.js");
});

beforeEach(() => {
  calls = [];
  backdrop.stopBackdrop();
  dom.window.localStorage.clear();
});

describe("starting it", () => {
  test("it builds without throwing", () => {
    assert.ok(backdrop.startBackdrop(), "the backdrop did not start");
    assert.ok(
      dom.window.document.getElementById("backdrop"),
      "no canvas was put on the page"
    );
  });

  /// The failure that took the window down. Whatever is wrong in here, the
  /// caller has to survive it: the tab row, the settings button and the page
  /// background are all built after this call.
  test("a broken shader does not escape", () => {
    const good = dom.window.HTMLCanvasElement.prototype.getContext;
    dom.window.HTMLCanvasElement.prototype.getContext = () => {
      throw new Error("no GPU today");
    };
    try {
      assert.doesNotThrow(() => backdrop.startBackdrop());
      assert.equal(backdrop.startBackdrop(), null);
    } finally {
      dom.window.HTMLCanvasElement.prototype.getContext = good;
    }
  });
});

describe("the styles", () => {
  test("every one names itself and contributes a body", () => {
    assert.ok(backdrop.BACKDROPS.length >= 5, "one shape suits one room");
    for (const b of backdrop.BACKDROPS) {
      assert.ok(b.id && b.label && b.hint, `${b.id} is missing its name or note`);
      assert.match(b.body, /base\s*=/, `${b.id} never sets a colour`);
    }
    const ids = backdrop.BACKDROPS.map((b) => b.id);
    assert.equal(new Set(ids).size, ids.length, "two styles share an id");
  });

  test("an unknown style falls back rather than failing", () => {
    assert.equal(backdrop.backdropStyle("no-such-thing").id, backdrop.BACKDROPS[0].id);
  });

  /// Switching style rebuilds the program and looks its uniforms up again. A
  /// location belongs to the program it came from, and reusing one across
  /// programs is how the second style you try comes out black.
  test("switching style keeps running", () => {
    backdrop.startBackdrop();
    for (const b of backdrop.BACKDROPS) {
      assert.doesNotThrow(
        () => backdrop.applyBackdropSettings({ style: b.id }),
        `switching to ${b.label} threw`
      );
    }
    // Back to the first one, which is already compiled.
    assert.doesNotThrow(() => backdrop.applyBackdropSettings({ style: backdrop.BACKDROPS[0].id }));
  });
});

/// The glass, which is one number and used to be two.
///
/// The preview pane had its own tint and its own slider, so the surface you
/// read from was the one surface that matched nothing around it — and the
/// slider that was supposed to fix that was a second control over an idea the
/// stylesheet only has one of. `--tint` is the opacity every sheet of glass
/// mixes its colour in at; the pane is a large card and takes the same one.
describe("the glass", () => {
  const css = () =>
    readFileSync(join(uiDir, "style.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  const ruleFor = (sel) =>
    new RegExp(`${sel}\\s*\\{[^}]*\\}`).exec(css())?.[0] ?? "";
  const backgroundOf = (sel) =>
    /background:\s*([^;]+);/.exec(ruleFor(sel))?.[1].replace(/\s+/g, " ").trim();

  /// The pane and the cards no longer look identical, and that is deliberate:
  /// a grid of forty translucent rectangles competes with the artwork inside
  /// them, while the pane is a surface you read from and needs something to
  /// read against. What must not drift is where the opacity comes from — the
  /// bug this file exists for was the pane having a tint and a slider of its
  /// own, which is how the two ended up disagreeing.
  test("the preview pane takes its opacity from the one variable", () => {
    assert.match(
      backgroundOf("#detail") ?? "",
      /var\(--tint\)/,
      "the preview pane has a private opacity again"
    );
  });

  /// Flattening the console grid was tried and reverted: with no pane and no
  /// outline there is nothing to say where one console ends and the next
  /// begins, and the grid reads as scattered logos rather than a list of
  /// things you can press. The Continue playing strip is the opposite case —
  /// five wide screenshots in a row, no column to align with — so it is the
  /// one place the frame comes off.
  test("cards keep their surface; only the recent strip is flat", () => {
    assert.match(
      backgroundOf("\\.card") ?? "",
      /var\(--tint\)/,
      "the console grid lost the edge that separates one card from the next"
    );
    const flat = ruleFor("\\.recent \\.gcard \\.art");
    assert.match(flat, /backdrop-filter:\s*none/, "the recent strip is not flattened");
    assert.match(flat, /border-color:\s*transparent/, "the recent strip kept its outline");
  });

  /// There is one slider for this, and Appearance is where a second one would
  /// reappear.
  test("Appearance offers one control for it, not two", () => {
    const pane = readFileSync(join(uiDir, "js/settings/appearance.js"), "utf8");
    const sliders = pane.match(/class="(glass-strength|pane-clarity)"/g) ?? [];
    assert.deepEqual(sliders, ['class="glass-strength"']);
  });

  /// The bug that made the pane solid, and it is worth stating on its own
  /// because it is invisible at the call site: `Number(null)` is `0`, `0` is
  /// finite and not negative, so a guard written as `n >= 0` accepts the
  /// absence of a setting as the setting `0`. Restore
  /// `Number(localStorage.getItem())` without the presence check and this fails.
  test("nothing stored means the default, not zero", () => {
    dom.window.localStorage.clear();
    assert.equal(backdrop.glassStrength(), 18, "an unset glass strength read as none");
  });

  /// The other half of the same guard: a zero somebody chose is not the same
  /// as a zero that fell out of an empty key, and it has to survive a restart.
  test("a zero that was chosen is kept", () => {
    dom.window.localStorage.setItem("glassStrength", "0");
    assert.equal(backdrop.glassStrength(), 0, "the glass slider snapped back off zero");
  });

  test("it is applied as a percentage the stylesheet can use", () => {
    backdrop.setGlassStrength(24);
    assert.equal(
      dom.window.document.documentElement.style.getPropertyValue("--tint"),
      "24%"
    );
  });
});

/// The three complaints about the shapes themselves, each with the arithmetic
/// that caused it.
///
/// jsdom has no GPU, so none of this can be checked by rendering it — these
/// read the shader source and assert the property that was wrong. That is
/// weaker than a picture and it is what is available; the failures they guard
/// are all "somebody edited the body back".
describe("the shapes", () => {
  test("every style says how fast it should run", () => {
    for (const b of backdrop.BACKDROPS) {
      assert.equal(typeof b.pace, "number", `${b.id} has no pace`);
      assert.ok(b.pace > 0, `${b.id} would never move`);
    }
  });

  /// Blobs drifts at 0.015 of `t` and Plasma sweeps at 0.31, so one number on
  /// the Motion slider meant twenty times the movement depending on which
  /// shape was drawing. Whatever the paces are tuned to, these two cannot end
  /// up being handed the same speed.
  test("the same Motion setting is not the same speed for every shape", () => {
    const speedFor = (style) => {
      backdrop.applyBackdropSettings({ style, speed: 5 });
      const seen = calls.filter((c) => c[0] === "uniform1f" && c[1] === "u_speed");
      return seen[seen.length - 1]?.[2];
    };
    backdrop.startBackdrop();
    const blobs = speedFor("blobs");
    const plasma = speedFor("plasma");
    assert.ok(blobs > 0 && plasma > 0, "no speed reached the shader");
    assert.ok(plasma < blobs, "Plasma is still being run as fast as Blobs");
  });

  /// Drift: a star's glow has a radius of 0.42 of a cell, so any star nearer
  /// than that to a cell edge had the rest of it drawn by a neighbouring cell
  /// that was never asked. What showed was points sliced flat along the same
  /// straight lines every frame. It has to look at the eight neighbours.
  test("Drift asks the cells around it, not only its own", () => {
    const body = backdrop.backdropStyle("stars").body;
    assert.match(body, /for\s*\(int/, "Drift is back to sampling one cell");
    assert.match(body, /cell \+ o/, "the neighbouring cell is not being hashed");
  });

  /// Grid: `max(p.y, 0.04)` froze the perspective divide across the bottom of
  /// the screen, so the lines stopped converging at a fixed height and ran
  /// straight down from it — and `fwidth` of a frozen coordinate is zero, so
  /// the same seam was also a divide by zero.
  test("Grid guards its divide instead of clamping what goes into it", () => {
    const body = backdrop.backdropStyle("grid").body;
    assert.doesNotMatch(body, /max\(p\.y,/, "the perspective input is clamped again");
    assert.match(body, /max\(fwidth\([^)]*\), *1e-4\)/, "fwidth can still be zero here");
  });
});
