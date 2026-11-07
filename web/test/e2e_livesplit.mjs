// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-04 and AC-13 against a real private terminumd (e2e_live.mjs): a window split under the shown
// pane (as Claude Code's teammates do) has the pane zoomed to the view, nothing drawn over the
// terminal. With a tmux client attached by hand on that window the layout stays: a line tmux wrapped
// at the narrow pane's width re-flows to the view's width at the chosen text size, with no sideways
// scrolling. Docked Files follows quick switches, which move the zoom, back to the pane's folder (AC-70).
import { spawn } from "node:child_process";
import { check, within } from "./check.mjs";

const LINE = Array.from({ length: 41 }, (_, i) => 1000 + i).join(" ");   // 204 characters: wider than any pane here

/** How the page draws LINE: its rows (the screen's run of rows ending in a hard end), its widest drawn
 *  line, the view's width, the text size, sideways scrolling, and the split banner of 0.16 gone. */
const drawn = (page) => page.evaluate((line) => {
  const term = document.getElementById("term"), cs = getComputedStyle(term), all = [...document.querySelectorAll("#screen .ln")];
  // the run of rows up to a hard end that holds LINE (a line filling its last row ends on an empty one)
  let rows = [];
  for (let end = all.length - 1; end >= 0 && !rows.length; end--) {
    if (!all[end].classList.contains("eol")) continue;
    let start = end;
    while (start > 0 && !all[start - 1].classList.contains("eol")) start--;
    if (all.slice(start, end + 1).map((r) => r.textContent).join("").trim() === line) rows = all.slice(start, end + 1);
  }
  const lines = new Map(), range = document.createRange();
  for (const r of rows) {
    range.selectNodeContents(r);
    for (const b of range.getClientRects()) {
      const k = Math.round(b.top), l = lines.get(k) ?? { left: Infinity, right: -Infinity };
      lines.set(k, { left: Math.min(l.left, b.left), right: Math.max(l.right, b.right) });
    }
  }
  return { rows: rows.length, widest: Math.round(Math.max(0, ...[...lines.values()].map((l) => l.right - l.left))),
    view: term.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight),
    scrolls: term.scrollWidth > term.clientWidth + 1, size: cs.fontSize, chosen: cs.getPropertyValue("--size").trim(),
    joined: rows.length > 1 && term.classList.contains("reflow"),
    banner: !!document.getElementById("splitnote") };
}, LINE);

export async function liveSplit(page, { tmux, work, socket }) {
  const panes = () => tmux("list-panes", "-a", "-F", "#{pane_id} #{window_id} #{pane_current_path}").split("\n").filter(Boolean).map((l) => l.split(" "));
  const [pane, win] = panes().find((p) => p[2] === work) ?? [];
  const of = (fmt) => tmux("display", "-p", "-t", pane, fmt);
  // the width the page sized the window to once it holds still for 1.5 s (a pane just shown is still
  // being sized under load: a width read mid-way changed by itself, 56 → 87, before the split did anything)
  let before = of("#{window_width}");
  for (let still = 0, t = 0; still < 15 && t < 100; t++) {
    await page.waitForTimeout(100);
    const now = of("#{window_width}");
    still = now === before ? still + 1 : 0;
    before = now;
  }
  tmux("split-window", "-h", "-d", "-l", "60%", "-t", pane);
  const zoomed = () => of("#{window_zoomed_flag} #{pane_active} #{pane_width} #{window_width}").split(" ");
  await within("AC-13 a split under the shown pane: it is zoomed to the whole window", async () => {
    for (let t = 0; t < 5000; t += 50) { const z = zoomed(); if (z[0] === "1" && z[1] === "1" && z[2] === z[3]) return; await page.waitForTimeout(50); }
    throw new Error(JSON.stringify(zoomed()));
  });
  check("AC-13 the zoom keeps the view's size", of("#{window_width}") === before, `${before} → ${of("#{window_width}")}`);
  const flat = await page.evaluate(() => { const t = document.getElementById("term"); return { reflow: t.classList.contains("reflow"), banner: !!document.getElementById("splitnote") }; });
  check("AC-13 zoomed, the pane is drawn as the program laid it out, nothing over the terminal", !flat.reflow && !flat.banner, JSON.stringify(flat));
  for (const [id, w] of panes()) if (w === win && id !== pane) tmux("kill-pane", "-t", id);
  // a client attached by hand with this window on its screen: the layout stays its own
  tmux("select-window", "-t", win);
  const hand = spawn("tmux", ["-L", socket, "-C", "attach", "-t", of("#{session_name}")], { stdio: ["pipe", "ignore", "ignore"] });
  let mate;
  try {
    await page.waitForTimeout(300);
    tmux("split-window", "-h", "-d", "-l", "60%", "-t", pane);
    mate = panes().find((p) => p[1] === win && p[0] !== pane)?.[0];
    tmux("send-keys", "-t", pane, "clear; seq -s ' ' 1000 1040", "Enter");   // the command itself is short: only its output is LINE
    let d = null;
    await within("AC-04 a line tmux wrapped in the split's narrow pane is drawn re-flowed", async () => {
      for (let t = 0; t < 5000; t += 100) { d = await drawn(page); if (d.joined) return; await page.waitForTimeout(100); }
      throw new Error(JSON.stringify({ d, pane: of("#{pane_width}"), rows: await page.evaluate(() => [...document.querySelectorAll("#screen .ln")].slice(0, 14).map((r) => `${r.classList.contains("eol") ? "|" : " "}${r.textContent}`)) }));
    });
    const cell = Number.parseFloat(d.size) * 0.6, paneW = Number(of("#{pane_width}"));
    check("AC-04 it fills the view's width, not the pane's", d.widest > paneW * cell * 1.2 && d.widest <= d.view + 1, JSON.stringify({ ...d, paneW }));
    check("AC-04 at the chosen text size, with no sideways scrolling", d.size === d.chosen && !d.scrolls, JSON.stringify(d));
    check("AC-13 nothing is drawn over the terminal for the split", !d.banner);
    check("AC-13 by a client attached by hand the window is neither resized nor zoomed", of("#{window_width}") === before && zoomed()[0] === "0", `${before} → ${zoomed()}`);
  } finally {
    hand.stdin.end();
  }
  await within("AC-13 that client gone: the pane is zoomed", async () => {
    for (let t = 0; t < 5000; t += 50) { if (zoomed()[0] === "1") return; await page.waitForTimeout(50); }
    throw new Error(JSON.stringify(zoomed()));
  });
  // a quick switch away and back: Files shows the pane's folder again (WebKit stayed blank)
  for (const id of [mate, pane, mate, pane]) { await page.click(`#sessions .item[data-id$=":${id}"]`); await page.waitForTimeout(250); }
  const files = () => page.evaluate(() => { const f = document.getElementById("files");
    return { moving: f.classList.contains("moving"), href: f.contentWindow?.location.href, text: f.contentDocument?.body?.innerText.slice(0, 120) ?? null }; });
  await within("AC-70 Files follows the pane back to its folder after quick switches", async () => {
    for (let t = 0; t < 8000; t += 200) { const f = await files(); if (f.text?.includes("alpha.txt") && !f.moving) return; await page.waitForTimeout(200); }
    throw new Error(JSON.stringify(await files()));
  });
  for (const [id, w] of panes()) if (w === win && id !== pane) tmux("kill-pane", "-t", id);
  tmux("send-keys", "-t", pane, "clear", "Enter");
}
