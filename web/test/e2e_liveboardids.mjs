// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Tasks' IDs by project, a task's window in parts and a parent's subtasks as boxes, against a real private
// terminumd (e2e_live.mjs; terminum-kanban.md AC-148, AC-149, AC-112).
import path from "node:path";
import { boardKit, nap } from "./boardkit.mjs";
import { check, until } from "./check.mjs";

export async function liveBoardIds(page, opts) {
  const { base, shots } = opts;
  const { j, card } = boardKit(page, opts);
  const project = (await j("/api/task-projects", "POST", { name: "Ledger" })).id;
  const make = async (title, more = {}) => (await j("/api/tasks", "POST", { title, project, ...more })).id;

  // AC-148: the project's letters and its own count; set anew, the old IDs still find them
  const first = await make("the first");
  const second = await make("the second");
  check("AC-148 a project's tasks are its letters and its own count", /^LEDG-\d+$/.test(first) && Number(second.split("-")[1]) === Number(first.split("-")[1]) + 1, `${first} ${second}`);
  const meta = await j("/api/tasks/meta");
  check("AC-148 the board knows the letters and the default template", meta.projects.find((p) => p.id === project)?.prefix === "LEDG" && /## Expected result/.test(meta.task_template));
  const set = await j(`/api/task-projects/${project}`, "PATCH", { prefix: "lg" });
  const now = await j(`/api/tasks/${first}`);
  check("AC-148 new letters keep the numbers, and the old ID finds the task", set.code === 200 && now.id === `LG-${first.split("-")[1]}`, JSON.stringify([set, now.id]));
  const refused = await j(`/api/task-projects/${project}`, "PATCH", { prefix: "T-1" });
  check("AC-148 letters of the wrong shape are refused, said", refused.code === 400 && /ID letters/.test(refused.error), JSON.stringify(refused));
  await page.goto(`${base}/kanban/${first}`);
  await page.waitForSelector("#taskbox[open]");
  check("AC-148 an old ID in the address opens the task and stands as its ID now", await until(() => new URL(page.url()).pathname === `/kanban/${now.id}`), page.url());
  await page.click("#taskclose");
  await page.goto(`${base}/kanban/${now.uid}`);
  await page.waitForSelector("#taskbox[open]");
  check("AC-148 its key's start names it too", (await page.textContent("#tasktitle")).startsWith(now.id) && /^[0-9a-f]{8}$/.test(now.uid), now.uid);

  // AC-149: the task, its subtasks, what was said of it: apart; comments newest first, turned by a key
  await j(`/api/tasks/${now.id}/comments`, "POST", { text: "older word" });
  await nap(1100);                                  // a second apart: their times differ
  const said = await j(`/api/tasks/${now.id}/comments`, "POST", { text: "newer word" });
  const edited = (await j(`/api/tasks/${now.id}`)).history.find((e) => e.text === "older word");
  await j(`/api/tasks/${now.id}/comments/${edited.id}`, "PATCH", { text: "older word, changed" });
  await page.reload();
  await page.waitForSelector("#taskbox[open] .activity");
  const parts = await page.$$eval("#taskbody > section", (s) => s.map((x) => x.className));
  check("AC-149 three parts: the task, its subtasks, comments and history", JSON.stringify(parts.slice(0, 3)) === JSON.stringify(["tasksec taskown", "tasksec subtasks", "tasksec activity"]), JSON.stringify(parts) + said.code);
  check("AC-149 the task says its key's start, when it was made and changed", new RegExp(`^${now.uid} · made .+ · changed `).test(await page.textContent("#taskbody .taskstamps")), await page.textContent("#taskbody .taskstamps"));
  const order = () => page.$$eval("#taskbody .history > li", (l) => l.map((x) => x.textContent));
  const comments = () => page.$$eval("#taskbody .history > li.ev-comment", (l) => l.map((x) => x.textContent));
  check("AC-149 newest first", await until(async () => (await comments())[0]?.includes("newer word") && /changed a comment/.test((await order())[0])), JSON.stringify((await order()).slice(0, 2)));
  check("AC-149 a changed comment says when it was edited", (await order()).some((t) => t.includes("older word, changed") && /edited /.test(t)));
  await page.click('#taskbody .activity button:has-text("Newest first")');
  check("AC-149 the key turns the order, and says it", await until(async () => /made it/.test((await order())[0] ?? "")) && await page.isVisible('#taskbody .activity button:has-text("Oldest first")'));
  await page.click('#taskbody .activity button:has-text("Oldest first")');
  if (shots) await page.screenshot({ path: path.join(shots, "task-parts.png") });
  await page.click("#taskclose");

  // AC-112: boxes on the card in their status's color; put under, refused, taken out
  const parent = await make("a parent");
  const kids = [];
  for (const status of ["TODO", "IN PROGRESS", "WAIT FOR HUMAN", "FAILED", "DONE"]) {
    const id = await make(`kid ${status}`, { parent });
    if (status !== "TODO") await j(`/api/tasks/${id}`, "PATCH", { status });
    kids.push(id);
  }
  await page.goto(`${base}/kanban/?project=${project}`);
  await page.waitForSelector(`${card(parent)} .cboxes`, { timeout: 8000 });
  const boxes = await page.$$eval(`${card(parent)} .cboxes .stbox`, (b) => b.map((x) => [x.textContent, getComputedStyle(x).backgroundColor, x.getAttribute("aria-label")]));
  check("AC-112 a box for each subtask, in the order made, saying its ID", JSON.stringify(boxes.map((b) => b[0])) === JSON.stringify(kids), JSON.stringify(boxes));
  check("AC-112 each status its own color", new Set(boxes.map((b) => b[1])).size === 5, JSON.stringify(boxes.map((b) => b[1])));
  check("AC-112 its status in words too", boxes[3][2] === `${kids[3]} · FAILED · kid FAILED`, boxes[3][2]);
  check("AC-112 the card counts the done", (await page.textContent(`${card(parent)} .csubs`)) === "1/5");
  if (shots) await page.locator(card(parent)).screenshot({ path: path.join(shots, "card-boxes.png") });
  await page.click(`${card(parent)} .cboxes .stbox >> nth=2`);
  check("AC-112 a box opens its subtask", await until(async () => (await page.textContent("#tasktitle")).startsWith(kids[2])));
  await page.click("#taskclose");

  const loose = await make("a loose one");
  await page.click(`${card(parent)} .ctitle`);   // a press on a box would open that subtask
  await page.waitForSelector("#taskbox[open] .subtasks");
  await page.click('#taskbody .subtasks button:has-text("Add existing")');
  await page.waitForSelector("#namebox[open]");
  await page.fill("#namefield", parent);
  await page.click("#nameok");
  check("AC-112 a refusal is said in the dialog, which stays", await until(async () => /A task cannot be its own subtask/.test(await page.textContent("#nameerror"))) && await page.isVisible("#namebox"));
  await page.fill("#namefield", loose.toLowerCase());
  await page.click("#nameok");
  if (shots) await page.screenshot({ path: path.join(shots, "task-subtasks.png") });
  check("AC-112 Add existing puts a task under it, its line saying status, owner and times", await until(async () => /a loose one/.test(await page.textContent("#taskbody .subtasks")))
    && /TODO · .+ · made /.test(await page.textContent(`#taskbody .sublist li:has-text("a loose one")`)));
  check("AC-112 both histories say so", (await j(`/api/tasks/${loose}`)).history.some((e) => e.text.endsWith(`made it a subtask of ${parent}`)) && (await j(`/api/tasks/${parent}`)).history.some((e) => e.text.endsWith(`made ${loose} its subtask`)));
  await page.click(`#taskbody .sublist li:has-text("a loose one") button[aria-label="Take ${loose} out"]`);
  check("AC-112 × takes it out; it stays a task", await until(async () => !(await j(`/api/tasks/${loose}`)).parent) && await until(async () => !/a loose one/.test(await page.textContent("#taskbody .subtasks"))));
  await page.click("#taskclose");
  await j(`/api/tasks/${kids[0]}/archive`, "POST", { on: true });
  check("AC-112 an archived subtask leaves its parent's boxes and count", await until(async () => (await page.$$(`${card(parent)} .cboxes .stbox`)).length === 4 && (await page.textContent(`${card(parent)} .csubs`)) === "1/4"));
  await page.goto(`${base}/kanban/`);
}
