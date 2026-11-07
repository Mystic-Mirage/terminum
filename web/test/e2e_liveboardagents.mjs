// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A project's agents on the board, against a real private terminumd (e2e_live.mjs; terminum-dispatcher.md
// AC-137, AC-138; terminum-kanban.md AC-139): kept, listed, changed and deleted by a human, refused with
// words; tasks run on one by a press or a drop, several at once; a project's own window.
// scripts/fake_agent.py speaks by the real hooks and tools.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { boardKit } from "./boardkit.mjs";
import { check, until } from "./check.mjs";

const COMMAND = "claude --model opus";
const FAKE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../scripts/fake_agent.py");

export async function liveBoardAgents(page, opts) {
  const { base, shots } = opts;
  const { j, backToSessions } = boardKit(page, opts);
  const project = (await j("/api/task-projects", "POST", { name: "AgentsHere" })).id;
  await page.goto(`${base}/kanban/?project=${project}`);
  await page.waitForSelector("#addagent");
  check("AC-137 a project with none says so", await until(async () => /No agents yet/.test(await page.textContent(".agents"))), await page.textContent(".agents"));

  await page.click("#addagent");
  await page.waitForSelector("#agentbox[open]");
  check("AC-137 the window starts in the project shown", await page.inputValue("#agentproject") === project);
  check("AC-137 its kind fills in name and command", await page.inputValue("#agentname") === "Claude Code" && await page.inputValue("#agentcommand") === "claude");
  await page.selectOption("#agentkind", "codex");
  check("AC-137 another kind changes them", await page.inputValue("#agentname") === "Codex" && await page.inputValue("#agentcommand") === "codex");
  await page.fill("#agentcommand", "codex --search");
  await page.selectOption("#agentkind", "claude");
  check("AC-137 what the user wrote stays", await page.inputValue("#agentcommand") === "codex --search" && await page.inputValue("#agentname") === "Claude Code");
  await page.fill("#agentname", "Opus reviewer");
  await page.fill("#agentcommand", COMMAND);
  if (shots) await page.screenshot({ path: path.join(shots, "agent-window.png") });

  // what is wrong is said in the window, nothing kept
  const refused = async (field, value, words) => {
    const was = await page.inputValue(field);
    await page.fill(field, value);
    await page.click("#agentok");
    check(`AC-137 refused: ${words}`, await until(async () => (await page.textContent("#agenterror")) === words) && await page.$("#agentbox[open]"), await page.textContent("#agenterror"));
    await page.fill(field, was);
  };
  await refused("#agentname", "", "An agent needs a name.");
  await refused("#agentcommand", "", "An agent needs a command.");
  await refused("#agentcommand", "x".repeat(1001), "An agent's command is 1000 characters at most.");
  await refused("#agentdir", "work/api", "A folder starts with / or ~.");
  const said = await j(`/api/task-projects/${project}/agents`, "POST", { name: "line", kind: "claude", command: "claude\nrm -rf x" });
  check("AC-137 refused: a line break in a command", said.code === 400 && /one line/.test(said.error), JSON.stringify(said));
  const elsewhere = await j(`/api/task-projects/${project}/agents`, "POST", { name: "far", kind: "claude", host: "nowhere.example", command: "claude" });
  check("AC-137 refused: a server it does not have", elsewhere.code === 400 && /nowhere.example/.test(elsewhere.error), JSON.stringify(elsewhere));
  const gone = await j("/api/task-projects/gone-id/agents", "POST", { name: "x", kind: "claude", command: "claude" });
  check("AC-137 refused: a project gone", gone.code === 400 && /gone/.test(gone.error), JSON.stringify(gone));
  check("AC-137 nothing was kept", (await j("/api/tasks/meta")).agents.filter((a) => a.project === project).length === 0);

  // a slip asks, Cancel means it
  await page.keyboard.press("Escape");
  check("AC-137 Esc with text typed asks", await until(() => page.$("#confirmbox[open]")));
  await page.click('#confirmbox button[value="cancel"]');
  check("AC-137 and keeps it", await page.$("#agentbox[open]") && await page.inputValue("#agentname") === "Opus reviewer");

  await page.press("#agentname", "Enter");
  const row = '.agents .agentcard:has-text("Opus reviewer")';
  check("AC-137 Enter keeps it and it stands under Agents", await until(() => page.isVisible(row)), await page.textContent(".agents"));
  check("AC-137 the window closes", !(await page.$("#agentbox[open]")));
  const hover = () => page.getAttribute(row, "title");
  check("AC-137 its rectangle says its kind; hovering, its server and command", await page.isVisible(`${row} .agmark img`) && /Claude Code/.test(await page.textContent(row)) && /This Mac/.test(await hover()) && (await hover()).includes(COMMAND), await hover());
  const stored = (await j("/api/tasks/meta")).agents.find((a) => a.project === project);
  check("AC-137 its command is kept as written", stored?.command === COMMAND && stored.kind === "claude" && stored.host === null && stored.dir === "", JSON.stringify(stored));
  if (shots) await page.screenshot({ path: path.join(shots, "agents-column.png") });

  // every page hears
  const other = await page.context().newPage();
  await other.goto(`${base}/kanban/?project=${project}`);
  await other.waitForSelector(row);
  await j(`/api/task-projects/${project}/agents`, "POST", { name: "Codex two", kind: "codex", dir: "~/work", command: "codex" });
  check("AC-137 every page hears within 2 s", await until(() => other.isVisible('.agents .agentcard:has-text("Codex two")'), 2500));
  await other.close();
  check("AC-137 this one too", await until(() => page.isVisible('.agents .agentcard:has-text("Codex two")'), 2500));

  // a name stands once in its project
  await page.click("#addagent");
  await page.fill("#agentname", "Opus reviewer");
  await page.click("#agentok");
  check("AC-137 a name its project has is refused", await until(async () => /already has an agent named Opus reviewer/.test(await page.textContent("#agenterror"))), await page.textContent("#agenterror"));
  await page.click('#agentbox button[value="cancel"]');
  check("AC-137 Cancel closes with no question", !(await page.$("#confirmbox[open]")) && !(await page.$("#agentbox[open]")));

  // changed and deleted
  await page.click(row);
  check("AC-137 a press opens it filled", await page.inputValue("#agentcommand") === COMMAND && await page.isVisible("#agentdrop") && await page.isDisabled("#agentproject"));
  await page.fill("#agentcommand", "claude --model sonnet");
  await page.click("#agentok");
  check("AC-137 Save keeps a change", await until(async () => (await hover()).includes("claude --model sonnet")), await hover());
  await page.click(row);
  await page.click("#agentdrop");
  await page.click("#confirmok");
  check("AC-137 Delete, confirmed, takes it", await until(async () => !(await page.isVisible(row))), await page.textContent(".agents"));

  // with all projects, each one's name beside it
  await page.goto(`${base}/kanban/`);
  await page.waitForSelector("#addagent");
  await page.click("#fproject summary");
  await page.click('#fproject button:has-text("All projects")');
  check("AC-137 with all projects its project is named", await until(async () => /AgentsHere/.test(await page.textContent(".agents"))), await page.textContent(".agents"));

  // a reload finds what stays
  await page.goto(`${base}/kanban/?project=${project}`);
  check("AC-137 a reload finds it", await until(() => page.isVisible('.agents .agentcard:has-text("Codex two")')));
  await agentPool(page, opts);
  await projectWindowChecks(page, opts);
  await page.goto(`${base}/kanban/`);
  await page.waitForSelector("#addagent");
  await page.click("#fproject summary");
  await page.click('#fproject button:has-text("All projects")');
  await backToSessions();
}

