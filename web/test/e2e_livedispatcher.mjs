// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Dispatcher against a real private terminumd and tmux (e2e_live.mjs; terminum-dispatcher.md AC-121 to
// AC-125, AC-131, AC-137; terminum-kanban.md AC-117): scripts/fake_agent.py draws the measured input box
// and speaks by the real hooks and tools; the clock runs 300 times as fast (TM_TEST_CLOCK_SCALE).
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { boardKit, nap } from "./boardkit.mjs";
import { check, until } from "./check.mjs";

const FAKE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../scripts/fake_agent.py");

const taskKit = (j) => {
  const status = async (id) => (await j(`/api/tasks/${id}`)).status;
  return {
    status,
    history: async (id) => (await j(`/api/tasks/${id}`)).history.map((e) => e.text ?? "").join(" | "),
    paneOf: async (id) => (await j(`/api/tasks/${id}`)).session?.key?.split(":").pop(),
    hasStatus: (id, s, ms = 8000) => until(async () => await status(id) === s, ms),
  };
};

export async function liveDispatcher(page, opts) {
  const { base, tmux, work, shots, stop, start } = opts;
  const { j, keyOf, card, open, backToSessions, cli } = boardKit(page, opts);
  const project = (await j("/api/task-projects", "POST", { name: "Runs", dir: work })).id;
  const agent = async (name, more = "", kind = "claude") => (await j(`/api/task-projects/${project}/agents`, "POST", { name, kind, command: `python3 -I ${FAKE} ${cli} --kind ${kind} ${more}` })).id;
  const plain = await agent("Plain");
  const task = async (title, text = "") => (await j("/api/tasks", "POST", { title, text, project })).id;
  // an agent takes more only when free (AC-138): a run here has its own, so one left open stands in no other's way
  let runs = 0;
  const run = async (id, body) => j(`/api/tasks/${id}/run`, "POST", body ?? { agent: await agent(`Run ${++runs}`) });
  const { status, history, paneOf, hasStatus } = taskKit(j);
  const said = (id, words, ms = 8000) => until(async () => (await history(id)).includes(words), ms);

  // AC-121 happy path, through the page: its play key asks which agent, the last chosen picked, one press
  const first = await task("Fix the retry loop FINISHES", "Its text.");
  await open();
  await page.goto(`${base}/kanban/?project=${project}`);
  await page.waitForSelector(card(first));
  await page.click(`${card(first)} .crun`);
  check("AC-121 a card's play key asks which agent", await until(() => page.$("#runbox[open]")));
  check("AC-121 it stays where it was until one takes it", await status(first) === "TODO");
  check("AC-121 the dialog offers the project's agents", await page.inputValue("#runagent") === plain);
  check("AC-121 it shows the command in full", (await page.textContent("#runcommand")).includes("fake_agent.py"), await page.textContent("#runcommand"));
  check("AC-121 and the server and the folder", await page.textContent("#runserver") === "This Mac" && await page.inputValue("#runfolder") === work, await page.inputValue("#runfolder"));
  const prompt = await page.textContent("#runprompt");
  check("AC-121 and the text that will be sent, in its order", /^The human ran task T-\d+: Fix the retry loop FINISHES\n\nIts text\.\n\nWhen it is done, call finish_task/.test(prompt), prompt);
  if (shots) await page.screenshot({ path: path.join(shots, "run-window.png") });
  await page.click("#rungo");
  check("AC-121 the window closes on the press", await until(async () => !(await page.$("#runbox[open]"))));
  // read while it stands: after DONE the test's fast clock closes the session soon (AC-125)
  let key;
  await until(async () => (key = (await j(`/api/tasks/${first}`)).session?.key));
  check("AC-121 its pane is marked as the run's", await until(() => tmux("display", "-p", "-t", key.split(":").pop(), "#{@terminum-run}").trim().length > 0));
  check("AC-121 it is a member of the project", await until(() => page.isVisible(`#sessions section.project[data-pid="${project}"] .item[data-id="${key}"]`)));
  check("AC-121 the agent takes it: IN PROGRESS", await hasStatus(first, "IN PROGRESS", 15000) || await status(first) === "DONE", await status(first));
  check("AC-123 finish_task is DONE", await hasStatus(first, "DONE", 15000), await history(first));
  const h = await history(first);
  check("AC-125 its start is on the task", /Session started: claude in window Runs:\d+, on this Mac\./.test(h), h);
  check("AC-121 the owner is the agent", (await j(`/api/tasks/${first}`)).owner_name === "claude · Runs", (await j(`/api/tasks/${first}`)).owner_name);
  check("AC-121 the task's history says Dispatcher moved it", /Dispatcher moved it from TODO to IN PROGRESS/.test(h), h);

  // AC-121: the same project's next run starts with the last chosen picked; a project with no agents says so
  const alone = (await j("/api/task-projects", "POST", { name: "NoAgents" })).id;
  const lonely = (await j("/api/tasks", "POST", { title: "no agent to run it", project: alone })).id;
  await page.goto(`${base}/kanban/?project=${alone}`);
  await page.waitForSelector(card(lonely));
  await page.click(`${card(lonely)} .crun`);
  check("AC-121 a project with no agents says so and offers one", await until(async () => /No agents in NoAgents yet/.test(await page.textContent("#runnone"))) && await page.isVisible("#runadd") && await page.isDisabled("#rungo"));
  await page.click('#runbox button[value="cancel"]');
  check("AC-121 Cancel moves nothing", await status(lonely) === "TODO");
  check("AC-121 no agent: the route says so", /No agents in/.test((await run((await j("/api/tasks", "POST", { title: "x", project: alone })).id, { agent: "nope" })).error));

  // refused before starting
  const badDir = await task("folder gone");
  const no = await run(badDir, { agent: plain, folder: "/no/such/folder/at/all" });
  check("AC-121 a folder that does not exist is said and nothing starts", /does not exist on this Mac/.test(no.error) && await status(badDir) === "TODO" && !(await j(`/api/tasks/${badDir}`)).session, JSON.stringify(no));
  const linked = await task("linked by hand");
  const otherPane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", "byhand").trim();
  const otherKey = await keyOf(otherPane);
  await j(`/api/tasks/${linked}`, "PATCH", { session: otherKey });
  const refused = await run(linked);
  check("AC-125 a session it did not start is not run", /Unlink the session/.test(refused.error), JSON.stringify(refused));
  check("AC-125 and never typed in", !/fake_agent|human ran/.test(tmux("capture-pane", "-p", "-t", otherPane)));

  // AC-121 failures of the start
  const fast = await agent("Fast", "--start fast");
  const quick = await task("program exits");
  await run(quick, { agent: fast });
  check("AC-121 a program that ends at once: FAILED with why", await hasStatus(quick, "FAILED", 20000) && /Session did not start: the program ended with exit 7/.test(await history(quick)), await history(quick));
  const none = await agent("Unknown", "--start none");
  const lost = await task("no box");
  await run(lost, { agent: none });
  check("AC-121 no box within its time: FAILED, with the screen", await hasStatus(lost, "FAILED", 20000) && /did not come in 60 s[\s\S]*something it does not know/.test(await history(lost)), await history(lost));
  check("AC-121 the window stays for a human", (await paneOf(lost)) && !(await j(`/api/tasks/${lost}`)).session?.closed);
  const silent = await task("taken by none SILENT");
  await run(silent);
  check("AC-121 a prompt not taken: FAILED", await hasStatus(silent, "FAILED", 20000) && /not taken in 30 s/.test(await history(silent)), await history(silent));

  // AC-121: its own question before the box: nothing is typed, a human is asked, and the prompt goes in after
  const trusting = await agent("Trusting", "--start trust");
  const asks = await task("a folder's trust FINISHES");
  await run(asks, { agent: trusting });
  check("AC-121 a question before the box: WAIT FOR HUMAN", await hasStatus(asks, "WAIT FOR HUMAN", 15000) && await said(asks, "Answer in its window"), await history(asks));
  const pane = await paneOf(asks);
  check("AC-121 nothing is typed on it", /Quick safety check/.test(tmux("capture-pane", "-p", "-t", pane)));
  tmux("send-keys", "-t", pane, "Down");
  check("AC-121 once answered the prompt goes in and it ends", await hasStatus(asks, "DONE", 15000), await history(asks));

  // AC-123 a turn with no result: one request through the gate, then a human
  const stops = await task("stops midway");
  await run(stops);
  check("AC-123 a turn with no result gets one request", await until(async () => /still open and you stopped/.test(tmux("capture-pane", "-p", "-S", "-30", "-t", await paneOf(stops))) || await status(stops) === "WAIT FOR HUMAN", 15000));
  check("AC-123 then WAIT FOR HUMAN", await hasStatus(stops, "WAIT FOR HUMAN", 15000) && await said(stops, "The agent stopped without a result"), await history(stops));
  check("AC-123 an hour of nothing: DONE, no result", await hasStatus(stops, "DONE", 30000) && await said(stops, "no activity for 60 minutes"), await history(stops));

  // AC-123 the gate never types on a draft
  const drafted = await task("a draft stands");
  await run(drafted);
  await hasStatus(drafted, "IN PROGRESS", 15000);
  tmux("send-keys", "-t", await paneOf(drafted), "-l", "my own words");
  check("AC-123 a draft in the box is not typed over: said", await said(drafted, "Not delivered: its input box holds a draft", 15000), await history(drafted));
  check("AC-123 it stands as it was", /my own words/.test(tmux("capture-pane", "-p", "-t", await paneOf(drafted))));

  // AC-123 failures and ends
  const limited = await task("out of limits LIMIT");
  await run(limited);
  check("AC-123 a failed turn is FAILED with its reason", await hasStatus(limited, "FAILED", 15000) && await said(limited, "rate_limit"), await history(limited));
  const dying = await task("dies DIES");
  await run(dying);
  check("AC-123 a dead agent is FAILED with its exit", await hasStatus(dying, "FAILED", 15000) && await said(dying, "The agent's process ended: exit 3."), await history(dying));
  const clean = await task("ends CLEAN");
  await run(clean);
  check("AC-123 a clean end is DONE, no result", await hasStatus(clean, "DONE", 15000) && await said(clean, "The session ended without finish_task."), await history(clean));
  await page.goto(`${base}/kanban/?project=${project}`);
  await page.waitForSelector(card(clean));
  check("AC-123 its card says DONE, no result", await until(async () => /DONE, no result/.test(await page.textContent(card(clean))), 4000), await page.textContent(card(clean)));

  // AC-124 waits
  const waits = await task("asks ASKS");
  await run(waits);
  check("AC-124 a question for 2 minutes: WAIT FOR HUMAN, with what it asks", await hasStatus(waits, "WAIT FOR HUMAN", 15000) && await said(waits, "Approve Bash"), await history(waits));
  check("AC-124 and a line says so on the page", await until(async () => /T-\d+ waits for you/.test(await page.textContent("#asks"))), await page.textContent("#asks"));
  if (shots) await page.screenshot({ path: path.join(shots, "needs-line.png") });

  // AC-122 running again: a live agent, a shell, a closed session
  const again = await task("again STOPS");
  await run(again);
  await hasStatus(again, "IN PROGRESS", 15000);
  await j(`/api/tasks/${again}`, "PATCH", { status: "FAILED" });
  // its agent may be busy with Dispatcher's request a moment: RUN says so, and takes it after
  let live;
  await until(async () => (live = await run(again, { comment: "Use the v2 API" })).run, 6000);
  check("AC-122 a live agent takes a comment", !!live.run && await hasStatus(again, "IN PROGRESS"), JSON.stringify(live));
  check("AC-122 through the gate", await until(async () => /new comment by/.test(tmux("capture-pane", "-p", "-S", "-30", "-t", await paneOf(again)))) || /comment/.test(await history(again)));
  await nap(500);
  const dead = await task("again from a shell DIES");
  await run(dead);
  await hasStatus(dead, "FAILED", 15000);
  const shell = await run(dead, { comment: "Once more FINISHES" });
  check("AC-122 only the shell remains: it starts there anew", !!shell.run && await hasStatus(dead, "DONE", 20000), JSON.stringify(shell) + await history(dead));

  // AC-122 RUN right after its agent exits: its last screen still shows a box, nothing is pasted into the shell
  const justEnded = await task("again at once CLEAN");
  await run(justEnded);
  await hasStatus(justEnded, "DONE", 15000);
  const pressed = await run(justEnded, { comment: "Once more FINISHES" });
  check("AC-122 RUN just after an exit starts anew in its pane", !!pressed.run, JSON.stringify(pressed));
  check("AC-122 and it ends", await until(async () => (await history(justEnded)).includes("All done."), 15000), await history(justEnded));

  // what the tty holds when its agent has gone never reaches the shell it leaves (§9 Window command)
  const nothing = path.join(work, "typed-into-the-shell");
  const sleepy = (await j(`/api/task-projects/${project}/agents`, "POST", { name: "Sleepy", kind: "claude", command: "sleep 2" })).id;
  const drained = await task("drained");
  await run(drained, { agent: sleepy });
  const drainedPane = await until(() => paneOf(drained)) && await paneOf(drained);
  tmux("send-keys", "-t", drainedPane, "-l", `touch ${nothing}`);
  tmux("send-keys", "-t", drainedPane, "Enter");
  check("AC-121 an agent's end with text in its tty: FAILED", await hasStatus(drained, "FAILED", 15000));
  await nap(1000);
  check("§9 what was typed for it does not run in its shell", !fs.existsSync(nothing));

  // RUN sends what the human saw: an agent may change the task's text meanwhile (AC-121)
  const changed = await task("the text changes", "as it was");
  const seen = await j(`/api/tasks/${changed}/run`);
  await j(`/api/tasks/${changed}`, "PATCH", { text: "as an agent left it" });
  const stale = await run(changed, { agent: plain, prompt_rev: seen.prompt_rev });
  check("AC-121 a press on other words is asked again, and nothing starts", /text changed/.test(stale.error) && await status(changed) === "TODO", JSON.stringify(stale));

  // AC-126 the budget
  const shortBudget = await task("short budget");
  const refusedBudget = await j(`/api/tasks/${shortBudget}`, "PATCH", { budget_minutes: 3 });
  check("AC-126 under 5 minutes is refused with its words", refusedBudget.error === "Budget: 5 minutes to 7 days", JSON.stringify(refusedBudget));
  const oddBudget = await j(`/api/tasks/${shortBudget}`, "PATCH", { budget_minutes: "sixty" });
  check("AC-126 what is no number is refused, never read as no budget", oddBudget.error === "Budget: 5 minutes to 7 days", JSON.stringify(oddBudget));
  const timed = await task("timed WORKS");
  await j(`/api/tasks/${timed}`, "PATCH", { budget_minutes: 62 });   // its last 2 minutes: one tick of this clock
  check("AC-126 the prompt says the budget", /You have 62 minutes for this\./.test((await j(`/api/tasks/${timed}/run`)).prompt));
  await run(timed);
  check("AC-126 at the limit the task is FAILED", await hasStatus(timed, "FAILED", 25000) && await said(timed, "Time budget of 62 minutes used: the agent was ended."), await history(timed));
  check("AC-126 the last warning went first", /last warning/.test(await history(timed)), await history(timed));
  const timedPane = await paneOf(timed);
  check("AC-126 the agent ended: SIGTERM, recorded", await until(() => tmux("display", "-p", "-t", timedPane, "#{@terminum-exit}").trim() !== "", 8000), tmux("display", "-p", "-t", timedPane, "#{@terminum-exit}"));
  check("AC-126 its pane and shell stay", tmux("list-panes", "-a", "-F", "#{pane_id}").split("\n").map((l) => l.trim()).includes(timedPane));
  const shellPid = tmux("display", "-p", "-t", timedPane, "#{pane_pid}").trim();
  check("AC-126 the shell is spared", (() => { try { process.kill(Number(shellPid), 0); return true; } catch { return false; } })(), shellPid);
  check("AC-126 a sibling pane is untouched", tmux("list-panes", "-a", "-F", "#{pane_id}").split("\n").some((l) => l.trim() === otherPane));
  // a program a human starts at the shell it left is none of a later limit: the exit is recorded
  tmux("send-keys", "-t", timedPane, "-l", "sleep 30");
  tmux("send-keys", "-t", timedPane, "Enter");
  await nap(1200);
  check("AC-126 a human's program there runs on", /sleep/.test(tmux("display", "-p", "-t", timedPane, "#{pane_current_command}")), tmux("display", "-p", "-t", timedPane, "#{pane_current_command}"));
  tmux("send-keys", "-t", timedPane, "C-c");
  await nap(1500);
  check("AC-123 the end it caused counts for nothing", await status(timed) === "FAILED");
  const stubborn = await agent("Stubborn", "--stubborn");
  const hard = await task("stubborn WORKS");
  await j(`/api/tasks/${hard}`, "PATCH", { budget_minutes: 5 });
  await run(hard, { agent: stubborn });
  check("AC-126 one that ignores SIGTERM is ended by SIGKILL", await hasStatus(hard, "FAILED", 25000) && await until(async () => /^(137|9|143)$/.test(tmux("display", "-p", "-t", await paneOf(hard), "#{@terminum-exit}").trim()), 10000), tmux("display", "-p", "-t", await paneOf(hard), "#{@terminum-exit}"));
  const changing = await task("budget changes STOPS");
  await run(changing);
  await hasStatus(changing, "IN PROGRESS", 15000);
  await j(`/api/tasks/${changing}`, "PATCH", { budget_minutes: 60 });
  check("AC-126 changed in flight: the agent is told", await until(async () => /budget is now 60 minutes/.test(tmux("capture-pane", "-p", "-S", "-40", "-t", await paneOf(changing))) || (await history(changing)).includes("set its budget"), 6000), await history(changing));
  await page.goto(`${base}/kanban/?project=${project}`);
  await page.waitForSelector('.col[data-status="TODO"]');
  check("AC-126 the card shows the time used and the budget", await until(async () => /\d+ of 60 min/.test(await page.textContent(card(changing)).catch(() => "")), 20000), await page.textContent(card(changing)).catch(() => ""));

  // AC-127 a subagent silent and idle: one request to the main agent, then it is said
  const silentSub = await task("has a subagent SUBAGENT");
  await page.goto(`${base}/kanban/?project=${project}`);
  await page.waitForSelector(card(silentSub));
  // watched from the start: the clock runs 300 times faster, and the session ends minutes of its time after this
  const idleShown = until(async () => /idle: reviewer sub-1/.test(await page.textContent(card(silentSub)).catch(() => "")), 30000);
  await run(silentSub);
  check("AC-127 a silent subagent gets the main agent one request", await said(silentSub, "The request about a silent subagent", 15000), await history(silentSub));
  check("AC-127 one that runs inside the agent cannot be ended alone: said", await said(silentSub, "Subagent sub-1 cannot be ended alone", 15000), await history(silentSub));
  check("AC-127 the card shows its kind and ID, as idle", await idleShown, await page.textContent(card(silentSub)));

  // AC-128 what it consumes: shown on its card; limits by project; a limit of 0 is off
  const badLimits = await j(`/api/task-projects/${project}/limits`, "PUT", { warn: 9, end: 3 });
  check("AC-128 a warning above the end is refused", /above the end/.test(badLimits.error), JSON.stringify(badLimits));
  await j(`/api/task-projects/${project}/limits`, "PUT", { warn: 0.001, end: 0 });
  const heavy = await task("uses memory WORKS");
  await run(heavy);
  await hasStatus(heavy, "IN PROGRESS", 15000);
  await page.goto(`${base}/kanban/?project=${project}`);
  await page.waitForSelector(card(heavy));
  check("AC-128 the card shows CPU, memory and running time", await until(async () => /CPU \d+% · [\d.]+ GiB · \d+ min/.test(await page.textContent(`${card(heavy)} .cusage`).catch(() => "")), 8000), await page.textContent(card(heavy)));
  check("AC-128 over the warning for 2 walks the agent is told", await said(heavy, "The memory warning", 10000), await history(heavy));
  check("AC-128 an end limit of 0 is off", await status(heavy) === "IN PROGRESS", await status(heavy));
  await j(`/api/task-projects/${project}/limits`, "PUT", { warn: 0, end: 0.001 });
  check("AC-128 over the end for 2 walks: FAILED, and it says how much", await hasStatus(heavy, "FAILED", 10000) && await said(heavy, "Memory limit of 0.001 GiB exceeded:"), await history(heavy));
  await j(`/api/task-projects/${project}/limits`, "PUT", { warn: null, end: null });
  check("AC-128 the global ones are back", !(await j("/api/tasks/meta")).limits.some((l) => l.project === project));

  // AC-129 tokens: three integers from its turn's end
  const tokens = await task("counts TOKENS");
  await run(tokens);
  await hasStatus(tokens, "IN PROGRESS", 15000);
  await page.goto(`${base}/kanban/?project=${project}`);
  await page.waitForSelector(card(tokens));
  check("AC-129 the card shows the tokens its session used", await until(async () => /1,239 tokens/.test(await page.textContent(`${card(tokens)} .cusage`).catch(() => "")), 8000), await page.textContent(card(tokens)));

  // AC-125 closing, with the screen kept when there is no result
  const closes = await task("closes CLEAN");
  await run(closes);
  await hasStatus(closes, "DONE", 15000);
  check("AC-125 it closes 2 hours after DONE", await said(closes, "Session ended: closed 2 hours after DONE.", 40000), await history(closes));
  check("AC-125 the card says session closed", await until(async () => (await j(`/api/tasks/${closes}`)).session?.closed === true));
  check("AC-125 with no result its screen stays on the task", (await j(`/api/tasks/${closes}`)).files.some((f) => /screen/.test(f.name)), JSON.stringify((await j(`/api/tasks/${closes}`)).files));

  // AC-122 resume: a closed session goes on in a new window
  const resumed = await run(closes, { comment: "go on FINISHES" });
  check("AC-122 a closed session goes on in a new window", !!resumed.run, JSON.stringify(resumed));
  check("AC-122 by the resume, its ID checked", await hasStatus(closes, "DONE", 20000) && await until(async () => (await history(closes)).split("Session started").length >= 3), await history(closes));

  // AC-131 terminumd restarts: the session goes on, the walk reads its run from the store
  const restart = await task("over a restart STOPS");
  await run(restart);
  await hasStatus(restart, "IN PROGRESS", 15000);
  await stop();
  await start();
  await page.goto(`${base}/kanban/?project=${project}`);
  await page.waitForSelector('.col[data-status="TODO"]');
  check("AC-131 after a restart it goes on where it was", await hasStatus(restart, "WAIT FOR HUMAN", 30000) || await status(restart) === "DONE", await history(restart));
  tmux("kill-pane", "-t", otherPane);
  // AC-117 on the session page too, every project's
  await page.goto(`${base}/sessions/`);
  check("AC-117 the session page says so too, and counts", await until(() => page.isVisible("#asks .taskline"), 8000) && await until(async () => /\d+/.test(await page.textContent("#tokanban .taskneed").catch(() => "")), 8000));
  await page.click("#asks .taskline .x");
  const left = await page.evaluate(() => document.querySelectorAll("#asks .taskline").length);
  await page.reload();
  await page.waitForSelector("#sessions .item");
  check("AC-117 a closed line stays closed", await until(async () => await page.evaluate(() => document.querySelectorAll("#asks .taskline").length) <= left, 4000));
  // this test's windows go, their agents' states with them: what follows measures a quiet page
  for (const line of tmux("list-panes", "-a", "-F", "#{pane_id} #{@terminum-run}").split("\n")) {
    const [id, mark] = line.trim().split(" ");
    if (mark) tmux("kill-pane", "-t", id);
  }
  await backToSessions();
}

