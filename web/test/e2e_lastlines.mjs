// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-96 checks of a session's last lines against a fake terminumd (fake.mjs), at a wide and a phone
// size: a row's menu has Show Last N Lines (N the setting's; none at 0), its window shows the lines
// from before the restart as plain text scrolled to the end, "Before restart | Now" switches,
// Copy copies the lines shown, nothing kept and terminumd's refusal are said in it, Esc closes it.
// Alone: node web/test/e2e_all.mjs webLastLines.

import { check, runningIn } from "./check.mjs";
import { drawn, group, item, openPage, pause, SIZES, spyCopies } from "./fake.mjs";

const API = "tmux:local:terminum:%1";
const DOCS = "tmux:local:terminum:%2";
const GONE = "tmux:local:terminum:%3";
const LINES = Array.from({ length: 300 }, (_, i) => `ended-${i + 101}`);
const KEPT = { session: "main", window: "api", dir: "/Users/alex/work/api", at: 1792000000 };
const ANSWERS = {
  [API]: { lines: 300, before: { ...KEPT, text: [...LINES.slice(0, 299), "<b>not html</b>"].join("\n") }, latest: { ...KEPT, at: 1792000100, text: "$ echo again\nagain" } },
  [DOCS]: { lines: 300, before: null, latest: null },
  [GONE]: { error: "This pane has closed. Pick another one." },
};

export async function webLastLines(browser, name) {
  for (const [size, device] of SIZES) {
    runningIn(`${name} ${size}`);
    const layout = [group("main", [item(API, "api"), item(DOCS, "docs", { index: "2" }), item(GONE, "gone", { index: "3" })])];
    const f = await openPage(browser, { device, layout, settings: { page: { lastLines: 300 } },
      hook: (m, say) => { if (m.t === "lastlines") setTimeout(() => say({ t: "lastlines", id: m.id, ...ANSWERS[m.id] }), 50); } });
    const { page, sent, errors } = f;
    await drawn(page);
    const rowMenu = async (id) => {
      await page.$eval(`#sessions .item[data-id="${id}"]`, (el) => { const r = el.getBoundingClientRect();
        el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 40, clientY: r.top + 10 })); });
      await page.waitForSelector("#rowmenu[open]");
    };
    const state = () => page.evaluate(() => {
      const box = document.getElementById("lastlines"), text = document.getElementById("lastlinestext"), r = box.getBoundingClientRect();
      return { open: box.open, title: document.getElementById("lastlinestitle").textContent, meta: document.getElementById("lastlinesmeta").textContent,
        none: !document.getElementById("lastlinesnone").hidden, seg: !document.getElementById("lastlinesseg").hidden,
        chosen: document.querySelector('#lastlinesseg [aria-selected="true"]')?.dataset.which, text: text.hidden ? "" : text.textContent,
        bold: !!text.querySelector("b"), atEnd: text.scrollHeight - text.scrollTop - text.clientHeight < 2, copy: !document.getElementById("lastlinescopy").disabled,
        inView: r.left >= 0 && r.right <= innerWidth + 1 && r.top >= 0 && r.bottom <= innerHeight + 1, font: getComputedStyle(text).fontFamily };
    });

    await rowMenu(API);
    const label = await page.textContent('#rowmenu button[data-action="lastlines"]').catch(() => null);
    await page.click('#rowmenu button[data-action="lastlines"]');
    await page.waitForFunction(() => !document.getElementById("lastlinestext").hidden, null, { timeout: 5000 }).catch(() => {});
    const before = await state();
    check("AC-96 a row's menu has Show Last 300 Lines; it asks terminumd for that pane's lines",
      label === "Show Last 300 Lines" && sent.some((m) => m.t === "lastlines" && m.id === API), label);
    check("AC-96 the window shows the lines from before the restart as plain text, scrolled to the end, on the screen",
      before.open && before.title === "Last 300 lines · api" && before.meta.startsWith("Before the restart · main · api · /Users/alex/work/api")
      && before.text.split("\n").length === 300 && before.text.endsWith("<b>not html</b>") && !before.bold && before.atEnd && before.inView && before.seg && before.chosen === "before",
      JSON.stringify({ ...before, text: before.text.slice(-40) }));
    await page.click('#lastlinesseg [data-which="latest"]');
    const now = await state();
    check("AC-96 Now shows the lines kept since the restart", now.text === "$ echo again\nagain" && now.chosen === "latest" && now.meta.startsWith("Kept · main"), JSON.stringify(now));
    await spyCopies(page);
    await page.click("#lastlinescopy");
    await pause(page);
    const copied = await page.evaluate(() => window.copied.at(-1));
    check("AC-96 Copy copies the lines shown", copied === "$ echo again\nagain", String(copied));
    await page.keyboard.press("Escape");
    await pause(page);
    check("AC-96 Esc closes the window", !(await state()).open);

    await rowMenu(DOCS);
    await page.click('#rowmenu button[data-action="lastlines"]');
    await page.waitForFunction(() => !document.getElementById("lastlinesnone").hidden, null, { timeout: 5000 }).catch(() => {});
    const empty = await state();
    check("AC-96 nothing kept yet is said; no switch, nothing to copy", empty.none && !empty.seg && !empty.copy && !empty.text, JSON.stringify(empty));
    await page.click("#lastlinesclose");

    await rowMenu(GONE);
    await page.click('#rowmenu button[data-action="lastlines"]');
    await page.waitForFunction(() => document.getElementById("lastlinesmeta").classList.contains("bad"), null, { timeout: 5000 }).catch(() => {});
    const refused = await state();
    check("AC-96 terminumd's refusal is said in the window", refused.meta === "This pane has closed. Pick another one." && !refused.none && !refused.text, JSON.stringify(refused));
    await page.click("#lastlinesclose");

    f.say({ t: "settings", page: { lastLines: 0 } });
    await pause(page);
    await rowMenu(API);
    const off = await page.$('#rowmenu button[data-action="lastlines"]');
    check("AC-96 lastLines 0: no Show Last Lines in the menu", !off);
    await page.keyboard.press("Escape");
    check("AC-96 no page errors", errors.length === 0, errors.join(" | "));
    await f.close();
  }
}
