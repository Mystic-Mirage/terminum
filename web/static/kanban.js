// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The board at /kanban/ (terminum-kanban.md): five columns of tasks, filters kept in the URL; a task moves by
// running it, dropped on an agent or by its key; terminumd owns the cards and every page hears of a change.
// signin.js comes first: every fetch then carries the sign-in.
import * as signin from "./signin.js";
import "./press.js";
import "./pop.js";
import { pageColors } from "./contrast.js";
import { withScheme } from "./schemes.js";
import { askConfirm } from "./dialogs.js";
import { sessionUrl, takeSelection } from "./tasklink.js";
import { taskWindow } from "./kanbanwin.js";
import { taskForm } from "./kanbanform.js";
import { archivedWindow } from "./kanbanarchive.js";
import { projectInk } from "./listparts.js";
import { seeFolders } from "./nav.js";
import { filterBar, filters, hasFilter, narrowed, query } from "./kanbanfilters.js";
import { boardSessions, sessionStrip } from "./kanbanside.js";
import { agentsSection } from "./kanbanagents.js";
import { projectWindow } from "./kanbanproject.js";
import "./dropdown.js";
import { runWindow } from "./kanbanrun.js";
import { taskLines } from "./tasklines.js";
import { $, COLUMNS, agentChip, api, button, el, iconButton, say } from "./kanbanutil.js";

const PAGE = 200;
let meta = { projects: [], users: [], sessions: [], agents: [], hosts: [] };
let cards = new Map(COLUMNS.map((c) => [c, []]));
let counts = {};
let shownColumn = "TODO";
const narrow = matchMedia("(max-width: 860px)"), still = matchMedia("(prefers-reduced-motion: reduce)");
let dragging = null;                               // the tasks of a drag
const picked = new Set();                          // the tasks ⌘ or Ctrl+click chose (terminum-dispatcher.md AC-138)
let slots = { agents: {}, full: [], full_projects: [] };   // what each agent's slots hold, and who is at a limit (AC-141)
let liveRows = new Map();                          // the session list's rows by key (AC-134)
let usage = {};                                    // what each task's session uses, by terminumd's walk
let loading = 0, changing = 0, chain = Promise.resolve();
const BUILD = document.querySelector('meta[name="build"]').content;

/** Above the board; a store set aside offers its way on, and only then is an empty one made (AC-119). */
function banner(text) {
  const box = $("boardbanner");
  box.textContent = text ?? "";
  box.hidden = !text;
  if (text && meta.set_aside) {
    box.append(" ", button(null, "Start empty", async () => {
      if (!await askConfirm("Start with an empty store? The one set aside stays where it is, untouched.", "Start empty")) return;
      try {
        await api("/api/tasks/start-empty", { method: "POST" });
        await loadMeta();
        await load();
      } catch (e) { say(e.message); }
    }));
  }
}

/** Each column's pages as shown; a failure keeps what is shown and says so (AC-105). The newest read
 *  stands, whichever answers last, and none over a change on its way: its end reads again (AC-133). */
async function load() {
  if (changing) return;
  const mine = ++loading;
  try {
    const parts = await Promise.all(COLUMNS.map(async (c, i) => {
      const pages = Math.max(1, Math.ceil(cards.get(c).length / PAGE));
      const got = await Promise.all(Array.from({ length: pages }, (_, n) => api(`/api/tasks?${query({ status: c, limit: PAGE, after: n ? n * PAGE : undefined, counts: i === 0 && !n ? 1 : undefined })}`)));
      return { tasks: got.flatMap((g) => g.tasks), counts: got[0].counts, slots: got[0].slots };
    }));
    if (mine !== loading || changing) return;
    cards = new Map(COLUMNS.map((c, i) => [c, parts[i].tasks]));
    counts = parts[0].counts;
    slots = parts[0].slots ?? slots;
    agents.draw();
    banner(meta.problem ?? "");
    draw();
  } catch (e) {
    banner(`The board is out of date: ${e.message}`);
  }
}

