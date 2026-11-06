// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The session list's menus: a row's on a right click or a long press (Close Session, Clone, Add to Project,
// Remove from Project, Mark as Unread or Read, Show Last 300 Lines (AC-96), Diagnostics (AC-100), Split Below, Right and Left (its quick shell, AC-88),
// Move to the Top, to the Bottom, Up, Down, Move to Session…; AC-07, AC-92, AC-93), and a project's, on its ⋯ or a right click on its header (New Window, Rename, the same moves among the
// projects, Delete Project, Close Project; AC-92), and a tmux session's group's, the same way (New Window,
// moves among the groups, Close Group; AC-07). A move reorders the row among its group's: a project's
// sessions in the project, a tmux session's windows in tmux; a project among the projects. Each is modal (a <dialog>): the
// page is shaded and takes no press while it is open; a press outside it closes it and does
// nothing else; Esc closes it.
import { askConfirm, askName, closeSession } from "./navtools.js";
import { icon } from "./icons.js";
import { inside, placeModal } from "./fabpane.js";
import { floats } from "./fold.js";
import { groupKey, groupRows, projectColor } from "./nav.js";
import { lastLinesView } from "./lastlines.js";
import { diagnostics } from "./diag.js";

const $ = (id) => document.getElementById(id);
const HOLD = 500;                // ms a finger rests before its menu opens
// each item's icon, by its action (icons.js); a destructive one's is a red ×
const MARKS = { clone: "copy-plus", project: "folder-plus", unproject: "folder-minus", plan: "list-checks", lastlines: "history",
  diag: "activity", top: "arrow-up-to-line", up: "arrow-up", down: "arrow-down", bottom: "arrow-down-to-line", movewin: "arrow-right-left",
  close: "x", new: "plus", newproject: "plus", rename: "pencil", ungroup: "ungroup", back: "chevron-left", session: "terminal",
  paste: "clipboard", copy: "copy", undo: "undo-2", redo: "redo-2" };
// the splits of the view (AC-88), side by side: what each makes, drawn
const SPLITS = [["Below", "bottom"], ["Right", "right"], ["Left", "left"]];

/** A finger resting on what `at(e)` finds in `el` (and not moving) for HOLD calls
 *  onHold(found, x, y); the click its lift makes is dropped. */
export function longPress(el, at, onHold) {
  let hold = null, held = false;
  el.addEventListener("pointerdown", (e) => {
    const found = e.pointerType === "touch" && at(e);
    if (!found) return;
    held = false;
    hold = { x: e.clientX, y: e.clientY, t: setTimeout(() => { held = true; onHold(found, hold.x, hold.y); }, HOLD) };
  });
  const cancel = () => { if (hold) clearTimeout(hold.t); hold = null; };
  el.addEventListener("pointermove", (e) => { if (hold && Math.hypot(e.clientX - hold.x, e.clientY - hold.y) > 10) cancel(); });
  for (const ev of ["pointerup", "pointercancel", "pointerleave"]) el.addEventListener(ev, cancel);
  el.addEventListener("click", (e) => { if (held) { held = false; e.preventDefault(); e.stopImmediatePropagation(); } }, true);
}

/** A list's new order (`ul`'s row ids): a project's sessions (AC-92), else a tmux session's windows. */
export function saveOrder(send, ul, ids) {
  if (ul.dataset.project) send({ t: "project", action: "members", project: ul.dataset.project, keys: ids });
  else if (ul.dataset.wid) send({ t: "reorder", group: ul.dataset.wid, ids });
}

/** list: #sessions; item(id): the row's layout item; projects(): terminumd's; clone(item);
 *  groups(): the layout's; split(item, side): its quick shell below, right or left;
 *  keepFolded(from, to): a renamed group's fold key `to` folded when `from` was;
 *  lastLines(): the lines terminumd keeps of a session (0: none); copy(text). */
