// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What the session page holds of the board: the session its address asks for (terminum-kanban.md
// AC-134) and the lines of tasks that need a human (AC-117).
import { addressedSession, sessionHash, taskUrl } from "./tasklink.js";
import { findItem, serverDown } from "./nav.js";
import { taskLines } from "./tasklines.js";

/** `box` holds the lines. */
export function pageTasks({ box, embedded, say, show }) {
  let asked = addressedSession(), then = new URLSearchParams(location.search).get("then");
  // Back and Forward go through the sessions pressed
  if (!embedded) addEventListener("popstate", () => { const id = addressedSession(); if (id) show(id); });
  const lines = taskLines(box, (id) => { location.href = taskUrl(id); });
  return {
    /** The session asked for, before any list. */
    asked,
    /** The first list with rows says whether it is there; the page then goes on as after any closing. */
    layout(groups) {
      if (!asked || !groups.length) return;
      if (!findItem(groups, asked) && !serverDown(groups, asked)) say("That session has ended.", 4000);
      else if (then === "plan") setTimeout(() => document.dispatchEvent(new Event("planopen")));
      then = null;
      asked = null;
    },
    marks(m) { if (!embedded) lines.came(m); },
    /** Every session has its own address (AC-134): `press`: a step of Back, any other change none. */
    address(id, press = false) {
      const hash = sessionHash(id);
      const search = new URLSearchParams(location.search);
      search.delete("then");                        // said once: Back never opens it again
      const url = `${location.pathname}${search.size ? `?${search}` : ""}${hash}`;
      if (!embedded && `${location.pathname}${location.search}${location.hash}` !== url) history[press ? "pushState" : "replaceState"](history.state, "", url);
    },
  };
}
