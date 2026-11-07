// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A project's window (terminum-kanban.md AC-139): a new empty one, or one to change: its name, server,
// folder, agents and memory limits. terminumd checks what is kept and says what is wrong.
import { $, DEFAULT, api, fillSelect, hostOptions } from "./kanbanutil.js";

/** `agents`: the agents' section; `deleteProject(id)`: asks, as the session list's menu does; `chosen(id)`: a project just made is shown. */
export function projectWindow({ getMeta, reload, agents, deleteProject, chosen }) {
  const box = $("projectbox");
  let shown = null;

  const say = (text) => { $("projecterror").textContent = text; $("projecterror").hidden = !text; };
  const drawAgents = () => {
    const rows = agents.rowsOf(shown.id);
    $("projectagentlist").replaceChildren(...rows);
    $("projectagentlist").hidden = !rows.length;
  };

  /** `p` to change, none for a new one. Default is no real project: it holds only agents and limits. */
  function open(p = null) {
    shown = p;
    const real = p?.id !== DEFAULT;
    fillSelect($("projecthost"), hostOptions(getMeta().hosts), p?.host ?? "");
    $("projecthost").disabled = !!p;                 // its sessions are on its server
    $("projectname").value = p?.name ?? "";
    $("projectdir").value = p?.dir ?? "";
    $("projecthead").textContent = p ? p.name : "New project";
    $("projectok").textContent = p ? "Done" : "Create";
    $("projectfields").hidden = !real;
    $("projectagents").hidden = !p;
    $("projectdelete").hidden = !p || !real;
    if (p) drawAgents();
    say("");
    box.showModal();
    if (real) $("projectname").focus();
  }

  /** Created, the window stays: now its agents can be added; else what changed is kept and it closes. */
  async function keep() {
    const name = $("projectname").value, dir = $("projectdir").value, host = $("projecthost").value;
    try {
      if (shown?.id === DEFAULT) return box.close();
      if (shown) {
        await api(`/api/task-projects/${shown.id}`, { method: "PATCH", body: { name, dir } });   // the board hears of it
        return box.close();
      }
      const { id } = await api("/api/task-projects", { method: "POST", body: { name, dir, host } });
      await reload();
      chosen(id);
      open(getMeta().projects.find((p) => p.id === id));
    } catch (e) { say(e.message); }
  }

  $("projectok").addEventListener("click", keep);
  box.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.isComposing && e.target.matches("input")) { e.preventDefault(); keep(); }
  });
  $("projectaddagent").addEventListener("click", () => agents.addTo(shown.id));
  $("projectlimits").addEventListener("click", () => agents.openLimits(shown.id));
  $("projectdelete").addEventListener("click", () => { box.close(); deleteProject(shown.id); });
  /** What terminumd now says of its agents. */
  const refresh = () => { if (box.open && shown) drawAgents(); };
  return { open, refresh };
}
