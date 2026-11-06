// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The banners over the terminal (#status: an error, #stall: a server that does not answer,
// #sized: a size another screen chose): one line of text and an × that puts it away until it says
// something new. `key`: what it is about (a counter's seconds are no news).
import { icon } from "./icons.js";

const $ = (id) => document.getElementById(id);

/** A banner's ×: an icon key (controls.css) that calls `onClick`. */
export function closeButton(label, onClick) {
  const x = document.createElement("button");
  x.type = "button";
  x.className = "x";
  x.setAttribute("aria-label", label);
  x.append(icon("x", 16));
  x.onclick = onClick;
  return x;
}

/** The lowest edge of what is said over the terminal (#over), no higher than `top`. */
export const underBanners = (top) => Math.max(top, ...[...document.querySelectorAll("#over > .banner")].filter((b) => !b.hidden).map((b) => b.getBoundingClientRect().bottom));

/** Shows `text` in banner `id`, or hides it (empty text: what it said is over). */
export function banner(id, text, key = text) {
  const el = $(id);
  if (!text) { el.hidden = true; delete el.dataset.away; return; }
  if (el.dataset.away === key) return;            // put away: until it says something else
  let line = el.querySelector(":scope > span");
  if (!line) {
    line = document.createElement("span");
    el.replaceChildren(line, closeButton("Close", () => { el.dataset.away = el.dataset.key; el.hidden = true; }));
  }
  el.dataset.key = key;
  line.textContent = text;
  el.hidden = false;
}
