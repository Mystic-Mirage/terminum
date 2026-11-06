// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A project's agents under the Sessions column, and their window (terminum-dispatcher.md AC-137):
// what RUN can start. terminumd checks what is kept and says what is wrong; every page hears.
import { askConfirm } from "./dialogs.js";
import { projectInk } from "./listparts.js";
import { agentMark, metaLine } from "./nav.js";
import { $, api, el, fillSelect, iconButton } from "./kanbanutil.js";

const KINDS = { claude: { name: "Claude Code", command: "claude" }, codex: { name: "Codex", command: "codex" } };
const THIS_MAC = "This Mac";

/** `chosen()`: the projects shown, none for all; `getMeta()`: terminumd's projects, agents and hosts. */
export function agentsSection({ getMeta, chosen, reload }) {
  const box = el("section", "agents"), head = el("h2", "colhead"), list = el("div", "agentlist");
  const add = iconButton("icon", "plus", "Add agent", "Add an agent to a project", () => open());
  add.id = "addagent";
  head.append(el("span", null, "Agents"), add);
  box.append(head, list);
  const field = { project: $("agentproject"), name: $("agentname"), kind: $("agentkind"), host: $("agenthost"), dir: $("agentdir"), command: $("agentcommand") };
  const dialog = $("agentbox"), error = $("agenterror");
  let editing = null, snapshot = "";

  const shownNow = () => {
    const picked = chosen();
    return getMeta().agents.filter((a) => !picked.length || picked.includes(a.project));
  };

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
    txt.append(line, metaLine([...(several ? [p?.name ?? "Default"] : []), a.host ?? THIS_MAC, a.command], false));
    b.append(bar, txt, el("span", "st"));
    b.title = [a.name, a.host ?? THIS_MAC, a.dir || "the project's folder", a.command].join(" · ");
    b.addEventListener("click", () => open(a));
    return b;
  }

  function draw() {
    const agents = shownNow();
    const several = chosen().length !== 1;
    list.replaceChildren(...agents.map((a) => row(a, several)));
    if (!agents.length) list.append(el("p", "colempty", "No agents yet: add one to run tasks."));
  }

  const values = () => Object.fromEntries(Object.entries(field).map(([k, e]) => [k, e.value]));
  const say = (text) => { error.textContent = text; error.hidden = !text; };

  /** What its kind fills in stays filled as the kind changes; what the user wrote stays. */
  function kindChanged(was) {
    const old = KINDS[was], now = KINDS[field.kind.value];
    if (!field.name.value.trim() || field.name.value === old.name) field.name.value = now.name;
    if (!field.command.value.trim() || field.command.value === old.command) field.command.value = now.command;
  }
  let kindWas = "claude";
  field.kind.addEventListener("change", () => { kindChanged(kindWas); kindWas = field.kind.value; });

  /** Its window: a new one in the project shown, or `a` to change. */
  function open(a = null) {
    const { projects, hosts } = getMeta();
    editing = a;
    const picked = chosen();
    const project = a?.project ?? (picked.length === 1 ? picked[0] : "default");
    fillSelect(field.project, projects.map((p) => [p.id, p.name]), project);
    field.project.disabled = !!a;
    const of = projects.find((p) => p.id === project);
    fillSelect(field.host, [["", THIS_MAC], ...hosts.map((h) => [h, h])], a ? a.host ?? "" : of?.host ?? "");
    field.kind.value = kindWas = a?.kind ?? "claude";
    field.name.value = a?.name ?? KINDS.claude.name;
    field.dir.value = a?.dir ?? "";
    field.dir.placeholder = of?.dir || "The project's folder";
    field.command.value = a?.command ?? KINDS.claude.command;
    $("agenthead").textContent = a ? a.name : "Add agent";
    $("agentok").textContent = a ? "Save" : "Add";
    $("agentdrop").hidden = !a;
    say("");
    snapshot = JSON.stringify(values());
    dialog.showModal();
    field.name.select();
  }

  async function keep() {
    const { project, ...body } = values();
    try {
      if (editing) await api(`/api/task-agents/${editing.id}`, { method: "PATCH", body });
      else await api(`/api/task-projects/${project}/agents`, { method: "POST", body });
      dialog.close();
      await reload();
    } catch (e) { say(e.message); }
  }

  $("agentok").addEventListener("click", keep);
  dialog.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.isComposing && e.target.matches("input")) { e.preventDefault(); keep(); }
  });
  $("agentdrop").addEventListener("click", async () => {
    if (!await askConfirm(`Delete the agent ${editing.name}? Its tasks and sessions stay.`, "Delete")) return;
    try {
      await api(`/api/task-agents/${editing.id}`, { method: "DELETE" });
      dialog.close();
      await reload();
    } catch (e) { say(e.message); }
  });
  // what was typed is not lost by a slip of Esc or × (terminum-kanban.md AC-133); Cancel means it
  dialog.addEventListener("cancel", async (e) => {
    if (JSON.stringify(values()) === snapshot) return;
    e.preventDefault();
    if (await askConfirm("Discard what you typed?", "Discard")) dialog.close();
  });

  return { box, draw };
}
