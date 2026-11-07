// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A project's agents over the columns, and their window (terminum-dispatcher.md AC-137, AC-138): what a
// task is dropped on to run: each a rectangle with its color, name, kind, effort and what it does now.
// terminumd checks what is kept and says what is wrong; every page hears.
import { askConfirm } from "./dialogs.js";
import { projectInk } from "./listparts.js";
import { agentMark, agentName, metaLine } from "./nav.js";
import { el, iconButton } from "./listparts.js";
import { $, DEFAULT, THIS_MAC, agentColor, api, fillSelect, hostOptions, showError } from "./kanbanutil.js";

const KINDS = ["claude", "codex"];                 // their commands are their names: what a kind fills in

/** `chosen()`: the projects shown, none for all; `getMeta()`: terminumd's projects, agents and hosts; `busy()`: what
 *  each busy agent does; `liveRow(key)`: a session's row; `dragging()`: the tasks of a drag; `given()`: those a press or a drop
 *  would run on an agent; `run(tasks, agent)`. */
export function agentsSection({ getMeta, chosen, reload, busy, liveRow, dragging, given, run, openSession }) {
  const box = el("section", "agents"), list = el("div", "agentlist");
  const add = iconButton("icon", "plus", "Add agent", "Add an agent to a project", () => open());
  add.id = "addagent";
  const hint = el("p", "pickhint");
  hint.hidden = true;
  box.append(list, add, hint);
  const field = { project: $("agentproject"), name: $("agentname"), kind: $("agentkind"), host: $("agenthost"), effort: $("agenteffort"), dir: $("agentdir"), command: $("agentcommand") };
  const dialog = $("agentbox"), error = $("agenterror");
  let editing = null, snapshot = "";

  /** What it does now, in a word or two; its tasks when busy. */
  function doing(now) {
    if (!now) return "free";
    const row = now.key ? liveRow(now.key) : null;
    const state = ["starting", "typed"].includes(now.state) ? "starting" : now.state === "asking" || row?.state === "waiting" ? "waits for you" : "working";
    return `${state}: ${now.tasks}`;
  }

  /** Its rectangle: its color, name, kind, effort and what it does now. */
  function row(a, several) {
    const p = getMeta().projects.find((x) => x.id === a.project), now = busy()[a.id];
    const b = el("button", `item agentcard${now ? " busy" : ""}`);
    b.type = "button";
    b.dataset.agent = a.id;
    const bar = el("span", `pbar${now ? " busy" : ""}`);
    bar.style.setProperty("--dot", agentColor(a));
    b.style.setProperty("--ag", agentColor(a));
    const txt = el("span", "txt"), line = el("span", "head"), words = doing(now);
    line.append(agentMark({ agent: a.kind }), el("span", "t", a.name));
    if (several) line.append(el("span", "agproj", p?.name ?? a.project));
    txt.append(line, metaLine([agentName(a.kind), ...(a.effort ? [a.effort] : [])], false), el("span", "agstatus", words));
    b.append(bar, txt);
    b.title = [a.name, words, a.host ?? THIS_MAC, a.dir || "the project's folder", a.line].join(" · ");
    return b;
  }

  /** Over the columns it takes tasks: dropped or pressed, terminumd says if it cannot; else a press opens what it does, or it. */
  function target(a, several) {
    const b = row(a, several), now = busy()[a.id];
    b.append(iconButton("agedit", "pencil", `Edit ${a.name}`, `Edit ${a.name}`, () => open(a)));
    b.addEventListener("click", () => {
      const tasks = given();
      if (tasks) run(tasks, a.id);
      else if (now?.key) openSession(now.key);
      else open(a);
    });
    b.addEventListener("dragover", (e) => {
      if (!dragging()) return;
      e.preventDefault();
      if (!now && dragging().every((t) => t.project === a.project)) b.classList.add("over");
    });
    b.addEventListener("dragleave", () => b.classList.remove("over"));
    b.addEventListener("drop", (e) => {
      const tasks = dragging();
      if (!tasks) return;                          // a file: the page's
      e.preventDefault();
      run(tasks, a.id);
    });
    return b;
  }

  let drawn = "";
  /** Anew only when what it shows changed, and not under a drag: a press spanning a read keeps its key. */
  function draw() {
    const picked = chosen(), tasks = given();
    const agents = getMeta().agents.filter((a) => !picked.length || picked.includes(a.project));
    const now = JSON.stringify([agents, picked.length !== 1, agents.map((a) => [busy()[a.id], doing(busy()[a.id])]), getMeta().projects, tasks?.length]);
    if (dragging() || now === drawn) return;
    drawn = now;
    list.replaceChildren(...agents.map((a) => target(a, picked.length !== 1)));
    if (!agents.length) list.append(el("p", "colempty", "No agents yet: add one to run tasks."));
    hint.hidden = !tasks;
    if (tasks) hint.textContent = `${tasks.length} chosen: press an agent or drop them on it. Esc lets them go.`;
  }

  /** The rows of `project`'s agents, for its own window: a press opens one. */
  const rowsOf = (project) => getMeta().agents.filter((a) => a.project === project).map((a) => {
    const b = row(a, false);
    b.addEventListener("click", () => open(a));
    return b;
  });

  /** The efforts of the kind chosen; one it does not take goes by the agent's own setting. */
  function fillEfforts(value) {
    const levels = getMeta().efforts?.[field.kind.value] ?? [];
    fillSelect(field.effort, [["", "Its own setting"], ...levels.map((e) => [e, e])], levels.includes(value) ? value : "");
  }

  const values = () => Object.fromEntries(Object.entries(field).map(([k, e]) => [k, e.value]));
  const say = (text) => showError(error, text);

  // what a kind fills in follows the kind; what the user wrote stays
  field.kind.addEventListener("change", () => {
    const kind = field.kind.value;
    if (!field.name.value.trim() || KINDS.some((k) => field.name.value.startsWith(agentName(k)))) field.name.value = freeName(kind, field.project.value);
    if (!field.command.value.trim() || KINDS.includes(field.command.value)) field.command.value = kind;
    fillEfforts(field.effort.value);
  });

  /** A kind's name not yet taken in `project`: Claude Code, Claude Code 2… */
  function freeName(kind, project) {
    const taken = new Set(getMeta().agents.filter((x) => x.project === project).map((x) => x.name));
    const base = agentName(kind);
    if (!taken.has(base)) return base;
    let n = 2;
    while (taken.has(`${base} ${n}`)) n++;
    return `${base} ${n}`;
  }

  /** Its window: a new one in the project shown, or `a` to change; `given`: a new one in that project. */
  function open(a = null, given = null) {
    const { projects, hosts } = getMeta();
    editing = a;
    const picked = chosen();
    const project = a?.project ?? given ?? (picked.length === 1 ? picked[0] : DEFAULT);
    const of = projects.find((p) => p.id === project);
    const v = a ?? { host: of?.host, dir: "", kind: "claude", name: freeName("claude", project), command: "claude", effort: "" };
    fillSelect(field.project, projects.map((p) => [p.id, p.name]), project);
    field.project.disabled = !!a;
    field.project.closest("label").hidden = !!a || !!given;   // its project's window said which
    fillSelect(field.host, hostOptions(hosts), v.host ?? "");
    for (const k of ["kind", "name", "dir", "command"]) field[k].value = v[k];
    fillEfforts(v.effort);
    field.dir.placeholder = of?.dir || "The project's folder";
    $("agenthead").textContent = a ? a.name : "Add agent";
    $("agentok").textContent = a ? "Save" : "Add";
    $("agentdrop").hidden = !a;
    say("");
    snapshot = JSON.stringify(values());
    dialog.showModal();
    field.name.select();
  }

  /** A change to terminumd: the window closes when it is kept, and says why when it is not. */
  async function send(path, options) {
    try {
      await api(path, options);
      dialog.close();
      await reload();
    } catch (e) { say(e.message); }
  }

  function keep() {
    const { project, ...body } = values();
    return send(editing ? `/api/task-agents/${editing.id}` : `/api/task-projects/${project}/agents`, { method: editing ? "PATCH" : "POST", body });
  }

  $("agentok").addEventListener("click", keep);
  dialog.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.isComposing && e.target.matches("input")) { e.preventDefault(); keep(); }
  });
  $("agentdrop").addEventListener("click", async () => {
    if (await askConfirm(`Delete the agent ${editing.name}? Its tasks and sessions stay.`, "Delete")) send(`/api/task-agents/${editing.id}`, { method: "DELETE" });
  });
  // what was typed is not lost by a slip of Esc or × (terminum-kanban.md AC-133); Cancel means it
  dialog.addEventListener("cancel", async (e) => {
    if (JSON.stringify(values()) === snapshot) return;
    e.preventDefault();
    if (await askConfirm("Discard what you typed?", "Discard")) dialog.close();
  });

  /** A project's memory limits (terminum-dispatcher.md AC-128): empty goes by the global ones; 0 turns one off. */
  function openLimits(project) {
    const box = $("limitsbox"), { projects, limits: all = [] } = getMeta();
    const now = all.find((l) => l.project === project);
    $("limitstext").textContent = `For ${projects.find((p) => p.id === project)?.name ?? project}; 0 turns one off.`;
    $("limitwarn").value = now?.warn ?? "";
    $("limitend").value = now?.end ?? "";
    $("limitserror").hidden = true;
    const keep = async (body) => {
      try {
        await api(`/api/task-projects/${project}/limits`, { method: "PUT", body });
        box.close();
        await reload();
      } catch (e) { $("limitserror").textContent = e.message; $("limitserror").hidden = false; }
    };
    const gib = (id) => ($(id).value === "" ? null : Number($(id).value));
    $("limitsok").onclick = () => keep({ warn: gib("limitwarn"), end: gib("limitend") });
    $("limitsglobal").onclick = () => keep({ warn: null, end: null });
    box.showModal();
  }

  /** A new one in `project`; once its window closes. */
  const addTo = (project) => {
    open(null, project);
    return new Promise((resolve) => dialog.addEventListener("close", resolve, { once: true }));
  };
  return { box, draw, addTo, rowsOf, openLimits };
}
