// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-37 against a real private terminumd (e2e_live.mjs): a page in sight out of focus on a computer
// (the user types in another app beside it) keeps drawing the pane's output; a hidden page gets
// none until it is in sight again. The page's own focus and visibility handlers send what it says;
// the browser's focus and visibility are played, as a test cannot take the window's focus away.
import { check, playLook, within } from "./check.mjs";

export async function liveFocus(page, { tmux, work }) {
  const shown = tmux("list-panes", "-a", "-F", "#{pane_id} #{pane_current_path}").split("\n").map((l) => l.split(" ")).find((p) => p[1] === work)?.[0];
  const pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", "focus", "-c", work);
  await page.waitForSelector(`#sessions .item[data-id$=":${pane}"]`, { timeout: 5000 });
  await page.click(`#sessions .item[data-id$=":${pane}"]`);
  await page.waitForTimeout(500);
  const look = (focused, hidden, ev) => page.evaluate(playLook, [focused, hidden, ev]);
  const onScreen = (t) => document.getElementById("screen").innerText.includes(t);
  const drawn = (text) => page.evaluate(onScreen, text), drawnSoon = (text) => page.waitForFunction(onScreen, text, { timeout: 3000 });
  const echo = (word) => tmux("send-keys", "-t", pane, `echo ${word}-$((6*7))`, "Enter");

  await look(false, false, "blur");
  echo("away");
  await within("AC-37 a page in sight out of focus draws the pane's output", () => drawnSoon("away-42"));
  await look(false, true, "visibilitychange");
  await page.waitForTimeout(300);
  echo("hidden");
  await page.waitForTimeout(1500);
  const whileHidden = await drawn("hidden-42");
  await look(false, false, "visibilitychange");
  await within("AC-37 a hidden page's stream comes back when it is in sight again, still out of focus", () => drawnSoon("hidden-42"));
  check("AC-37 a hidden page gets no output while hidden", !whileHidden);

  await look(true, false, "focus");
  await page.evaluate(() => { delete document.hasFocus; delete document.hidden; });
  tmux("kill-window", "-t", pane);
  await page.click(`#sessions .item[data-id$=":${shown}"]`);
  await page.waitForTimeout(500);
}
