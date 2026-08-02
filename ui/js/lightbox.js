// Full-size artwork over a dimmed backdrop, inside the main window.
//
// In-window rather than a second OS window: no dock icon, no window
// management, and Esc closes it.

import { el, convertFileSrc } from "./state.js";

const lb = { items: [], index: 0, open: false };

/// Called before opening so a running slideshow cannot swap the image out.
let onOpen = () => {};
export function setOpenHook(fn) {
  onOpen = fn;
}

export function openLightbox(items, index = 0) {
  if (!items.length) return;
  Object.assign(lb, { items, index, open: true });
  el.lb.hidden = false;
  onOpen();
  render();
}

export function closeLightbox() {
  lb.open = false;
  el.lb.hidden = true;
  // Clear the stage so a playing video does not keep its audio going.
  el.lb.querySelector(".lb-stage").innerHTML = "";
}

function step(delta) {
  if (!lb.open || lb.items.length < 2) return;
  lb.index = (lb.index + delta + lb.items.length) % lb.items.length;
  render();
}

function render() {
  const it = lb.items[lb.index];
  const stage = el.lb.querySelector(".lb-stage");
  stage.innerHTML =
    it.kind === "video"
      ? `<video src="${it.src}" controls autoplay loop></video>`
      : it.kind === "pdf"
        // WKWebView renders PDFs natively, so an iframe is the whole viewer.
        ? `<iframe src="${it.src}" title="Manual"></iframe>`
        : `<img src="${it.src}" alt="" />`;
  el.lb.querySelector("figcaption").textContent =
    lb.items.length > 1
      ? `${it.caption} — ${lb.index + 1} of ${lb.items.length}`
      : it.caption;
  const multi = lb.items.length > 1;
  el.lb.querySelector(".lb-prev").disabled = !multi;
  el.lb.querySelector(".lb-next").disabled = !multi;
}

/// Everything in the detail pane, as one navigable set.
export function detailMedia(d) {
  const items = (d.screenshots || []).map((s, i) => ({
    src: convertFileSrc(s),
    kind: "image",
    caption: `Screenshot ${i + 1}`,
  }));
  if (d.cover) items.push({ src: convertFileSrc(d.cover), kind: "image", caption: "Cover" });
  if (d.video) items.push({ src: convertFileSrc(d.video), kind: "video", caption: "Video" });
  return items;
}

el.lb.querySelector(".lb-close").addEventListener("click", closeLightbox);
el.lb.querySelector(".lb-prev").addEventListener("click", () => step(-1));
el.lb.querySelector(".lb-next").addEventListener("click", () => step(1));
// Clicking the backdrop closes; clicking the artwork itself does not.
el.lb.addEventListener("click", (ev) => {
  if (ev.target === el.lb || ev.target.tagName === "FIGURE") closeLightbox();
});
window.addEventListener("keydown", (ev) => {
  if (!lb.open) return;
  if (ev.key === "Escape") closeLightbox();
  else if (ev.key === "ArrowLeft") step(-1);
  else if (ev.key === "ArrowRight") step(1);
});
