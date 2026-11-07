// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A project's window (terminum-kanban.md AC-139): a new empty one, or one to change: its name, server,
// folder and memory limits, the pool's keys beside them. terminumd checks what is kept and says what is wrong.
import { $, DEFAULT, api, fillSelect, hostOptions } from "./kanbanutil.js";

/** `agents`: the agents' section; `archived`: its Archived list (AC-118); `deleteProject(id)`: asks, as the session list's menu does; `chosen(id)`: a project just made is shown. */
export function projectWindow({ getMeta, reload, agents, archived, deleteProject, chosen }) {
  const box = $("projectbox");
  let shown = null;

  const say = (text) => { $("projecterror").textContent = text; $("projecterror").hidden = !text; };
  /** A number field: empty is none. */
  const numberOrNull = (id) => ($(id).value === "" ? null : Number($(id).value));

  /** Its memory limits (terminum-dispatcher.md AC-128): empty goes by the global ones; 0 turns one off. */
  function openLimits(project) {
    const limits = $("limitsbox"), { projects, limits: all = [] } = getMeta();
    const now = all.find((l) => l.project === project);
    $("limitstext").textContent = `For ${projects.find((p) => p.id === project)?.name ?? project}; 0 turns one off.`;
    $("limitwarn").value = now?.warn ?? "";
    $("limitend").value = now?.end ?? "";
    $("limitserror").hidden = true;
    const kept = async (body) => {
      try {
        await api(`/api/task-projects/${project}/limits`, { method: "PUT", body });
        limits.close();
        await reload();
      } catch (e) { $("limitserror").textContent = e.message; $("limitserror").hidden = false; }
    };
    $("limitsok").onclick = () => kept({ warn: numberOrNull("limitwarn"), end: numberOrNull("limitend") });
    $("limitsglobal").onclick = () => kept({ warn: null, end: null });
    limits.showModal();
  }
  const drawAgents = () => {
    const rows = agents.rows();
    $("projectagentlist").replaceChildren(...rows);
    $("projectagentlist").hidden = !rows.length;
  };

  /** `p` to change, none for a new one. Default's place is fixed: only its limits change. */
  function open(p = null) {
    shown = p;
    const real = p?.id !== DEFAULT;
    fillSelect($("projecthost"), hostOptions(getMeta().hosts), p?.host ?? "");
    $("projecthost").disabled = !!p;                 // its sessions are on its server
    $("projectname").value = p?.name ?? "";
    $("projectdir").value = p?.dir ?? "";
    $("projectatonce").value = p?.agent_limit ?? "";
    $("projecthead").textContent = p ? p.name : "New project";
    $("projectok").textContent = p ? "Done" : "Create";
    $("projectfields").hidden = !real;
    $("projectagents").hidden = !p;
    $("projectdelete").hidden = !p || !real;
    if (p) drawAgents();
    say("");
    box.showModal();
    if (real) $(p && !p.dir ? "projectdir" : "projectname").focus();
  }

  /** Created, the window stays, with the pool's keys; else what changed is kept and it closes. */
  async function keep() {
    const name = $("projectname").value, dir = $("projectdir").value, host = $("projecthost").value;
    const atOnce = { limit: numberOrNull("projectatonce") };
    try {
      if (shown) {
        await api(`/api/task-projects/${shown.id}/agents-at-once`, { method: "PUT", body: atOnce });   // refused first: nothing else changes
        if (shown.id !== DEFAULT) await api(`/api/task-projects/${shown.id}`, { method: "PATCH", body: { name, dir } });   // the board hears of it
        return box.close();
      }
      // made first, its limit then: refused, the window stays on the new project, saying why
      const { id } = await api("/api/task-projects", { method: "POST", body: { name, dir, host } });
      await reload();
      chosen(id);
      open(getMeta().projects.find((p) => p.id === id));
      await api(`/api/task-projects/${id}/agents-at-once`, { method: "PUT", body: atOnce });
    } catch (e) { say(e.message); }
  }

  $("projectok").addEventListener("click", keep);
  box.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.isComposing && e.target.matches("input")) { e.preventDefault(); keep(); }
  });
  $("projectaddagent").addEventListener("click", () => agents.add());
  $("projectlimits").addEventListener("click", () => openLimits(shown.id));
  $("projectarchived").addEventListener("click", () => archived.open(shown));
  $("projectdelete").addEventListener("click", () => { box.close(); deleteProject(shown.id); });
  /** What terminumd now says of the pool. */
  const refresh = () => { if (box.open && shown) drawAgents(); };
  return { open, refresh };
}