async function more(column) {
  const have = cards.get(column);
  try {
    const page = await api(`/api/tasks?${query({ status: column, limit: PAGE, after: have.length })}`);
    cards.set(column, [...have, ...page.tasks]);
    draw();
  } catch (e) { say(e.message); }
}

const projectOf = (id) => meta.projects.find((p) => p.id === id);

/** What a session's row cannot say (AC-136): an agent gone quiet with its task open, why it failed, a closed one. */
function sessionLine(t) {
  const s = t.session;
  if (!s && !t.run) return null;
  const open = t.status === "IN PROGRESS";
  const line = (cls, words, why) => { const e = el("span", `cstate ${cls}`, words); if (why) e.title = why; return e; };
  if (t.run === "no_result") return line("stopped", "DONE, no result");
  if (t.run) return line("waiting", t.run === "asking" ? "answer in its window" : "starting");
  if (s.closed) return line("closed", "session closed");
  if (s.state === "failed") return line("failed", `failed: ${(s.why || "an error").replaceAll("_", " ")}`);
  if (s.state === "waiting") return line("waiting", "waits for you", s.why);
  if (s.state === "working" || !open) return null;
  return line("stopped", s.agent ? "stopped, task open" : "no agent");
}

/** What its session uses and what stands idle in it (terminum-dispatcher.md AC-127 to AC-129): as of the last walk. */
function usageWords(id) {
  const u = usage[id];
  if (!u) return "";
  const words = u.not_measured ? [u.not_measured] : [`CPU ${u.cpu}%`, `${u.gib} GiB`, `${u.minutes} min`];
  if (u.budget_total) words.unshift(`${Math.floor(u.budget_used / 60)} of ${u.budget_total / 60} min`);
  if (u.tokens) words.push(`${(u.tokens.input + u.tokens.output + u.tokens.cached).toLocaleString("en")} tokens`);
  else if (u.tokens === null) words.push("Tokens not known");
  for (const s of u.idle_subs ?? []) words.push(`idle: ${s.kind || "subagent"} ${s.id}`);
  return words.join(" · ");
}

function showUsage(c, id) {
  const words = usageWords(id), line = c.querySelector(".cusage");
  if (line?.textContent === words) return;
  line?.remove();
  if (words) c.querySelector(".cfoot").append(el("span", "cusage", words));
}

function showState(foot, t) {
  foot.querySelector(".cstate")?.remove();
  const line = sessionLine(t);
  if (line) foot.append(line);
}

/** Its session's row, the list's own, drawn anew only when that row changed (AC-134). */
function showSession(c, t) {
  const item = t.session && !t.session.closed ? liveRows.get(t.session.key) : null, p = projectOf(t.project);
  const now = item ? JSON.stringify([item, p?.color]) : "";
  if (now === c.rowDrawn) return;
  c.rowDrawn = now;
  c.querySelector(".csession")?.remove();
  if (item) c.append(sessionStrip(item, p));
}

