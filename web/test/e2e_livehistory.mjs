// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The Message history and the Dashboard against a real private terminumd and tmux (e2e_live.mjs;
// terminum.md AC-151, AC-152; terminum-terminal.md AC-90): an agent's prompt from its hooks and a
// shell's command from the page land in the store; words find them by rank, filters narrow them in
// the address, Back undoes one; Open in session shows the pane at the message; the Dashboard counts
// the human's only and a day leads to the same count in the history.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { boardKit, nap } from "./boardkit.mjs";
import { check, until } from "./check.mjs";

/** SQL into the private store beside terminumd, which holds it now and then: the CLI waits for it (as terminumd's
 *  own connections do), and the FTS triggers need a trusted schema (rusqlite's default, not the CLI's). */
const intoStore = (app, sql) => execFileSync("sqlite3", ["-cmd", ".timeout 15000", path.join(app, "kanban.sqlite"), `PRAGMA trusted_schema = ON; ${sql}`]);
const rows = (page) => page.$$eval("#results .msg", (r) => r.map((x) => x.querySelector(".mtext").textContent));

export async function liveHistory(page, opts) {
  const { base, app, work, shots } = opts;
  const { newPane, keyOf, hook, endPanes } = boardKit(page, opts);
  const agent = newPane("histagent"), key = await keyOf(agent);
  hook(agent, "SessionStart", { source: "startup" });
  hook(agent, "UserPromptSubmit", { prompt: "make the board wait for the board selector", prompt_id: "h1" });
  hook(agent, "UserPromptSubmit", { prompt: "Исправь поиск по сообщениям", prompt_id: "h2" });
  // what Dispatcher typed, as its gate keeps it (a real run needs an agent: e2e_livedispatcher.mjs)
  intoStore(app, `INSERT INTO messages (at, sender, via, agent, pane, host, cwd, text) VALUES (CAST(strftime('%s','now') AS INTEGER) * 1000, 'dispatcher', 'hook', 'claude', '${key}', 'local', '${work}', 'T-0007: the board task for Dispatcher')`);
  // a shell's command, typed in the page: what stood after the prompt, with its line
  await page.goto(`${base}/sessions/`);
  await page.waitForFunction(() => document.querySelectorAll("#sessions .item").length > 0, null, { timeout: 15000 });
  await page.click("#sessions .item");
  await page.waitForSelector("#term", { state: "visible" });
  await page.evaluate(() => document.getElementById("kbd").focus());
  await page.keyboard.type("echo history probe words");
  // Enter tells what the screen shows at the cursor: the typed line, once it is drawn
  await until(() => page.evaluate(() => document.getElementById("screen").innerText.includes("echo history probe words")), 8000);
  await page.keyboard.press("Enter");
  await nap(600);

  check("AC-151 the session page's head leads to the Message history", await page.getAttribute("#tohistory", "href") === "/history/");
  await page.goto(`${base}/history/`);
  check("AC-151 newest first, every message kept, Dispatcher's too", await until(async () => (await rows(page)).length >= 4, 8000), JSON.stringify(await rows(page)));
  const first = await rows(page);
  check("AC-151 the newest is the shell's command", first[0] === "echo history probe words", first[0]);
  check("AC-151 under a day's head with its count", /^Today · \d+/.test(await page.textContent("#results .dayhead")), await page.textContent("#results .dayhead"));
  check("AC-151 Dispatcher's is marked", await page.isVisible("#results .msg .mbot"));
  check("AC-151 the day strip draws a column a day", (await page.$$("#stripplot rect.hit")).length >= 30);

  await page.fill("#q", "boar");
  check("AC-151 words find by their start, the denser match first", await until(async () => (await rows(page)).length === 2 && (await rows(page))[0].includes("make the board"), 6000), JSON.stringify(await rows(page)));
  check("AC-151 the hits are marked", (await page.textContent("#results .msg mark.hit")) === "board");
  check("AC-151 ranked by relevance with words", await page.getAttribute('#hsort [data-sort="best"]', "aria-pressed") === "true");
  check("AC-151 the words are in the address", new URL(page.url()).searchParams.get("q") === "boar", page.url());
  await page.click("#fsender summary");
  await page.click('#fsender .pname:text-is("You")');
  check("AC-151 From: You leaves Dispatcher's out", await until(async () => (await rows(page)).length === 1, 6000), JSON.stringify(await rows(page)));
  check("AC-151 the filter is in the address", new URL(page.url()).searchParams.get("sender") === "human", page.url());
  await page.goBack();
  check("AC-151 Back undoes the filter", await until(async () => (await rows(page)).length === 2, 6000) && !new URL(page.url()).searchParams.get("sender"), page.url());
  await page.fill("#q", "поиск");
  check("AC-151 a Cyrillic word is found, any case", await until(async () => JSON.stringify(await rows(page)) === JSON.stringify(["Исправь поиск по сообщениям"]), 6000), JSON.stringify(await rows(page)));
  await page.fill("#q", "-board");
  check("AC-151 a -word alone ranks nothing: newest first", await until(async () => (await rows(page)).length >= 4, 6000) && await page.isDisabled('#hsort [data-sort="best"]'));
  await page.fill("#q", "");
  await until(async () => (await rows(page)).length >= 4, 6000);
  await page.click("#fkind summary");
  await page.click('#fkind .pname:text-is("Shells\' commands")');
  check("AC-151 Where: shells only", await until(async () => JSON.stringify(await rows(page)) === JSON.stringify(["echo history probe words"]), 6000), JSON.stringify(await rows(page)));
  if (shots) await page.screenshot({ path: path.join(shots, "history.png") });

  // Open in session: the pane, the message brought into view (AC-90's Jump)
  await page.click("#results .msg");
  check("AC-151 a pressed row opens with its keys", await page.isVisible("#results .msg.open .macts"));
  await page.click('#results .msg.open .macts button:has-text("Context")');
  check("AC-151 Context: what was sent around it in that pane", await until(() => page.isVisible("#results .ctx .ctxrow.self"), 4000));
  const href = await page.getAttribute("#results .msg.open .macts a", "href");
  check("AC-151 Open in session goes to its pane, at the message", /^\/sessions\/\?then=msg:\d+#\//.test(href ?? ""), href);
  await page.click("#results .msg.open .macts a");
  await page.waitForURL(/\/sessions\//);
  check("AC-151 the session page opens without its hand-off in the address", await until(() => !page.url().includes("then="), 8000), page.url());
  check("AC-151 the message is brought into view where it was sent", await until(() => page.evaluate(() => [...document.querySelectorAll("#screen .ln")].some((l) => l.textContent.includes("echo history probe words"))), 8000));

  // the Dashboard: the human's only
  await page.goto(`${base}/dashboard/`);
  await page.waitForSelector("#tiles .tile");
  const tile = await page.textContent("#tiles .tile .tvalue");
  check("AC-152 Messages counts the human's, never Dispatcher's", tile === "3", tile);
  check("AC-152 every card draws", (await page.$$("#perday rect.hit")).length === 30 && (await page.$$("#heat rect.cell")).length === 7 * 24 && await page.isVisible("#projects .rrow"));
  await page.focus("#perday");
  await page.keyboard.press("End");
  check("AC-152 a day chosen by the keys says its count", /Today so far · 3 messages/.test(await page.textContent("#perdaysay")), await page.textContent("#perdaysay"));
  await page.keyboard.press("Enter");
  await page.waitForURL(/\/history\/\?/);
  check("AC-152 a day leads to the history of that day, the human's", await until(async () => /^3 messages/.test(await page.textContent("#hsum")), 6000), await page.textContent("#hsum"));
  await page.goto(`${base}/dashboard/?days=7`);
  check("AC-152 the period is in the address", await until(async () => (await page.$$("#perday rect.hit")).length === 7, 6000));
  if (shots) {
    // a month of made-up work to look at: three projects, days of different length, a weekend off
    const seed = [];
    for (let d = 1; d < 30; d++) {
      if (d % 7 === 2) continue;
      for (let k = 0; k < 20 + ((d * 37) % 60); k++) {
        const at = `(CAST(strftime('%s', date('now', 'localtime', '-${d} days'), '+${8 + (k * 7) % 12} hours', '+${(k * 13) % 60} minutes', 'utc') AS INTEGER) * 1000)`;
        const project = ["pa", "pb", "pc", null][k % 4];
        seed.push(`(${at}, 'human', 'hook', ${k % 3 ? "'claude'" : "NULL"}, 'k${k % 5}', 'local', '${work}', ${project ? `'${project}', 'Project ${project}'` : "NULL, NULL"}, 'seeded message ${d} ${k}')`);
      }
    }
    intoStore(app, `INSERT INTO messages (at, sender, via, agent, pane, host, cwd, project, project_name, text) VALUES ${seed.join(",")}`);
    await page.goto(`${base}/dashboard/`);
    await page.waitForSelector("#tiles .tile");
    await nap(500);
    await page.screenshot({ path: path.join(shots, "dashboard.png"), fullPage: true });
    await page.goto(`${base}/history/?q=seeded`);
    await page.waitForSelector("#results .msg");
    await page.screenshot({ path: path.join(shots, "history-seeded.png") });
  }
  endPanes();
}

/** Both pages at a phone's width: one column, no sideways scroll, the filters behind one key. */
export async function liveHistoryPhone(page, { base, shots }) {
  for (const p of ["history", "dashboard"]) {
    await page.goto(`${base}/${p}/`);
    await page.waitForSelector(p === "history" ? "#results .msg, #results .mempty" : "#tiles .tile");
    const wide = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
    check(`AC-${p === "history" ? 151 : 152} ${p} at a phone's width has no sideways scroll`, wide <= 0, String(wide));
    if (shots) await page.screenshot({ path: path.join(shots, `${p}-phone.png`), fullPage: true });
  }
  check("AC-151 the phone's filters stand behind one key", await page.goto(`${base}/history/`).then(() => page.isVisible("#hfilters")) && !(await page.isVisible("#fsender")));
}
