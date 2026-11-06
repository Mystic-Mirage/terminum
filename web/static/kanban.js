// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The board at /kanban/ (terminum-kanban.md): five columns of tasks, filters kept in the URL, moves
// by drag or Move to…; terminumd owns the cards and every page hears of a change. signin.js comes
// first: every fetch then carries the sign-in, and a page with none goes to /sessions/, whose form
// signs in.
import * as signin from "./signin.js";
import { pageColors } from "./contrast.js";
import { taskWindow } from "./kanbanwin.js";
import { taskForm } from "./kanbanform.js";
import { COLUMNS, api, ask, el, say } from "./kanbanutil.js";

const $ = (id) => document.getElementById(id);
const SEVERITIES = ["P0", "P1", "P2", "P3"];
const PAGE = 200;
let meta = { projects: [], users: [], sessions: [] };
let cards = new Map(COLUMNS.map((c) => [c, []]));
let counts = {};
let shownColumn = "TODO";
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

function banner(text) {
  $("boardbanner").textContent = text ?? "";
  $("boardbanner").hidden = !text;
}

/** Each column's first page; a failure keeps what is shown and says so (AC-105). */
async function load() {
  try {
    const parts = await Promise.all(COLUMNS.map((c) => api(`/api/tasks?${query({ status: c, limit: PAGE })}`)));
    cards = new Map(COLUMNS.map((c, i) => [c, parts[i].tasks]));
    counts = parts[0].counts;
    banner(meta.problem ?? "");
    draw();
  } catch (e) {
    if (e.status === 401) { location.href = "/sessions/"; return; }
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
/** A project's color, as the session list's (nav.js projectColor, the page's palette). */
const projectHue = (p) => (p?.color >= 0 ? `hsl(${(p.color * 36 + 8) % 360} 62% 72%)` : "var(--muted)");

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
  const menu = el("button", "cmove", "Move to…");
  menu.type = "button";
  menu.setAttribute("aria-haspopup", "true");
  menu.addEventListener("click", (e) => { e.stopPropagation(); moveMenu(t, menu); });
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

/** Move to…: every drag also has a menu item (CLAUDE.md §1); the only way on a phone. */
function moveMenu(t, anchor) {
  document.querySelector(".movemenu")?.remove();
  const m = el("div", "box movemenu");
  m.setAttribute("role", "menu");
  for (const c of COLUMNS.filter((x) => x !== t.status)) {
    const b = el("button", null, c);
    b.type = "button";
    b.setAttribute("role", "menuitem");
    b.addEventListener("click", (e) => { e.stopPropagation(); m.remove(); moveTo(t, c); });
    m.append(b);
  }
  anchor.closest(".card").append(m);
  m.querySelector("button")?.focus();
  m.addEventListener("keydown", (e) => { if (e.key === "Escape") { e.stopPropagation(); m.remove(); } });
  setTimeout(() => addEventListener("click", () => m.remove(), { once: true }));
}

/** A move stands once terminumd said so; refused, the card stays and the toast says why. */
async function moveTo(t, status) {
  if (status === "IN PROGRESS" && !t.session) {
    const ok = await ask(`${t.id} has no session. Move it to IN PROGRESS?`, "Link a session in its window to have an agent work on it.", "Move");
    if (!ok) return;
  }
  try {
    await api(`/api/tasks/${t.id}`, { method: "PATCH", body: { status } });
  } catch (e) { say(e.message); }
  load();
}

async function dropOn(column, before) {
  const t = dragging;
  if (!t) return;
  try {
    if (t.status !== column) await moveTo(t, column);
    else await api(`/api/tasks/${t.id}`, { method: "PATCH", body: before ? { before } : { order_end: true } });
  } catch (e) { say(e.message); }
  load();
}

function draw() {
  const board = $("board");
  board.replaceChildren(...COLUMNS.map((column) => {
    const col = el("section", "col");
    col.dataset.status = column;
    col.classList.toggle("shown", column === shownColumn);
    const head = el("h2", "colhead");
    head.append(el("span", null, column), el("span", "colcount", String(counts[column] ?? 0)));
    const list = el("div", "cards");
    const have = cards.get(column);
    list.append(...have.map(card));
    if (!have.length) list.append(el("p", "colempty", hasFilter() ? "No tasks match" : "No tasks"));
    if (have.length < (counts[column] ?? 0)) {
      const b = el("button", "colmore", `Show ${Math.min(PAGE, counts[column] - have.length)} more`);
      b.type = "button";
      b.addEventListener("click", () => more(column));
      list.append(b);
    }
    col.addEventListener("dragover", (e) => { if (dragging) { e.preventDefault(); col.classList.add("over"); } });
    col.addEventListener("dragleave", (e) => { if (!col.contains(e.relatedTarget)) col.classList.remove("over"); });
    col.addEventListener("drop", (e) => {
      e.preventDefault();
      col.classList.remove("over");
      const under = [...list.querySelectorAll(".card")].find((c) => { const r = c.getBoundingClientRect(); return e.clientY < r.top + r.height / 2 && c.dataset.id !== dragging?.id; });
      dropOn(column, under?.dataset.id);
    });
    col.append(head, list);
    return col;
  }));
  $("colswitch").replaceChildren(...COLUMNS.map((c) => {
    const b = el("button", null, `${c} ${counts[c] ?? 0}`);
    b.type = "button";
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", String(c === shownColumn));
    b.addEventListener("click", () => { shownColumn = c; draw(); });
    return b;
  }));
}

const hasFilter = () => { const f = filters(); return f.project.length || f.severity.length || f.owner || f.q; };

function picks(box, options, chosen, onChange) {
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
  box.classList.toggle("on", chosen.length > 0);
}

function drawFilters() {
  const f = filters();
  const toggled = (list, id, on) => (on ? [...list, id] : list.filter((x) => x !== id));
  picks($("fproject"), meta.projects, f.project, (id, on) => setFilters({ project: toggled(filters().project, id, on) }));
  picks($("fseverity"), SEVERITIES.map((s) => ({ id: s, name: s })), f.severity, (id, on) => setFilters({ severity: toggled(filters().severity, id, on) }));
  picks($("fowner"), [{ id: "humans", name: "People" }, { id: "agents", name: "Agents" }], f.owner ? [f.owner] : [], (id, on) => setFilters({ owner: on ? id : "" }));
  if ($("find").value !== f.q) $("find").value = f.q;
  $("clearfilters").hidden = !hasFilter();
}

async function loadMeta() {
  try {
    meta = await api("/api/tasks/meta");
    if (meta.theme) pageColors(meta.theme);
    const gone = filters().project.filter((p) => !meta.projects.some((x) => x.id === p));
    if (gone.length) { say(`A filter named a project that is gone: ${gone.join(", ")}`); setFilters({ project: filters().project.filter((p) => !gone.includes(p)) }); }
    drawFilters();
  } catch (e) {
    if (e.status === 401) location.href = "/sessions/";
    else banner(`The board is out of date: ${e.message}`);
  }
}

const windowed = taskWindow({ getMeta: () => meta, onChange: load });
const form = taskForm({ getMeta: () => meta, getFilter: filters, onMade: load });

function openTask(id) {
  history.replaceState(null, "", `/kanban/${id}${location.search}`);
  windowed.open(id).catch((e) => say(e.message));
}

$("taskbox").addEventListener("close", () => history.replaceState(null, "", `/kanban/${location.search}`));
$("newtask").addEventListener("click", () => form.open());
$("find").addEventListener("input", () => { clearTimeout($("find").wait); $("find").wait = setTimeout(() => setFilters({ q: $("find").value }), 200); });
$("clearfilters").addEventListener("click", () => setFilters({ project: [], severity: [], owner: "", q: "" }));
$("newproject").addEventListener("click", async () => {
  const { askName } = await import("./kanbanutil.js");
  await askName("New project", "", "Create", async (name) => {
    await api("/api/task-projects", { method: "POST", body: { name } });
    await loadMeta();
  });
});

/** Every page hears of a task's change: the columns are read again, once a burst ends. */
function listen() {
  let wait = 0;
  const again = () => { clearTimeout(wait); wait = setTimeout(() => { load(); windowed.refresh(); }, 100); };
  signin.prove().then(() => {
    const ws = new WebSocket(`${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`, signin.protocols());
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data);
      if (m.t === "taskchange") again();
      else if (m.t === "projects" || m.t === "layout") { clearTimeout(ws.wait); ws.wait = setTimeout(() => { loadMeta(); again(); }, 500); }
    };
    ws.onclose = () => setTimeout(() => { listen(); load(); }, 2000);
  });
}

await loadMeta();
await load();
listen();
const wanted = /^\/kanban\/(T-\d+)$/.exec(location.pathname)?.[1];
if (wanted) openTask(wanted);
const asked = new URLSearchParams(location.search);
if (asked.get("new") === "1") {
  form.open({ session: asked.get("session"), project: asked.get("for"), text: signin.held() ? form.takeSelection() : "" });
  for (const k of ["new", "session", "for"]) asked.delete(k);
  history.replaceState(null, "", `/kanban/${asked.size ? `?${asked}` : ""}`);
}