/** A press where a window opened over another modal one reports "element is inert" on some Playwrights.
    At rest first: a window flies in (motion.css), and a button measured in flight has moved by the press. */
const pressAt = async (page, sel) => {
  await page.waitForSelector(sel, { state: "visible" });
  await page.$eval(sel, (e) => Promise.allSettled(e.closest("dialog").getAnimations({ subtree: true }).map((a) => a.finished)));
  const b = await (await page.$(sel)).boundingBox();
  await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2);
};

/** AC-138: tasks chosen and dropped on a free agent, one session; busy refuses more; it stays on their cards. */
async function agentPool(page, opts) {
  const { base, work, tmux, shots } = opts;
  const { j, card, cli } = boardKit(page, opts);
  const project = (await j("/api/task-projects", "POST", { name: "Pool", dir: work })).id;
  const fixer = (await j(`/api/task-projects/${project}/agents`, "POST", { name: "Fast fixer", kind: "claude", effort: "high", command: `python3 -I ${FAKE} ${cli} --kind claude` })).id;
  const task = async (title) => (await j("/api/tasks", "POST", { title, project })).id;
  const a = await task("first of two"), b = await task("second of two"), c = await task("a third");
  const status = async (id) => (await j(`/api/tasks/${id}`)).status;
  const hasStatus = (id, s, ms = 15000) => until(async () => await status(id) === s, ms);
  const paneOf = async (id) => (await j(`/api/tasks/${id}`)).session?.key?.split(":").pop();
  const shown = async () => { await page.goto(`${base}/kanban/?project=${project}`); await page.waitForSelector(card(a)); };
  const pool = `#agentbar .agentcard[data-agent="${fixer}"]`, words = () => page.textContent(`${pool} .agstatus`);
  await shown();

  const said = await page.textContent(pool);
  check("AC-138 a rectangle shows the agent's name, kind and effort", /Fast fixer/.test(said) && /Claude Code/.test(said) && /high/.test(said), said);
  check("AC-138 and that it is free", await words() === "free", await words());
  check("AC-138 a card names no project when one is chosen", !(await page.$(`${card(a)} .cproj`)));
  check("AC-138 a task on no agent shows its owner", await page.isVisible(`${card(a)} .cowner`) && !(await page.$(`${card(a)} .cagent`)));

  // ⌘ or Ctrl+click chooses; Esc lets them go
  await page.click(`${card(a)} .ctitle`, { modifiers: ["Meta"] });
  await page.click(`${card(b)} .ctitle`, { modifiers: ["Meta"] });
  check("AC-138 ⌘+click chooses tasks, no window opens", await page.locator(".card.picked").count() === 2 && !(await page.$("#taskbox[open]")));
  check("AC-138 a line says what to do with them", /2 chosen/.test(await page.textContent(".pickhint")) && await page.isVisible(".pickhint"), await page.textContent(".pickhint"));
  await page.keyboard.press("Escape");
  check("AC-138 Esc lets them go", await page.locator(".card.picked").count() === 0 && !(await page.isVisible(".pickhint")));
  await page.click(`${card(a)} .ctitle`, { modifiers: ["Meta"] });
  await page.click(`${card(b)} .ctitle`, { modifiers: ["Meta"] });

  // dropped on a free agent: its window, one press
  await page.dragAndDrop(card(a), pool);
  check("AC-138 two tasks dropped on a free agent open the run window", await until(() => page.$("#runbox[open]")));
  const head = await page.textContent("#runhead");
  check("AC-138 its title names both and the agent", head.includes(a) && head.includes(b) && head.endsWith("on Fast fixer"), head);
  check("AC-138 no agent to choose: it was dropped on one", !(await page.isVisible("#runagent")));
  check("AC-138 what will be sent stands open for several", await page.evaluate(() => document.querySelector("#runwhat").open) && /The human ran 2 tasks: /.test(await page.textContent("#runprompt")), await page.textContent("#runprompt"));
  check("AC-121 where and how stays folded, filled", !(await page.evaluate(() => document.querySelector("#runhow").open)) && await page.inputValue("#runfolder") === work && (await page.textContent("#runcommand")).includes("fake_agent.py"));
  if (shots) await page.screenshot({ path: path.join(shots, "run-several.png") });
  await page.fill("#runmessage", "WORKS");
  await page.press("#runmessage", "Meta+Enter");
  check("AC-138 ⌘Enter in the message runs", await until(async () => !(await page.$("#runbox[open]"))));
  check("AC-138 both go IN PROGRESS", await hasStatus(a, "IN PROGRESS") && await hasStatus(b, "IN PROGRESS"), `${await status(a)} ${await status(b)}`);
  const key = (await j(`/api/tasks/${a}`)).session?.key;
  check("AC-138 in one session", !!key && (await j(`/api/tasks/${b}`)).session?.key === key, key);
  check("AC-138 what was chosen is let go", await until(async () => await page.locator(".card.picked").count() === 0));
  check("AC-138 the agent says it works on both", await until(async () => { const w = await words(); return w.startsWith("working") && w.includes(a) && w.includes(b); }), await words());
  check("AC-138 a task in progress has no play key, its card names the agent", await until(async () => !(await page.$(`${card(a)} .crun`)) && (await page.textContent(`${card(a)} .cagent`).catch(() => "")) === "Fast fixer"));
  check("AC-138 and hides the owner", !(await page.$(`${card(a)} .cowner`)));

  // busy: a third is refused, on the page and by terminumd
  await page.click(`${card(c)} .ctitle`, { modifiers: ["Meta"] });
  await page.click(pool);
  check("AC-138 a busy agent refuses a third: said", await until(async () => /is busy/.test(await page.textContent("#toast"))), await page.textContent("#toast"));
  check("AC-138 and no window opens", !(await page.$("#runbox[open]")));
  await page.keyboard.press("Escape");
  const refused = await j(`/api/tasks/${c}/run`, "POST", { agent: fixer });
  check("AC-138 terminumd refuses it too", refused.code >= 400 && /is busy/.test(refused.error), JSON.stringify(refused));
  check("AC-138 the third stays", await status(c) === "TODO");

  // a press on a busy one shows its session; its pencil still edits
  await page.click(`${pool} .agstatus`);
  await page.waitForURL("**/sessions/**", { timeout: 8000 });
  check("AC-138 a press on a busy agent opens its session", await until(() => page.evaluate((k) => document.querySelector("#sessions .item.on")?.dataset.id === k, key), 8000));
  await shown();
  await page.hover(pool);
  await page.click(`${pool} .agedit`);
  check("AC-138 its pencil edits it", await until(() => page.$("#agentbox[open]")) && await page.inputValue("#agentname") === "Fast fixer" && await page.inputValue("#agenteffort") === "high");
  await page.click('#agentbox button[value="cancel"]');

  // both finish, in the words its prompt of several names them; they stay on that agent, which is free
  const pane = await paneOf(a);
  tmux("send-keys", "-t", pane, "-l", `The human ran 2 tasks: ${a}, ${b}. FINISHES`);
  tmux("send-keys", "-t", pane, "Enter");
  check("AC-138 both finish", await hasStatus(a, "DONE") && await hasStatus(b, "DONE"), `${await status(a)} ${await status(b)}`);
  check("AC-138 and stay on that agent", await until(async () => await page.textContent(`.col[data-status="DONE"] ${card(a)} .cagent`).catch(() => "") === "Fast fixer" && await page.textContent(`.col[data-status="DONE"] ${card(b)} .cagent`).catch(() => "") === "Fast fixer"));
  check("AC-138 which is free again", await until(async () => await words() === "free"), await words());
  await page.click(`${card(a)} .ctitle`);
  await page.waitForSelector("#taskbox[open]");
  check("AC-138 a task's window names its agent", await until(() => page.isVisible("#taskbody .fieldvalue .cagent"), 2000), await page.textContent("#taskbody .fieldvalue"));
  await page.click("#taskclose");
  await page.waitForFunction(() => /^\/kanban\/$/.test(location.pathname));

  // by its play key, the agent chosen in the window; closing the session frees the agent
  await page.click(`${card(c)} .crun`);
  await page.waitForSelector("#runbox[open]");
  check("AC-138 a play key asks for the agent, the free one picked", await page.isVisible("#runagent") && await page.inputValue("#runagent") === fixer);
  await page.fill("#runmessage", "WORKS");
  await page.click("#rungo");
  check("AC-138 it takes the third", await hasStatus(c, "IN PROGRESS"), await status(c));
  check("AC-138 busy again", await until(async () => (await words()).includes(c)), await words());
  tmux("kill-pane", "-t", await paneOf(c));
  check("AC-138 closing a session frees its agent", await until(async () => await words() === "free", 15000), await words());

  // made and run at once: the new-task window's Run
  await page.click("#newtask");
  await page.waitForSelector("#newbox[open]");
  check("AC-138 a new task offers a free agent", await page.inputValue("#newagent") === fixer, await page.inputValue("#newagent"));
  await page.fill("#newtitle", "made and run FINISHES");
  await page.click("#newrun");
  let made;
  await until(async () => (made = (await j(`/api/tasks?q=${encodeURIComponent("made and run")}`)).tasks?.[0]?.id));
  check("AC-138 Run creates it and starts it at once on that agent", !!made && await hasStatus(made, "DONE") && await until(async () => await page.textContent(`${card(made)} .cagent`).catch(() => "") === "Fast fixer"), made);
  check("AC-138 no window on the way", !(await page.$("#runbox[open]")));
  await page.click("#newtask");
  await page.waitForSelector("#newbox[open]");
  await page.fill("#newtitle", "made, its agent not chosen");
  await page.selectOption("#newagent", "");
  await page.click("#newrun");
  check("AC-138 with none chosen it opens the run window", await until(() => page.$("#runbox[open]")));
  await page.click('#runbox button[value="cancel"]');

  // Archive hides it, Archived shows it, Bring back returns it
  const shelved = await task("not needed");
  await page.waitForSelector(card(shelved), { timeout: 5000 });
  await page.click(card(shelved));
  await page.waitForSelector("#taskbox[open]");
  await page.click('#taskbody .newcomment button:has-text("Archive")');
  check("AC-118 Archive turns its key to Bring back", await until(() => page.isVisible('#taskbody .newcomment button:has-text("Bring back")'), 2000));
  await page.click("#taskclose");
  check("AC-118 an archived task leaves the board", await until(async () => !(await page.$(card(shelved)))));
  await page.click("#farchived");
  check("AC-118 Archived shows it, in the address", await until(() => page.isVisible(card(shelved))) && new URL(page.url()).searchParams.get("archived") === "1", page.url());
  await page.click(card(shelved));
  await page.waitForSelector("#taskbox[open]");
  const back = await until(() => page.isVisible('#taskbody .newcomment button:has-text("Bring back")'), 2000);
  check("AC-118 an archived task's window offers Bring back", back);
  // with no such key the route brings it back, so what follows still runs
  if (back) await page.click('#taskbody .newcomment button:has-text("Bring back")');
  else await j(`/api/tasks/${shelved}/archive`, "POST", { on: false });
  await until(() => page.isVisible('#taskbody .newcomment button:has-text("Archive")'));
  await page.click("#taskclose");
  await page.waitForFunction(() => /^\/kanban\/$/.test(location.pathname));
  await page.click("#farchived");
  check("AC-118 Bring back returns it to its column", await until(() => page.isVisible(`.col[data-status="TODO"] ${card(shelved)}`)), page.url());

  // Give up: a run that holds its tasks lets go, they fail, its window stays
  const held = await task("given up one"), held2 = await task("given up two");
  await page.waitForSelector(card(held), { timeout: 5000 });
  const ran = await j(`/api/tasks/${held}/run`, "POST", { agent: fixer, comment: "WORKS", also: [held2] });
  check("AC-138 a press of several by the route starts", !!ran.run, JSON.stringify(ran));
  await hasStatus(held, "IN PROGRESS");
  const heldPane = await paneOf(held);
  await page.click(card(held));
  await page.waitForSelector("#taskbox[open]");
  check("AC-138 a task its run holds offers Give up, not Run", await until(() => page.isVisible('#taskbody .newcomment button:has-text("Give up")')) && !(await page.isVisible('#taskbody .newcomment button:has-text("Run")')));
  await page.click('#taskbody .newcomment button:has-text("Give up")');
  await pressAt(page, "#confirmok");
  check("AC-138 Give up fails its open tasks", await hasStatus(held, "FAILED") && await hasStatus(held2, "FAILED"), `${await status(held)} ${await status(held2)}`);
  check("AC-138 its window stays", tmux("list-panes", "-a", "-F", "#{pane_id}").split("\n").map((l) => l.trim()).includes(heldPane));
  await page.click("#taskclose");
  check("AC-138 the agent is free after it", await until(async () => await words() === "free", 15000), await words());
  tmux("kill-pane", "-t", heldPane);
}

