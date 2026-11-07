// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A project's agents over the columns, and their window (terminum-dispatcher.md AC-137):
// what RUN can start. terminumd checks what is kept and says what is wrong; every page hears.
import { askConfirm } from "./dialogs.js";
import { projectInk } from "./listparts.js";
import { agentMark, agentName, metaLine } from "./nav.js";
import { $, THIS_MAC, api, el, fillSelect, iconButton } from "./kanbanutil.js";

const KINDS = ["claude", "codex"];                 // their commands are their names: what a kind fills in

/** `chosen()`: the projects shown, none for all; `getMeta()`: terminumd's projects, agents and hosts. */
export function agentsSection({ getMeta, chosen, reload }) {
  const box = el("section", "agents"), list = el("div", "agentlist");
  const add = iconButton("icon", "plus", "Add agent", "Add an agent to a project", () => open());
  add.id = "addagent";
  box.append(list, add);
  const field = { project: $("agentproject"), name: $("agentname"), kind: $("agentkind"), host: $("agenthost"), dir: $("agentdir"), command: $("agentcommand") };
  const dialog = $("agentbox"), error = $("agenterror");
  let editing = null, snapshot = "";

  function row(a, several) {
    const { projects } = getMeta();
    const p = projects.find((x) => x.id === a.project);
    const b = el("button", "item");
    b.type = "button";
    b.dataset.agent = a.id;
    const bar = el("span", "pbar");
    bar.style.setProperty("--dot", projectInk(p));
    const txt = el("span", "txt"), line = el("span", "head");
    line.append(agentMark({ agent: a.kind }), el("span", "t", a.name));
    txt.append(line, metaLine([...(several ? [p?.name ?? a.project] : []), a.host ?? THIS_MAC, a.command], false));
    b.append(bar, txt, el("span", "st"));
    b.title = [a.name, a.host ?? THIS_MAC, a.dir || "the project's folder", a.command].join(" · ");
    b.addEventListener("click", () => edit(a));
    return b;
  }

  function draw() {
    const picked = chosen();
    const agents = getMeta().agents.filter((a) => !picked.length || picked.includes(a.project));
    list.replaceChildren(...agents.map((a) => row(a, picked.length !== 1)));
    if (!agents.length) list.append(el("p", "colempty", "No agents yet: add one to run tasks."));
  }

  /** The rows of `project`'s agents, for its own window. */
  const rowsOf = (project) => getMeta().agents.filter((a) => a.project === project).map((a) => row(a, false));

  const values = () => Object.fromEntries(Object.entries(field).map(([k, e]) => [k, e.value]));
  const say = (text) => { error.textContent = text; error.hidden = !text; };

  // what a kind fills in follows the kind; what the user wrote stays
  field.kind.addEventListener("change", () => {
    const kind = field.kind.value;
    if (!field.name.value.trim() || KINDS.some((k) => agentName(k) === field.name.value)) field.name.value = agentName(kind);
    if (!field.command.value.trim() || KINDS.includes(field.command.value)) field.command.value = kind;
  });

  /** Its window: a new one in the project shown, or `a` to change; `given`: a new one in that project. */
  function open(a = null, given = null) {
    const { projects, hosts } = getMeta();
    editing = a;
    const picked = chosen();
    const project = a?.project ?? given ?? (picked.length === 1 ? picked[0] : "default");
    const of = projects.find((p) => p.id === project);
    const v = a ?? { host: of?.host, dir: "", kind: "claude", name: agentName("claude"), command: "claude" };
    fillSelect(field.project, projects.map((p) => [p.id, p.name]), project);
    field.project.disabled = !!a;
    fillSelect(field.host, [["", THIS_MAC], ...hosts.map((h) => [h, h])], v.host ?? "");
    for (const k of ["kind", "name", "dir", "command"]) field[k].value = v[k];
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

  const closed = () => new Promise((resolve) => dialog.addEventListener("close", resolve, { once: true }));
  /** A new one in `project`; once its window closes. */
  const addTo = (project) => { open(null, project); return closed(); };
  const edit = (a) => { open(a); return closed(); };
  return { box, draw, addTo, edit, rowsOf, openLimits };
}
