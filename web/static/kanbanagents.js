// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The pool over the columns, and its window (terminum-dispatcher.md AC-137, AC-138): what a task is dropped
// on to run: each a rectangle with its color, name, kind, effort and a dot per live session. None is a project's.
// terminumd checks what is kept and says what is wrong; every page hears.
import { askConfirm } from "./dialogs.js";
import { agentMark, agentName, metaLine } from "./nav.js";
import { $, agentColor, api, el, fillSelect, iconButton } from "./kanbanutil.js";

const KINDS = ["claude", "codex"];                 // their commands are their names: what a kind fills in

/** `getMeta()`: terminumd's agents; `chosen()`: the projects shown, none for all; `slots()`: what each agent's slots hold, as terminumd counts them;
 *  `liveRow(key)`: a session's row; `dragging()`: the tasks of a drag; `given()`: those a press or a drop would run on an
 *  agent; `run(tasks, agent)`. */
export function agentsSection({ getMeta, chosen, reload, slots: counted, liveRow, dragging, given, run, openSession }) {
  const box = el("section", "agents"), list = el("div", "agentlist");
  const addKey = iconButton("icon", "plus", "Add agent", "Add an agent to the pool", () => open());
  addKey.id = "addagent";
  const hint = el("p", "pickhint");
  hint.hidden = true;
  box.append(list, addKey, hint);
  const field = { name: $("agentname"), kind: $("agentkind"), effort: $("agenteffort"), command: $("agentcommand"), at_once: $("agentatonce") };
  const dialog = $("agentbox"), error = $("agenterror");
  let editing = null, snapshot = "";

  const slots = (a) => counted().agents[a.id] ?? [];
  /** At a limit, as terminumd says: its own, or the one project shown's. */
  const full = (a) => counted().full.includes(a.id) || (chosen().length === 1 && counted().full_projects.includes(chosen()[0]));

  const WORDS = { starting: "starting", working: "working", waits: "waits for you", idle: "idle" };
  /** What a session does now. */
  function doing(s) {
    if (s.idle) return "idle";
    const row = s.key ? liveRow(s.key) : null;
    return ["starting", "typed"].includes(s.state) ? "starting" : s.state === "asking" || row?.state === "waiting" ? "waits" : "working";
  }

  /** A session's dot: blinking while it works, still when it waits or idles; a press on it opens it. */
  function dot(s) {
    const now = doing(s), d = el("span", `agdot ${now === "working" || now === "starting" ? "on" : now}`);
    d.dataset.key = s.key ?? "";
    d.title = [s.tasks, getMeta().projects.find((p) => p.id === s.project)?.name, WORDS[now]].filter(Boolean).join(" · ");
    return d;
  }

  /** Its rectangle: its color, name, kind, effort and a dot for each session of what is shown; its limit counts them all. */
  function row(a) {
    const shown = chosen(), mine = slots(a).filter((s) => !shown.length || shown.includes(s.project));
    const b = el("button", `item agentcard${full(a) ? " busy" : ""}`);
    b.type = "button";
    b.dataset.agent = a.id;
    const bar = el("span", `pbar${slots(a).length ? " busy" : ""}`);
    bar.style.setProperty("--dot", agentColor(a));
    b.style.setProperty("--ag", agentColor(a));
    const txt = el("span", "txt"), line = el("span", "head"), dots = el("span", "agdots");
    dots.append(...mine.map(dot));
    if (a.at_once > 1) dots.append(el("small", "agcount", `${slots(a).length}/${a.at_once}`));
    line.append(agentMark({ agent: a.kind }), el("span", "t", a.name));
    txt.append(line, metaLine([agentName(a.kind), ...(a.effort ? [a.effort] : [])], false), dots);
    b.append(bar, txt);
    b.title = [a.name, `${slots(a).length} of ${a.at_once} at once`, a.line].join(" · ");
    return b;
  }

  /** Over the columns it takes tasks: dropped or pressed, terminumd says if it cannot; a dot opens its session, else a press opens its window. */
  function target(a) {
    const b = row(a);
    b.append(iconButton("agedit", "pencil", `Edit ${a.name}`, `Edit ${a.name}`, () => open(a)));
    b.addEventListener("click", (e) => {
      const tasks = given(), key = e.target.closest?.(".agdot")?.dataset.key;
      if (tasks) run(tasks, a.id);
      else if (key) openSession(key);
      else open(a);
    });
    b.addEventListener("dragover", (e) => {
      if (!dragging()) return;
      e.preventDefault();
      if (!full(a)) b.classList.add("over");
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
    const tasks = given(), agents = getMeta().agents;
    const now = JSON.stringify([agents, chosen(), counted().full, counted().full_projects, agents.map((a) => slots(a).map((s) => [s, doing(s)])), tasks?.length]);
    if (dragging() || now === drawn) return;
    drawn = now;
    list.replaceChildren(...agents.map(target));
    if (!agents.length) list.append(el("p", "colempty", "No agents yet: add one to run tasks."));
    hint.hidden = !tasks;
    if (tasks) hint.textContent = `${tasks.length} chosen: press an agent or drop them on it. Esc lets them go.`;
  }

  /** The pool's rows, for a project's window: a press opens one. */
  const rows = () => getMeta().agents.map((a) => {
    const b = row(a);
    b.addEventListener("click", () => open(a));
    return b;
  });

  /** The efforts of the kind chosen; one it does not take goes by the agent's own setting. */
  function fillEfforts(value) {
    const levels = getMeta().efforts?.[field.kind.value] ?? [];
    fillSelect(field.effort, [["", "Its own setting"], ...levels.map((e) => [e, e])], levels.includes(value) ? value : "");
  }

  const values = () => Object.fromEntries(Object.entries(field).map(([k, e]) => [k, e.value]));
  const say = (text) => { error.textContent = text; error.hidden = !text; };

  // what a kind fills in follows the kind; what the user wrote stays
  field.kind.addEventListener("change", () => {
    const kind = field.kind.value;
    if (!field.name.value.trim() || KINDS.some((k) => field.name.value.startsWith(agentName(k)))) field.name.value = freeName(kind);
    if (!field.command.value.trim() || KINDS.includes(field.command.value)) field.command.value = kind;
    fillEfforts(field.effort.value);
  });

  /** A kind's name not yet taken in the pool: Claude Code, Claude Code 2… */
  function freeName(kind) {
    const taken = new Set(getMeta().agents.map((x) => x.name));
    const base = agentName(kind);
    if (!taken.has(base)) return base;
    let n = 2;
    while (taken.has(`${base} ${n}`)) n++;
    return `${base} ${n}`;
  }

  /** Its window: a new one, or `a` to change. */
  function open(a = null) {
    editing = a;
    const v = a ?? { kind: "claude", name: freeName("claude"), command: "claude", effort: "", at_once: 1 };
    for (const k of ["kind", "name", "command", "at_once"]) field[k].value = v[k];
    fillEfforts(v.effort);
    showSessions(a);
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
    const body = { ...values(), at_once: Number(field.at_once.value) };
    return send(editing ? `/api/task-agents/${editing.id}` : "/api/task-agents", { method: editing ? "PATCH" : "POST", body });
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

  /** Its sessions in its window, each with Let go: a + Session's, or one on a host out of reach (AC-142). */
  function showSessions(a) {
    const list = $("agentsessions"), mine = a ? slots(a) : [];
    list.replaceChildren(...mine.map((s) => {
      const li = el("li");
      const go = el("button", null, "Let go");
      go.type = "button";
      go.addEventListener("click", async () => {
        if (!await askConfirm(`Let go of ${s.tasks || "this session"}? Its open tasks fail; the window stays.`, "Let go")) return;
        try { await api(`/api/runs/${s.run}/let-go`, { method: "POST" }); await reload(); showSessions(a); } catch (e) { say(e.message); }
      });
      li.append(el("span", null, [s.tasks || "+ Session", getMeta().projects.find((p) => p.id === s.project)?.name].filter(Boolean).join(" · ")), go);
      return li;
    }));
    list.hidden = !mine.length;
  }

  /** A new one; once its window closes. */
  const add = () => {
    open();
    return new Promise((resolve) => dialog.addEventListener("close", resolve, { once: true }));
  };
  return { box, draw, add, rows };
}
