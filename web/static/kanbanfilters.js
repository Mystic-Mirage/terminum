// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The board's filters, kept in its URL (terminum-kanban.md AC-111), and the project it shows, which
// this browser remembers (AC-135).
import { projectColor } from "./listparts.js";
import { $, SEVERITIES, button, el, read, write } from "./kanbanutil.js";

const CHOSEN = "board.project";

export const filters = () => {
  const q = new URLSearchParams(location.search);
  const list = (k) => (q.get(k) ?? "").split(",").filter(Boolean);
  return { project: list("project"), severity: list("severity"), owner: q.get("owner") ?? "", q: q.get("q") ?? "" };
};

export const hasFilter = () => { const f = filters(); return f.project.length || f.severity.length || f.owner || f.q; };

export function query(extra = {}) {
  const f = filters(), p = new URLSearchParams();
  for (const [k, v] of Object.entries({ project: f.project.join(","), severity: f.severity.join(","), owner: f.owner, q: f.q, ...extra })) if (v !== "" && v !== undefined) p.set(k, v);
  return p.toString();
}

/** A project's color, the session list's; Default has none. */
export const projectHue = (p) => (p?.color >= 0 ? projectColor(p) : "var(--muted)");

/** The filters' keys over `getMeta()`'s projects; `changed` after any change, `newProject` for its menu's item. */
export function filterBar({ getMeta, changed, newProject }) {
  function setFilters(next) {
    const u = new URL(location.href), was = filters();
    for (const [k, v] of Object.entries({ ...was, ...next })) {
      const s = Array.isArray(v) ? v.join(",") : v;
      if (s) u.searchParams.set(k, s); else u.searchParams.delete(k);
    }
    history.replaceState(null, "", u);
    if ("project" in next) write(CHOSEN, next.project.join(",") || null);
    draw();
    changed();
  }

  /** The ticks of one filter's menu. */
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
    const summary = box.querySelector("summary");
    summary.querySelector(".count")?.remove();
    if (chosen.length) summary.append(el("span", "count", String(chosen.length)));
  }

  const dot = (p) => {
    const d = el("span", "pdot");
    d.style.setProperty("--proj", projectHue(p));
    return d;
  };

  /** Its menu: a name chooses that project alone, a tick adds it to those shown (AC-135). */
  function drawProjects(chosen) {
    const box = $("fproject"), { projects } = getMeta();
    const only = (ids) => { box.open = false; setFilters({ project: ids }); };
    const rows = projects.map((p) => {
      const line = el("div", "projrow");
      const tick = el("label", "projtick");
      const c = document.createElement("input");
      c.type = "checkbox";
      c.checked = chosen.includes(p.id);
      c.setAttribute("aria-label", `Show ${p.name} too`);
      c.addEventListener("change", () => setFilters({ project: c.checked ? [...filters().project, p.id] : filters().project.filter((x) => x !== p.id) }));
      tick.append(c);
      const name = button(null, "", () => only([p.id]));
      name.append(dot(p), el("span", "pname", p.name));
      line.append(tick, name);
      return line;
    });
    box.querySelector("div").replaceChildren(button(null, "All projects", () => only([])), ...rows, newProject());
    const first = projects.find((p) => p.id === chosen[0]);
    const words = !chosen.length ? "All projects" : `${first?.name ?? chosen[0]}${chosen.length > 1 ? ` +${chosen.length - 1}` : ""}`;
    box.querySelector("summary").replaceChildren(...(first ? [dot(first)] : []), el("span", "pname", words));
  }

  function draw() {
    const f = filters();
    drawProjects(f.project);
    picks($("fseverity"), SEVERITIES.map((s) => ({ id: s, name: s })), f.severity, (id, on) => setFilters({ severity: on ? [...filters().severity, id] : filters().severity.filter((x) => x !== id) }));
    picks($("fowner"), [{ id: "humans", name: "People" }, { id: "agents", name: "Agents" }], f.owner ? [f.owner] : [], (id, on) => setFilters({ owner: on ? id : "" }));
    if ($("find").value !== f.q) $("find").value = f.q;
    const on = f.severity.length + (f.owner ? 1 : 0);
    $("clearfilters").hidden = !(on || f.q);
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
  $("clearfilters").addEventListener("click", () => setFilters({ severity: [], owner: "", q: "" }));

  // an address that names none: the project this browser last chose
  const kept = read(CHOSEN);
  if (kept && !new URLSearchParams(location.search).has("project")) {
    const u = new URL(location.href);
    u.searchParams.set("project", kept);
    history.replaceState(history.state, "", u);
  }
  return { draw, setFilters };
}
