// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-92 against a real private terminumd and tmux (e2e_live.mjs): real panes closed in tmux, with
// terminumd's own order of messages. The shown session's close shows its project's first session, and
// its row never stands in its tmux group on the way; its project's only session closed, the project
// goes and the top of the list is shown.
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { check, shownPane, until, within } from "./check.mjs";

export async function liveListPlace(page, { tmux, work, app }) {
  const shown = await shownPane(page, { tmux, work });
  const row = (pane) => `#sessions .item[data-id$=":${pane}"]`;
  const open = async (name) => {
    const pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", name, "-c", work);
    await page.waitForSelector(row(pane), { timeout: 5000 });
    return pane;
  };
  const projectMenu = async (pane) => {
    await page.click(row(pane), { button: "right" });
    await page.click('#rowmenu button[data-action="project"]');
  };
  const toProject = async (pane, make) => {
    await projectMenu(pane);
    if (make) {
      await page.click('#rowmenu button[data-action="newproject"]');
      await page.waitForSelector("#namebox[open]", { timeout: 3000 });
      await page.fill("#namefield", make);
      await page.press("#namefield", "Enter");
    } else await page.click('#rowmenu button:has-text("Place")');
    await page.waitForSelector(`#sessions section.project .item[data-id$=":${pane}"]`, { timeout: 5000 });
  };
  const a = await open("place-a"), b = await open("place-b");
  await toProject(a, "Place");
  await toProject(b, null);
  const stored = new DatabaseSync(path.join(app, "kanban.sqlite"), { readOnly: true });
  // one order for projects and tmux groups: a group goes above the project, kept by terminumd, on a reload too
  const parts = () => page.evaluate(() => [...document.querySelectorAll("#sessions section.grp[data-part]")].map((s) => s.dataset.part));
  const wid = (await parts()).find((p) => p.startsWith("g:"));
  await page.click(`#sessions section[data-part="${wid}"] > h2`, { button: "right" });
  await page.click('#rowmenu button[data-action="top"]');
  check("AC-92 live: a tmux group's Move to the Top stands above the project", await until(async () => (await parts())[0] === wid), JSON.stringify(await parts()));
  check("AC-92 live: terminumd keeps the one order", await until(() => stored.prepare("SELECT part FROM list_order ORDER BY place").get()?.part === wid), String(stored.prepare("SELECT count(*) AS n FROM list_order").get().n).slice(0, 200));
  await page.reload();
  await page.waitForSelector("#sessions section.grp[data-part]", { timeout: 8000 });
  check("AC-92 live: after a reload too", await until(async () => (await parts())[0] === wid), JSON.stringify(await parts()));
  const members = (name) => stored.prepare("SELECT count(*) AS n FROM project_members m JOIN projects p ON p.id = m.project WHERE p.name = ?").get(name).n;

  // a member just added and closed: the list without it is told at once, the projects when its 5 s
  // end, with no list to go with them: the row menu offers only the projects the page was told of.
  // Those 5 s also end a's and b's, added before: below they leave with the read that misses them
  const c = await open("place-c");
  await toProject(c, "Spared");
  tmux("kill-pane", "-t", c);
  const gone = await until(() => members("Spared") === 0, 10000);
  await page.waitForTimeout(300);
  await projectMenu(shown);
  const offered = await page.locator('#rowmenu button:has-text("Spared")').count();
  await page.keyboard.press("Escape");
  check("AC-110 a member just added closes: it leaves after 5 s, its project stays, with no other change", gone && offered === 1, JSON.stringify({ gone, offered }));

  await page.click(row(a));
  await page.waitForTimeout(300);
  await page.evaluate((id) => {   // what is drawn after each message of terminumd
    window.hopped = false;
    window.hopWatch = new MutationObserver(() => { window.hopped ||= !!document.querySelector(`#sessions section:not(.project) .item[data-id$=":${id}"]`); });
    window.hopWatch.observe(document.getElementById("sessions"), { childList: true, subtree: true });
  }, a);
  tmux("kill-pane", "-t", a);
  await within("AC-92 a real pane closes: the page shows its project's first session", () =>
    page.waitForFunction((id) => document.querySelector('#sessions .item[aria-current="true"]')?.dataset.id?.endsWith(`:${id}`), b, { timeout: 5000 }));
  check("AC-92 a closed session's row never stands in its tmux group", !await page.evaluate(() => { window.hopWatch.disconnect(); return window.hopped; }));

  const top = await page.evaluate((p) => [...document.querySelectorAll("#sessions .item:not(.child)")].map((e) => e.dataset.id).find((id) => !id.endsWith(`:${p}`)), b);
  tmux("kill-pane", "-t", b);
  await within("AC-92 its project's only session closes: the page shows the top of the list", () =>
    page.waitForFunction((id) => document.querySelector('#sessions .item[aria-current="true"]')?.dataset.id === id, top, { timeout: 5000 }));
  await within("AC-110 and the project stays, with no session", () => page.waitForFunction(() => [...document.querySelectorAll("#sessions section.project .label")].some((l) => l.textContent === "Place"), null, { timeout: 2000 }));
  check("AC-110 the project of a closed only session stays in the store", await until(() => members("Place") === 0, 10000) && stored.prepare("SELECT count(*) AS n FROM projects WHERE name = 'Place'").get().n === 1);
  stored.close();

  await page.click(row(shown));
  await page.waitForTimeout(500);
}