function card(t) {
  const c = el("article", `card sev-${t.severity}${picked.has(t.id) ? " picked" : ""}`);
  c.dataset.id = t.id;
  c.draggable = true;
  c.tabIndex = 0;
  c.rowDrawn = "";
  const head = el("div", "chead");
  head.append(el("span", "cid", t.id), el("span", `csev ${t.severity}`, t.severity));
  if (t.parent) head.append(el("span", "cparent", `in ${t.parent}`));
  if (t.agent) head.append(agentChip(t.agent));
  // on an open session its run has it; with none it can always start: nothing else takes it out of IN PROGRESS
  if (t.status !== "IN PROGRESS" || !t.session || t.session.closed) head.append(iconButton("icon crun", "play", `Run ${t.id}`, "Run: choose the agent that does it", () => runThem([t])));
  const foot = el("div", "cfoot"), one = filters().project.length === 1;
  const p = projectOf(t.project);
  if (!one) {                                      // its project is said once: by the head when one is shown
    const proj = el("span", "cproj", p?.name ?? "Default");
    proj.style.setProperty("--proj", projectInk(p));
    foot.append(proj);
  }
  if (!t.agent) foot.append(el("span", "cowner", t.by_agent ? `by agent · ${t.owner_name}` : t.owner_name));   // on an agent, the agent says who
  if (t.subtasks) foot.append(el("span", "csubs", `${t.subtasks_done}/${t.subtasks}`));
  if (t.files) foot.append(el("span", "cfiles", `${t.files} file${t.files === 1 ? "" : "s"}`));
  showState(foot, t);
  c.append(head, el("div", "ctitle", t.title), foot);
  showUsage(c, t.id);
  showSession(c, t);
  c.addEventListener("click", (e) => { if (e.metaKey || e.ctrlKey) pick(t.id, c); else openTask(t.id); });
  c.addEventListener("keydown", (e) => { if (e.key === "Enter" && e.target === c) openTask(t.id); });
  c.addEventListener("dragstart", (e) => {
    dragging = picked.has(t.id) ? pickedTasks() : [t];
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", dragging.map((d) => d.id).join(" "));
    for (const d of dragging) document.querySelector(`.card[data-id="${d.id}"]`)?.classList.add("lifted");
    document.body.classList.add("dragtask");
  });
  c.addEventListener("dragend", () => {
    dragging = null;
    document.querySelectorAll(".card.lifted").forEach((x) => x.classList.remove("lifted"));
    document.body.classList.remove("dragtask");
    for (const { col } of columns.values()) col.classList.remove("over");
    draw();
    agents.draw();
  });
  return c;
}

const pickedTasks = () => [...cards.values()].flat().filter((t) => picked.has(t.id));

/** ⌘ or Ctrl+click: several tasks for one agent; Esc lets them go. */
function pick(id, c) {
  if (!picked.delete(id)) picked.add(id);
  c.classList.toggle("picked", picked.has(id));
  agents.draw();
}
function unpick() {
  picked.clear();
  document.querySelectorAll(".card.picked").forEach((c) => c.classList.remove("picked"));
  agents.draw();
}
document.addEventListener("keydown", (e) => { if (e.key === "Escape" && picked.size && !document.querySelector("dialog[open]")) unpick(); });

/** RUN's window for `tasks`, on `agent` if chosen; what is said of a refusal is terminumd's. */
async function runThem(tasks, agent) {
  try {
    if (await runs.open(tasks, { agent }) === true) unpick();
  } catch (e) { say(e.message); }
}

/** The board's changes go one after another, as made, so the last drop stands; when none is left
 *  the columns are read again: a refused one's card goes back, and the toast says why (AC-133). */
function change(t, body) {
  changing++;
  chain = chain
    .then(() => api(`/api/tasks/${t.id}`, { method: "PATCH", body }))
    .catch((e) => say(e.message))
    .finally(() => { if (!--changing) readSoon(); });
}

/** Within its column a task's place is the human's; to another it goes only by running (AC-138). */
function dropOn(before) {
  const t = dragging[0], list = cards.get(t.status);
  list.splice(list.indexOf(t), 1);
  const at = before ? list.findIndex((c) => c.id === before) : -1;
  list.splice(at >= 0 ? at : list.length, 0, t);
  change(t, before ? { before } : { order_end: true });
}

function tabFor(name) {
  const tab = button(null, name, () => showColumn(name));
  tab.dataset.status = name;
  tab.setAttribute("role", "tab");
  return tab;
}

