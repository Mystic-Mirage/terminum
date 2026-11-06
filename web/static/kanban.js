// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The board at /kanban/ (terminum-kanban.md): five columns of tasks, filters kept in the URL, moves
// by drag or Move to…; terminumd owns the cards and every page hears of a change, this one too.
// signin.js comes first: every fetch then carries the sign-in.
import * as signin from "./signin.js";
import "./press.js";
import "./pop.js";
import { pageColors } from "./contrast.js";
import { askConfirm, askName } from "./dialogs.js";
import { projectColor } from "./listparts.js";
import { takeSelection } from "./tasklink.js";
import { taskWindow } from "./kanbanwin.js";
import { taskForm } from "./kanbanform.js";
import { $, COLUMNS, SEVERITIES, api, button, el, iconButton, say } from "./kanbanutil.js";

const PAGE = 200;
let meta = { projects: [], users: [], sessions: [] };
let cards = new Map(COLUMNS.map((c) => [c, []]));
let counts = {};
let shownColumn = "TODO";
const narrow = matchMedia("(max-width: 860px)"), still = matchMedia("(prefers-reduced-motion: reduce)");
let dragging = null;
let loading = 0, changing = 0, chain = Promise.resolve();
const BUILD = document.querySelector('meta[name="build"]').content;

const filters = () => {
  const q = new URLSearchParams(location.search);
  const list = (k) => (q.get(k) ?? "").split(",").filter(Boolean);
  return { project: list("project"), severity: list("severity"), owner: q.get("owner") ?? "", q: q.get("q") ?? "" };
};

function setFilters(next) {
  const u = new URL(location.href);
  for (const [k, v] of Object.entries({ ...filters(), ...next })) {
    const s = Array.isArray(v) ? v.join(",") : v;
    if (s) u.searchParams.set(k, s); else u.searchParams.delete(k);
  }
  history.replaceState(null, "", u);
  drawFilters();
  load();
}

function query(extra = {}) {
  const f = filters(), p = new URLSearchParams();
  for (const [k, v] of Object.entries({ project: f.project.join(","), severity: f.severity.join(","), owner: f.owner, q: f.q, ...extra })) if (v !== "" && v !== undefined) p.set(k, v);
  return p.toString();
}

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
      return { tasks: got.flatMap((g) => g.tasks), counts: got[0].counts };
    }));
    if (mine !== loading || changing) return;
    cards = new Map(COLUMNS.map((c, i) => [c, parts[i].tasks]));
    counts = parts[0].counts;
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
/** A project's color, the session list's; Default has none. */
const projectHue = (p) => (p?.color >= 0 ? projectColor(p) : "var(--muted)");

/** What a task's session does now (AC-134): an agent gone quiet with its task open is said. */
function sessionLine(t) {
  const s = t.session;
  if (!s) return null;
  const open = t.status === "IN PROGRESS";
  const line = (cls, words, why) => { const e = el("span", `cstate ${cls}`, words); if (why) e.title = why; return e; };
  if (s.closed) return line("closed", "session closed");
  if (s.state === "failed") return line("failed", `failed: ${(s.why || "an error").replaceAll("_", " ")}`);
  if (s.state === "waiting") return line("waiting", "waits for you", s.why);
  if (s.state === "working") return line("working", "working");
  if (!s.agent) return open ? line("stopped", "no agent") : null;
  return open ? line("stopped", "stopped, task open") : line("", "idle");
}

function showState(foot, t) {
  foot.querySelector(".cstate")?.remove();
  const line = sessionLine(t);
  if (line) foot.append(line);
}

