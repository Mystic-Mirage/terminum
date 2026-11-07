// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What the session page holds of the board: the session its address asks for (terminum-kanban.md
// AC-134) and the lines of tasks that need a human (AC-117).
import { askedSession, taskUrl } from "./tasklink.js";
import { findItem, serverDown } from "./nav.js";
import { taskLines } from "./tasklines.js";

/** `box` holds the lines. */
export function pageTasks({ box, embedded, say }) {
  let asked = askedSession();
  const lines = taskLines(box, (id) => { location.href = taskUrl(id); });
  return {
    /** The session asked for, before any list. */
    asked,
    /** The first list with rows says whether it is there; the page then goes on as after any closing. */
    layout(groups) {
      if (!asked || !groups.length) return;
      if (!findItem(groups, asked) && !serverDown(groups, asked)) say("That session has ended.", 4000);
      asked = null;
    },
    marks(m) { if (!embedded) lines.came(m); },
  };
}