/** The columns stand once; a draw fills them, each keeping its scroll. */
const columns = new Map(COLUMNS.map((column) => {
  const col = el("section", "col"), head = el("h2", "colhead"), count = el("span", "count"), list = el("div", "cards");
  col.dataset.status = column;
  head.append(el("span", null, column), count);
  col.append(head, list);
  col.addEventListener("dragover", (e) => { if (dragging?.length === 1 && dragging[0].status === column) { e.preventDefault(); col.classList.add("over"); } });
  col.addEventListener("dragleave", (e) => { if (!col.contains(e.relatedTarget)) col.classList.remove("over"); });
  col.addEventListener("drop", (e) => {
    e.preventDefault();
    col.classList.remove("over");
    const under = [...list.querySelectorAll(".card")].find((c) => { const r = c.getBoundingClientRect(); return e.clientY < r.top + r.height / 2 && c.dataset.id !== dragging[0].id; });
    dropOn(under?.dataset.id);
  });
  return [column, { col, count, list, tab: tabFor(column), drawn: "" }];
}));
let socket = null;
/** A press in the session list: as the session page's, over the one socket. */
function send(m) {
  if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(m));
  else say("Not connected");
}
const openSession = (key) => { location.href = sessionUrl(key); };
const side = boardSessions({ chosen: () => filters().project, openTask, send, narrow });
const runs = runWindow({ addAgent: () => agents.add(), noFolder: askFolder, done: () => readSoon() });
const agents = agentsSection({ getMeta: () => meta, chosen: () => filters().project, reload: loadMeta, slots: () => slots, liveRow: (key) => liveRows.get(key),
  dragging: () => dragging, given: () => dragging ?? (picked.size ? pickedTasks() : null), run: runThem, openSession });
$("agentbar").append(agents.box);
$("board").replaceChildren(...[...columns.values()].map((c) => c.col));
$("colswitch").replaceChildren(...[...columns.values()].map((p) => p.tab));

/** A column is built anew only when what it shows changed; a drag's card stays until it ends. */
function draw() {
  if (dragging) return;
  const focused = document.activeElement?.closest?.(".card")?.dataset.id, filtered = narrowed();
  for (const [column, part] of columns) {
    const have = cards.get(column), n = counts[column] ?? 0, shown = JSON.stringify([have.map((t) => ({ ...t, session: t.session?.key })), n, !!filtered, filters().project.length === 1]);   // a session's state changes on its card alone (sessionsCame)
    part.count.textContent = String(n);
    part.tab.textContent = `${column} ${n}`;
    if (shown === part.drawn) continue;
    part.drawn = shown;
    const scroll = part.list.scrollTop;
    part.list.replaceChildren(...have.map(card));
    if (!have.length) part.list.append(el("p", "colempty", filtered ? "No tasks match" : "No tasks"));
    if (have.length < n) part.list.append(button("textkey colmore", `Show ${Math.min(PAGE, n - have.length)} more`, () => more(column)));
    part.list.scrollTop = scroll;
  }
  if (focused) document.querySelector(`.card[data-id="${focused}"]`)?.focus({ preventScroll: true });
  markTab();
}

/** A phone's one column, by its tab or a swipe. */
function showColumn(column) {
  shownColumn = column;
  columns.get(column).col.scrollIntoView({ inline: "start", block: "nearest", behavior: still.matches ? "auto" : "smooth" });
  markTab();
}

function markTab() {
  for (const [name, { tab }] of columns) {
    const on = name === shownColumn;
    if (tab.getAttribute("aria-selected") === String(on)) continue;
    tab.setAttribute("aria-selected", String(on));
    if (on) tab.scrollIntoView({ inline: "nearest", block: "nearest" });
  }
}

$("board").addEventListener("scroll", () => {
  if (!narrow.matches) return;
  const b = $("board"), now = [...columns.keys()][Math.round(b.scrollLeft / Math.max(1, b.clientWidth))];
  if (now && now !== shownColumn) { shownColumn = now; markTab(); }
}, { passive: true });