function card(t) {
  const c = el("article", `card sev-${t.severity}`);
  c.dataset.id = t.id;
  c.draggable = true;
  c.tabIndex = 0;
  const head = el("div", "chead");
  head.append(el("span", "cid", t.id), el("span", `csev ${t.severity}`, t.severity));
  const menu = iconButton("icon cmove", "ellipsis", "Move to…", "Move to…", (b) => moveMenu(t, b));
  menu.setAttribute("aria-haspopup", "true");
  head.append(menu);
  const foot = el("div", "cfoot");
  const p = projectOf(t.project);
  const proj = el("span", "cproj", p?.name ?? "Default");
  proj.style.setProperty("--proj", projectHue(p));
  foot.append(proj, el("span", "cowner", t.by_agent ? `by agent · ${t.owner_name}` : t.owner_name));
  if (t.subtasks) foot.append(el("span", "csubs", `${t.subtasks_done}/${t.subtasks}`));
  if (t.files) foot.append(el("span", "cfiles", `${t.files} file${t.files === 1 ? "" : "s"}`));
  showState(foot, t);
  c.append(head, el("div", "ctitle", t.title), foot);
  c.addEventListener("click", () => openTask(t.id));
  c.addEventListener("keydown", (e) => { if (e.key === "Enter" && e.target === c) openTask(t.id); });
  c.addEventListener("dragstart", (e) => { dragging = t; e.dataTransfer.effectAllowed = "move"; e.dataTransfer.setData("text/plain", t.id); c.classList.add("lifted"); });
  c.addEventListener("dragend", () => {
    dragging = null;
    c.classList.remove("lifted");
    for (const { col } of columns.values()) col.classList.remove("over");
    draw();
  });
  return c;
}

/** Move to…: every drag also has a menu item (CLAUDE.md §1); the only way on a phone. A window of
 *  its own: nothing clips it, Esc and a press outside close it; beside its key, a sheet on a phone. */
function moveMenu(t, anchor) {
  const box = $("movebox"), rows = $("moverows");
  rows.replaceChildren(el("p", "movehead", `Move ${t.id} to`), ...COLUMNS.filter((c) => c !== t.status).map((c) => {
    const b = button(null, c, () => { box.close(); moveTo(t, c); });
    b.setAttribute("role", "menuitem");
    return b;
  }));
  box.setAttribute("aria-label", `Move ${t.id} to`);
  box.showModal();
  if (narrow.matches) { rows.focus(); return; }   // a finger's: no row looks chosen before it presses one
  const r = anchor.getBoundingClientRect(), w = box.offsetWidth, h = box.offsetHeight;
  box.style.left = `${Math.max(8, Math.min(r.right - w, innerWidth - w - 8))}px`;
  box.style.top = `${r.bottom + h + 4 < innerHeight ? r.bottom + 4 : Math.max(8, r.top - h - 4)}px`;
}
$("movebox").addEventListener("click", (e) => { if (e.target === $("movebox")) $("movebox").close(); });

/** The board's changes go one after another, as made, so the last drop stands; when none is left
 *  the columns are read again: a refused one's card goes back, and the toast says why (AC-133). */
function change(t, body) {
  changing++;
  chain = chain
    .then(() => api(`/api/tasks/${t.id}`, { method: "PATCH", body }))
    .catch((e) => say(e.message))
    .finally(() => { if (!--changing) readSoon(); });
}

/** Shown at once where it goes: on top of its severity's cards. */
function placeCard(t, column) {
  const from = cards.get(t.status), to = cards.get(column);
  from.splice(from.indexOf(t), 1);
  counts[t.status] = Math.max(0, (counts[t.status] ?? 1) - 1);
  counts[column] = (counts[column] ?? 0) + 1;
  t.status = column;
  const top = to.findIndex((c) => c.severity >= t.severity);
  to.splice(top >= 0 ? top : to.length, 0, t);
  draw();
}

async function moveTo(t, status) {
  if (status === "IN PROGRESS" && !t.session
    && !await askConfirm(`${t.id} has no session. Move it to IN PROGRESS? Link a session in its window to have an agent work on it.`, "Move")) return;
  placeCard(t, status);
  change(t, { status });
}

