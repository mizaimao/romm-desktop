// Open the biggest console and scroll to the bottom, in steps, so the process
// can be weighed from outside while it fills with artwork.
(async () => {
  const log = (m) => { console.log("MEASURE " + m); document.title = "M " + m; };
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  try {
    const lib = await import("./js/library.js");
    log("opening arcade");
    await lib.showRoms("arcade");
    await wait(3000);
    const list = document.getElementById("list");
    const steps = 40;
    for (let i = 1; i <= steps; i++) {
      list.scrollTop = (list.scrollHeight - list.clientHeight) * (i / steps);
      list.dispatchEvent(new Event("scroll"));
      await wait(700);
      if (i % 5 === 0) log(`scrolled ${Math.round((i / steps) * 100)}%`);
    }
    log("done");
  } catch (e) {
    log("failed: " + e);
  }
})();
