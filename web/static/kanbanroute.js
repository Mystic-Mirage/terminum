// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A task's window has its own address on the board, /kanban/TERM-7 (terminum-kanban.md AC-133, AC-148): Back closes
// it, a task that is not there is said, a name it had (T-12, its key's start) stands as the one it has now.
import { $, say } from "./kanbanutil.js";

/** The task the address names: terminumd finds it by its ID, one it had or its key's start, or says there is none. */
const addressed = () => /^\/kanban\/([A-Za-z0-9-]+)$/.exec(location.pathname)?.[1];

/** windowed: kanbanwin.js's window; needs: tasklines.js's lines of tasks that wait. */
export function taskAddress({ windowed, needs }) {
  /** Its window has its own address: Back closes it, a task that is not there is said. */
  function openTask(id, push = true) {
    // one step of Back for the window, whichever tasks it showed
    if (push) history[addressed() ? "replaceState" : "pushState"]({ win: true }, "", `/kanban/${id}${location.search}`);
    needs.opened(id);
    windowed.open(id).then(() => {
      if (addressed() === id && windowed.shownId() !== id) history.replaceState(history.state, "", `/kanban/${windowed.shownId()}${location.search}`);
    }, (e) => {
      say(e.message);
      history.replaceState(null, "", `/kanban/${location.search}`);
    });
  }

  async function route() {
    const id = addressed();
    if (id) { if (!windowed.shows(id)) openTask(id, false); return; }
    // Back with an edit not saved: asked; kept, the window keeps its address
    if ($("taskbox").open && !await windowed.tryClose()) history.pushState({ win: true }, "", `/kanban/${windowed.shownId()}${location.search}`);
  }
  addEventListener("popstate", route);

  $("taskbox").addEventListener("close", () => {
    if (!addressed()) return;
    if (history.state?.win) history.back();
    else history.replaceState(null, "", `/kanban/${location.search}`);
  });
  return { openTask, route };
}
