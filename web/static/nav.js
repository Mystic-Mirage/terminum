// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Session navigator: tmux sessions (a host's session: the host) as groups, their windows and
// panes as rows.
// A row: its folder's bar (full for the session shown, pulsing while its agent works), an orange
// strip when it has bookmarks (AC-57), the window
// number and title, then host · program · folder · titles, and a coding agent's state in a
// column of its own.
import { folderHues, folderKey } from "./dircolors.js";
import { subOf } from "./follow.js";
import { icon } from "./icons.js";
import { planPct, planTrack } from "./planrow.js";
import { textForm } from "./render.js";

// A coding agent's state: waiting for the user, failed; working is its bar's pulse (folderBar), done nothing
// but a quiet terminal mark while a shell, a monitor or a task it started still runs (terminumd reads
// Claude Code's footer, AC-39).
const STATES = {
  waiting: { label: "Waiting for your answer", icon: "circle-help" },
  failed: { label: "Failed", icon: "triangle-alert" },
  background: { label: "Done; a shell, a monitor or a task it started still runs", icon: "terminal", size: 14 },
};
// Waiting sessions the user showed (AC-39): their ? goes at once, until they wait anew.
const seenWaits = new Set();
/** Marks the waiting rows of `groups` the user saw (`shown`: the session shown now) with `seen`;
 *  a row that stopped waiting is forgotten. */
export function seeWaits(groups, shown) {
  for (const it of (groups ?? []).flatMap((g) => g.items ?? [])) {
    if (it.state !== "waiting") { seenWaits.delete(it.id); continue; }
    if (it.id === shown) seenWaits.add(it.id);
    it.seen = seenWaits.has(it.id);
  }
}
/** A row's state as its mark shows it: a waiting one the user saw shows none, a done one whose
 *  shell still runs "background". */
export const shownState = (it) => (it.state === "waiting" && it.seen ? null : it.state === "done" && it.background ? "background" : it.state);

export function stateIcon(state, size) {
  const s = STATES[state];
  if (!s) return null;
  size ??= s.size ?? 16;
  const wrap = document.createElement("span");
  wrap.className = `state ${state}`;
  wrap.title = s.label;
  wrap.setAttribute("role", "img");
  wrap.setAttribute("aria-label", s.label);
  wrap.append(icon(s.icon, size));
  return wrap;
}

let hues = new Map();                  // each folder's hue, for the list as last drawn (AC-01)
// a coding agent's own mark (their owners' trademarks, outside terminum's licenses: NOTICE)
const MARKS = { claude: ["agents/claude.svg", "Claude Code"], codex: ["agents/codex.svg", "Codex"] };
// a program shown by its mark alone, by its name as tmux has it (no coding agent's input box: its own full screen)
const PROGRAM_MARKS = { opencode: ["agents/opencode.svg", "OpenCode"] };
const markOf = (item) => MARKS[item.agent] ?? PROGRAM_MARKS[[...(item.sub ?? []), item.title].find((p) => Object.hasOwn(PROGRAM_MARKS, p))];

/** A coding agent's name ("Claude Code"), undefined for another program. */
export const agentName = (agent) => MARKS[agent]?.[1];

/** What runs in a pane, as an icon: a coding agent's or OpenCode's mark, else the terminal's (also when a mark
 *  file was left out of the build). */
export function agentMark(item, size = 14) {
  const [src, name] = markOf(item) ?? [];
  const wrap = el("span", "agmark");
  wrap.title = name ?? "Terminal";
  const terminal = () => { wrap.classList.add("term"); return icon("terminal", Math.round(size * 0.72)); };   // white on a dark gray tile (style.css)
  if (!src) { wrap.append(terminal()); return wrap; }
  const img = Object.assign(document.createElement("img"), { src, alt: "", width: size, height: size, draggable: false });
  img.addEventListener("error", () => img.replaceWith(terminal()));
  wrap.append(img);
  return wrap;
}
let lastShown;                         // the session shown when the list was last drawn

/** The bar beside a session's two lines in its folder's color: at full strength for the session
 *  the page shows, half otherwise, pulsing while a coding agent in it works. (A dot read as "unread".) */
export function folderBar(item, shown = false, busy = false, project = null) {
  const d = el("span", "pbar" + (shown ? " on" : "") + (busy ? " busy" : ""));
  const key = folderKey(item), hue = key && hues.get(key);
  if (project) d.style.setProperty("--dot", projectColor(project));   // a project's rows: its color, not the folder's (AC-92)
  else if (hue != null) d.style.setProperty("--dot", `oklch(var(--dir-l) var(--dir-c) ${hue.toFixed(1)})`);
  d.title = (key ? `Folder ${key.split("\n")[1]}` : "No folder") + (busy ? ", working" : "");
  return d;
}

