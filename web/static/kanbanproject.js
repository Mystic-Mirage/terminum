// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A project's window (terminum-kanban.md AC-138): a new empty one, or one to change: its name, server,
// folder, agents and memory limits. terminumd checks what is kept and says what is wrong.
import { askConfirm } from "./dialogs.js";
import { $, THIS_MAC, api, fillSelect } from "./kanbanutil.js";

const DEFAULT = "default";

/** `agents`: the agents' section; `send(m)`: to terminumd; `chosen(id)`: a project just made is shown. */
export function projectWindow({ getMeta, reload, agents, send, chosen }) {
  const box = $("projectbox");
  let shown = null;

  const say = (text) => { $("projecterror").textContent = text; $("projecterror").hidden = !text; };
  const drawAgents = () => {
    const rows = agents.rowsOf(shown.id);
    $("projectagentlist").replaceChildren(...rows);
    $("projectagentlist").hidden = !rows.length;
  };

  /** What it shows: `p` none for a new one. Default is no real project: it holds only agents and limits. */
  function draw(p) {
    shown = p;
    const real = p?.id !== DEFAULT;
    $("projecthead").textContent = p ? p.name : "New project";
    $("projectok").textContent = p ? "Done" : "Create";
    $("projectfields").hidden = !real;
    $("projectagents").hidden = !p;
    $("projectdelete").hidden = !p || !real;
    if (p) drawAgents();
  }

  function open(p = null) {
    fillSelect($("projecthost"), [["", THIS_MAC], ...getMeta().hosts.map((h) => [h, h])], p?.host ?? "");
    $("projecthost").disabled = !!p;                 // its sessions are on its server
    $("projectname").value = p?.name ?? "";
    $("projectdir").value = p?.dir ?? "";
    draw(p);
    say("");
    box.showModal();
    if (p?.id !== DEFAULT) $("projectname").focus();
  }

  /** Created, the window stays: now its agents can be added; else what changed is kept and it closes. */
  async function keep() {
    const name = $("projectname").value, dir = $("projectdir").value, host = $("projecthost").value;
    try {
      if (shown?.id === DEFAULT) return box.close();
      if (shown) {
        await api(`/api/task-projects/${shown.id}`, { method: "PATCH", body: { name, dir } });
        await reload();
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
  $("projectaddagent").addEventListener("click", async () => {
    await agents.addTo(shown.id);
    drawAgents();
  });
  $("projectagentlist").addEventListener("click", () => { document.getElementById("agentbox").addEventListener("close", drawAgents, { once: true }); }, true);
  $("projectlimits").addEventListener("click", () => agents.openLimits(shown.id));
  $("projectdelete").addEventListener("click", async () => {
    if (!await askConfirm(`Delete the project ${shown.name}? Its sessions and tasks move to Default; nothing is closed.`, "Delete Project")) return;
    send({ t: "project", action: "delete", project: shown.id });
    box.close();
  });
  return { open };
}
