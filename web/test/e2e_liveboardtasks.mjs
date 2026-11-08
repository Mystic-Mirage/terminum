// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A task's family and its end on the board, against a real private terminumd (e2e_live.mjs;
// terminum-kanban.md AC-112, AC-118): subtasks made in a window and shown on demand; DONE for 2 days leaves
// by itself; a project's own Archived, with Find and Return.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { boardKit, nap } from "./boardkit.mjs";
import { check, until } from "./check.mjs";

const card = (id) => `.card[data-id="${id}"]`;

/** Sets when a task's moves happened, from outside, as a store two days old has them. */
function ageMoves(app, id, seconds) {
  const script = `import sqlite3, sys
store = sqlite3.connect(sys.argv[1], timeout=8)
store.execute("UPDATE events SET at = at - ? WHERE task = (SELECT id FROM tasks WHERE number = ?)", (int(sys.argv[2]), int(sys.argv[3])))
store.commit()`;
  execFileSync("python3", ["-I", "-c", script, path.join(app, "kanban.sqlite"), String(seconds), id.slice(2)]);
}

export async function liveBoardTasks(page, opts) {
  const { base, app } = opts;
  const { j } = boardKit(page, opts);
  const project = (await j("/api/task-projects", "POST", { name: "Family" })).id;
  await page.goto(`${base}/kanban/?project=${project}`);
  await page.waitForSelector("#newtask");

  // AC-112: New subtask in its parent's window
  const parent = (await j("/api/tasks", "POST", { title: "the parent", project })).id;
  await page.waitForSelector(card(parent), { timeout: 8000 });
  await page.click(card(parent));
  await page.waitForSelector("#taskbox[open]");
  await page.click('#taskbody .subtasks button:has-text("New subtask")');
  await page.waitForSelector("#newbox[open]");
  check("AC-112 the new-task window says whose subtask it makes", await page.textContent("#newhead") === `New subtask of ${parent}`);
  check("AC-112 its project is its parent's, fixed", await page.inputValue("#newproj") === project && await page.isDisabled("#newproj"));
  await page.fill("#newtitle", "the first subtask");
  await page.click("#newcreate");
  check("AC-112 the parent's window lists it and counts it", await until(async () => /the first subtask/.test(await page.textContent("#taskbody .subtasks")) && /Subtasks 0\/1/.test(await page.textContent("#taskbody .subtasks"))));
  await page.click("#taskclose");
  await page.waitForFunction(() => /^\/kanban\/$/.test(location.pathname));
  const kid = (await j(`/api/tasks/${parent}`)).children?.[0]?.id;
  check("AC-112 a subtask stands in its parent's project", (await j(`/api/tasks/${kid}`)).project === project);
  check("AC-112 its parent's card shows 0/1", await until(async () => (await page.textContent(`${card(parent)} .csubs`)) === "0/1"));
  check("AC-112 off, a subtask is no card of its own", !(await page.$(card(kid))));
  await page.click("#fsubtasks");
  check("AC-112 Show subtasks makes it a card, saying whose", await until(async () => /in T-/.test(await page.textContent(`${card(kid)} .cparent`).catch(() => ""))));
  check("AC-112 Show subtasks stands in the address", new URL(page.url()).searchParams.get("subtasks") === "1");
  await page.click(card(kid));
  await page.waitForSelector("#taskbox[open]");
  check("AC-112 a subtask's window says its parent and offers no New subtask", await until(() => page.isVisible("#taskbody .subof")) && !(await page.$("#taskbody .subtasks")));
  // the subtask's window reading itself again, slowly, as its parent opens: its answer draws nothing over the parent
  await page.route(`**/api/tasks/${kid}`, async (r) => { await nap(1000); await r.continue(); });
  await j(`/api/tasks/${kid}`, "PATCH", { text: "read again" });
  await nap(300);
  await page.click("#taskbody .subof button");
  await nap(1200);
  await page.unroute(`**/api/tasks/${kid}`);
  check("AC-112 its parent opens from there", await until(async () => (await page.textContent("#tasktitle")).startsWith(parent)), await page.textContent("#tasktitle"));
  await page.click("#taskclose");
  await page.click("#fsubtasks");
  const refused = await page.evaluate(async (up) => (await fetch("/api/tasks", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ title: "second level", parent: up }) })).status, kid);
  check("AC-112 a second level is refused", refused >= 400, refused);

  // AC-118: DONE for 2 days leaves by itself, at a walk; its history says who
  const old = (await j("/api/tasks", "POST", { title: "done long ago", project })).id;
  const young = (await j("/api/tasks", "POST", { title: "done just now", project })).id;
  for (const id of [old, young]) await j(`/api/tasks/${id}`, "PATCH", { status: "DONE" });
  ageMoves(app, old, 3 * 24 * 3600);
  check("AC-118 DONE for 2 days archives itself", await until(async () => (await j(`/api/tasks/${old}`)).archived_at, 45000));
  check("AC-118 it leaves the board", await until(async () => !(await page.$(card(old)))));
  check("AC-118 its history says Dispatcher did", (await j(`/api/tasks/${old}`)).history.some((e) => e.text === "Archived by Dispatcher"));
  check("AC-118 it keeps its status", (await j(`/api/tasks/${old}`)).status === "DONE");
  check("AC-118 one DONE just now stays", !(await j(`/api/tasks/${young}`)).archived_at && !!(await page.$(card(young))));

  // AC-118: a project's own Archived
  await j(`/api/tasks/${young}/archive`, "POST", { on: true });
  await page.click(`#fproject summary`);
  await page.click('#fproject button[aria-label="Edit Family"]');
  await page.waitForSelector("#projectbox[open]");
  await page.click("#projectarchived");
  await page.waitForSelector("#archivedbox[open]");
  check("AC-118 the project's window opens its Archived", (await page.textContent("#archivedhead")) === "Archived in Family");
  const rows = () => page.$$eval("#archivedlist li .linklike", (l) => l.map((b) => b.textContent));
  check("AC-118 newest first", await until(async () => (await rows()).length === 2 && (await rows())[0].startsWith(young), 4000), JSON.stringify(await rows()));
  await page.fill("#archivedfind", "long ago");
  check("AC-118 Find narrows it", await until(async () => (await rows()).length === 1 && (await rows())[0].startsWith(old), 4000));
  await page.click('#archivedlist li button:has-text("Return")');
  check("AC-118 Return takes it off the list", await until(async () => !(await rows()).length, 4000));
  check("AC-118 and it stands where it stood", await until(async () => { const t = await j(`/api/tasks/${old}`); return !t.archived_at && t.status === "DONE"; }));
  await page.fill("#archivedfind", "nothing like this");
  check("AC-118 an empty list says so", await until(() => page.isVisible("#archivednone"), 4000));
  await page.click('#archivedbox button[value="cancel"]');
  await page.click('#projectbox button[value="cancel"]');
  await page.goto(`${base}/kanban/`);
}
