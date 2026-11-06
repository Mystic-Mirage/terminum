// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The board at /kanban/ (terminum-kanban.md): five columns of tasks, filters kept in the URL, moves
// by drag or Move to…; terminumd owns the cards and every page hears of a change, this one too.
// signin.js comes first: every fetch then carries the sign-in.
import * as signin from "./signin.js";
import "./press.js";
import "./pop.js";
import { pageColors } from "./contrast.js";
import { askConfirm, askName } from "./dialogs.js";
import { icon } from "./icons.js";
import { projectColor } from "./nav.js";
import { takeSelection } from "./tasklink.js";
import { taskWindow } from "./kanbanwin.js";
import { taskForm } from "./kanbanform.js";
import { $, COLUMNS, SEVERITIES, api, button, el, say } from "./kanbanutil.js";

const PAGE = 200;
let meta = { projects: [], users: [], sessions: [] };
let cards = new Map(COLUMNS.map((c) => [c, []]));
let counts = {};
let shownColumn = "TODO";
const phone = () => matchMedia("(max-width: 860px)").matches;
let dragging = null;

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

/** Each column's first page; a failure keeps what is shown and says so (AC-105). */
async function load() {
  try {
    const parts = await Promise.all(COLUMNS.map((c, i) => api(`/api/tasks?${query({ status: c, limit: PAGE, counts: i === 0 ? 1 : undefined })}`)));
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

function sessionLine(t) {
  if (!t.session) return null;
  if (t.session.closed) return el("span", "cstate closed", "session closed");
  const s = t.session.state;
  return el("span", `cstate ${s ?? "idle"}`, s ?? "idle");
}

function card(t) {
  const c = el("article", `card sev-${t.severity}`);
  c.dataset.id = t.id;
  c.draggable = true;
  c.tabIndex = 0;
  const head = el("div", "chead");
  head.append(el("span", "cid", t.id), el("span", `csev ${t.severity}`, t.severity));
  const menu = button("icon cmove", null, (e) => { e.stopPropagation(); moveMenu(t, menu); });
  menu.append(icon("ellipsis", 14));
  menu.title = "Move to…";
  menu.setAttribute("aria-label", "Move to…");
  menu.setAttribute("aria-haspopup", "true");
  head.append(menu);
  const foot = el("div", "cfoot");
  const p = projectOf(t.project);
  const proj = el("span", "cproj", p?.name ?? "Default");
  proj.style.setProperty("--proj", projectHue(p));
  foot.append(proj, el("span", "cowner", t.by_agent ? `by agent · ${t.owner_name}` : t.owner_name));
  if (t.subtasks) foot.append(el("span", "csubs", `${t.subtasks_done}/${t.subtasks}`));
  if (t.files) foot.append(el("span", "cfiles", `${t.files} file${t.files === 1 ? "" : "s"}`));
  const state = sessionLine(t);
  if (state) foot.append(state);
  c.append(head, el("div", "ctitle", t.title), foot);
  c.addEventListener("click", () => openTask(t.id));
  c.addEventListener("keydown", (e) => { if (e.key === "Enter") openTask(t.id); });
  c.addEventListener("dragstart", (e) => { dragging = t; e.dataTransfer.effectAllowed = "move"; e.dataTransfer.setData("text/plain", t.id); c.classList.add("lifted"); });
  c.addEventListener("dragend", () => { dragging = null; c.classList.remove("lifted"); });
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
  if (phone()) { rows.focus(); return; }   // a finger's: no row looks chosen before it presses one
  const r = anchor.getBoundingClientRect(), w = box.offsetWidth, h = box.offsetHeight;
  box.style.left = `${Math.max(8, Math.min(r.right - w, innerWidth - w - 8))}px`;
  box.style.top = `${r.bottom + h + 4 < innerHeight ? r.bottom + 4 : Math.max(8, r.top - h - 4)}px`;
}
$("movebox").addEventListener("click", (e) => { if (e.target === $("movebox")) $("movebox").close(); });

/** A change stands once terminumd said so, and every page hears of it; refused, the toast says why. */
const change = (t, body) => api(`/api/tasks/${t.id}`, { method: "PATCH", body }).catch((e) => say(e.message));

async function moveTo(t, status) {
  if (status === "IN PROGRESS" && !t.session
    && !await askConfirm(`${t.id} has no session. Move it to IN PROGRESS? Link a session in its window to have an agent work on it.`, "Move")) return;
  await change(t, { status });
}

function dropOn(column, before) {
  const t = dragging;
  if (!t) return;
  if (t.status !== column) moveTo(t, column);
  else change(t, before ? { before } : { order_end: true });
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
  return [column, { col, count, list }];
}));
$("board").replaceChildren(...[...columns.values()].map((c) => c.col));

