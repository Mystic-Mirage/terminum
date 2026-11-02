// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Codex in a pane (AC-40) against a fake terminumd (fake.mjs), at a wide and a phone size: its
// own full screen (the alternate screen) stays in Lines mode, its input (the "›" row on a
// background of its own) is a box drawn between two lines as Claude Code's, and on a phone its
// footer waits behind ⓘ, also while it is typed into or works. Backspace over a selection in its
// input deletes exactly it (AC-40).

import { check, runningIn } from "./check.mjs";
import { drawn, group, item, line, openPage, pause, SIZES } from "./fake.mjs";

const SID = "tmux:local:terminum:%5";
const LAYOUT = [group("main", [item(SID, "Codex", { agent: "codex", shell: false, sub: ["node", "~/p"] })])];
const BG = "#1e1e1e";
const blank = { r: [], e: true };
/** Codex's screen: output, its input (typed, else its dim placeholder), its footer (a state word). */
const codex = (typed, state = "Ready") => [line("• Done: 56 tests pass."), blank, blank,
  { r: [["›", null, BG, 1], [" ", null, BG, 0], ...(typed ? [[typed, null, BG, 0], [" ", null, BG, 256]]
    : [["A", null, BG, 2 | 256], ["sk Codex to do anything", null, BG, 2]])], e: true },
  blank, line(`  GPT-6.1 high · ~/p · main · ${state} · Context 61% left`), line("  ← for agents · ? for shortcuts")];

export async function webCodex(browser, name) {
  for (const [size, device] of SIZES) {
    runningIn(`${name} ${size}`);
    const phone = size.startsWith("390");
    const f = await openPage(browser, { device, layout: LAYOUT });
    const { page } = f;
    await drawn(page);
    const frame = (rows) => f.say({ t: "screen", sid: SID, full: true, n: rows.length, cols: 60, rows: rows.length, alt: true, ch: rows.map((r, i) => [i, r]) });
    const shownText = (t) => page.waitForFunction((t) => document.getElementById("screen").textContent.includes(t), t, { timeout: 3000 }).catch(() => {});
    frame(codex(""));                                            // idle: its status lines are learnt
    await shownText("Ask Codex");
    await pause(page, 200);
    const look = () => page.evaluate(() => {
      const t = document.getElementById("term"), box = document.querySelector("#screen .blk.inbox"), vis = (e) => getComputedStyle(e).display !== "none";
      const edges = [...document.querySelectorAll("#screen .ln.boxedge")];
      return { screenmode: t.classList.contains("screenmode"), box: !!box?.textContent.includes("›"), edges: edges.length,
        lines: edges.every((e) => getComputedStyle(e).backgroundImage.includes("gradient")),
        edgesShown: edges.some((e) => vis(e) && e.getBoundingClientRect().width > 200 && e.getBoundingClientRect().height >= 10),
        foot: [...document.querySelectorAll("#screen .blk.foot .ln")].map(vis), fab: vis(document.getElementById("footfab")),
        mark: document.getElementById("curscreen").getBoundingClientRect().width };
    });
    const idle = await look();
    frame(codex("fix the tests", "Working"));                    // typed into while it works: its state word changed
    await shownText("Working");
    await pause(page, 200);
    const busy = await look();
    check("Codex: its own full screen stays in Lines mode, no full-screen mark (AC-05); its input is a box between two lines, as Claude Code's",
      !idle.screenmode && !idle.mark && idle.box && idle.edges === 2 && idle.lines && (phone ? !idle.edgesShown : idle.edgesShown), JSON.stringify(idle));
    if (phone) check("Codex: on a phone its footer waits behind ⓘ, also while it is typed into and works",
      idle.foot.join() === "false,false" && busy.foot.join() === "false,false" && idle.fab && busy.fab, JSON.stringify({ idle, busy }));
    else check("Codex: on a computer its footer stays in sight", idle.foot.join() === "true,true" && busy.foot.join() === "true,true", JSON.stringify({ idle, busy }));
    if (!phone) {
      await page.mouse.move(400, 300);
      await page.mouse.wheel(0, -400);
      await pause(page, 200);
      const note = await page.evaluate(() => !document.getElementById("toast").hidden && document.getElementById("toast").textContent);
      check("Codex: scrolling up on its alternate screen (no scrollback) says why and how", typeof note === "string" && note.includes("--no-alt-screen"), String(note));
      // a selection in its input (an emoji in it) and Backspace: ← to its end, a backspace a character
      const typed = "fix the 😀 parser bug", G = (t) => [...new Intl.Segmenter().segment(t)].map((x) => x.segment);
      frame(codex(typed));
      await shownText("parser");
      const from = f.sent.length;
      const selected = await page.evaluate(() => {
        const walk = document.createTreeWalker(document.getElementById("screen"), NodeFilter.SHOW_TEXT);
        let t = walk.nextNode();
        while (t && !t.data.includes("😀")) t = walk.nextNode();
        if (!t || !t.data.includes("bug")) return false;
        const r = document.createRange(); r.setStart(t, t.data.indexOf("😀")); r.setEnd(t, t.data.indexOf("bug"));
        document.activeElement?.blur(); getSelection().removeAllRanges(); getSelection().addRange(r);
        return true;
      });
      await page.keyboard.press("Backspace");
      await pause(page, 200);
      const keys = f.sent.slice(from).filter((m) => m.t === "in" && m.data).map((m) => m.data);
      const g = G(typed), a = G(typed.slice(0, typed.indexOf("😀"))).length, b = G(typed.slice(0, typed.indexOf("bug"))).length;
      check("Codex: Backspace over a selection in its input (an emoji in it) deletes exactly it",
        selected && keys.length === 1 && keys[0] === "\x1b[D".repeat(g.length - b) + "\x7f".repeat(b - a), JSON.stringify({ selected, keys }));
    }
    check("Codex: no page errors", !f.errors.length, f.errors.join(" | "));
    await f.close();
  }
  runningIn(name);
}
