// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-56 / AC-32 checks of the web app's folds (fold.py's, found by the bridge) on the fake bridge of
// e2e_widgets.mjs: a diff and a blob are one line each, Show and Copy ask for their lines, lines that
// scrolled off fold without a request, rendered output is never folded, a listing's pages fold as one,
// lines the bridge cannot send say why, a cleared scrollback takes its folds along.

import { check } from "./check.mjs";
import { line, open } from "./e2e_widgets.mjs";

export async function webWidgetFolds(browser, name = "") {
  const at = `AC-56 web app${name}:`;
  // folds the bridge found (fold.py): a diff and a blob are one line each and their lines are not
  // sent; Show asks for them; Copy copies them; lines that scrolled off fold without a request
  const b64 = "iVBORw0KGgoAAAANSUhEUgAAALgAAABQCAQAAAAALXejAAAAAmJLR0QA/4ePzL8AAAAHdElNRQfqCR0J";
  const foldHist = ["⏺ Update(src/app.ts)", "  ⎿  Updated src/app.ts with 6 additions and 6 removals",
    ...Array.from({ length: 12 }, (_, k) => `      ${k} +  line ${k}`), "",
    "     --logo: url('data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAALgAAABQ", ...Array(30).fill("     " + b64), "     ErkJggg==');", "done"].map((t) => line(t));
  const foldItems = [{ n: 2, to: 13, label: "⋯ 12 lines of diff" }, { n: 16, to: 45, label: "⋯ 2.3 KB of base64 · 30 lines" }];
  const fo = await open(browser, { history: foldHist, screen: [], folds: foldItems });
  await fo.page.waitForFunction(() => document.querySelectorAll("#hist .ln.wg-fold").length === 2, null, { timeout: 5000 }).catch(() => {});
  const folded = await fo.page.evaluate(() => ({
    labels: [...document.querySelectorAll("#hist .ln.wg-fold")].map((h) => h.shadowRoot.querySelector('[data-act="fold"]').textContent),
    hidden: document.querySelectorAll("#hist .ln.wg-hid").length,
    around: [...document.querySelectorAll("#hist .ln")].filter((el) => /Updated|url\(|Jggg|^done/.test(el.textContent.trim())).every((el) => !!el.offsetParent) }));
  check(`${at} folds the bridge found are one line each ("⋯ 12 lines of diff", "⋯ 2.3 KB of base64 · 30 lines"), the lines around them in sight`,
    JSON.stringify(folded.labels) === JSON.stringify(foldItems.map((f) => f.label)) && folded.hidden === 40 && folded.around, JSON.stringify(folded));
  await fo.page.evaluate(() => { window.copied = []; document.addEventListener("copy", () => window.copied.push(String(getSelection()) || document.activeElement?.value)); });
  const press = (k, act) => fo.page.evaluate(([k, act]) => document.querySelectorAll("#hist .ln.wg-fold, #hist .ln")[0] && [...document.querySelectorAll("#hist .ln")]
    .filter((el) => el.shadowRoot?.querySelector(".fold"))[k].shadowRoot.querySelector(`[data-act="${act}"]`).click(), [k, act]);
  await press(0, "fold");
  await fo.page.waitForFunction(() => [...document.querySelectorAll("#hist .ln")].some((el) => el.textContent === "      5 +  line 5" && el.offsetParent), null, { timeout: 5000 }).catch(() => {});
  const shown = await fo.page.evaluate(() => ({ asked: 0, label: [...document.querySelectorAll("#hist .ln")].map((el) => el.shadowRoot?.querySelector('[data-act="fold"]')?.textContent).find(Boolean),
    line5: [...document.querySelectorAll("#hist .ln")].some((el) => el.textContent === "      5 +  line 5" && el.offsetParent) }));
  check(`${at} … Show asks the bridge for the fold's lines and shows them ("Hide …" then)`,
    shown.line5 && /^Hide 12 lines of diff$/.test(shown.label) && fo.sent.some((m) => m.t === "unfold" && m.n === 2 && m.to === 13), JSON.stringify(shown));
  // over plain HTTP the browser copies only in the click: lines asked for, then Copy again
  await press(1, "foldcopy");
  await fo.page.waitForFunction(() => /press Copy again/.test(document.getElementById("toast").textContent), null, { timeout: 5000 }).catch(() => {});
  const again = await fo.page.evaluate(() => document.getElementById("toast").textContent);
  await press(1, "foldcopy");
  const copiedBlob = await fo.page.evaluate(() => window.copied);
  check(`${at} … Copy asks for the lines when needed ("press Copy again" over plain HTTP), then copies them`,
    /press Copy again/.test(again) && copiedBlob.some((c) => c === Array(30).fill("     " + b64).join("\n")), JSON.stringify({ again, copied: copiedBlob.map((c) => c?.length) }));
  // a fold of lines the page has (they scrolled off the screen): hidden, nothing asked
  const askedBefore = fo.sent.filter((m) => m.t === "unfold").length;
  fo.say({ t: "hist", mode: "append", sid: "S1", first: foldHist.length, oldest: 0, truncated: false,
    lines: ["⏺ Bash(ls)", "  ⎿  a", ...Array.from({ length: 25 }, (_, k) => `     file${k}.txt`), ""].map((t) => line(t)) });
  fo.say({ t: "folds", sid: "S1", items: [{ n: foldHist.length + 10, to: foldHist.length + 26, label: "⋯ 17 more lines of output" }] });
  await fo.page.waitForFunction(() => document.querySelectorAll("#hist .ln.wg-fold").length === 3, null, { timeout: 5000 }).catch(() => {});
  const appended = await fo.page.evaluate(() => [...document.querySelectorAll("#hist .ln.wg-fold")].map((h) => h.shadowRoot.querySelector('[data-act="fold"]').textContent));
  check(`${at} … lines that scrolled off fold when the bridge says so, without asking for them`,
    appended.at(-1) === "⋯ 17 more lines of output" && fo.sent.filter((m) => m.t === "unfold").length === askedBefore, JSON.stringify(appended));
  // rendered output is never folded: a fold the bridge tells over a drawn table's lines (the page has
  // them) gives way, the table stays drawn (it turned back into plain text)
  const at2 = foldHist.length + 28;
  fo.say({ t: "hist", mode: "append", sid: "S1", first: at2, oldest: 0, truncated: false,
    lines: ["⏺ Bash(cat t.md)", "  ⎿  the table:", "     | Field | Value |", "     |---|---|", ...Array.from({ length: 8 }, (_, k) => `     | f${k} | ${k} |`), "",
      ...Array.from({ length: 12 }, (_, k) => `after ${k}`)].map((t) => line(t)) });   // lines after it: the table is final, not live
  await fo.page.waitForFunction(() => [...document.querySelectorAll("#hist .ln.wg-host")].some((h) => h.shadowRoot.querySelector('.wg[data-kind="table"]')), null, { timeout: 5000 }).catch(() => {});
  const prior = await fo.page.evaluate(() => [...document.querySelectorAll("#hist .ln.wg-host")].map((h) => h.shadowRoot.querySelector(".wg")?.dataset.kind ?? "fold").join());
  fo.say({ t: "folds", sid: "S1", items: [{ n: at2 + 4, to: at2 + 10, label: "⋯ 7 more lines of output" }] });
  await fo.page.waitForTimeout(400);
  const kept = await fo.page.evaluate(() => ({
    drawn: [...document.querySelectorAll("#hist .ln.wg-host")].some((h) => h.shadowRoot.querySelector('.wg[data-kind="table"] table')),
    shown: [...document.querySelectorAll("#hist .ln:not(.wg-host)")].filter((el) => /^\s*\|/.test(el.textContent) && el.offsetParent).length,
    folded: [...document.querySelectorAll("#hist .ln.wg-fold")].map((h) => h.shadowRoot.querySelector('[data-act="fold"]').textContent).includes("⋯ 7 more lines of output") }));
  check(`${at} … a fold over rendered output (a table whose lines the page has) gives way: the table stays drawn, its lines hidden`, kept.drawn && !kept.folded && kept.shown === 0, JSON.stringify({ kept, prior }));
  check(`${at} … no page errors with folds`, !fo.errors.length, JSON.stringify(fo.errors));
  await fo.page.close();

  // a long listing comes in pages, folded each by itself: back-to-back folds of nearly equal lines are one
  const listing = Array.from({ length: 60 }, (_, k) => line(`vpatch/ws/pkg${k}/file${k}.js`));
  const pages = await open(browser, { history: [...listing, line("$ ")], screen: [],
    folds: [{ n: 3, to: 29, label: "⋯ 27 similar lines" }, { n: 30, to: 56, label: "⋯ 27 similar lines" }] });
  await pages.page.waitForFunction(() => document.querySelector("#hist .ln.wg-fold"), null, { timeout: 5000 }).catch(() => {});
  const one = await pages.page.evaluate(() => [...document.querySelectorAll("#hist .ln.wg-fold")].map((h) => h.shadowRoot.querySelector('[data-act="fold"]').textContent));
  await pages.page.evaluate(() => document.querySelector("#hist .ln.wg-fold").shadowRoot.querySelector('[data-act="fold"]').click());
  await pages.page.waitForFunction(() => [...document.querySelectorAll("#hist .ln")].some((el) => el.textContent === "vpatch/ws/pkg40/file40.js" && el.offsetParent), null, { timeout: 5000 }).catch(() => {});
  check(`${at} back-to-back folds of nearly equal lines (a listing's pages) are one line, shown together`,
    JSON.stringify(one) === JSON.stringify(["⋯ 54 similar lines"]) && pages.sent.some((m) => m.t === "unfold" && m.n === 3 && m.to === 56), JSON.stringify(one));
  await pages.page.close();

  // lines the bridge cannot send say why; a cleared scrollback takes its folds along
  const gone = await open(browser, { history: ["a", "b", "c", "d", "e"].map((t) => line(t)), screen: [],
    folds: [{ n: 1, to: 3, label: "⋯ 3 similar lines", fail: true }] });
  await gone.page.waitForFunction(() => document.querySelector("#hist .ln.wg-fold"), null, { timeout: 5000 }).catch(() => {});
  await gone.page.evaluate(() => document.querySelector("#hist .ln.wg-fold").shadowRoot.querySelector('[data-act="fold"]').click());
  await gone.page.waitForFunction(() => !document.getElementById("toast").hidden, null, { timeout: 5000 }).catch(() => {});
  const why = await gone.page.evaluate(() => ({ toast: document.getElementById("toast").textContent, still: !!document.querySelector("#hist .ln.wg-fold") }));
  check(`${at} lines the bridge cannot send: the page says why, the fold stays`, why.toast === "These lines are no longer in iTerm's history." && why.still, JSON.stringify(why));
  gone.say({ t: "hist", mode: "reset", sid: "S1", first: 0, oldest: 0, truncated: false, lines: ["new 1", "new 2", "new 3", "new 4"].map((t) => line(t)) });
  await gone.page.waitForFunction(() => [...document.querySelectorAll("#hist .ln")].some((el) => el.textContent === "new 2"));
  const cleared = await gone.page.evaluate(() => ({ folds: document.querySelectorAll("#hist .ln.wg-fold").length, hidden: document.querySelectorAll("#hist .ln.wg-hid").length }));
  check(`${at} a cleared scrollback (/clear, ⌘K) takes its folds along: new lines with the same numbers show`, cleared.folds === 0 && cleared.hidden === 0, JSON.stringify(cleared));
  await gone.page.close();
}
