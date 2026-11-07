// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The session page's tools (AC-07, AC-52, AC-81, AC-89): the list's (listtools.js), the line for a tmux
// server that does not answer, the pencil that names the shown pane, and the page's confirmation dialog.

import { listTools } from "./listtools.js";
import { banner } from "./banners.js";
import { stalls } from "./nav.js";
import { askConfirm } from "./dialogs.js";

const $ = (id) => document.getElementById(id);

/** Close Session (the menu's, a row's): asked first, as the program in it ends; then sent. */
export async function closeSession(item, send, tasks = []) {
  const prog = item.sub?.[0] || "a program";
  const open = tasks.length ? ` ${tasks.join(", ")} ${tasks.length === 1 ? "is" : "are"} open in it.` : "";
  if (await askConfirm(`Close the session ${item.title}? The program running in it (${prog}) ends.${open}`, "Close Session")) send({ t: "kill", id: item.id });
}

/** Wires the tools; `current()` is the shown pane's {group, item} or null, `onResize()` refits the terminal. */
export function navTools({ send, store, current, groups, onResize, onRenamed }) {
  const list = listTools({ send, store, groups, onResize });

  // ---------- a tmux server not answering (AC-03, AC-14) ----------
  // Screens, keys and histories wait for it: say so, with the time it has not answered.
  let stallTimer = 0;
  function showStall(m) {
    const key = m.key ?? m.server ?? "local", what = m.what ?? `tmux on ${(m.server ?? "local") === "local" ? "this Mac" : m.server}`;
    if (m.clear) stalls.clear();
    else if (m.ok) stalls.delete(key);
    else if (!stalls.has(key)) stalls.set(key, [what, performance.now() - (m.ms ?? 0)]);
    clearInterval(stallTimer);
    sayStalls();
    if (stalls.size) stallTimer = setInterval(sayStalls, 1000);
  }
  // Only the shown pane's server: another's trouble is not this pane's (AC-132); its group in the
  // list says when its link drops.
  function sayStalls() {
    const found = current(), shown = found?.group.server, mine = found ? [...stalls].filter(([key]) => !shown || key === shown) : [];
    if (!mine.length) return banner("stall", "");
    banner("stall", mine.map(([, [what, since]]) =>
      `${what} is not responding (${Math.round((performance.now() - since) / 1000)} s)`).join(". "), mine.map(([key]) => key).join());   // the seconds are no news
  }

  // The pencil names the shown pane's window (and the pane's title).
  function startRename() {
    const found = current();
    if (!found) return;
    $("renamename").value = found.item.title;
    $("current").hidden = $("rename").hidden = true;
    $("renamebox").hidden = false;
    $("renamename").focus();
    $("renamename").select();
  }
  // Done or cancelled: the keys go back to the terminal (on a touch screen the keyboard closes).
  function endRename(refocus = false) {
    $("renamename").blur();
    $("renamebox").hidden = true;
    $("current").hidden = false;
    $("rename").hidden = !current();
    if (refocus) onRenamed();
  }
  $("rename").onclick = (e) => { e.stopPropagation(); startRename(); };   // not the header's own click
  $("renamecancel").onclick = () => endRename(true);
  $("renamebox").addEventListener("submit", (e) => {
    e.preventDefault();
    send({ t: "rename", id: current()?.item.id, name: $("renamename").value });
    endRename(true);
  });
  $("renamename").addEventListener("keydown", (e) => { if (e.key === "Escape") { e.stopPropagation(); endRename(true); } });

  return { ...list, showStall, sayStalls, endRename, renaming: () => !$("renamebox").hidden };
}
