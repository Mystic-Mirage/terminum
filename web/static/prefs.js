// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Five of the menu's settings (AC-19): rendered Markdown's text size (this browser's step on
// settings.json's page.markdownSize: widgets, sent history, reminders, and Files and View in
// their frames), the terminal's history lines (settings.json's tmux.history, which tmux
// keeps for the panes made from then on), the idle agents' exit days (settings.json's
// agentExitDays, AC-153), the side marks on the terminal's right edge (on unless this browser
// turned them off: body.nomarks, and a "sidemarks" event for the rail), and Follow the agent's
// folder (follow.js).
import { followSwitch, onFollow } from "./follow.js";

const $ = (id) => document.getElementById(id);
const MD_MIN = 10, MD_MAX = 32;

/** redraw(): the list and the header, again (the switch changes their folders at once). */
export function menuPrefs({ store, send, toast, redraw }) {
  let base = 14, step = store.get("mdStep", 0), px = base;
  const setIn = (doc) => { try { doc?.documentElement.style.setProperty("--md-size", `${px}px`); } catch { /* another origin */ } };
  function show() {
    px = Math.max(MD_MIN, Math.min(MD_MAX, base + step));
    setIn(document);
    for (const f of document.querySelectorAll("iframe")) setIn(f.contentDocument);
    $("mdval").textContent = String(px);
  }
  // a frame loaded later (Files, View) gets it too
  document.addEventListener("load", (e) => { if (e.target.tagName === "IFRAME") setIn(e.target.contentDocument); }, true);
  const by = (d) => { step = Math.max(MD_MIN, Math.min(MD_MAX, base + step + d)) - base; store.set("mdStep", step); show(); };
  const follow = followSwitch(store, $("followagent"));
  onFollow(redraw);
  const marks = (on) => { document.body.classList.toggle("nomarks", !on); $("sidemarks").checked = on; };
  marks(store.get("sideMarks", true) !== false);
  $("sidemarks").addEventListener("change", () => {
    store.set("sideMarks", $("sidemarks").checked);
    marks($("sidemarks").checked);
    document.dispatchEvent(new Event("sidemarks"));
  });
  $("mdsmaller").onclick = () => by(-1);
  $("mdbigger").onclick = () => by(1);
  $("histlines").addEventListener("change", () => {
    const n = Number($("histlines").value);
    if (!Number.isInteger(n) || n < 1000 || n > 1_000_000) return toast("History lines must be 1000–1000000", 4000);
    send({ t: "history", lines: n });
    toast(`Terminals opened from now on keep ${n.toLocaleString()} lines`, 4000);
  });
  $("agentexit").addEventListener("change", () => {
    const n = Number($("agentexit").value), max = Number($("agentexit").max);
    if (!Number.isInteger(n) || n > max) return toast(`Days must be a whole number up to ${max}, 0 for off`, 4000);
    send({ t: "agentexit", days: n });                      // under 1 is off: terminumd keeps 0
    toast(n < 1 ? "Idle agents are not asked to exit" : `Agents idle and not opened for ${n} day${n === 1 ? "" : "s"} are asked to exit`, 4000);
  });
  return {
    /** settings.json's page part (settings.js). */
    page(p) {
      follow.page(p);
      base = Number.isInteger(p.markdownSize) ? p.markdownSize : 14;
      if (Number.isInteger(p.history) && document.activeElement !== $("histlines")) $("histlines").value = String(p.history);
      if (Number.isInteger(p.agentExitDays) && document.activeElement !== $("agentexit")) $("agentexit").value = String(p.agentExitDays);
      show();
    },
  };
}