/** AC-139: a project made empty, its window stays for its agents; a pencil opens it; a change is kept. */
async function projectWindowChecks(page, opts) {
  const { base, work } = opts;
  const { j } = boardKit(page, opts);
  await page.goto(`${base}/kanban/`);
  await page.waitForSelector("#newproject");
  const named = async (name) => (await j("/api/tasks/meta")).projects.find((p) => p.name === name);
  await page.click("#newproject");
  await page.waitForSelector("#projectbox[open]");
  check("AC-139 a new project's window asks no agents yet", !(await page.isVisible("#projectagents")) && !(await page.isVisible("#projectdelete")));
  await page.fill("#projectname", "Fresh");
  await page.click("#projectok");
  check("AC-139 + Project makes it empty", await until(async () => !!(await named("Fresh"))) && (await j(`/api/tasks?project=${(await named("Fresh")).id}&limit=5`)).tasks.length === 0);
  check("AC-139 the window stays, for its agents", await until(() => page.isVisible("#projectaddagent")) && await page.textContent("#projecthead") === "Fresh");
  const fresh = (await named("Fresh")).id;
  check("AC-139 it is the project shown", await until(() => new URL(page.url()).searchParams.get("project") === fresh), page.url());
  await pressAt(page, "#projectaddagent");
  check("AC-139 Add agent opens the agent's window on it", await until(() => page.$("#agentbox[open]")) && await page.inputValue("#agentproject") === fresh && !(await page.isVisible("#agentproject")));
  await page.fill("#agentname", "Its first");
  await pressAt(page, "#agentok");
  check("AC-139 the agent stands in the project's window", await until(() => page.isVisible('#projectagentlist .agentcard:has-text("Its first")')));
  await pressAt(page, "#projectlimits");
  await page.waitForSelector("#limitsbox[open]");
  await page.fill("#limitwarn", "2");
  await page.fill("#limitend", "3");
  await pressAt(page, "#limitsok");
  check("AC-139 limits are kept from the project's window", await until(async () => (await j("/api/tasks/meta")).limits.some((l) => l.project === fresh && l.warn === 2 && l.end === 3)), JSON.stringify((await j("/api/tasks/meta")).limits));
  await pressAt(page, "#projectok");
  await page.waitForSelector("#projectbox", { state: "hidden" });

  const pencil = async (name) => {
    await page.click("#fproject summary");
    await page.click(`#fproject .projrow:has-text("${name}") .projedit`);
    await page.waitForSelector("#projectbox[open]");
  };
  await pencil("Fresh");
  check("AC-139 the pencil on a project's row opens it", await page.inputValue("#projectname") === "Fresh" && await page.isVisible("#projectdelete") && await page.isDisabled("#projecthost"));
  await page.fill("#projectname", "Fresher");
  await page.fill("#projectdir", work);
  await page.click("#projectok");
  await page.waitForSelector("#projectbox", { state: "hidden" });
  check("AC-139 an edit of name and folder is kept", await until(async () => (await named("Fresher"))?.dir === work), JSON.stringify(await named("Fresher")));
  check("AC-139 the head says the new name", await until(async () => /Fresher/.test(await page.textContent("#fproject summary"))));
  await page.reload();
  await page.waitForSelector("#fproject summary");
  await until(async () => /Fresher/.test(await page.textContent("#fproject summary")));
  await pencil("Fresher");
  check("AC-139 and stands after a reload", await page.inputValue("#projectname") === "Fresher" && await page.inputValue("#projectdir") === work, await page.inputValue("#projectdir"));
  await page.click('#projectbox button[value="cancel"]');
  await page.waitForSelector("#projectbox", { state: "hidden" });
}
