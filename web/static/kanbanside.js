// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The session list on the board, and the session row on a card (terminum-kanban.md AC-134, AC-135):
// the session page's widget, its code and its markup: a press opens the session on its page. Only its
// foot stays there; it shows the projects the board shows.
import { sessionDrawer } from "./drawer.js";
import { listTools } from "./listtools.js";
import { colored, markedSessions } from "./listparts.js";
import { findItem, renderNav, row, setTaskMarks } from "./nav.js";
import { arranged, bindReorder, listOrderer, ordered } from "./reorder.js";
import { rowMenus, saveOrder } from "./rowmenu.js";
import { sessionUrl } from "./tasklink.js";
import { copyText } from "./touchcopy.js";
import { $, DEFAULT, el, read, say, write } from "./kanbanutil.js";

let opened = null;                                 // the session a press last opened
const openSession = (key) => { opened = key; location.href = sessionUrl(key); };
const store = { get: (k, d) => read(k) ?? d, set: write };

const copy = async (got) => say((await copyText(got)).said);

/** `chosen()`: the projects shown; a task's mark opens its window by `openTask`; `send(m)`: to terminumd. */
export function boardSessions({ chosen, openTask, send, narrow }) {
  const st = { groups: [], projects: [], listOrder: [] };
  const list = $("sessions"), collapsed = new Set(store.get("collapsed", []));   // the session page's: one list, one memory
  const marks = new Map();
  let drawn = "", lastLines = 0;
  const drawnGroups = () => ordered(st.groups, store.get("groupOrder", []));
  const item = (id) => findItem(st.groups, id)?.item ?? null;
  const clone = (it) => send({ t: "clone", id: it.id });
  const tools = listTools({ send, store, groups: () => st.groups });
  const orderList = listOrderer(st, () => arranged(drawnGroups(), st.projects, st.listOrder).map((p) => p.id), send, () => draw(true));
  const menus = rowMenus({ list, send, item, projects: () => st.projects, clone, orderList, lastLines: () => lastLines, copy, say,
    groups: () => st.groups,
    keepFolded: (from, to) => { if (collapsed.has(from)) { collapsed.add(to); store.set("collapsed", [...collapsed]); } } });
  const reorder = bindReorder(list, (kind, parent, ids) => {
    if (kind === "part") orderList(ids);
    else saveOrder(send, parent, ids);
  }, () => draw(true), menus.held);

  function draw(anew = false) {
    if (reorder.dragging()) return;                // the next layout redraws it
    const picked = chosen(), marked = markedSessions(marks.values());
    const now = JSON.stringify([st, picked, [...collapsed], $("filter").value, [...marked]]);
    if (!anew && now === drawn) return;
    drawn = now;
    renderNav(list, drawnGroups(), null, $("filter").value, openSession, collapsed, toggle, marked, tools.hostMenu, copy, tools.importTmux, {
      projects: st.projects, order: st.listOrder, onClone: clone, onProjectNew: (p, last) => menus.newInProject(p, last?.id),
      onGroupNew: (g, last) => (last ? clone(last) : send({ t: "new", group: g.wid })), onProjectMenu: menus.headMenu, onProjectRename: menus.rename,
      onGroupMenu: menus.groupMenu, onReconnect: tools.reconnect, onTask: openTask, empty: "No sessions here.",
      show: (project) => !picked.length || picked.includes(project ? project.id : DEFAULT) });
  }

  function toggle(key) {
    if (!collapsed.delete(key)) collapsed.add(key);
    store.set("collapsed", [...collapsed]);
    draw();
  }

  sessionDrawer({ narrow, store, drawNav: () => draw(), hiddenKey: "board.sessionsHidden" });
  // the row's Plan: its pane is on the session page
  document.addEventListener("planopen", () => { if (opened) location.href = sessionUrl(opened, "plan"); });

  /** What terminumd says for the list. */
  function seen(m) {
    menus.seen(m);
    tools.seen(m);
    switch (m.t) {
      case "layout": st.groups = m.groups ?? []; tools.layoutCame(st.groups); draw(); return;
      case "projects": st.projects = m.items ?? []; st.listOrder = m.order ?? []; draw(); return;
      case "taskmarks": setTaskMarks(m); draw(true); return;
      case "marks": marks.clear(); for (const i of m.items ?? []) marks.set(i.id, i); draw(); return;
      case "mark": marks.set(m.item.id, m.item); draw(); return;
      case "unmark": marks.delete(m.id); draw(); return;
      case "settings": lastLines = m.page?.lastLines ?? 0; return;
      case "note": say(m.msg); return;
      case "error": if (m.msg && !tools.formError(m.msg)) say(m.msg); return;
    }
  }
  /** Asked as the list's own menu asks. */
  const deleteProject = (id) => { const p = st.projects.find((x) => x.id === id); if (p) menus.deleteProject(p); };
  return { draw, seen, deleteProject };
}

/** A card's strip: its session's own row and what the agent says it does now; `project` colors its bar. */
export function sessionStrip(item, project) {
  const strip = el("ul", "csession");
  const li = row(item, null, openSession, false, false, new Map(), { project: colored(project) ? project : null });
  if (item.plan?.doing && !item.plan.stale) {
    const doing = el("span", "cdoing", item.plan.doing);
    doing.title = item.plan.doing;
    li.querySelector(".txt").append(doing);
  }
  strip.append(li);
  strip.addEventListener("click", (e) => e.stopPropagation());   // the session, not the task's window
  return strip;
}
