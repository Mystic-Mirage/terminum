// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-02 against a real private terminumd (e2e_live.mjs): at tmux's history-limit, where tmux drops
// the oldest tenth at once, rows that scroll off under a redrawn bottom row (a coding agent's
// spinner) are each in the history once; the screen's rows were taken for more that scrolled off,
// so an agent's answer showed twice.
import fs from "node:fs";
import path from "node:path";
import { check, shownPane, within } from "./check.mjs";

export async function liveLimit(page, { tmux, work }) {
  const shown = await shownPane(page, { tmux, work });
  const limit = tmux("show", "-gv", "history-limit");
  // a tenth (100) past the screen's rows, as at the default 12000; tmux applies it to panes made from now on
  tmux("set", "-g", "history-limit", "1000");
  const pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", "limit", "-c", work);
  tmux("set", "-g", "history-limit", limit);
  const file = path.join(work, "spin.sh");
  // 1000 rows bring the history near its limit, then one row at a time scrolls off while the bottom row is redrawn
  fs.writeFileSync(file, "clear; seq 1 1000 | sed 's/^/row /'\nfor i in $(seq 1001 1150); do printf '\\r\\033[Krow %d\\nspin %d' $i $i; sleep 0.02; done\nprintf '\\r\\033[Kspun\\n'\n");
  await page.waitForSelector(`#sessions .item[data-id$=":${pane}"]`, { timeout: 5000 });
  await page.click(`#sessions .item[data-id$=":${pane}"]`);
  await page.waitForTimeout(500);
  tmux("send-keys", "-t", pane, `bash ${file}`, "Enter");
  await within("AC-02 the rows scrolled off at the history's limit reach the page", () => page.waitForFunction(() => document.getElementById("screen").innerText.includes("spun"), null, { timeout: 15000 }));
  await page.waitForTimeout(500);
  const seen = await page.evaluate(() => {
    const rows = (sel) => [...document.querySelectorAll(sel)].map((r) => r.textContent.trim());
    const all = [...rows("#hist .ln"), ...rows("#screen .ln")], counts = {};
    for (const t of all) { const m = /^row (\d+)$/.exec(t); if (m) counts[m[1]] = (counts[m[1]] ?? 0) + 1; }
    return { twice: Object.entries(counts).filter(([, n]) => n > 1).map(([r]) => r), spins: rows("#hist .ln").filter((t) => t.startsWith("spin")).length, last: counts["1150"] ?? 0 };
  });
  check("AC-02 at tmux's history-limit each row that scrolled off under a redrawn row is drawn once", seen.twice.length === 0 && seen.spins === 0 && seen.last === 1, JSON.stringify({ ...seen, twice: seen.twice.slice(0, 8), n: seen.twice.length }));
  tmux("kill-window", "-t", pane);
  fs.rmSync(file, { force: true });
  await page.click(`#sessions .item[data-id$=":${shown}"]`);
  await page.waitForTimeout(500);
}
