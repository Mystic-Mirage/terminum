// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The board's filters, kept in its URL (terminum-kanban.md AC-111), and the project it shows, which
// this browser remembers (AC-135).
import { projectInk } from "./listparts.js";
import { $, SEVERITIES, button, el, iconButton, read, write } from "./kanbanutil.js";

const CHOSEN = "board.project";
const toggled = (list, id, on) => (on ? [...list, id] : list.filter((x) => x !== id));
const tick = (checked, onChange) => {
  const c = document.createElement("input");
  c.type = "checkbox";
  c.checked = checked;
  c.addEventListener("change", () => onChange(c.checked));
  return c;
};

/** The address with `next` over its filters; history stays where it is. */
function address(next) {
  const u = new URL(location.href);
  for (const [k, v] of Object.entries({ ...filters(), ...next })) {
    const s = Array.isArray(v) ? v.join(",") : v;
    if (s) u.searchParams.set(k, s); else u.searchParams.delete(k);
  }
  history.replaceState(history.state, "", u);
}

export const filters = () => {
  const q = new URLSearchParams(location.search);
  const list = (k) => (q.get(k) ?? "").split(",").filter(Boolean);
  return { project: list("project"), severity: list("severity"), owner: q.get("owner") ?? "", q: q.get("q") ?? "", archived: q.get("archived") ?? "", subtasks: q.get("subtasks") ?? "" };
};

/** What Clear filters takes off: the project is a choice, not a filter (AC-135). */
export const narrowed = () => { const f = filters(); return f.severity.length || f.owner || f.q || f.archived || f.subtasks; };
/** A task made now may not show. */
export const hasFilter = () => narrowed() || filters().project.length;

export function query(extra = {}) {
  const f = filters(), p = new URLSearchParams();
  for (const [k, v] of Object.entries({ project: f.project.join(","), severity: f.severity.join(","), owner: f.owner, q: f.q, archived: f.archived, subtasks: f.subtasks, ...extra })) if (v !== "" && v !== undefined) p.set(k, v);
  return p.toString();
}

/** The filters' keys over `getMeta()`'s projects; `changed` after any change, `editProject(p)` from a project's pencil. */
export function filterBar({ getMeta, changed, editProject }) {
  function setFilters(next) {
    address(next);
    if ("project" in next) write(CHOSEN, next.project.join(",") || null);
    draw();
    changed();
  }

  /** The ticks of one filter's menu; its key says how many are on. */
  function picks(box, options, chosen, onChange) {
    const inner = box.querySelector("div");
    inner.replaceChildren(...options.map(({ id, name }) => {
      const label = el("label", "pickrow");
      label.append(tick(chosen.includes(id), (on) => onChange(id, on)), document.createTextNode(name));
      return label;
    }));
    box.classList.toggle("on", chosen.length > 0);
    const summary = box.querySelector("summary");
    summary.querySelector(".count")?.remove();
    if (chosen.length) summary.append(el("span", "count", String(chosen.length)));
  }

  const dot = (p) => {
    const d = el("span", "pdot");
    d.style.setProperty("--proj", projectInk(p));
    return d;
  };

  /** Its menu: a name chooses that project alone, a tick adds it to those shown, a pencil edits it (AC-135). */
  function drawProjects(chosen) {
    const box = $("fproject"), { projects } = getMeta();
    const only = (ids) => { box.open = false; setFilters({ project: ids }); };
    const rows = projects.map((p) => {
      const line = el("div", "projrow");
      const also = el("label", "projtick");
      const c = tick(chosen.includes(p.id), (on) => setFilters({ project: toggled(filters().project, p.id, on) }));
      c.setAttribute("aria-label", `Show ${p.name} too`);
      also.append(c);
      const name = button("projname", "", () => only([p.id]));
      name.append(dot(p), el("span", "pname", p.name));
      if (!p.dir) name.append(el("small", "projnofolder", "no folder"));   // no agent can start there (terminum-dispatcher.md AC-140)
      const edit = iconButton("projedit", "pencil", `Edit ${p.name}`, `Edit ${p.name}: its folder and limits`, () => { box.open = false; editProject(p); });
      line.append(also, name, edit);
      return line;
    });
    box.querySelector("div").replaceChildren(button(null, "All projects", () => only([])), ...rows);
    const first = projects.find((p) => p.id === chosen[0]);
    const words = !chosen.length ? "All projects" : `${first?.name ?? chosen[0]}${chosen.length > 1 ? ` +${chosen.length - 1}` : ""}`;
    box.querySelector("summary").replaceChildren(...(first ? [dot(first)] : []), el("span", "pname", words));
  }

  function draw() {
    const f = filters();
    drawProjects(f.project);
    picks($("fseverity"), SEVERITIES.map((s) => ({ id: s, name: s })), f.severity, (id, on) => setFilters({ severity: toggled(filters().severity, id, on) }));
    picks($("fowner"), [{ id: "humans", name: "People" }, { id: "agents", name: "Agents" }], f.owner ? [f.owner] : [], (id, on) => setFilters({ owner: on ? id : "" }));
    if ($("find").value !== f.q) $("find").value = f.q;
    const on = f.severity.length + (f.owner ? 1 : 0) + (f.archived ? 1 : 0) + (f.subtasks ? 1 : 0);
    $("farchived").setAttribute("aria-pressed", String(!!f.archived));
    $("fsubtasks").setAttribute("aria-pressed", String(!!f.subtasks));
    $("clearfilters").hidden = !narrowed();
    $("filtersbtn").textContent = on ? `Filters ${on}` : "Filters";
    if (on) $("picks").classList.add("open");
  }

  /** One picker open at a time (their `name`); a press elsewhere or Esc closes it. */
  const closePickers = () => document.querySelectorAll(".pick[open]").forEach((d) => { d.open = false; });
  document.addEventListener("click", (e) => { if (!e.target.closest?.(".pick")) closePickers(); });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") closePickers(); });
  $("filtersbtn").addEventListener("click", () => {
    const open = $("picks").classList.toggle("open");
    $("filtersbtn").setAttribute("aria-expanded", String(open));
  });
  $("find").addEventListener("input", () => { clearTimeout($("find").wait); $("find").wait = setTimeout(() => setFilters({ q: $("find").value }), 200); });
  $("farchived").addEventListener("click", () => setFilters({ archived: filters().archived ? "" : "1" }));
  $("fsubtasks").addEventListener("click", () => setFilters({ subtasks: filters().subtasks ? "" : "1" }));
  $("clearfilters").addEventListener("click", () => setFilters({ severity: [], owner: "", q: "", archived: "", subtasks: "" }));

  // an address that names none: the project this browser last chose
  const kept = read(CHOSEN);
  if (kept && !new URLSearchParams(location.search).has("project")) address({ project: kept });
  return { draw, setFilters };
}