/** The session list changed: the pickers' sessions, the cards' rows and states come with it, no task did. */
function sessionsCame(groups) {
  const items = groups.flatMap((g) => g.items ?? []);
  meta.sessions = items.map((it) => ({ key: it.id, title: it.title }));
  liveRows = new Map(items.map((it) => [it.id, it]));
  agents.draw();
  seeFolders(groups);
  const shown = new Map([...document.querySelectorAll(".card")].map((c) => [c.dataset.id, c]));
  for (const t of [...cards.values()].flat()) {
    const it = t.session && liveRows.get(t.session.key);
    if (!t.session || (!it && t.session.closed)) continue;
    const now = it ? { key: it.id, state: it.state, agent: it.agent, why: it.why, closed: false } : { key: t.session.key, closed: true };
    const c = shown.get(t.id);
    if (JSON.stringify(now) !== JSON.stringify(t.session)) {
      t.session = now;
      if (c) showState(c.querySelector(".cfoot"), t);
    }
    if (c) showSession(c, t);
  }
}

// the page's theme with the color scheme every page shows over it (AC-12)
const colors = () => { if (meta.theme) pageColors(withScheme(meta.theme, meta.scheme)); };

async function loadMeta() {
  try {
    meta = await api("/api/tasks/meta");
    colors();
    const gone = filters().project.filter((p) => !meta.projects.some((x) => x.id === p));
    if (gone.length) { say(`A filter named a project that is gone: ${gone.join(", ")}`); bar.setFilters({ project: filters().project.filter((p) => !gone.includes(p)) }); }
    bar.draw();
    agents.draw();
    project.refresh();
  } catch (e) {
    banner(`The board is out of date: ${e.message}`);
  }
}

/** A task a filter hides is said when made: it does not just vanish. terminumd says what its filters show. */
async function madeHidden(t) {
  if (!hasFilter() || t.parent) return;            // a subtask shows in its parent's window
  const { tasks } = await api(`/api/tasks?${query({ id: t.id })}`);
  if (!tasks.length) say(`${t.id} is kept; a filter hides it.`);
}

const bar = filterBar({ getMeta: () => meta, changed: () => { load(); side.draw(); agents.draw(); }, editProject: (p) => project.open(p) });
const project = projectWindow({ getMeta: () => meta, reload: loadMeta, agents, archived: archivedWindow({ openTask }), deleteProject: side.deleteProject, chosen: (id) => bar.setFilters({ project: [id] }) });
$("newproject").addEventListener("click", () => project.open());
const needs = taskLines($("asks"), (id) => openTask(id));
const windowed = taskWindow({ getMeta: () => meta, runs, openTask, newSubtask: (t) => form.open({ parent: t.id, project: t.project }) });
const form = taskForm({ getMeta: () => meta, getFilter: filters, onMade: madeHidden, onRun: runMade, slots: () => slots });

/** Run in the new-task window: at once on its agent; what stands in the way opens RUN's window, saying it. */
async function runMade(t, agent) {
  madeHidden(t);
  if (!agent) return runThem([t]);
  try {
    await api(`/api/tasks/${t.id}/run`, { method: "POST", body: { agent } });
    (meta.last_agents ??= {})[t.project] = agent;
    readSoon();
  } catch (e) {
    runs.open([t], { why: e.message }).catch((no) => say(no.message));
  }
}

/** A project with no folder starts nothing: said, and its window opens at the folder (terminum-dispatcher.md AC-140). */
function askFolder(id, words) {
  say(words);
  project.open(projectOf(id));
}

const addressed = () => /^\/kanban\/(T-\d+)$/.exec(location.pathname)?.[1];