function dropOn(column, before) {
  const t = dragging;
  if (!t) return;
  if (t.status !== column) return moveTo(t, column);
  const list = cards.get(column);
  list.splice(list.indexOf(t), 1);
  const at = before ? list.findIndex((c) => c.id === before) : -1;
  list.splice(at >= 0 ? at : list.length, 0, t);
  change(t, before ? { before } : { order_end: true });
}

/** The columns stand once; a draw fills them, each keeping its scroll. */
const columns = new Map(COLUMNS.map((column) => {
  const col = el("section", "col"), head = el("h2", "colhead"), count = el("span", "count"), list = el("div", "cards");
  col.dataset.status = column;
  head.append(el("span", null, column), count);
  col.append(head, list);
  col.addEventListener("dragover", (e) => { if (dragging) { e.preventDefault(); col.classList.add("over"); } });
  col.addEventListener("dragleave", (e) => { if (!col.contains(e.relatedTarget)) col.classList.remove("over"); });
  col.addEventListener("drop", (e) => {
    e.preventDefault();
    col.classList.remove("over");
    const under = [...list.querySelectorAll(".card")].find((c) => { const r = c.getBoundingClientRect(); return e.clientY < r.top + r.height / 2 && c.dataset.id !== dragging?.id; });
    dropOn(column, under?.dataset.id);
  });
  const tab = button(null, column, () => showColumn(column));
  tab.dataset.status = column;
  tab.setAttribute("role", "tab");
  return [column, { col, count, list, tab, drawn: "" }];
}));
$("board").replaceChildren(...[...columns.values()].map((c) => c.col));
$("colswitch").replaceChildren(...[...columns.values()].map((c) => c.tab));

