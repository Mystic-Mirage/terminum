// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-92 against a real private terminumd and tmux (e2e_live.mjs): real panes closed in tmux, with
// terminumd's own order of messages. The shown session's close shows its project's first session;
// its project's only session closed, the project goes and the top of the list is shown.
import fs from "node:fs";
import path from "node:path";
import { check, within } from "./check.mjs";

export async function liveListPlace(page, { tmux, work, app }) {
  const current = () => page.evaluate(() => document.querySelector('#sessions .item[aria-current="true"]')?.dataset.id ?? null);
  const shown = await current();
  const row = (pane) => `#sessions .item[data-id$=":${pane}"]`;
  const open = async (name) => {
    const pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", name, "-c", work);
    await page.waitForSelector(row(pane), { timeout: 5000 });
    return pane;
  };
  const toProject = async (pane, make) => {
    await page.click(row(pane), { button: "right" });
    await page.click('#rowmenu button[data-action="project"]');
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

  await page.click(row(a));
  await page.waitForTimeout(300);
  tmux("kill-pane", "-t", a);
  await within("AC-92 a real pane closes: the page shows its project's first session", () =>
    page.waitForFunction((id) => document.querySelector('#sessions .item[aria-current="true"]')?.dataset.id?.endsWith(`:${id}`), b, { timeout: 5000 }));

  const top = await page.evaluate((p) => [...document.querySelectorAll("#sessions .item:not(.child)")].map((e) => e.dataset.id).find((id) => !id.endsWith(`:${p}`)), b);
  tmux("kill-pane", "-t", b);
  await within("AC-92 its project's only session closes: the page shows the top of the list", () =>
    page.waitForFunction((id) => document.querySelector('#sessions .item[aria-current="true"]')?.dataset.id === id, top, { timeout: 5000 }));
  await within("AC-92 and the project goes (5 s spare a member just added)", () => page.waitForFunction(() => ![...document.querySelectorAll("#sessions section.project .label")].some((l) => l.textContent === "Place"), null, { timeout: 2000 }));
  const file = path.join(app, "projects.json");
  const kept = () => { try { return fs.readFileSync(file, "utf8").includes('"Place"'); } catch { return false; } };
  for (let i = 0; i < 40 && kept(); i++) await page.waitForTimeout(250);
  check("AC-92 the project of a closed only session leaves projects.json", !kept(), String(await current()));

  await page.click(`#sessions .item[data-id="${shown}"]`);
  await page.waitForTimeout(500);
}