// Projects' colors (AC-92): ten hues far apart, in pastel (projects.css), so two projects never read as one
const PROJECT_HUES = [25, 145, 265, 85, 205, 325, 55, 175, 295, 115];
const hueColor = (h) => `oklch(var(--proj-l) var(--proj-c) ${h})`;
const projectHue = (p) => PROJECT_HUES[(p.color ?? 0) % PROJECT_HUES.length];
export const projectColor = (p) => hueColor(projectHue(p));

/** The hue of the `n`th group after the projects (AC-92): the palette's from its turn on, the first
 *  at least 60° from `above`'s (the group over it; null: none). */
export function groupHue(n, above) {
  const far = (h) => above === null || Math.abs(((h - above + 540) % 360) - 180) >= 60;
  const turn = PROJECT_HUES.map((_, i) => PROJECT_HUES[(n + i) % PROJECT_HUES.length]);
  return turn.find(far) ?? turn[0];
}

/** A row's second line (AC-01): host and folder first, then its titles; what runs there (the
 *  program, a quick shell's mark) at the right edge. */
function rowLine(item) {
  const sub = subOf(item), dir = sub.findIndex((p) => /^[~/]/.test(p));
  const progs = dir > 0 ? sub.slice(0, dir) : dir < 0 ? sub.slice(0, 1) : [];
  const rest = dir >= 0 ? sub.slice(dir) : sub.slice(1);
  const s = metaLine([...(item.host ? [item.host] : []), ...rest], !!item.host);
  if (progs.length) {
    const r = el("span", "progs");
    progs.forEach((p, i) => { if (i) r.append(el("span", "sep", "·")); r.append(el("span", ["prog", AGENT_WORDS[p]].filter(Boolean).join(" "), p)); });
    s.append(r);
  }
  return s;
}

const AGENT_WORDS = { claude: "ag-claude", codex: "ag-codex", opencode: "ag-opencode" };   // the program's own name, in its color

/** "a · b · c" as spans: the last part (a title, else the folder) shortens first, a program's
 *  name (a word right after the host) never. */
export function metaLine(parts, hostFirst) {
  const s = el("span", "s");
  const prog = hostFirst ? 1 : 0;
  parts.forEach((p, i) => {
    if (i) s.append(el("span", "sep", "·"));
    const cls = i === 0 && hostFirst ? "host" : i === parts.length - 1 ? "last" : i === prog && /^[\w.+-]+$/.test(p) ? "prog" : "";
    s.append(el("span", [cls, AGENT_WORDS[p]].filter(Boolean).join(" "), p));
  });
  return s;
}

/** The key a group's collapsed state is remembered by: its window or tmux session, and host. */
export const groupKey = (g) => `${g.kind}\n${g.label}${g.session ? " " + g.session : ""}\n${g.host ?? ""}`;

function matches(item, q) {
  if (!q) return true;
  return [item.title, item.host ?? "", ...subOf(item)].join(" ").toLowerCase().includes(q);
}

export function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = textForm(String(text));
  return e;
}

/** A group's count: its sessions (its rows, not their panes or quick shells under them), said in words. */
const sessionCount = (n) => el("span", "count", n ? `${n} session${n === 1 ? "" : "s"}` : "");   // a group without rows says its state instead

/** A small icon button of the list (a header's, a row's Clone): its class, icon, label for a screen reader, tooltip. */
function iconButton(cls, name, label, title, onClick) {
  const b = el("button", cls);
  b.type = "button";
  b.title = title;
  b.setAttribute("aria-label", label);
  b.append(icon(name, 14));
  b.addEventListener("click", (e) => { e.stopPropagation(); onClick(b); });
  return b;
}

