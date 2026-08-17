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
    uniform1f: () => {},
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
