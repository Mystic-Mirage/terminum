// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A task that needs a human (terminum-kanban.md AC-117): a line as a waiting agent's, on both pages,
// and how many on Kanban in the page's head. terminumd says which; a closed line stays closed here.
import { icon } from "./icons.js";

const SEEN = "tm.taskseen";

function kept() {
  try { return new Set(JSON.parse(localStorage.getItem(SEEN) ?? "[]")); } catch { return new Set(); }
}

/** Lines in `box`; `open(id)` shows a task. */
export function taskLines(box, open) {
  let seen = kept(), last = [];
  const keep = () => { try { localStorage.setItem(SEEN, JSON.stringify([...seen])); } catch { /* private mode: this page's */ } };
  const said = (l) => `${l.id}|${l.words}`;

  function line(l) {
    const c = document.createElement("div");
    c.className = "ask taskline";
    c.dataset.task = l.id;
    const go = document.createElement("button");
    go.type = "button";
    go.className = "go";
    const failed = l.words.includes(" failed");
    const mark = document.createElement("span");
    mark.className = `state ${failed ? "failed" : "waiting"}`;
    mark.append(icon(failed ? "triangle-alert" : "circle-help", 14));
    const text = document.createElement("span");
    text.className = "what";
    text.textContent = l.words;
    go.append(mark, text);
    go.title = `Open ${l.id}`;
    go.addEventListener("click", () => { shut(l); open(l.id); });
    const x = document.createElement("button");
    x.type = "button";
    x.className = "x";
    x.setAttribute("aria-label", "Close this line");
    x.append(icon("x", 14));
    x.addEventListener("click", () => shut(l));
    c.append(go, x);
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
        const n = document.createElement("span");
        n.className = "taskneed";
        n.textContent = String(m.need);
        n.title = `${m.need} task${m.need === 1 ? "" : "s"} wait for you or failed`;
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