function row(item, current, onPick, movable, child = false, marked = new Map(), opts = {}) {
  const li = document.createElement("li");
  if (movable) li.dataset.drag = "row";           // dragged by its body (reorder.js); not while a filter shows a subset
  const b = el("button", "item" + (child ? " child" : "") + (item.id === current ? " on" : ""));
  b.type = "button";
  b.dataset.id = item.id;
  if (item.id === current) b.setAttribute("aria-current", "true");
  const txt = el("span", "txt");
  const head = el("span", "head");
  const idx = el("span", "idx", child ? "" : item.index || "");
  if (item.index && !child) idx.title = item.host ?? "This machine";     // where the window runs
  head.append(idx, agentMark(item), el("span", "t", item.title));
  if (item.plan) head.append(planPct(item, onPick));
  txt.append(head);
  if (item.plan) txt.append(planTrack(item.plan));
  if (item.host || item.sub?.length) txt.append(rowLine(item));
  const st = el("span", "st");
  const clone = opts.onClone && !child;
  if (item.unread) {                               // its agent finished a turn nobody has looked at (AC-93): the row hatched (projects.css)
    b.classList.add("unread");
    b.title = "Unread: its agent finished";
    b.setAttribute("aria-description", b.title);
  }
  const mark = stateIcon(shownState(item));
  const said = item.why || item.tool;              // what its agent's hooks say it waits for or runs (AC-39)
  if (mark && said) { mark.title = `${mark.title}: ${said}`; mark.setAttribute("aria-label", mark.title); }
  if (mark) st.append(mark);
  if (item.subagents) {
    const n = el("span", "subs", String(item.subagents));
    n.title = `${item.subagents} subagent${item.subagents > 1 ? "s" : ""} running`;
    st.append(n);
  }
  // the bar's column holds the orange strip beside it: the row's grid keeps its three columns
  const bar = folderBar(item, item.id === current, item.state === "working", opts.project);
  // the bookmarks and reminders it shows (its project's, its own): a wavy orange and a wavy pink line beside the folder's bar
  const has = marked.get(item.id);
  if (has) {
    const both = el("span", "bars");
    both.append(bar);
    for (const [on, cls, title] of [[has.mark, "wave mark", "It has bookmarks"], [has.remind, "wave remind", "It has open reminders"]]) {
      if (!on) continue;
      const w = el("span", cls);
      w.title = title;
      both.append(w);
    }
    b.append(both, txt, st);
  } else b.append(bar, txt, st);
  b.addEventListener("click", () => onPick(item.id));
  li.append(b);
  // a new window beside it, in its folder (AC-07)
  if (clone) li.append(iconButton("clone", "copy-plus", `Clone ${item.title}`, "Clone: a new window in its folder", () => opts.onClone(item)));
  return li;
}

/** A host group's state, on a line under its name (AC-81): "" when attached, unless its record
 *  changed and is not used yet (AC-87); terminumd's `why` says "reconnecting (2 s)" or ssh's
 *  last line, which can be long. */
export function hostState(g) {
  if (g.state === "attached") return g.why || "";
  if (g.state === "reconnecting") return g.why || "reconnecting";
  if (g.state === "failed") return g.why || "failed";
  if (g.state === "off") return "off";
  if (g.state === "absent") return g.why || "not running";
  return "";
}

// A group's header collapses and expands it; while a filter is typed every match is shown. A
// host's group has a menu of its own (onHost(group, button)), also while it has no rows, and
// while its tmux runs other sessions an icon that offers them (onImport(group), AC-89).
// Projects (AC-92) come first, each its sessions with their panes and quick shells, which
// their tmux groups then leave out; every row has its Clone button (onClone(item)).
/** Which rows go where: a project's session takes its window's other panes and its quick shells
 *  with it. {byId, kids(top), taken}: taken, the ids a project lists (they leave their group). */
function membership(groups, projects) {
  const all = groups.flatMap((g) => g.items), win = (it) => it.id.slice(0, it.id.lastIndexOf(":") + 1) + it.window;
  const byId = new Map(all.map((it) => [it.id, it]));
  const under = new Map();                         // a window's first pane or a quick shell's pane -> what goes under it
  const firsts = new Map(all.filter((it) => !(it.pane > 1)).map((it) => [win(it), it]));
  for (const it of all) {
    const top = it.quickOf ? byId.get(it.quickOf) : it.pane > 1 ? firsts.get(win(it)) : null;
    if (top) under.set(top.id, [...(under.get(top.id) ?? []), it]);
  }
  const kids = (top) => under.get(top.id) ?? [];
  const taken = new Set(projects.flatMap((p) => p.members).flatMap((k) => [k, ...(byId.has(k) ? kids(byId.get(k)).map((it) => it.id) : [])]));
  return { byId, kids, taken };
}

/** The sessions (a window's first pane, not the panes and quick shells under it) the tmux group
 *  `wid` lists, in order, whether it is collapsed or filtered: its menu's New Window and Close Group. */
export function groupRows(groups, projects, wid) {
  const g = groups.find((x) => x.wid === wid);
  if (!g) return [];
  const { taken } = membership(groups, projects), left = g.items.filter((it) => !taken.has(it.id));
  return left.filter((it) => !(it.pane > 1) && !(it.quickOf && left.some((x) => x.id === it.quickOf))).map((it) => it.id);
}

