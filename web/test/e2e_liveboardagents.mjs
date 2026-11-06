// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A project's agents on the board, against a real private terminumd (e2e_live.mjs;
// terminum-dispatcher.md AC-137): kept, listed, changed and deleted by a human, refused with words.
import path from "node:path";
import { boardKit } from "./boardkit.mjs";
import { check, until } from "./check.mjs";

const COMMAND = "claude --model opus";

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
  const row = '.agents .item:has-text("Opus reviewer")';
  check("AC-137 Enter keeps it and it stands under Agents", await until(() => page.isVisible(row)), await page.textContent(".agents"));
  check("AC-137 the window closes", !(await page.$("#agentbox[open]")));
  check("AC-137 its row says its kind, server and command", await page.isVisible(`${row} .agmark img`) && /This Mac/.test(await page.textContent(row)) && (await page.textContent(row)).includes(COMMAND), await page.textContent(row));
  const stored = (await j("/api/tasks/meta")).agents.find((a) => a.project === project);
  check("AC-137 its command is kept as written", stored?.command === COMMAND && stored.kind === "claude" && stored.host === null && stored.dir === "", JSON.stringify(stored));
  if (shots) await page.screenshot({ path: path.join(shots, "agents-column.png") });

  // every page hears
  const other = await page.context().newPage();
  await other.goto(`${base}/kanban/?project=${project}`);
  await other.waitForSelector(row);
  await j(`/api/task-projects/${project}/agents`, "POST", { name: "Codex two", kind: "codex", dir: "~/work", command: "codex" });
  check("AC-137 every page hears within 2 s", await until(() => other.isVisible('.agents .item:has-text("Codex two")'), 2500));
  await other.close();
  check("AC-137 this one too", await until(() => page.isVisible('.agents .item:has-text("Codex two")'), 2500));

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
  check("AC-137 Save keeps a change", await until(async () => (await page.textContent(row)).includes("claude --model sonnet")), await page.textContent(row));
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
  check("AC-137 a reload finds it", await until(() => page.isVisible('.agents .item:has-text("Codex two")')));
  await page.click("#fproject summary");
  await page.click('#fproject button:has-text("All projects")');
  await backToSessions();
}
