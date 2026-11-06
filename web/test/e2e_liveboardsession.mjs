// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A card's session and the project's sessions beside the columns, against a real private terminumd and
// tmux (e2e_live.mjs; terminum-kanban.md AC-134, AC-135): the session list's own row, following the
// session with no task read; a press opens the session; the project chosen in the head, remembered.
import path from "node:path";
import { boardKit } from "./boardkit.mjs";
import { check, until } from "./check.mjs";

export async function liveBoardSessions(page, opts) {
  const { base, shots } = opts;
  const { j, newPane, keyOf, hook, tool, endPanes, card, open } = boardKit(page, opts);
  /** The project key's menu, and a press in it. */
  const inProjects = async (what) => { await page.click("#fproject summary"); await page.click(`#fproject ${what}`); };
  await open();
  const a = newPane("cardrow"), b = newPane("cardother"), key = await keyOf(a), other = await keyOf(b);
  const project = (await j("/api/task-projects", "POST", { name: "Billing" })).id;
  const made = (await j("/api/tasks", "POST", { title: "row on a card", session: key })).id;
  await j(`/api/tasks/${made}`, "PATCH", { project, session_choice: "move" });   // its session goes with it, into Billing
  const empty = (await j("/api/task-projects", "POST", { name: "Empty" })).id;
  const bare = (await j("/api/tasks", "POST", { title: "no session", project })).id;
  await open();

  // AC-134: the list's row on the card, whatever the column
  const row = `${card(made)} .csession .item`;
  check("AC-134 a card with a live session carries the list's row", await until(() => page.isVisible(row)), await page.textContent(card(made)));
  check("AC-134 a card with no session has none", !(await page.$(`${card(bare)} .csession`)) && !(await page.$(`${card(bare)} .cstate`)));
  hook(a, "SessionStart", { source: "startup" }); hook(a, "UserPromptSubmit", { prompt: "go" });
  check("AC-134 its bar pulses while the agent works", await until(() => page.isVisible(`${row} .pbar.busy`), 4000));
  const color = await page.evaluate((s) => getComputedStyle(document.querySelector(s)).getPropertyValue("--dot"), `${row} .pbar`);
  check("AC-134 the bar is the project's color", /oklch/.test(color), color);
  check("AC-134 the word working goes: the bar says it", !/working/.test(await page.textContent(`${card(made)} .cfoot`)));
  const planned = tool(a, "update_session_plan", { items: [{ text: "one", status: "completed" }, { text: "two", status: "in_progress" }], doing: "Reading the retries", eta_minutes: 30 });
  check("AC-134 the card shows the plan's percent, with no task read", await until(async () => (await page.textContent(`${row} .plpct`).catch(() => "")) === "50%", 4000), planned.slice(0, 200));
  check("AC-134 and its track", await page.isVisible(`${row} .pltrack`));
  check("AC-134 and what the agent says it does now", await until(async () => /Reading the retries/.test(await page.textContent(row)), 4000), await page.textContent(row));
  if (shots) await page.screenshot({ path: path.join(shots, "card-session.png") });
  await page.evaluate((s) => { document.querySelector(s).dataset.same = "1"; }, card(made));
  hook(a, "Notification", { notification_type: "permission_prompt", message: "needs a yes" });
  check("AC-134 a waiting agent's mark comes to the row", await until(() => page.isVisible(`${row} .state.waiting`), 4000));
  check("AC-134 and its words stay on the card", /waits for you/.test(await page.textContent(card(made))), await page.textContent(card(made)));
  check("AC-134 only the row changed, not the card", await page.evaluate((s) => document.querySelector(s).dataset.same === "1", card(made)));
  await j(`/api/tasks/${made}`, "PATCH", { status: "IN PROGRESS" });
  check("AC-134 it goes with the task to IN PROGRESS", await until(() => page.isVisible(`.col[data-status="IN PROGRESS"] ${row}`), 4000));
  await j(`/api/tasks/${made}`, "PATCH", { status: "DONE" });
  check("AC-134 and to DONE", await until(() => page.isVisible(`.col[data-status="DONE"] ${row}`), 4000));
  await j(`/api/tasks/${made}`, "PATCH", { status: "IN PROGRESS" });
  await until(() => page.isVisible(`.col[data-status="IN PROGRESS"] ${row}`), 4000);

  // a drag by its row moves the card, opens nothing
  await page.dragAndDrop(row, '.col[data-status="FAILED"]', { force: true });
  check("AC-134 a drag by the row moves the card", await until(() => page.isVisible(`.col[data-status="FAILED"] ${row}`), 4000));
  check("AC-134 and opens no session", new URL(page.url()).pathname === "/kanban/", page.url());

  // a press opens the session, not the task's window
  await page.click(row);
  await page.waitForURL("**/sessions/**", { timeout: 8000 });
  check("AC-134 a press shows that session", await until(() => page.evaluate((k) => document.querySelector(`#sessions .item.on`)?.dataset.id === k, key), 8000), key);
  check("AC-134 the address goes back to the page's", await until(() => new URL(page.url()).pathname + new URL(page.url()).search === "/sessions/"), page.url());
  await page.goBack();
  await page.waitForSelector(row, { timeout: 8000 });
  check("AC-134 Back returns to the board, no window open", !(await page.evaluate(() => document.querySelector("#taskbox").open)));

  // gone by the time it opens
  await page.goto(`${base}/sessions/?open=${encodeURIComponent("tmux:local:nobody:%999")}`);
  check("AC-134 a session gone by then is said", await until(async () => /has ended/.test(await page.textContent("#toast")), 8000), await page.textContent("#toast"));
  await page.waitForSelector("#sessions .item.on", { timeout: 8000 });

  // AC-135: the project in the head, its sessions beside the columns
  await open();
  check("AC-135 with no project chosen the key says All projects", /All projects/.test(await page.textContent("#fproject summary")), await page.textContent("#fproject summary"));
  await inProjects('.projrow button:has-text("Billing")');
  check("AC-135 a name chooses that project, in the URL", await until(() => new URL(page.url()).searchParams.get("project") === project), page.url());
  check("AC-135 the key says it", /Billing/.test(await page.textContent("#fproject summary")) && await page.isVisible("#fproject summary .pdot"));
  check("AC-135 its menu closes", !(await page.evaluate(() => document.querySelector("#fproject").open)));
  check("AC-135 the columns hold its tasks", await until(() => page.isVisible(card(made))));
  check("AC-135 and the Sessions column stands first", await page.evaluate(() => { const s = document.querySelector("#sidecol").getBoundingClientRect(), t = document.querySelector('.col[data-status="TODO"]').getBoundingClientRect(); return s.right <= t.left && s.width >= 240; }));
  const listed = () => page.evaluate(() => [...document.querySelectorAll("#sidecol .item")].map((e) => e.dataset.id));
  check("AC-135 Billing's session stands beside Billing", await until(async () => (await listed()).includes(key)), (await listed()).join());
  check("AC-135 in the list's own group, in its color", await page.isVisible(`#sidecol section.project[data-pid="${project}"] .item[data-id="${key}"]`));
  check("AC-135 a session in no project does not", !(await listed()).includes(other), (await listed()).join());
  await page.goto(`${base}/kanban/?project=${empty}`);
  check("AC-135 a project with no session says so", await until(async () => /No sessions here/.test(await page.textContent("#sidecol")), 4000), await page.textContent("#sidecol"));
  await inProjects('.projrow button:has-text("Default")');
  check("AC-135 Default holds the sessions in no project", await until(async () => (await listed()).includes(other)), (await listed()).join());
  await inProjects('.projrow:has-text("Billing") input');
  check("AC-135 a tick adds a project", await until(() => /,/.test(new URL(page.url()).searchParams.get("project") ?? "")), page.url());
  check("AC-135 the key says how many", /\+1/.test(await page.textContent("#fproject summary")), await page.textContent("#fproject summary"));
  await page.click('#fproject button:has-text("All projects")');
  check("AC-135 All projects clears it", await until(() => !new URL(page.url()).searchParams.has("project")), page.url());
  check("AC-135 and the column holds the whole list", await until(async () => (await listed()).includes(other)));
  await inProjects('.projrow button:has-text("Default")');
  await page.goto(`${base}/kanban/`);
  check("AC-135 this browser remembers the project", await until(() => new URL(page.url()).searchParams.get("project") === "default"), page.url());
  await page.goto(`${base}/kanban/?project=${project}`);
  check("AC-135 an address that names one wins", await until(async () => /Billing/.test(await page.textContent("#fproject summary"))));
  await inProjects('button:has-text("All projects")');
  await page.waitForFunction(() => !new URL(location.href).searchParams.has("project"));

  // a press on a row opens it; a task's mark stays on the board
  await until(async () => (await listed()).includes(other));
  await page.click(`#sidecol .item[data-id="${other}"]`);
  await page.waitForURL("**/sessions/**", { timeout: 8000 });
  check("AC-135 a press opens the session", await until(() => page.evaluate((k) => document.querySelector("#sessions .item.on")?.dataset.id === k, other), 8000));
  await open();
  await page.click(`#sidecol .item[data-id="${key}"] .taskmark`, { timeout: 4000 });
  check("AC-135 a task's mark opens its window, no page loading", await until(() => page.evaluate(() => document.querySelector("#taskbox").open)));
  await page.click("#taskclose");

  // it can be hidden, and it stays so
  await page.click("#sidetoggle");
  check("AC-135 the key hides the column", !(await page.isVisible("#sidecol")));
  const roomy = await page.evaluate(() => document.querySelector('.col[data-status="TODO"]').getBoundingClientRect().left < 40);
  check("AC-135 the columns take its room", roomy);
  await page.reload();
  await page.waitForSelector('.col[data-status="TODO"]');
  check("AC-135 and this browser remembers it", !(await page.isVisible("#sidecol")));
  await page.click("#sidetoggle");
  check("AC-135 the key brings it back", await page.isVisible("#sidecol"));

  // a project that is gone
  await page.goto(`${base}/kanban/?project=gone-project`);
  check("AC-135 a project gone is said, and all show", await until(async () => /gone-project/.test(await page.textContent("#toast")) && !new URL(page.url()).searchParams.has("project"), 4000), page.url());

  // a field's own text in a window is the text's color, not its label's muted (controls.css)
  await page.goto(`${base}/kanban/`);
  await page.waitForSelector("#board .col");
  await page.click("#newtask");
  const inks = await page.evaluate(() => {
    const t = document.querySelector("#newtitle"), s = document.querySelector("#newproj"), body = getComputedStyle(document.body).color;
    return [getComputedStyle(t).color, getComputedStyle(s).color, body];
  });
  check("AC-135 look: a window's fields read in the text's color", inks[0] === inks[2] && inks[1] === inks[2], inks.join(" | "));
  await page.click("#newclose");

  endPanes();
  await page.goto(`${base}/sessions/`);
  await page.waitForFunction(() => document.querySelectorAll("#sessions .item").length > 0, null, { timeout: 10000 });
  await page.click("#sessions .item");
  await page.waitForSelector("#term", { state: "visible", timeout: 5000 });
}

