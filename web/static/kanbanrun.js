// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// RUN's window (terminum-dispatcher.md AC-121, AC-122): which of the project's agents starts the
// task, where, by what command and with what words; the last chosen is already picked, one press.
import { $, THIS_MAC, api, fillSelect, showError } from "./kanbanutil.js";

/** `addAgent(project)` opens the agents' window; `done()` after a run started. */
export function runWindow({ addAgent, done }) {
  const box = $("runbox");
  let task = null, offer = null, comment = "", attached = [], fresh = false, answer = () => {};

  const say = (text) => showError($("runerror"), text);
  const chosen = () => offer.agents.find((a) => a.id === $("runagent").value);

  function showAgent() {
    const a = chosen(), dir = a?.dir || offer.project_dir;
    $("runserver").textContent = a?.host ?? THIS_MAC;
    $("runcommand").textContent = a?.command ?? "";
    $("runfolder").placeholder = dir || "Where it runs";
    if (!$("runfolder").dataset.typed) $("runfolder").value = dir ?? "";
  }

  function draw(scratch) {
    fresh = scratch;
    const first = fresh || offer.first, none = first && !offer.agents.length;
    $("runhead").textContent = first ? `Run ${task.id}` : `Run ${task.id} again`;
    $("runnone").hidden = !none && first;
    $("runnone").textContent = none ? `No agents in ${offer.project_name} yet.` : first ? "" : "It goes on in its session, with what you wrote.";
    for (const id of ["runfields", "runfacts"]) $(id).hidden = !first || none;
    $("runprompt").hidden = !first;
    $("runmarked").hidden = !first || !offer.by_agent;
    $("runadd").hidden = !none;
    $("runfresh").hidden = first;
    $("rungo").disabled = none;
    if (first) {
      fillSelect($("runagent"), offer.agents.map((a) => [a.id, a.name]), offer.last ?? offer.agents[0]?.id);
      $("runprompt").textContent = offer.prompt;
      delete $("runfolder").dataset.typed;
      showAgent();
    }
  }

  const read = async () => { offer = await api(`/api/tasks/${task.id}/run`); };

  /** Opens it for task `t`: "run" when a run started, "move" for Only move it, else neither; a refusal
   *  throws terminumd's words. `words`, `files`: the comment that goes with it. */
  async function open(t, { words = "", files = [], canMove = false } = {}) {
    task = t;
    comment = words;
    attached = files;
    await read();
    draw(false);
    $("runjust").hidden = !canMove;
    say("");
    box.returnValue = "";
    box.showModal();
    $("rungo").focus();
    return new Promise((resolve) => { answer = resolve; });
  }

  async function go() {
    say("");
    $("rungo").disabled = true;
    try {
      await api(`/api/tasks/${task.id}/run`, { method: "POST", body: { agent: $("runagent").value, folder: $("runfolder").value, comment, files: attached, fresh, prompt_rev: offer.prompt_rev } });
      box.close("run");
      done();
    } catch (e) { say(e.message); }
    $("rungo").disabled = false;
  }

  $("rungo").addEventListener("click", go);
  $("runagent").addEventListener("change", showAgent);
  $("runfolder").addEventListener("input", () => { $("runfolder").dataset.typed = "1"; });
  $("runfresh").addEventListener("click", () => draw(true));
  $("runjust").addEventListener("click", () => box.close("move"));
  $("runadd").addEventListener("click", async () => {
    await addAgent(offer.project);
    await read();
    draw(true);
  });
  box.addEventListener("close", () => answer(box.returnValue));
  return { open };
}
