// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The page's toast: a short message at the top, where the eye is, never over an agent's input at
// the bottom: 12 px under the terminal's top (whatever the header's height), under the bookmark bar
// or the Paused chip when one shows there; under the header while the terminal is hidden (a
// phone's Files). Clicks go through it (style.css).
const $ = (id) => document.getElementById(id);
const GAP = 12;
let timer = 0;

/** Where the toast's top goes in #main, or "" (style.css's) when nothing is laid out. */
function top() {
  const term = $("term"), bar = $("bar");
  const from = term.offsetParent ? term.offsetTop : bar.offsetParent ? bar.offsetTop + bar.offsetHeight : null;
  if (from === null) return "";
  const over = ["markbar", "paused"].map($).filter((e) => !e.hidden && e.offsetParent);
  const said = [...$("over").children].filter((e) => !e.hidden).map((e) => $("over").offsetTop + e.offsetTop + e.offsetHeight + 8);   // what is said over the terminal
  return `${Math.max(from + GAP, ...said, ...over.map((e) => e.offsetTop + e.offsetHeight + 8))}px`;
}

/** Shows `text` for `ms`. */
export function toast(text, ms = 1800) {
  const el = $("toast");
  el.style.top = top();
  el.textContent = text;
  el.hidden = false;
  clearTimeout(timer);
  timer = setTimeout(() => { el.hidden = true; }, ms);
}
