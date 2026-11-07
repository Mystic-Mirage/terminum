// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// RUN's window (terminum-dispatcher.md AC-121, AC-122, AC-138): which of the pool's free agents starts the
// tasks and with what message; where is the project's (AC-140), said here; the agent dropped on, or the last
// chosen, already picked: one press. Several tasks go by their numbers; what each says shows here first.
import { $, THIS_MAC, api, fillAgents, onSend } from "./kanbanutil.js";

/** `addAgent()` opens the agents' window; `noFolder(project, words)`: terminumd says nothing can start there; `done()` after a run started. */
export function runWindow({ addAgent, noFolder, done }) {
  const box = $("runbox");
  let tasks = [], offer = null, attached = [], fresh = false, dropped = null, answer = () => {};

  const say = (text) => { $("runerror").textContent = text; $("runerror").hidden = !text; };
  const chosen = () => offer.agents.find((a) => a.id === $("runagent").value);
  const free = () => offer.agents.filter((a) => !a.busy);

  function showAgent() {
    $("runserver").textContent = offer.host ?? THIS_MAC;
    $("runfolder").textContent = offer.dir;
    $("runcommand").textContent = chosen()?.line ?? "";
  }

  function draw(scratch) {
    fresh = scratch;
    const first = fresh || offer.first, none = first && !offer.agents.length, allBusy = first && !none && !free().length;
    const names = tasks.map((t) => t.id).join(", "), on = offer.agents.find((a) => a.id === dropped);
    $("runhead").textContent = !first ? `Run ${names} again` : on ? `Run ${names} on ${on.name}` : `Run ${names}`;
    const words = none ? "No agents yet." : allBusy ? `All agents are busy: one takes more when its session closes or its tasks end.` : first ? "" : "It goes on in its session, with what you write.";
    $("runnone").textContent = words;
    $("runnone").hidden = !words;
    // the agent it was dropped on is said by the title; what a person checks only sometimes stays folded
    $("runagentrow").hidden = !first || none || !!on;
    $("runhow").hidden = !first || none;
    $("runwhat").hidden = !first;
    $("runwhat").open = offer.by_agent || tasks.length > 1;   // several, or an agent's words: read before they go (AC-121)
    $("runmarked").hidden = !offer.by_agent;
    $("runadd").hidden = !none;
    $("runfresh").hidden = first;
    $("rungo").disabled = none || allBusy;
    if (first) {
      fillAgents($("runagent"), offer.agents, (a) => a.busy, [dropped, offer.last]);
      $("runprompt").textContent = offer.prompt;
      showAgent();
    }
  }

  const others = () => tasks.slice(1).map((t) => t.id);
  const read = async () => {
    const q = new URLSearchParams({ also: others().join(","), ...(dropped ? { agent: dropped } : {}) });
    offer = await api(`/api/tasks/${tasks[0].id}/run?${q}`);
  };

  /** Opens it for `list`, on `agent` if chosen: true when a run started, else false; a refusal throws terminumd's
   *  words. `words`, `files`: the comment that goes with it; `why`: what a press already found wrong. */
  async function open(list, { words = "", files = [], agent = null, why = "" } = {}) {
    tasks = list;
    dropped = agent;
    attached = files;
    $("runmessage").value = words;
    await read();
    if (offer.no_folder) {
      noFolder(offer.project, offer.no_folder);
      return false;
    }
    draw(false);
    say(why);
    box.returnValue = "";
    box.showModal();
    $("runmessage").focus();
    return new Promise((resolve) => { answer = resolve; });
  }

  async function go() {
    say("");
    $("rungo").disabled = true;
    try {
      await api(`/api/tasks/${tasks[0].id}/run`, { method: "POST", body: { agent: fresh || offer.first ? $("runagent").value : null, comment: $("runmessage").value, files: attached, fresh, also: others(), prompt_rev: offer.prompt_rev } });   // going on, it is its session's agent
      box.close("run");
      done();
    } catch (e) { say(e.message); }
    $("rungo").disabled = false;
  }

  $("rungo").addEventListener("click", go);
  onSend($("runmessage"), go);
  $("runagent").addEventListener("change", showAgent);
  $("runfresh").addEventListener("click", () => draw(true));
  $("runadd").addEventListener("click", async () => {
    await addAgent();
    await read();
    draw(true);
  });
  box.addEventListener("close", () => answer(box.returnValue === "run"));
  return { open };
}