/** A phone: Sessions the last page of the swipe, the row's press a finger's (AC-134, AC-135). */
export async function liveBoardSessionsPhone(page, { base }) {
  await page.goto(`${base}/kanban/`);
  await page.waitForSelector("#colswitch button");
  const tabs = await page.evaluate(() => [...document.querySelectorAll("#colswitch button")].map((t) => t.dataset.status));
  check("AC-135 a phone's switch ends with Sessions, TODO first", tabs[0] === "TODO" && tabs.at(-1) === "Sessions", tabs.join());
  check("AC-135 the project key stands left of Find", await page.evaluate(() => { const p = document.querySelector("#fproject").getBoundingClientRect(), f = document.querySelector("#filters .search").getBoundingClientRect(); return p.right <= f.left && Math.abs(p.top - f.top) < 8; }));
  await page.click('#colswitch button[data-status="Sessions"]');
  check("AC-135 Sessions is a page of its own", await until(() => page.evaluate(() => { const r = document.querySelector("#sidecol").getBoundingClientRect(); return r.left > -5 && r.right < innerWidth + 5; })));
  const wide = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
  check("AC-135 no sideways scroll on a phone", wide <= 0, String(wide));
  check("AC-137 a phone's Sessions page holds the agents", await page.isVisible(".agents .colhead"));
  await page.click("#addagent");
  const fits = await page.evaluate(() => { const r = document.querySelector("#agentbox").getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth && document.querySelector("#agentok").getBoundingClientRect().bottom <= innerHeight; });
  check("AC-137 its window fits a phone, its keys in reach", fits);
  await page.click('#agentbox button[value="cancel"]');
}
