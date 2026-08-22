// A scripted browse, for measuring what the app weighs while it fills with
// artwork. Run with `ROMM_MEASURE=tools/browse.js` and weigh the process from
// outside; the notes below say where it has got to.
(async () => {
  const { invoke } = window.__TAURI__.core;
  const note = (m) => invoke("measure_note", { text: m }).catch(() => {});
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  try {
    note("script running");
    const list = document.getElementById("list");
    // Clicked, not called. Going straight at `showRoms` skips whatever the
    // click sets up first and hangs there instead, which cost one run.
    note("opening arcade");
    const card = document.querySelector('[data-slug="arcade"]');
    if (!card) throw new Error("no arcade card on this screen");
    card.click();
    for (let i = 0; i < 40 && !list.querySelector(".gcard,.row"); i++) await wait(500);
    await wait(4000);
    note(`arcade open, ${list.querySelectorAll(".gcard,.row").length} cards`);
    const steps = 30;
    for (let i = 1; i <= steps; i++) {
      list.scrollTop = (list.scrollHeight - list.clientHeight) * (i / steps);
      list.dispatchEvent(new Event("scroll"));
      await wait(900);
      if (i % 5 === 0) note(`scrolled ${Math.round((i / steps) * 100)}%`);
    }
    note("done");
  } catch (e) {
    note("failed: " + e);
  }
})();
