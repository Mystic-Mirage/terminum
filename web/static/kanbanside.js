// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The chosen project's sessions beside the board's columns, and the session row on a card
// (terminum-kanban.md AC-134, AC-135): the session list's own code draws both; a press opens the session.
import { renderNav, row, setTaskMarks } from "./nav.js";
import { ordered } from "./reorder.js";
import { sessionUrl } from "./tasklink.js";
import { $, el, iconButton, read, write } from "./kanbanutil.js";

const DEFAULT = "default", HIDDEN = "board.sessionsHidden";
const openSession = (key) => { location.href = sessionUrl(key); };

/** Its place, made here with the columns: a phone's swipe snaps to what stands first. */
export function sessionsPage() {
  const col = el("aside", "col"), head = el("h2", "colhead"), count = el("span", "count"), list = el("div");
  col.id = "sidecol";
  col.setAttribute("aria-label", "Sessions");
  count.id = "sidecount";
  list.id = "sessions";
  head.append(el("span", null, "Sessions"), count);
  col.append(head, list);
  return col;
}

/** `chosen()`: the projects shown; `onCount(n)`: the rows now; a task's mark opens its window by `openTask`. */
export function sessionsColumn({ chosen, openTask, onCount }) {
  let groups = [], projects = [], order = [], marks = {}, drawn = "";
  const collapsed = new Set(read("collapsed") ?? []);   // the session list's: one list, one memory
  const list = $("sessions");

  function draw() {
    const picked = chosen();
    const now = JSON.stringify([groups, projects, order, marks, picked, [...collapsed]]);
    if (now === drawn) return;
    drawn = now;
    renderNav(list, ordered(groups, read("groupOrder") ?? []), null, "", openSession, collapsed, toggle, new Map(), null, null, null,
      { projects, order, empty: "No sessions here.", show: (project) => !picked.length || picked.includes(project ? project.id : DEFAULT) });
    const n = list.querySelectorAll(".item").length;
    $("sidecount").textContent = String(n);
    onCount(n);
  }

  function toggle(key) {
    if (!collapsed.delete(key)) collapsed.add(key);
    write("collapsed", [...collapsed]);
    draw();
  }

  // a task's mark stays on this page: its window, no page loading (nav.js's own click goes to its address)
  list.addEventListener("click", (e) => {
    const mark = e.target.closest?.(".taskmark");
    if (!mark) return;
    e.stopPropagation();
    openTask(mark.textContent);
  }, true);

  // hidden on a wide window, remembered; a phone's is a page of the swipe (kanban.css)
  const board = $("board");
  const key = iconButton("icon", "panel-left", "Sessions beside the columns", "Show or hide the sessions beside the columns", () => hide(!board.classList.contains("noside")));
  const hide = (on) => {
    board.classList.toggle("noside", on);
    key.setAttribute("aria-pressed", String(!on));
    write(HIDDEN, on);
  };
  key.id = "sidetoggle";
  $("boardacts").prepend(key);
  hide(!!read(HIDDEN));

  return {
    draw,
    layout(g) { groups = g; draw(); },
    projects(items, ids) { projects = items; order = ids; draw(); },
    marks(m) { setTaskMarks(m); marks = m; draw(); },
  };
}

/** A card's strip: its session's own row and what the agent says it does now; `project` colors its bar. */
export function sessionStrip(item, project) {
  const strip = el("ul", "csession");
  const li = row(item, null, openSession, false, false, new Map(), { project: project?.color >= 0 ? project : null });
  if (item.plan?.doing && !item.plan.stale) {
    const doing = el("span", "cdoing", item.plan.doing);
    doing.title = item.plan.doing;
    li.querySelector(".txt").append(doing);
  }
  strip.append(li);
  strip.addEventListener("click", (e) => e.stopPropagation());   // the session, not the task's window
  return strip;
}