/** Its window has its own address: Back closes it, a task that is not there is said (AC-133). */
function openTask(id, push = true) {
  // one step of Back for the window, whichever tasks it showed
  if (push) history[addressed() ? "replaceState" : "pushState"]({ win: true }, "", `/kanban/${id}${location.search}`);
  needs.opened(id);
  windowed.open(id).catch((e) => {
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

// A file dropped anywhere never takes the page away: on the board it starts a new task (AC-133)
const hasFiles = (e) => e.dataTransfer?.types.includes("Files");
document.addEventListener("dragover", (e) => { if (hasFiles(e)) e.preventDefault(); });
document.addEventListener("drop", (e) => {
  if (!hasFiles(e) || e.defaultPrevented) return;
  e.preventDefault();
  const list = [...e.dataTransfer.files];
  if (!list.length || document.querySelector("dialog[open]")) return;
  form.open();
  form.add(list);
});

// What the on-screen keyboard leaves: windows stand in it, their keys never under it (sheets.css)
const seen = window.visualViewport;
const fit = () => document.documentElement.style.setProperty("--app-h", `${Math.round(seen.height)}px`);
if (seen) { fit(); seen.addEventListener("resize", fit); }

// A phone's page wakes: what it missed is read; its WebSocket says for itself when it is gone
document.addEventListener("visibilitychange", () => { if (!document.hidden) load(); });

let reloading = false;
/** terminumd was upgraded: this page reloads when no window is open (a draft is kept anyway), once. */
function buildCame(id) {
  if (id === BUILD || reloading || sessionStorage.getItem("web.reloaded-for") === id) return;
  reloading = true;
  const go = () => {
    if (document.querySelector("dialog[open]")) { setTimeout(go, 1000); return; }
    try { sessionStorage.setItem("web.reloaded-for", id); } catch { /* private mode: at worst asks again */ }
    location.reload();
  };
  go();
}
$("newtask").addEventListener("click", () => form.open());
/** The columns are read again once a burst ends. */
let readWait = 0, windowToo = false;
function readSoon() {
  clearTimeout(readWait);
  readWait = setTimeout(() => { load(); if (windowToo) windowed.refresh(); windowToo = false; }, 40);
}

/** Every page hears of a task's change; an open window reads again only when it shows that task. */
function listen() {
  const again = (id) => {
    windowToo ||= !id || windowed.shows(id);
    readSoon();
  };
  signin.prove().then(() => {
    const ws = socket = new WebSocket(`${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`, signin.protocols());
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data);
      side.seen(m);
      if (m.t === "taskchange") again(m.id);
      else if (m.t === "layout") { sessionsCame(m.groups ?? []); form.refresh(); }
      else if (m.t === "build") buildCame(m.id);
      else if (m.t === "taskmarks") needs.came(m);
      else if (m.t === "taskusage") {
        usage = m.items ?? {};
        slots = m.slots ?? slots;
        agents.draw();
        for (const c of document.querySelectorAll(".card")) showUsage(c, c.dataset.id);
      }
      else if (m.t === "settings") { meta.scheme = m.page.colorScheme; colors(); }
      else if (m.t === "projects") { clearTimeout(ws.wait); ws.wait = setTimeout(async () => { await loadMeta(); form.refresh(); again(); }, 500); }
    };
    ws.onclose = () => setTimeout(() => { listen(); load(); }, 2000);
  });
}

// the user's own stylesheet (terminum.md AC-19): the last one at once, the newest when it comes
try { $("customcss").textContent = localStorage.getItem(signin.CSS_KEY) ?? ""; } catch { /* no storage: fetched below */ }
fetch("/custom.css", { cache: "no-store" }).then((r) => (r.ok ? r.text() : null)).then((css) => {
  if (css === null) return;
  $("customcss").textContent = css;
  try { localStorage.setItem(signin.CSS_KEY, css); } catch { /* no storage: fetched at every load */ }
}).catch(() => {});
await loadMeta();
await load();
listen();
route();
const asked = new URLSearchParams(location.search);
if (asked.get("new") === "1") {
  form.open({ session: asked.get("session"), project: asked.get("for"), text: takeSelection() });
  for (const k of ["new", "session", "for"]) asked.delete(k);
  history.replaceState(null, "", `/kanban/${asked.size ? `?${asked}` : ""}`);
}
