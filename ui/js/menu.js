// The right-click menu.
//
// One implementation, because there were two places that wanted one and the
// first was written inline for save states — with no stylesheet, which is why
// it appeared past the end of the page and looked like a menu that did not
// work at all.
//
// A menu of our own rather than the browser's: this is an application, and
// WebKit's offers "Open Image in New Window" and "Copy Image" for a console
// icon, which are not things this app does.

let open = null;

/// Show a menu of `items` at `(x, y)`.
///
/// Each item is `{ label, danger, run }`; a null item draws a separator. The
/// menu closes on the first click, key, or scroll after it opens, whichever
/// comes first — a menu that survives the next interaction is a menu people
/// end up clicking through.
export function showMenu(items, x, y) {
  closeMenu();
  const usable = items.filter(Boolean);
  if (!usable.length) return null;

  const menu = document.createElement("div");
  menu.className = "ctx-menu";
  for (const item of items) {
    if (!item) {
      menu.appendChild(document.createElement("hr"));
      continue;
    }
    const b = document.createElement("button");
    b.textContent = item.label;
    if (item.danger) b.className = "danger";
    b.addEventListener("click", () => {
      closeMenu();
      item.run();
    });
    menu.appendChild(b);
  }

  // Positioned before measuring, then pulled back inside the window: a menu
  // opened near an edge would otherwise hang off it with no way to reach the
  // items that fell outside.
  menu.style.left = `${x}px`;
  menu.style.top = `${y}px`;
  document.body.appendChild(menu);
  const box = menu.getBoundingClientRect();
  if (box.bottom > window.innerHeight) menu.style.top = `${Math.max(0, y - box.height)}px`;
  if (box.right > window.innerWidth) menu.style.left = `${Math.max(0, x - box.width)}px`;

  open = menu;
  // Next frame, or the same click that opened this closes it again.
  setTimeout(() => {
    window.addEventListener("pointerdown", closeMenu, { once: true });
    window.addEventListener("keydown", closeMenu, { once: true });
    window.addEventListener("wheel", closeMenu, { once: true, passive: true });
  }, 0);
  return menu;
}

export function closeMenu() {
  open?.remove();
  open = null;
}

export function menuOpen() {
  return !!open?.isConnected;
}