export function rowMenus({ list, send, item, projects, clone, orderList, groups, split, keepFolded, lastLines, copy, say }) {
  const kept = lastLinesView({ send, copy, title: (id) => item(id)?.title });
  const diag = diagnostics({ send, show: kept.showText, say, item });
  const box = $("rowmenu");
  const hide = () => {
    if (!box.open) return;
    box.close();
    if (back?.isConnected && box.contains(document.activeElement)) back.focus({ preventScroll: true });
  };
  let openedAt = -Infinity, back = null;           // when a finger opened it; what had the focus
  const button = (label, action, run, enabled = true, mark = MARKS[action]) => {
    const b = document.createElement("button");
    b.type = "button"; b.textContent = label; b.dataset.action = action; b.disabled = !enabled;
    b.setAttribute("role", "menuitem");
    if (mark) b.prepend(icon(mark, 16));
    // the lift of the finger that opened it is no press of what is under it now
    b.onclick = (e) => { e.stopPropagation(); if (performance.now() - openedAt > 350) run(); };
    b.addEventListener("mousedown", (e) => e.preventDefault());   // the input keeps the keys (a menu opened with them, open)
    return b;
  };
  /** The menu's items under a head: what it is for, and × to close it. */
  function fill(title, ...items) {
    const head = document.createElement("div");
    head.className = "mhead";
    const t = document.createElement("span");
    t.textContent = title;
    const x = button("×", "x", hide);
    x.title = "Close this menu";
    x.setAttribute("aria-label", "Close this menu");
    x.removeAttribute("role");
    head.append(t, x);
    box.replaceChildren(head, ...items);
  }
  /** keepKeys: the field with the keys keeps them (the on-screen keyboard stays up, the sheet with it):
   *  opened without showModal (it takes the focus and makes the page inert), over a sheet's shade. */
  function open(x, y, byFinger = false, keepKeys = false) {
    const keep = keepKeys && floats() && !box.open;
    if (!box.open) { back = document.activeElement; if (keep) box.setAttribute("open", ""); else box.showModal(); }
    openedAt = byFinger ? performance.now() : -Infinity;
    const w = box.offsetWidth, h = box.offsetHeight;
    if (!placeModal(box, keep ? hide : null, !keep)) {   // a computer's: at the pointer
      box.style.left = `${inside(x, w, innerWidth)}px`;
      box.style.top = `${inside(y, h, innerHeight)}px`;
    }
    // never on Close Session, a row's first: Enter after opening never asks to close it
    if (!keep) box.querySelector("button:not(:disabled):not([data-action=x]):not([data-action=close])")?.focus({ preventScroll: true });
  }

  /** The rows beside `li` in its list, and where its order goes. */
  function siblings(li) {
    const ul = li.parentElement;
    const ids = [...ul.children].map((c) => c.querySelector(":scope > .item")?.dataset.id).filter(Boolean);
    const save = (order) => saveOrder(send, ul, order);
    return { ids, save, movable: !!(ul.dataset.project || ul.dataset.wid) };
  }

  function rowMenu(li, x, y, byFinger = false) {
    const id = li.querySelector(":scope > .item")?.dataset.id, it = id && item(id);
    if (!it) return;
    const mine = projects().find((p) => p.members.includes(id));
    const { ids, save, movable } = siblings(li), i = ids.indexOf(id), last = ids.length - 1;
    const move = (to) => { const o = ids.filter((k) => k !== id); o.splice(to, 0, id); save(o); hide(); };
    fill(it.title,
      button("Close Session", "close", () => {
        hide();
        closeSession(it, send);
      }),
      document.createElement("hr"),
      button("Clone", "clone", () => { hide(); clone(it); }),
      button("Add to Project…", "project", () => projectMenu(it, mine, x, y)),
      ...(mine ? [button(`Remove from ${mine.name}`, "unproject", () => { hide(); send({ t: "project", action: "remove", id }); })] : []),
      // as if nobody had opened it since its agent finished, on every page, until it is shown again (AC-93)
      // the Plan pane is the shown session's (AC-97): shown first, then opened
      button("Plan and Notes…", "plan", () => { hide(); li.querySelector(":scope > .item").click(); document.dispatchEvent(new Event("planopen")); }),
      button(it.unread ? "Mark as Read" : "Mark as Unread", "unread", () => { hide(); send({ t: "unread", id, on: !it.unread }); }, true, it.unread ? "mail-open" : "mail"),
      // what it showed before its tmux restarted, kept by terminumd (AC-96)
      ...(lastLines() > 0 ? [button(`Show Last ${lastLines()} Lines`, "lastlines", () => { hide(); kept.open(id, lastLines()); })] : []),
      // what terminumd and tmux see of the pane, for a bug report (AC-100)
      button("Diagnostics…", "diag", () => { hide(); diag.open(id); }),
      document.createElement("hr"),
      // the view's splits are the session's quick shell (AC-88): it is shown, its shell beside it
      splits(it),
      document.createElement("hr"),
      button("Move to the Top", "top", () => move(0), movable && i > 0),
      button("Move Up", "up", () => move(i - 1), movable && i > 0),
      button("Move Down", "down", () => move(i + 1), movable && i < last),
      button("Move to the Bottom", "bottom", () => move(last), movable && i < last),
      ...(sessionsFor(it).length ? [button("Move to Session…", "movewin", () => moveMenu(it, x, y))] : []));
    box.dataset.for = id;
    open(x, y, byFinger);
  }

  /** Split Below, Right and Left in one row, each a drawing of the split over its side's word;
   *  a phone's has Below only. */
  function splits(it) {
    const row = document.createElement("div");
    row.className = "splits";
    row.setAttribute("role", "group");
    row.setAttribute("aria-label", "Split");
    row.append(...SPLITS.map(([word, side]) => {
      const b = button(word, `split-${side}`, () => { hide(); split(it, side); });
      b.prepend(icon(`split-${side}`, 20));
      b.title = b.ariaLabel = `Split ${word}`;
      if (side !== "bottom") b.classList.add("wide-only");
      return b;
    }));
    return row;
  }

  /** The other sessions of the tmux server `it`'s window is in: where it may move (AC-07). */
  function sessionsFor(it) {
    const own = groups().find((g) => g.items.some((x) => x.id === it.id));
    if (!own || own.kind !== "tmux" || !own.wid) return [];
    return groups().filter((g) => g.kind === "tmux" && g.wid && g.wid !== own.wid && (g.host ?? null) === (own.host ?? null) && g.server === own.server);
  }

  function moveMenu(it, x, y) {
    const again = () => { const li = list.querySelector(`.item[data-id="${CSS.escape(it.id)}"]`)?.closest("li"); if (li) rowMenu(li, x, y); else hide(); };
    fill("Move to Session", button("Back", "back", again), document.createElement("hr"),
      ...sessionsFor(it).map((g) => {
        const b = button(g.label, "session", () => { hide(); send({ t: "move", id: it.id, session: g.wid }); });
        b.dataset.session = g.wid;
        return b;
      }));
    open(x, y);
  }

  function projectMenu(it, mine, x, y) {
    const pick = (project) => { hide(); send({ t: "project", action: "add", id: it.id, ...(project ? { project } : {}) }); };
    // a new project is named at once (its suggestion: the lowest "Group N" free)
    const named = async () => {
      hide();
      const taken = new Set(projects().map((p) => p.name));
      let n = 1;
      while (taken.has(`Group ${n}`)) n++;
      const name = await askName("Name the new project", `Group ${n}`, "Create");
      if (name) send({ t: "project", action: "add", id: it.id, name });
    };
    const others = projects().filter((p) => p !== mine).map((p) => {
      const b = button(p.name, "join", () => pick(p.id));
      const mark = icon("group", 15);
      mark.classList.add("picon");
      mark.style.setProperty("--proj", projectColor(p));
      b.prepend(mark);
      return b;
    });
    const again = () => { const li = list.querySelector(`.item[data-id="${CSS.escape(it.id)}"]`)?.closest("li"); if (li) rowMenu(li, x, y); else hide(); };
    fill("Add to Project", button("Back", "back", again),
      document.createElement("hr"), ...others, button("New project…", "newproject", named));
    open(x, y);
  }

  async function rename(p) {
    const name = await askName(`Rename ${p.name}`, p.name);
    if (name && name !== p.name) send({ t: "project", action: "rename", project: p.id, name });
  }

  /** A project's menu, under its ⋯ `b` or at x, y (a right click on its header): a new session in
   *  its folder, Rename, moves among the projects as the list shows them, Delete Project (asks; its
   *  sessions stay, back in their tmux groups, its tasks in Default), Close Project (its sessions
   *  end, after asking; the project stays). */
  /** A new window of project `p`, after its session `last`; with none, on its host in its folder,
   *  asked for once (AC-110). */
  async function newInProject(p, last) {
    if (last) return send({ t: "clone", id: last, project: p.id });
    const dir = p.dir ? "" : (await askName(`The folder of ${p.name}`, "", "Open", 4096))?.trim();
    if (dir === undefined || (!p.dir && !dir)) return;
    send({ t: "projectwindow", project: p.id, dir });
  }

  function plural(n, one) { return n === 1 ? `1 ${one}` : `${n} ${one}s`; }

  /** Deleted only once asked (AC-110): nothing closes, nothing is lost. */
  async function deleteProject(p, sessions) {
    hide();
    const them = `Its ${plural(sessions, "session")} and ${plural(p.tasks ?? 0, "task")} move to Default; nothing is closed.`;
    if (await askConfirm(`Delete the project ${p.name}? ${them}`, "Delete Project")) send({ t: "project", action: "delete", project: p.id });
  }

  function headMenu(p, b, x, y) {
    if (b && box.open && box.dataset.for === p.id) return hide();
    const here = p.members.filter((k) => item(k));
    const { i, last, move } = mover(`p:${p.id}`);
    const closeAll = async () => {
      hide();
      const n = here.length, them = n === 1 ? "its session" : `its ${n} sessions`;
      if (await askConfirm(`Close the project ${p.name} and ${them}? The programs running in them end.`, "Close Project")) for (const id of here) send({ t: "kill", id });
    };
    fill(p.name,
      button("New Window", "new", () => { hide(); newInProject(p, here.at(-1)); }),
      button("Rename…", "rename", () => { hide(); rename(p); }),
      document.createElement("hr"),
      button("Move to the Top", "top", () => move(0), i > 0),
      button("Move Up", "up", () => move(i - 1), i > 0),
      button("Move Down", "down", () => move(i + 1), i >= 0 && i < last),
      button("Move to the Bottom", "bottom", () => move(last), i >= 0 && i < last),
      document.createElement("hr"),
      button("Delete Project…", "deleteproject", () => deleteProject(p, here.length)),
      button("Close Project…", "close", closeAll, here.length > 0));
    box.dataset.for = p.id;
    const r = b?.getBoundingClientRect();
    open(r ? r.right - 210 : x, r ? r.bottom + 4 : y);
  }

  /** Where `id` stands among the list's projects and groups as drawn, and its move there. */
  function mover(id) {
    const ids = [...list.querySelectorAll("section.grp[data-part]")].map((s) => s.dataset.part), i = ids.indexOf(id), last = ids.length - 1;
    return { ids, i, last, move: (to) => { hide(); const o = ids.filter((k) => k !== id); o.splice(to, 0, id); orderList(o); } };
  }

  /** A tmux session's group's menu, under its ⋯ `b` or at x, y: a new window after its last row,
   *  Rename (its tmux session; its place and fold kept), moves in the list (every page's, as a
   *  drag), Close Group (its windows end, after asking). */
  function groupMenu(wid, b, x, y) {
    if (b && box.open && box.dataset.for === wid) return hide();
    const sec = [...list.querySelectorAll("section.grp[data-wid]")].find((s) => s.dataset.wid === wid);
    const g = groups().find((x) => x.wid === wid);
    if (!sec || !g) return;
    const label = sec.querySelector("h2 .label")?.textContent ?? wid;
    // from the list's model, not its rows: a collapsed group draws none (and a filter only some)
    const rows = groupRows(groups(), projects(), wid).filter((id) => item(id));
    const { ids, i, last, move } = mover(`g:${wid}`);
    const rename = async () => {
      hide();
      const name = (await askName(`Rename ${label}`, label))?.trim();
      if (!name || name === label) return;
      const to = `${wid.slice(0, wid.indexOf("/") + 1)}${name}`;
      keepFolded?.(groupKey(g), groupKey({ ...g, label: name }));
      orderList(ids.flatMap((k) => (k === `g:${wid}` ? [`g:${to}`, k] : [k])));   // the renamed group keeps its place, its old name beside it until it is one (a name refused: nothing moved)
      send({ t: "renamegroup", group: wid, name });
    };
    const closeAll = async () => {
      hide();
      const n = rows.length, them = n === 1 ? "its session" : `its ${n} sessions`;
      if (await askConfirm(`Close the group ${label} and ${them}? The programs running in them end.`, "Close Group")) for (const id of rows) send({ t: "kill", id });
    };
    fill(label,
      button("New Window", "new", () => { hide(); clone(item(rows.at(-1))); }, rows.length > 0 && !!g.new),
      button("Rename…", "rename", rename),
      document.createElement("hr"),
      button("Move to the Top", "top", () => move(0), i > 0),
      button("Move Up", "up", () => move(i - 1), i > 0),
      button("Move Down", "down", () => move(i + 1), i < last),
      button("Move to the Bottom", "bottom", () => move(last), i < last),
      document.createElement("hr"),
      button("Close Group…", "close", closeAll, rows.length > 0));
    box.dataset.for = wid;
    const r = b?.getBoundingClientRect();
    open(r ? r.right - 210 : x, r ? r.bottom + 4 : y);
  }

  // a right click, the keyboard's menu key or ⇧F10 on a row (at the row), or a finger resting
  // on it (below the finger; its click that follows is dropped)
  list.addEventListener("contextmenu", (e) => {
    const head = e.target.closest("section[data-pid] > h2"), p = head && projects().find((x) => x.id === head.parentElement.dataset.pid);
    if (p) {                                       // a project's header: its menu
      e.preventDefault();
      const r = head.getBoundingClientRect(), keys = !e.clientX && !e.clientY;
      return headMenu(p, null, keys ? r.left + 24 : e.clientX, keys ? r.bottom : e.clientY);
    }
    const gh = e.target.closest("section.grp[data-wid] > h2");
    if (gh) {                                      // a tmux session's group's header: its menu
      e.preventDefault();
      const r = gh.getBoundingClientRect(), keys = !e.clientX && !e.clientY;
      return groupMenu(gh.parentElement.dataset.wid, null, keys ? r.left + 24 : e.clientX, keys ? r.bottom : e.clientY);
    }
    const li = e.target.closest(".item:not(.child)")?.closest("li");
    if (!li) return;
    e.preventDefault();
    const r = li.getBoundingClientRect(), keys = !e.clientX && !e.clientY;
    rowMenu(li, keys ? r.left + 24 : e.clientX, keys ? r.bottom : e.clientY);
  });
  box.addEventListener("keydown", (e) => {          // ↑ ↓ through its items, as a menu
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = [...box.querySelectorAll("button:not(:disabled)")], i = items.indexOf(document.activeElement);
    items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
  });
  // a row that can move: its menu is a lift let go (reorder.js); one that cannot (a filter shows): a hold
  /** A finger lifted `li` and let go at x, y without moving, or held one that cannot move. */
  const held = (li, x, y) => rowMenu(li, x - 24, y + 28, true);
  longPress(list, (e) => !e.target.closest(".clone") && e.target.closest(".item:not(.child)")?.closest("li:not([data-drag])"), held);
  // a press elsewhere closes it (pointerdown: iOS sends no click for a tap on a plain area)
  // a press on the shade (the dialog itself, outside its box) closes it, and reaches nothing
  // under it; the lift of the finger that opened it does not
  box.addEventListener("click", (e) => {
    if (e.target !== box) return;
    const r = box.getBoundingClientRect(), inside = e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
    if (!inside && performance.now() - openedAt > 350) hide();
  });
  box.addEventListener("cancel", (e) => { e.preventDefault(); hide(); });   // Esc: the focus goes back too
  /** A menu of `items` ([label, run]) at x, y (the input's Paste, Undo and Redo); byFinger: a
   *  finger still down opened it; keepKeys: the input has the keys and keeps them (open). */
  function show(items, x, y, byFinger = false, keepKeys = false) {
    fill("Edit", ...items.map(([label, run]) => button(label, label.toLowerCase(), () => { hide(); run(); })));
    box.dataset.for = "";
    open(x, y, byFinger, keepKeys);
  }
  return { headMenu, groupMenu, newInProject, hide, show, rename, held, seen: (m) => { kept.seen(m); diag.seen(m); } };
}
