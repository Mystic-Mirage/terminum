// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-02 against a real private terminumd (e2e_live.mjs): a coding agent's redraw that reaches tmux
// in two parts (Claude Code's cursor up to its spinner row, the row, the cursor back; measured on
// Claude Code: the cursor hidden mid-redraw now and then) is never a frame: the input box stays
// one block in one place and the cursor in it, frame after frame.
import fs from "node:fs";
import path from "node:path";
import { check } from "./check.mjs";

// every 100 ms the spinner row rewritten in two writes 2 ms apart, the cursor on that row between
const PROGRAM = `const G = ["·", "✢", "✳", "✶", "✻", "✽"];
let k = 0;
const spin = () => G[k % G.length] + " Beaming… (" + k + "s)";
// drawn whole at the start and at each new size (the page sizes the window it shows), the cursor in the box
let back;
const draw = () => {
  clearTimeout(back);    // a size between the two writes: drawn whole, the cursor already in the box
  const rule = "─".repeat(process.stdout.columns);
  const lines = ["⏺ Bash(npm test)", "  ⎿  Running…", "", spin(), "", rule, "❯\\u00a0", rule, "  ~/p Opus 5.5 (high) main", "  ⏵⏵ bypass permissions on (shift+tab to cycle)"];
  process.stdout.write("\\x1b[2J\\x1b[H" + lines.join("\\r\\n") + "\\x1b[3A\\r\\x1b[2C");
};
draw();
process.stdout.on("resize", draw);
setInterval(() => {
  k++;
  process.stdout.write("\\x1b[3A\\r\\x1b[2K" + spin());
  back = setTimeout(() => process.stdout.write("\\x1b[3B\\r\\x1b[2C"), 2);
}, 100);`;

export async function liveJitter(page, { tmux, work }) {
  const prog = path.join(work, "standin-claude.js");
  fs.writeFileSync(prog, PROGRAM);
  // a window named claude (-n: tmux renames it no more): its pane is a coding agent's (titles.rs); its row found by its pane
  const pane = tmux("new-window", "-P", "-F", "#{pane_id}", "-n", "claude", `node ${prog}`);
  try {
    // right after the restart checks (e2e_live.mjs): the page settled first (no status line; a hidden one keeps its old text), then the row shown
    await page.waitForFunction(() => document.getElementById("status").hidden, null, { timeout: 8000 }).catch(() => {});
    await page.click(`#sessions .item[data-id$=":${pane}"]`);
    const ready = await page.waitForFunction(() => document.querySelector("#screen .blk.inbox .cur"), null, { timeout: 8000 }).then(() => "", (e) => e.message);
    if (ready) {
      // the precondition failed: what the page showed, to tell a wrong row, a hidden page or a lost link from a redraw bug
      const state = await page.evaluate(() => {
        const on = document.querySelector("#sessions .item.on");
        return { row: on?.dataset.id, title: on?.innerText.split("\n")[0], focus: document.hasFocus(), visibility: document.visibilityState,
          status: document.getElementById("status").hidden ? "" : document.getElementById("status").innerText.trim(), hasbox: document.getElementById("term").classList.contains("hasbox"),
          screen: [...document.querySelectorAll("#screen .ln")].map((l) => l.textContent).filter((t) => t.trim()).slice(0, 6).map((t) => t.slice(0, 40)) };
      });
      state.tmux = tmux("capture-pane", "-p", "-t", pane).split("\n").filter(Boolean).slice(0, 4);
      check("AC-02 the coding agent's input box with the cursor in it is shown (precondition)", false, `${ready} ${JSON.stringify(state)}`);
      return;
    }
    await page.waitForTimeout(500);                          // sized for the view and drawn again
    const got = await page.evaluate(() => new Promise((done) => {
      const screen = document.getElementById("screen"), seen = { frames: 0, lost: 0, away: 0, boxes: new Set(), tops: new Set() };
      const t0 = performance.now();
      const tick = () => {
        seen.frames++;
        const box = screen.querySelector(".blk.inbox");
        if (!box) seen.lost++;
        else {
          seen.boxes.add(box);
          seen.tops.add(Math.round(box.getBoundingClientRect().top));
          if (!box.querySelector(".cur")) seen.away++;
        }
        if (performance.now() - t0 < 3000) requestAnimationFrame(tick);
        else done({ frames: seen.frames, lost: seen.lost, away: seen.away, boxes: seen.boxes.size, tops: [...seen.tops],
          shown: document.querySelector("#sessions .item.on")?.innerText.split("\n")[0], hasbox: document.getElementById("term").classList.contains("hasbox"),
          screen: [...screen.querySelectorAll(".ln")].slice(-6).map((l) => l.textContent.slice(0, 30)) });
      };
      requestAnimationFrame(tick);
    }));
    if (got.lost) got.tmux = { pane, screen: tmux("capture-pane", "-p", "-t", pane).split("\n").filter(Boolean).slice(0, 4), cmd: tmux("display", "-p", "-t", pane, "#{pane_current_command} #{pane_dead}") };
    check("AC-02 a coding agent's redraw that reaches tmux in two parts is never a frame: its input box one block in one place, the cursor in it, for 3 s",
      got.frames > 60 && got.lost === 0 && got.away === 0 && got.boxes === 1 && got.tops.length === 1, JSON.stringify(got));
  } finally {
    tmux("kill-window", "-t", pane);
    fs.rmSync(prog, { force: true });
  }
}
