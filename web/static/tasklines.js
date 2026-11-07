// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A task that needs a human (terminum-kanban.md AC-117): a line as a waiting agent's, on both pages,
// and how many on Kanban in the page's head. terminumd says which; a closed line stays closed here.
import { icon } from "./icons.js";
import { el, iconButton, plural, read, write } from "./listparts.js";

const SEEN = "tm.taskseen";

/** Lines in `box`; `open(id)` shows a task. */
export function taskLines(box, open) {
  let seen = new Set(read(SEEN) ?? []), last = [];
  const keep = () => write(SEEN, [...seen]);
  const said = (l) => `${l.id}|${l.words}`;

  function line(l) {
    const c = el("div", "ask taskline");
    c.dataset.task = l.id;
    const go = el("button", "go"), mark = el("span", `state ${l.failed ? "failed" : "waiting"}`);
    go.type = "button";
    mark.append(icon(l.failed ? "triangle-alert" : "circle-help", 14));
    go.append(mark, el("span", "what", l.words));
    go.title = `Open ${l.id}`;
    go.addEventListener("click", () => { shut(l); open(l.id); });
    c.append(go, iconButton("x", "x", "Close this line", "Close this line", () => shut(l)));
    return c;
  }

  function shut(l) {
    seen.add(said(l));
    keep();
    draw();
  }

  function draw() {
    const due = last.filter((l) => !seen.has(said(l)));
    for (const c of box.querySelectorAll(".taskline")) c.remove();
    box.append(...due.map(line));
    box.hidden = !box.childElementCount;
  }

  return {
    /** terminumd's `taskmarks`: the lines due, and how many tasks need a human. */
    came(m) {
      last = m.lines ?? [];
      const now = new Set(last.map(said));
      const was = seen.size;
      seen = new Set([...seen].filter((s) => now.has(s)));   // asks anew: said anew
      if (seen.size !== was) keep();
      draw();
      for (const a of document.querySelectorAll('a[href="/kanban/"]')) {
        a.querySelector(".taskneed")?.remove();
        if (!m.need) continue;
        const n = el("span", "taskneed", m.need);
        n.title = `${plural(m.need, "task")} wait for you or failed`;
        a.append(n);
      }
    },
    /** A task's window opened: its line goes. */
    opened(id) {
      for (const l of last.filter((x) => x.id === id)) seen.add(said(l));
      keep();
      draw();
    },
  };
}