/** The real programs, when asked (TM_TEST_REAL_AGENTS=1, TM_TEST_CLOCK_SCALE=1): the user's Claude Code
 *  and Codex logins, a few short model calls; outside every gate. Their own screens and hooks hold the
 *  whole way: box, gate, prompt, hook, finish_task, the exit, the resume. */
export async function liveDispatcherReal(page, opts) {
  const { work } = opts;
  const { j, card } = boardKit(page, opts);
  const project = (await j("/api/task-projects", "POST", { name: "RealRuns", dir: work })).id;
  const add = async (name, kind, command) => (await j(`/api/task-projects/${project}/agents`, "POST", { name, kind, command })).id;
  const { status, history, paneOf, hasStatus } = taskKit(j);
  const claude = await add("Real Claude", "claude", "claude --allowedTools mcp__plugin_terminum_terminum__finish_task");
  const t = (await j("/api/tasks", "POST", { title: "Reply with the word ok, then call finish_task with the result ok", project })).id;
  const ran = await j(`/api/tasks/${t}/run`, "POST", { agent: claude });
  check("AC-121 real Claude Code: RUN starts", !!ran.run, JSON.stringify(ran));
  // a folder it does not know: Dispatcher waits for the human's answer, which leaves that folder's trust entry in ~/.claude.json
  await until(async () => {
    const pane = await paneOf(t), screen = pane ? opts.tmux("capture-pane", "-p", "-t", pane) : "";
    if (/trust this folder/.test(screen) && /❯ No/.test(screen)) opts.tmux("send-keys", "-t", pane, "Down", "Enter");
    return /[❯>] *$/m.test(screen) || ["IN PROGRESS", "DONE"].includes(await status(t));
  }, 30000);
  check("AC-121 real Claude Code: the gate reads its box, the hook says it took the prompt", await until(async () => ["IN PROGRESS", "DONE"].includes(await status(t)), 90000), await history(t));
  check("AC-123 real Claude Code: finish_task is DONE", await hasStatus(t, "DONE", 180000), await history(t));
  await page.goto(`${opts.base}/kanban/?project=${project}`);
  await page.waitForSelector(card(t));
  check("AC-128 real Claude Code: the card shows CPU, memory and running time", await until(async () => /CPU \d+% · [\d.]+ GiB · \d+ min/.test(await page.textContent(`${card(t)} .cusage`).catch(() => "")), 60000), await page.textContent(card(t)));
  // the installed `terminum hook` is what a real agent runs: this holds once this build is installed
  if (process.env.TM_TEST_REAL_TOKENS) check("AC-129 real Claude Code: and the tokens of its transcript", await until(async () => /[\d,]+ tokens/.test(await page.textContent(`${card(t)} .cusage`).catch(() => "")), 60000), await page.textContent(card(t)));
  // its own /exit: the exit code; then RUN goes on by the resume, in the same pane
  const pane = await paneOf(t);
  opts.tmux("send-keys", "-t", pane, "-l", "/exit");
  opts.tmux("send-keys", "-t", pane, "Enter");
  check("AC-123 real Claude Code: /exit records its exit code", await until(() => opts.tmux("display", "-p", "-t", pane, "#{@terminum-exit}").trim() === "0", 30000), opts.tmux("display", "-p", "-t", pane, "#{@terminum-exit}"));
  await nap(2000);
  const again = await j(`/api/tasks/${t}/run`, "POST", { comment: "Reply with the word ok again, then call finish_task with the result ok again" });
  check("AC-122 real Claude Code: the resume starts in its pane", !!again.run, JSON.stringify(again));
  check("AC-122 real Claude Code: the conversation goes on: the agent took it again", await until(async () => (await history(t)).split("The agent took it.").length === 3, 90000), await history(t));
  check("AC-122 real Claude Code: and it ends", await hasStatus(t, "DONE", 180000), await history(t));
  console.log("REAL claude history:", (await history(t)).slice(0, 900));
  // two tasks in one press: they go by number, the agent reads each by get_task and finishes each (AC-138)
  const pool = await add("Real pool", "claude", "claude --allowedTools mcp__plugin_terminum_terminum__finish_task mcp__plugin_terminum_terminum__get_task");
  const makes = async (title, text) => (await j("/api/tasks", "POST", { title, text, project })).id;
  const [first, second] = [await makes("First of two", "Finish this task with the result one."), await makes("Second of two", "Finish this task with the result two.")];
  const both = await j(`/api/tasks/${first}/run`, "POST", { agent: pool, also: [second], comment: "Do both, briefly." });
  check("AC-138 real Claude Code: one press runs two tasks", !!both.run, JSON.stringify(both));
  check("AC-138 real Claude Code: its agent is busy", /is busy with/.test((await j(`/api/tasks/${await makes("A third", "")}/run`, "POST", { agent: pool })).error ?? ""));
  check("AC-138 real Claude Code: both end DONE, from one session", await hasStatus(first, "DONE", 240000) && await hasStatus(second, "DONE", 120000) && await paneOf(first) === await paneOf(second), `${await history(first)} || ${await history(second)}`);
  check("AC-138 real Claude Code: and each result is its own", /\bone\b/.test(await history(first)) && /\btwo\b/.test(await history(second)), `${await history(first)} || ${await history(second)}`);
  check("AC-138 real Claude Code: its agent is free again, the cards stay on it", await until(async () => !(await j("/api/tasks?counts=1")).busy[pool], 90000) && (await j(`/api/tasks/${first}`)).agent?.name === "Real pool");
  if (!process.env.TM_TEST_REAL_CODEX) return;
  const codex = await add("Real Codex", "codex", "codex");
  const x = (await j("/api/tasks", "POST", { title: "Reply with the word ok", project })).id;
  const xr = await j(`/api/tasks/${x}/run`, "POST", { agent: codex });
  check("AC-121 real Codex: RUN starts", !!xr.run, JSON.stringify(xr));
  const seen = await until(async () => ["WAIT FOR HUMAN", "IN PROGRESS", "FAILED"].includes(await status(x)), 60000);
  console.log("REAL codex first:", await status(x), (await history(x)).slice(0, 700));
  if (seen && await status(x) === "WAIT FOR HUMAN") opts.tmux("send-keys", "-t", await paneOf(x), "Escape");
  await until(async () => ["DONE", "FAILED"].includes(await status(x)) || /stopped/.test(await history(x)), 90000);
  console.log("REAL codex then:", await status(x), (await history(x)).slice(0, 900));
  console.log("REAL codex screen:", opts.tmux("capture-pane", "-p", "-t", await paneOf(x)).slice(-600));
  check("AC-121 real Codex: its box is read and the prompt goes in", /Reply with the word ok/.test(opts.tmux("capture-pane", "-p", "-S", "-60", "-t", await paneOf(x))), await history(x));
}
