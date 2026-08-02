// Formatting and small DOM helpers.

import { el } from "./state.js";

export function human(bytes) {
  if (!bytes) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes, i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(1)} ${units[i]}`;
}

export function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]
  );
}

/// Omit a definition row entirely when empty, rather than rendering a blank.
export function row(label, value) {
  if (value === null || value === undefined || value === "") return "";
  return `<dt>${label}</dt><dd>${escapeHtml(String(value))}</dd>`;
}

/// RomM stores ratings 0-100; show five stars plus the raw number.
export function starBar(rating) {
  const n = Math.round((rating / 100) * 5 * 2) / 2;
  const full = Math.floor(n);
  const half = n - full >= 0.5;
  const stars = "★".repeat(full) + (half ? "⯨" : "") + "☆".repeat(5 - full - (half ? 1 : 0));
  return `<div class="rating"><span class="stars">${stars}</span>
          <span class="num">${Math.round(rating)}/100</span></div>`;
}

let toastTimer;
export function toast(msg, ms = 4000) {
  el.toast.textContent = msg;
  el.toast.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.toast.hidden = true), ms);
}