export function renderNav(list, groups, current, query, onPick, collapsed, onToggle, marked = new Map(), onHost = null, onCopy = null, onImport = null,
  { projects = [], onClone = null, onProjectNew = null, onProjectMenu = null, onProjectRename = null, onGroupMenu = null } = {}) {
  const q = query.trim().toLowerCase();
  const frag = document.createDocumentFragment();
  // a session of the user's tmux on a host that is down waits for that host: the same state, the same look
  hues = folderHues(groups.flatMap((g) => g.items.map(folderKey)));
  const down = new Map(groups.filter((g) => g.host && !g.attached && g.state && g.state !== "attached").map((g) => [g.host, g.state]));
  let shown = 0;
  const { byId, kids, taken } = membership(groups, projects);
  const box = el("div", "projects");
  let above = null, tinted = 0;                    // the hue of the group drawn last; the other groups tinted so far
  for (const p of projects) {
    const tops = p.members.map((k) => byId.get(k)).filter(Boolean);
    if (!tops.length) continue;                    // its sessions closed: terminumd forgets it at its next read
    const rows = tops.filter((t) => [t, ...kids(t)].some((it) => matches(it, q)));
    if (q && !rows.length) continue;
    shown += rows.length;
    const key = `project\n${p.id}`, closed = !q && collapsed.has(key);
    const sec = el("section", "grp project");
    sec.dataset.pid = p.id;
    sec.style.setProperty("--proj", projectColor(p));
    above = projectHue(p);
    const h = el("h2");
    if (!q) h.dataset.drag = "project";
    const t = el("button", "toggle");
    t.type = "button";
    t.setAttribute("aria-expanded", String(!closed));
    t.addEventListener("click", () => onToggle(key));
    const count = sessionCount(tops.length);
    if (closed && tops.some((it) => it.id === current || kids(it).some((k) => k.id === current))) count.classList.add("on");
    const name = el("span", "label", p.name);
    if (onProjectRename) {                         // its name, double-clicked, is renamed (also ⋯ Rename…)
      name.title = "Double-click to rename";
      name.addEventListener("dblclick", (e) => { e.stopPropagation(); onProjectRename(p); });
    }
    t.append(icon("chevron-down", 14), name, count);
    const acts = el("span", "acts");               // a fixed column: the counts line up whatever buttons follow
    h.append(t, acts);
    if (onProjectNew) acts.append(iconButton("new", "plus", `New window in ${p.name}`, `A new window in ${p.dir || "its folder"}`, () => onProjectNew(p, tops.at(-1))));
    if (onProjectMenu) {
      const m = iconButton("hostbtn", "ellipsis", `Project ${p.name}`, `${p.name}: new session, rename, move, ungroup, close`, (b) => onProjectMenu(p, b));
      m.setAttribute("aria-haspopup", "true");
      acts.append(m);
    }
    sec.append(h);
    if (!closed) {
      const ul = el("ul");
      ul.dataset.project = p.id;
      for (const top of rows) {
        const li = row(top, current, onPick, !q, false, marked, { onClone, project: p });
        const ks = kids(top);
        if (ks.length) {
          const kl = el("ul", "kids");
          for (const k of ks) kl.append(row(k, current, onPick, false, true, marked, { project: p }));
          li.append(kl);
        }
        ul.append(li);
      }
      sec.append(ul);
    }
    box.append(sec);
  }
  if (box.children.length) frag.append(box);
  for (const g of groups) {
    const left = g.items.filter((it) => !taken.has(it.id)), items = left.filter((it) => matches(it, q));
    const own = g.host || g.attached;            // a host's group, or a session of the user's tmux (AC-89)
    if (!items.length && !(own && !q)) continue;
    shown += items.length;
    const key = groupKey(g);
    const closed = !q && collapsed.has(key);
    const gs = g.state === "absent" && g.host && down.has(g.host) ? down.get(g.host) : g.state;
    const sec = el("section", `grp tinted ${g.kind}` + (g.host ? " hostgrp" : "") + (gs && gs !== "attached" ? ` ${gs}` : ""));
    above = groupHue(tinted++, above);
    sec.style.setProperty("--proj", hueColor(above));
    if (g.wid) sec.dataset.wid = g.wid;
    const h = el("h2");
    if (g.wid && !q) h.dataset.drag = "group";
    const t = el("button", "toggle");
    t.type = "button";
    t.setAttribute("aria-expanded", String(!closed));
    t.addEventListener("click", () => onToggle(key));
    t.append(icon("chevron-down", 14), el("span", "label", g.label));
    if (g.session) t.append(el("span", "tsess", g.session));      // a tmux group: its session's name; the rows name the host
    else if (g.host && g.host !== g.label) t.append(el("span", "host", g.host));
    if (g.where) t.append(el("span", "where", g.where));
    // its sessions: the rows it lists (a project's are counted there), not the panes and quick shells under them
    const n = left.filter((it) => !(it.pane > 1) && !(it.quickOf && left.some((x) => x.id === it.quickOf))).length;
    const count = sessionCount(n);
    if (closed && items.some((it) => it.id === current)) count.classList.add("on");   // the shown session is inside
    t.append(count);
    const acts = el("span", "acts");
    h.append(t, acts);
    if (g.attach && onCopy) {                    // attach from another terminal (tmux -L … / ssh -t …)
      const c = el("button", "attach");
      c.type = "button";
      c.title = `Copy: ${g.attach}`;
      c.setAttribute("aria-label", `Copy the command that attaches to ${g.session ?? g.label}`);
      c.append(icon("copy", 13));
      c.addEventListener("click", (e) => { e.stopPropagation(); onCopy(g.attach); });
      acts.append(c);
    }
    if (g.host && !g.attached && g.others > 0 && onImport) {
      const n = el("button", "import");
      n.type = "button";
      n.title = `Attach tmux sessions running on ${g.host} (${g.others})`;
      n.setAttribute("aria-label", n.title);
      n.append(icon("import", 14));
      n.addEventListener("click", (e) => { e.stopPropagation(); onImport(g); });
      acts.append(n);
    }
    if (own && onHost) {
      const m = el("button", "hostbtn");
      m.type = "button";
      m.title = g.attached ? `${g.label}: detach from terminum` : `${g.host}: retry, off, on, remove`;
      m.setAttribute("aria-label", g.attached ? `Session ${g.label} of your tmux${g.host ? ` on ${g.host}` : ""}` : `Host ${g.host}`);
      m.setAttribute("aria-haspopup", "true");
      m.append(icon("ellipsis", 14));
      m.addEventListener("click", (e) => { e.stopPropagation(); onHost(g, m); });
      acts.append(m);
    } else if (g.wid && onGroupMenu) {             // a tmux session's group: its menu (as a right click on its header)
      const m = iconButton("hostbtn", "ellipsis", `Session group ${g.label}`, `${g.label}: new session, move, close`, (b) => onGroupMenu(g.wid, b));
      m.setAttribute("aria-haspopup", "true");
      acts.append(m);
    }
    sec.append(h);
    const state = own ? hostState(g) : "";
    if (state) sec.append(el("p", "gstate", state));   // under the name, wrapped: ssh's words can be long
    if (!closed) {
      const ul = el("ul");
      if (g.wid) ul.dataset.wid = g.wid;
      // a tab's panes after its first (a coding agent's subagents) go under it, and a pane's quick
      // shell (AC-88) under that pane wherever tmux has its window; a filter shows them flat
      const quickOf = new Map();
      if (!q) for (const it of items) if (it.quickOf && items.some((x) => x.id === it.quickOf)) quickOf.set(it.quickOf, [...(quickOf.get(it.quickOf) ?? []), it]);
      let parent = null;
      const kid = (it) => {
        if (!parent.kids) parent.append(parent.kids = el("ul", "kids"));
        parent.kids.append(row(it, current, onPick, false, true, marked));   // no Clone: a pane of its window
      };
      const quicks = (it) => { for (const k of quickOf.get(it.id) ?? []) kid(k); };
      for (const it of items) {
        if (quickOf.has(it.quickOf)) continue;     // listed under its pane
        if (!q && it.pane > 1 && parent) { kid(it); quicks(it); continue; }
        parent = row(it, current, onPick, !!g.wid && !q, false, marked, { onClone: g.new ? onClone : null });   // no tmux, no Clone
        ul.append(parent);
        quicks(it);
      }
      sec.append(ul);
    }
    frag.append(sec);
  }
  if (!shown) {
    frag.append(el("p", "empty", q ? "No session matches. Clear the filter to see all." : "No tmux sessions."));
  }
  if (lastShown !== undefined && current !== lastShown) frag.querySelector(".item.on")?.classList.add("fresh");   // it settles once (motion.css)
  lastShown = current;
  list.replaceChildren(frag);
}

export function findItem(groups, id) {
  for (const g of groups) for (const it of g.items) if (it.id === id) return { group: g, item: it };
  return null;
}