/** A column is built anew only when what it shows changed; a drag's card stays until it ends. */
function draw() {
  if (dragging) return;
  const focused = document.activeElement?.closest?.(".card")?.dataset.id, filtered = hasFilter();
  for (const [column, part] of columns) {
    const have = cards.get(column), n = counts[column] ?? 0, shown = JSON.stringify([have, n, !!filtered]);
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
  for (const [column, { tab }] of columns) {
    const on = column === shownColumn;
    if (tab.getAttribute("aria-selected") === String(on)) continue;
    tab.setAttribute("aria-selected", String(on));
    if (on) tab.scrollIntoView({ inline: "nearest", block: "nearest" });
  }
}

$("board").addEventListener("scroll", () => {
  if (!narrow.matches) return;
  const b = $("board"), now = COLUMNS[Math.round(b.scrollLeft / Math.max(1, b.clientWidth))];
  if (now && now !== shownColumn) { shownColumn = now; markTab(); }
}, { passive: true });

/** The session list changed: the pickers' sessions and the cards' states come with it, no task did. */
function sessionsCame(groups) {
  const items = groups.flatMap((g) => g.items ?? []);
  meta.sessions = items.map((it) => ({ key: it.id, title: it.title }));
  const live = new Map(items.map((it) => [it.id, it]));
  for (const t of [...cards.values()].flat()) {
    if (!t.session) continue;
    const it = live.get(t.session.key);
    const now = it ? { key: it.id, state: it.state, agent: it.agent, why: it.why, closed: false } : { key: t.session.key, closed: true };
    if (JSON.stringify(now) === JSON.stringify(t.session)) continue;
    t.session = now;
    const foot = document.querySelector(`.card[data-id="${t.id}"] .cfoot`);
    if (foot) showState(foot, t);
  }
}

const hasFilter = () => { const f = filters(); return f.project.length || f.severity.length || f.owner || f.q; };

function picks(box, options, chosen, onChange, extra) {
  const inner = box.querySelector("div");
  inner.replaceChildren(...options.map(({ id, name }) => {
    const label = el("label", "pickrow");
    const c = document.createElement("input");
    c.type = "checkbox";
    c.checked = chosen.includes(id);
    c.addEventListener("change", () => onChange(id, c.checked));
    label.append(c, document.createTextNode(name));
    return label;
  }));
  if (extra) inner.append(extra);
  box.classList.toggle("on", chosen.length > 0);
  const summary = box.querySelector("summary");
  summary.querySelector(".count")?.remove();
  if (chosen.length) summary.append(el("span", "count", String(chosen.length)));
}

function newProject() {
  const b = button(null, "New project…", () => {
    $("fproject").open = false;
    askName("New project", "", "Create", 40, async (name) => {
      await api("/api/task-projects", { method: "POST", body: { name } });
      await loadMeta();
    });
  });
  b.id = "newproject";
  return b;
}

/** One picker open at a time (their `name`); a press elsewhere or Esc closes it. */
const closePickers = () => document.querySelectorAll(".pick[open]").forEach((d) => { d.open = false; });
document.addEventListener("click", (e) => { if (!e.target.closest?.(".pick")) closePickers(); });
document.addEventListener("keydown", (e) => { if (e.key === "Escape") closePickers(); });
$("filtersbtn").addEventListener("click", () => {
  const open = $("picks").classList.toggle("open");
  $("filtersbtn").setAttribute("aria-expanded", String(open));
});

function drawFilters() {
  const f = filters();
  const toggled = (list, id, on) => (on ? [...list, id] : list.filter((x) => x !== id));
  picks($("fproject"), meta.projects, f.project, (id, on) => setFilters({ project: toggled(filters().project, id, on) }), newProject());
  picks($("fseverity"), SEVERITIES.map((s) => ({ id: s, name: s })), f.severity, (id, on) => setFilters({ severity: toggled(filters().severity, id, on) }));
  picks($("fowner"), [{ id: "humans", name: "People" }, { id: "agents", name: "Agents" }], f.owner ? [f.owner] : [], (id, on) => setFilters({ owner: on ? id : "" }));
  if ($("find").value !== f.q) $("find").value = f.q;
  $("clearfilters").hidden = !hasFilter();
  const on = f.project.length + f.severity.length + (f.owner ? 1 : 0);
  $("filtersbtn").textContent = on ? `Filters ${on}` : "Filters";
  if (on) $("picks").classList.add("open");
}

async function loadMeta() {
  try {
    meta = await api("/api/tasks/meta");
    if (meta.theme) pageColors(meta.theme);
    const gone = filters().project.filter((p) => !meta.projects.some((x) => x.id === p));
    if (gone.length) { say(`A filter named a project that is gone: ${gone.join(", ")}`); setFilters({ project: filters().project.filter((p) => !gone.includes(p)) }); }
    drawFilters();
  } catch (e) {
    banner(`The board is out of date: ${e.message}`);
  }
}

/** A task a filter hides is said when made: it does not just vanish. terminumd says what its filters show. */
async function madeHidden(t) {
  if (!hasFilter()) return;
  const { tasks } = await api(`/api/tasks?${query({ id: t.id })}`);
  if (!tasks.length) say(`${t.id} is kept; a filter hides it.`);
}

const windowed = taskWindow({ getMeta: () => meta });
const form = taskForm({ getMeta: () => meta, getFilter: filters, onMade: madeHidden });

const addressed = () => /^\/kanban\/(T-\d+)$/.exec(location.pathname)?.[1];

/** Its window has its own address: Back closes it, a task that is not there is said (AC-133). */
function openTask(id, push = true) {
  // one step of Back for the window, whichever tasks it showed
  if (push) history[addressed() ? "replaceState" : "pushState"]({ win: true }, "", `/kanban/${id}${location.search}`);
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
$("find").addEventListener("input", () => { clearTimeout($("find").wait); $("find").wait = setTimeout(() => setFilters({ q: $("find").value }), 200); });
$("clearfilters").addEventListener("click", () => setFilters({ project: [], severity: [], owner: "", q: "" }));
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
    const ws = new WebSocket(`${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`, signin.protocols());
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data);
      if (m.t === "taskchange") again(m.id);
      else if (m.t === "layout") { sessionsCame(m.groups ?? []); form.refresh(); }
      else if (m.t === "build") buildCame(m.id);
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