function draw() {
  const focused = document.activeElement?.closest?.(".card")?.dataset.id;
  for (const [column, { count, list }] of columns) {
    const have = cards.get(column), scroll = list.scrollTop, n = counts[column] ?? 0;
    count.textContent = String(n);
    list.replaceChildren(...have.map(card));
    if (!have.length) list.append(el("p", "colempty", hasFilter() ? "No tasks match" : "No tasks"));
    if (have.length < n) {
      const moreKey = button("key colmore", `Show ${Math.min(PAGE, n - have.length)} more`, () => more(column));
      list.append(moreKey);
    }
    list.scrollTop = scroll;
  }
  if (focused) document.querySelector(`.card[data-id="${focused}"]`)?.focus({ preventScroll: true });
  $("colswitch").replaceChildren(...COLUMNS.map((c) => {
    const b = button(null, `${c} ${counts[c] ?? 0}`, () => showColumn(c));
    b.dataset.status = c;
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", String(c === shownColumn));
    return b;
  }));
}

/** A phone's one column, by its tab or a swipe. */
function showColumn(column) {
  shownColumn = column;
  columns.get(column).col.scrollIntoView({ inline: "start", block: "nearest", behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  markTab();
}

function markTab() {
  for (const b of $("colswitch").children) {
    const on = b.dataset.status === shownColumn;
    b.setAttribute("aria-selected", String(on));
    if (on) b.scrollIntoView({ inline: "nearest", block: "nearest" });
  }
}

$("board").addEventListener("scroll", () => {
  if (!phone()) return;
  const b = $("board"), now = COLUMNS[Math.round(b.scrollLeft / Math.max(1, b.clientWidth))];
  if (now && now !== shownColumn) { shownColumn = now; markTab(); }
}, { passive: true });

/** The session list changed: the pickers' sessions and the cards' states come with it, no task did. */
function sessionsCame(groups) {
  const items = groups.flatMap((g) => g.items ?? []);
  meta.sessions = items.map((it) => ({ key: it.id, title: it.title }));
  const live = new Map(items.map((it) => [it.id, it]));
  const shownCards = new Map([...cards.values()].flat().map((t) => [t.id, t]));
  for (const c of $("board").querySelectorAll(".card")) {
    const t = shownCards.get(c.dataset.id);
    if (!t?.session) continue;
    const it = live.get(t.session.key);
    t.session = it ? { key: it.id, state: it.state, closed: false } : { key: t.session.key, closed: true };
    c.querySelector(".cstate")?.replaceWith(sessionLine(t));
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

/** One picker open at a time; a press elsewhere or Esc closes it. */
const pickers = () => [...document.querySelectorAll(".pick")];
for (const d of pickers()) d.addEventListener("toggle", () => { if (d.open) for (const o of pickers()) if (o !== d) o.open = false; });
document.addEventListener("click", (e) => { if (!e.target.closest?.(".pick")) for (const d of pickers()) d.open = false; });
document.addEventListener("keydown", (e) => { if (e.key === "Escape") for (const d of pickers()) d.open = false; });
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

const windowed = taskWindow({ getMeta: () => meta });
const form = taskForm({ getMeta: () => meta, getFilter: filters });

function openTask(id) {
  history.replaceState(null, "", `/kanban/${id}${location.search}`);
  windowed.open(id).catch((e) => say(e.message));
}

$("taskbox").addEventListener("close", () => history.replaceState(null, "", `/kanban/${location.search}`));
$("newtask").addEventListener("click", () => form.open());
$("find").addEventListener("input", () => { clearTimeout($("find").wait); $("find").wait = setTimeout(() => setFilters({ q: $("find").value }), 200); });
$("clearfilters").addEventListener("click", () => setFilters({ project: [], severity: [], owner: "", q: "" }));
/** Every page hears of a task's change: the columns are read again once a burst ends, an open
 *  window only when it shows a task that changed. */
function listen() {
  let wait = 0, windowToo = false;
  const again = (id) => {
    windowToo ||= !id || windowed.shows(id);
    clearTimeout(wait);
    wait = setTimeout(() => { load(); if (windowToo) windowed.refresh(); windowToo = false; }, 100);
  };
  signin.prove().then(() => {
    const ws = new WebSocket(`${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`, signin.protocols());
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data);
      if (m.t === "taskchange") again(m.id);
      else if (m.t === "layout") sessionsCame(m.groups);
      else if (m.t === "projects") { clearTimeout(ws.wait); ws.wait = setTimeout(() => { loadMeta(); again(); }, 500); }
    };
    ws.onclose = () => setTimeout(() => { listen(); load(); }, 2000);
  });
}

fetch("/custom.css", { cache: "no-store" }).then((r) => (r.ok ? r.text() : "")).then((css) => { $("customcss").textContent = css; }).catch(() => {});   // the user's own stylesheet (terminum.md AC-19)
await loadMeta();
await load();
listen();
const wanted = /^\/kanban\/(T-\d+)$/.exec(location.pathname)?.[1];
if (wanted) openTask(wanted);
const asked = new URLSearchParams(location.search);
if (asked.get("new") === "1") {
  form.open({ session: asked.get("session"), project: asked.get("for"), text: takeSelection() });
  for (const k of ["new", "session", "for"]) asked.delete(k);
  history.replaceState(null, "", `/kanban/${asked.size ? `?${asked}` : ""}`);
}
