// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Reminders (AC-91): bookmarks of the Reminder kind, of a project or a session (projects.js).
// terminumd puts one up each time an agent there ends a turn; then the pale fuchsia button under the
// bookmarks' shows its count filled while a session of the project is shown, until it is pressed (one
// seen there is marked again only once up anew). Its pane is the bookmarks' navigator (marknav.js) of
// the project's open reminders, those up first: the same list, view, actions and keys. The session
// list marks a session with open reminders by a wavy pink line. The pane is the shown session's: it
// closes when another is shown. Reminders up are said at the terminal's top for 4 s, once each in a
// page: the shown session's when they come up, another's when it is shown next.
import { fabPane } from "./fabpane.js";
import { openReminder } from "./marks.js";
import { MarkNav } from "./marknav.js";

const $ = (id) => document.getElementById(id);

const SAY_FOR = 4000;              // ms the toast of reminders up stays
const upFirst = (a, b) => Number(!!b.due) - Number(!!a.due) || b.at - a.at;

/** The open reminders of `items` (bookmarks' wire form), those up first, newest first. */
export function openReminders(items) { return [...items].filter(openReminder).sort(upFirst); }

/** marks: marks.js's Marks (its items, alive, jump, nav; it shows and places this button with the
 *  bookmarks'). */
export function reminders({ marks }) {
  const seen = new Set();          // reminders up that the user saw in the pane: no longer marked
  const told = new Set();          // reminders up a toast said
  let of = null;                   // the session the pane opened over
  let saidOf = null;               // marks.here()'s list place() last said
  const fab = $("remindfab");
  const nav = new MarkNav(marks, { pane: "remindpane", what: "Reminders", close: () => pane.toggle(false), only: (b) => b.remind, order: upFirst, peer: marks.nav });
  const pane = fabPane({ fab, pane: $("remindpane"), close: nav.frame.close, onToggle: (on) => { of = on ? marks.sid() : null; draw(); } });
  // another session shown, or its button hidden: the pane is no more of the session shown
  const stale = () => pane.open() && (fab.hidden || marks.sid() !== of);

  function draw() {
    const all = openReminders(marks.here()), up = all.filter((i) => i.due).length;   // the shown pane's project's
    for (const id of seen) if (!all.some((i) => i.id === id && i.due)) seen.delete(id);   // put down: marked when up again
    if (pane.open()) for (const i of all) if (i.due) seen.add(i.id);
    fab.classList.toggle("due", all.some((i) => i.due && !seen.has(i.id)));
    $("remindfabn").textContent = all.length ? String(all.length) : "";
    fab.title = up ? `Reminders: ${up} up of ${all.length}` : `Reminders: ${all.length}`;
    fab.setAttribute("aria-label", fab.title);
    if ((!all.length && pane.open() && !nav.entries().length) || stale()) return pane.toggle(false);
    nav.draw();                    // nothing while closed
    say(all);
  }

  /** "<project>: 16 reminders, 3 new" for reminders up the shown session's bell has not said yet. */
  function say(all) {
    if (fab.hidden) return;
    all ??= openReminders(marks.here());
    const fresh = all.filter((i) => i.due && !seen.has(i.id));
    if (!fresh.some((i) => !told.has(i.id))) return;
    for (const i of fresh) told.add(i.id);
    const where = marks.hereProject()?.name ?? marks.alive(marks.sid()) ?? "This session";
    marks.toast(`${where}: ${all.length} reminder${all.length === 1 ? "" : "s"}, ${fresh.length} new`, SAY_FOR);
  }

  // placed on every frame: says only for another list (another session shown, its button shown again); draw() says for a change
  function place() {
    if (!fab.hidden && marks.here() !== saidOf) { saidOf = marks.here(); say(); }
    return stale() ? pane.toggle(false) : pane.place();
  }

  return { draw, place, open: () => pane.toggle(true) };
}
