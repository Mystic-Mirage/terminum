// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-31 a table a real program printed, on a real terminumd and tmux (e2e_live.mjs): a head of two
// lines, output scrolling it off line by line and in batches, then a block redrawn in place as a
// coding agent does: no frame shows its rows, its <table> is drawn once, its head stays whole.
import fs from "node:fs";
import path from "node:path";
import { check, shownPane, until } from "./check.mjs";
import { watch } from "./e2e_widgetsteady.mjs";

const TABLE = ["┌───┬──────────┬──────────────┐", "│ # │ Server   │ Apps /       │", "│   │          │ web, with DB │", "├───┼──────────┼──────────────┤",
  "│ 1 │ alpha    │ 50 / 13      │", "├───┼──────────┼──────────────┤", "│ 2 │ beta     │ 3 / 2        │", "└───┴──────────┴──────────────┘"];
const heads = (page) => page.evaluate(() => [...document.querySelectorAll("#term .ln.wg-host")].filter((h) => h.shadowRoot?.querySelector("table")).map((h) => ({
  rule: h.textContent.trim()[0], head: [...h.shadowRoot.querySelectorAll("th")].map((c) => c.textContent).join() })));
const HEAD = "#,Server,Apps / web, with DB";
// its top lost by the terminal, as a coding agent's table was: a row wrapped over two lines first
const CUT = ["  A heading", "  │ Comes in parts (a slow link)       │ Rows stay hidden for 150 ms, then    │", "  │                                    │ the text shows.                      │",
  "  ├────────────────────────────────────┼──────────────────────────────────────┤", "  │ A repeated rule flashed            │ The old region no longer shows it.   │",
  "  │ as text                            │                                      │", "  ├────────────────────────────────────┼──────────────────────────────────────┤",
  "  │ Far tables stayed text             │ Drawn at once.                       │", "  └────────────────────────────────────┴──────────────────────────────────────┘", "  After it."];

export async function liveWidgetSteady(page, { tmux, work }) {
  const shown = await shownPane(page, { tmux, work });
  const pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", "steady", "-c", work);
  const row = `#sessions .item[data-id$=":${pane}"]`;
  await page.waitForSelector(row, { timeout: 5000 });
  await page.click(row);
  await page.waitForTimeout(500);
  const table = path.join(work, "steady-table.txt"), script = path.join(work, "steady.sh");
  fs.writeFileSync(table, TABLE.join("\n") + "\n");
  fs.writeFileSync(script, `clear
i=0; while [ $i -lt 4 ]; do echo "before $i"; i=$((i+1)); done
sleep 0.4
cat ${table}
sleep 0.6
i=0; while [ $i -lt 60 ]; do echo "line $i"; i=$((i+1)); sleep 0.02; done
seq 1 200 | sed 's/^/batch /'
echo done-scroll
sleep 0.5
clear
echo above
cat ${table}
echo status
i=0; while [ $i -lt 15 ]; do printf '\\033[1Astatus %s\\033[K\\n' $i; i=$((i+1)); sleep 0.1; done
n=${TABLE.length + 1}
i=0; while [ $i -lt 15 ]; do printf '\\033[%sA\\033[J' $n; cat ${table}; echo "status $i"; i=$((i+1)); sleep 0.1; done
echo done-redraw
`);
  await page.evaluate(watch);
  tmux("send-keys", "-t", pane, `sh ${script}`, "Enter");
  const said = (word) => until(() => page.evaluate((w) => document.getElementById("term").innerText.includes(w), word), 30000);
  check("AC-31 live: the script's scrolling part ran", await said("done-scroll"));
  await page.waitForTimeout(200);
  const scrolled = await page.evaluate(() => window.steady()), rows = await heads(page);
  check("AC-31 live: a real table scrolled off by real output, no frame shows its rows", scrolled.tables >= 1 && !scrolled.raw, JSON.stringify(scrolled));
  check("AC-31 live: its <table> is drawn once", scrolled.tables === 1, `${scrolled.tables}`);
  check("AC-31 live: it keeps its top rule and whole head of two lines", rows.length === 1 && rows[0].rule === "┌" && rows[0].head === HEAD, JSON.stringify(rows));
  check("AC-31 live: the script's redrawing part ran", await said("done-redraw"));
  await page.waitForTimeout(200);
  const redrawn = await page.evaluate(() => window.steady());
  check("AC-31 live: a block redrawn in place: no frame shows its rows", !redrawn.raw, JSON.stringify(redrawn));
  check("AC-31 live: nor is its <table> drawn again, though the same table is in the history", redrawn.tables === 2, `${redrawn.tables}`);
  // the table that lost its top: a widget of its rows, as any other
  const cut = path.join(work, "steady-cut.txt");
  fs.writeFileSync(cut, CUT.join("\n") + "\n");
  tmux("send-keys", "-t", pane, `clear; cat ${cut}; echo done-cut`, "Enter");
  check("AC-31 live: the table that lost its top was printed", await said("done-cut"));
  await page.waitForTimeout(400);
  const read = await page.evaluate(() => {
    const host = [...document.querySelectorAll("#term .ln.wg-host")].at(-1);
    return { head: host?.shadowRoot?.querySelectorAll("th").length, first: host?.shadowRoot?.querySelector("tbody td")?.textContent, rows: host?.shadowRoot?.querySelectorAll("tbody tr").length,
      text: [...document.querySelectorAll("#screen .ln:not(.wg-host):not(.wg-hid)")].map((el) => el.textContent.trim()).filter((t) => /^[│├└]/.test(t)).length,
      around: ["A heading", "After it."].every((w) => document.getElementById("term").innerText.includes(w)) };
  });
  check("AC-31 live: a real table that lost its top is a widget, none of its rows left as text, the lines around it there",
    read.head === 0 && read.first === "Comes in parts (a slow link)" && read.rows === 3 && read.text === 0 && read.around, JSON.stringify(read));
  fs.rmSync(cut, { force: true });
  await page.click(`#sessions .item[data-id$=":${shown}"]`);
  tmux("kill-pane", "-t", pane);
  await page.waitForTimeout(300);
  fs.rmSync(table, { force: true });
  fs.rmSync(script, { force: true });
}
