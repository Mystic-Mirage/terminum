// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// RUN's window (terminum-dispatcher.md AC-121, AC-122): which of the project's agents starts the
// task, where, by what command and with what words; the last chosen is already picked, one press.
import { $, api, fillSelect } from "./kanbanutil.js";

const THIS_MAC = "This Mac";

/** `addAgent(project)` opens the agents' window; `done()` after a run started. */
export function runWindow({ addAgent, done }) {
  const box = $("runbox");
  let task = null, offer = null, comment = "", attached = [], answer = () => {};

  const say = (text) => { $("runerror").textContent = text; $("runerror").hidden = !text; };
  const chosen = () => offer.agents.find((a) => a.id === $("runagent").value);

  function showAgent() {
    const a = chosen();
    $("runserver").textContent = a?.host ?? THIS_MAC;
    $("runcommand").textContent = a?.command ?? "";
    $("runfolder").placeholder = a?.dir || offer.project_dir || "Where it runs";
    if (!$("runfolder").dataset.typed) $("runfolder").value = a?.dir || offer.project_dir || "";
  }

  function draw(fresh) {
    const first = fresh || offer.way === "first";
    const none = first && !offer.agents.length;
    $("runhead").textContent = first ? `Run ${task.id}` : `Run ${task.id} again`;
    $("runnone").hidden = !none && first;
    $("runnone").textContent = none ? `No agents in ${offer.project_name ?? "this project"} yet.` : first ? "" : "It goes on in its session, with what you wrote.";
    $("runfields").hidden = !first || none;
    $("runfacts").hidden = !first || none;
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
    offer.fresh = fresh;
  }

  async function read() {
    offer = await api(`/api/tasks/${task.id}/run`);
  }

  /** Opens it for task `t`: true when a run started, "move" for Only move it, else false.
   *  `words`: the comment that goes with it. */
  async function open(t, { words = "", files = [], canMove = false } = {}) {
    task = t;
    comment = words;
    attached = files;
    await read();
    if (offer.way === "off") throw new Error(offer.why);
    draw(false);
    $("runjust").hidden = !canMove;
    say("");
    box.showModal();
    $("rungo").focus();
    return new Promise((resolve) => { answer = resolve; });
  }

  async function go() {
    say("");
    $("rungo").disabled = true;
    try {
      await api(`/api/tasks/${task.id}/run`, { method: "POST", body: { agent: $("runagent").value, folder: $("runfolder").value, comment, files: attached, fresh: !!offer.fresh } });
      const end = answer;
      answer = () => {};
      box.close();
      end(true);
      done();
    } catch (e) { say(e.message); }
    $("rungo").disabled = false;
  }

  $("rungo").addEventListener("click", go);
  $("runagent").addEventListener("change", showAgent);
  $("runfolder").addEventListener("input", () => { $("runfolder").dataset.typed = "1"; });
  $("runfresh").addEventListener("click", () => draw(true));
  $("runjust").addEventListener("click", () => { const end = answer; answer = () => {}; box.close(); end("move"); });
  $("runadd").addEventListener("click", async () => {
    await addAgent(offer.project);
    await read();
    draw(true);
  });
  box.addEventListener("close", () => { const end = answer; answer = () => {}; end(false); });
  return { open };
}
