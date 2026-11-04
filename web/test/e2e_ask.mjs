// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Ask's dialog (AC-37), run by e2e_marks.mjs on its page: Add to Chat's numbered Markdown quote pasted in
// a shell and typed in a coding agent's box, Send to Agent, Esc, a narrow window.

import { check } from "./check.mjs";

/** page: e2e_marks's page; sent: what it sent terminumd; say(msg): from terminumd; layout: its
 *  list; selectRow(page, text): selects a row's text. */
export async function askChecks({ page, sent, say, at, layout, selectRow }) {
  // Ask (AC-37): the dialog shows the selection; Add to Chat is off until a question is typed; the
  // quote numbered by terminumd and the question pasted at the cursor, compact, no Enter; Esc pastes nothing
  await page.click("#bottom").catch(() => {});
  await selectRow(page, "out 2500 lorem");
  await page.waitForSelector("#askfab:not([hidden])", { timeout: 3000 }).catch(() => {});
  await page.click("#askfab");
  await page.waitForSelector("#askbox[open]", { timeout: 3000 }).catch(() => {});
  const dialog = await page.evaluate(() => ({ text: document.getElementById("asktext").textContent, off: document.querySelector("#askbox .primary").disabled, noSend: document.getElementById("asksend").hidden,
    focused: document.activeElement?.id, gone: document.getElementById("askfab").hidden, label: !!document.querySelector("#askbox label"),
    named: document.getElementById("askq").getAttribute("aria-label"), fill: getComputedStyle(document.getElementById("askq")).backgroundImage }));
  check(`${at} Ask: its field has no "Question" label (a comment or a task as often), a name for screen readers, and no gradient in focus`,
    !dialog.label && !!dialog.named && dialog.fill === "none" && dialog.focused === "askq", JSON.stringify(dialog));
  const placed = await page.evaluate(() => { const box = document.getElementById("askbox"), b = box.getBoundingClientRect(), t = document.getElementById("term").getBoundingClientRect();
    return { width: box.offsetWidth, dx: Math.round(b.left + b.width / 2 - (t.left + t.width / 2)), dy: Math.round(b.top + b.height / 2 - (t.top + t.height / 2)), termLeft: Math.round(t.left) }; });
  check(`${at} Ask: its dialog 600 px wide (a quarter larger), centered on the terminal, not on the window`,
    placed.width === 600 && Math.abs(placed.dx) <= 1 && Math.abs(placed.dy) <= 1 && placed.termLeft > 0, JSON.stringify(placed));
  // ⌘Enter while Add to Chat is off does nothing; a plain Enter is a new line in the field
  let k = sent.length;
  await page.press("#askq", "Meta+Enter");
  await page.press("#askq", "Enter");
  const keys = await page.evaluate(() => ({ open: document.getElementById("askbox").open, value: document.getElementById("askq").value }));
  check(`${at} Ask: ⌘Enter with Add to Chat off sends nothing, a plain Enter is a new line in the field`,
    keys.open && keys.value === "\n" && !sent.slice(k).some((x) => x.t === "askno"), JSON.stringify(keys));
  await page.fill("#askq", "why does it never reset?");
  k = sent.length;
  const top = () => page.evaluate(() => Math.round(document.getElementById("term").scrollTop));
  let was = await top();
  await page.press("#askq", "Meta+Enter");                       // as Add to Chat (AC-37)
  await page.waitForFunction(() => !document.getElementById("askbox").open, null, { timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(300);
  let stayed = await top();
  const asked = sent.slice(k), quoted = asked.filter((x) => x.t === "in").map((x) => x.data).join("");
  check(`${at} Ask: a dialog with the selection, Add to Chat (⌘Enter) off until a question, no Send to Agent in a shell; the quote numbered by terminumd on its first line and the question pasted, no Enter; the view stays where it was`,
    dialog.text.includes("out 2500 lorem ipsum") && dialog.off && dialog.noSend && dialog.focused === "askq" && dialog.gone && asked.some((x) => x.t === "askno")
    && quoted.startsWith("\x1b[200~> Quote #0007\r> out 2500 lorem ipsum") && quoted.endsWith("\r\rwhy does it never reset?\r\r\x1b[201~") && was > 0 && stayed === was,
    JSON.stringify({ dialog, quoted, was, stayed }));
  // the quote stays marked: terminumd keeps it with its number and the line its selection began on,
  // and its answer puts a light blue wavy line (the Ask button's) down the quote's rows' left, as a
  // reminder's quote has; no underline
  const keep = asked.find((x) => x.t === "asked");
  say({ t: "askedmarks", sid: "S1", items: keep ? [{ n: keep.n, text: keep.text, line: keep.line }] : [] });
  // the view stayed on the quote: it is marked there
  await page.waitForFunction(() => [...document.querySelectorAll("#term .askline")].some((l) => !l.hidden), null, { timeout: 3000 }).catch(() => {});
  const marked = await page.evaluate(() => {
    const lines = [...document.querySelectorAll("#term .askline")].filter((l) => !l.hidden);
    const row = [...document.querySelectorAll("#term .ln")].find((el) => el.textContent.includes("out 2500 lorem"))?.getBoundingClientRect();
    const term = document.getElementById("term").getBoundingClientRect(), text = document.querySelector("#term .ln span")?.getBoundingClientRect();
    const l = lines[0]?.getBoundingClientRect(), cs = lines[0] && getComputedStyle(lines[0]);
    const ask = getComputedStyle(document.getElementById("askfab")).backgroundColor;
    return l && { shown: lines.length, covers: Math.abs(l.top - row.top) <= 1 && Math.abs(l.bottom - row.bottom) <= 1, gutter: l.left >= term.left && l.right <= (text?.left ?? Infinity),
      color: cs.backgroundColor === ask, wavy: /url\(/.test(cs.maskImage || cs.webkitMaskImage), under: !!globalThis.CSS?.highlights?.get("asked") };
  });
  check(`${at} Ask: the quote is kept with its number and line, and keeps a light blue wavy line down its left where it was selected`,
    keep?.n === 7 && Number.isInteger(keep.line) && keep.text.includes("out 2500 lorem ipsum") && marked?.shown === 1 && marked.covers && marked.gutter && marked.color && marked.wavy && !marked.under,
    JSON.stringify({ keep, marked }));
  // the quote as sent (Claude Code shows the message after it) is never marked, even where it stands
  // nearest the line kept: the agent reprinted its answer and the original moved away
  const row = (text) => ({ r: [[text, null, null, 0]], e: true });
  say({ t: "screen", sid: "S1", full: false, n: 3, cols: 80, rows: 3, ch: [[0, row("❯ > Quote #0008")], [1, row(`  > ${keep?.text}`)], [2, row("  and why?")]] });
  say({ t: "askedmarks", sid: "S1", items: [{ n: 8, text: keep?.text, line: 3000 }] });
  await page.click("#bottom").catch(() => {});
  await page.waitForTimeout(400);
  const copy = await page.evaluate(() => ({ shown: [...document.querySelectorAll("#term .askline")].filter((l) => !l.hidden).length,
    there: [...document.querySelectorAll("#screen .ln")].some((el) => el.textContent.includes("> out 2500 lorem")) }));
  check(`${at} Ask: the quote as sent, in the message after it, never gets the wavy line`, copy.there && copy.shown === 0, JSON.stringify(copy));
  say({ t: "askedmarks", sid: "S1", items: [] });
  say({ t: "screen", sid: "S1", full: false, n: 3, cols: 80, rows: 40, ch: [] });   // a pane of 40 rows: a short quote fits typed
  // in a coding agent's box the new line after it is typed (Ctrl+J), none before
  say({ t: "layout", groups: [{ ...layout[0], items: [{ ...layout[0].items[0], agent: "claude", shell: false }, layout[0].items[1]] }] });
  await page.waitForTimeout(150);
  await selectRow(page, "out 2500 lorem");
  await page.click("#askfab");
  await page.fill("#askq", "and now?");
  k = sent.length;
  was = await top();
  await page.press("#askq", "Control+Enter");                    // Ctrl+Enter as ⌘Enter
  await page.waitForFunction(() => !document.getElementById("askbox").open, null, { timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(300);
  stayed = await top();
  const typed = sent.slice(k).filter((x) => x.t === "in").map((x) => x.data), typedFocus = await page.evaluate(() => document.activeElement?.id);
  check(`${at} Ask in a coding agent's box (Ctrl+Enter): typed, not pasted (shown whole, not folded), new lines as Ctrl+J, a blank line after the quote and after the question; nothing sent; the view stays where it was; the keys go back to the terminal`,
    typed.length === 1 && typed[0].startsWith("> Quote #0007\n> out 2500 lorem") && typed[0].endsWith("\n\nand now?\n\n") && !typed[0].includes("\x1b[200~") && was > 0 && stayed === was && typedFocus === "kbd", JSON.stringify({ typed, was, stayed, typedFocus }));
  // Send to Agent (a coding agent's pane only): the same paste, then Enter a moment after
  await selectRow(page, "out 2500 lorem");
  await page.click("#askfab");
  await page.fill("#askq", "send it");
  k = sent.length;
  await page.click("#asksend");
  await page.waitForFunction(() => !document.getElementById("askbox").open, null, { timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(400);
  const now = sent.slice(k).filter((x) => x.t === "in");
  // the keys go back to the terminal: typed next, they reach the pane, not the page's body (the agent's next question)
  const sentFocus = await page.evaluate(() => document.activeElement?.id);
  k = sent.length;
  await page.keyboard.type("11");
  await page.waitForTimeout(200);
  const next = sent.slice(k).filter((x) => x.t === "in").map((x) => x.data).join("");
  check(`${at} Ask's Send to Agent in a coding agent's pane: the paste, then Enter; the keys typed after it reach the pane`,
    now.length === 2 && now[0].data.startsWith("> Quote #0007\n> out 2500 lorem") && now[0].data.endsWith("\n\nsend it") && now[1].data === "\r" && sentFocus === "kbd" && next === "11",
    JSON.stringify({ now: now.map((x) => x.data), sentFocus, next }));
  say({ t: "layout", groups: layout });
  await page.waitForTimeout(150);
  await selectRow(page, "out 2500 lorem");
  await page.click("#askfab");
  await page.fill("#askq", "never mind");
  k = sent.length;
  await page.keyboard.press("Escape");                                // in its field, as the user does
  await page.waitForTimeout(200);
  const escFocus = await page.evaluate(() => document.activeElement?.id);
  check(`${at} Ask: Esc closes it and pastes nothing; the keys go back to the terminal`,
    !await page.evaluate(() => document.getElementById("askbox").open) && !sent.slice(k).some((x) => x.t === "in" || x.t === "askno") && escFocus === "kbd", JSON.stringify({ escFocus }));
  await selectRow(page, "out 2500 lorem");
  await page.click("#askfab");
  await page.waitForSelector("#askbox[open]", { timeout: 3000 }).catch(() => {});
  k = sent.length;
  await page.click('#askbox button[value="cancel"]');
  await page.waitForTimeout(200);
  const cancelFocus = await page.evaluate(() => document.activeElement?.id);
  await page.keyboard.type("11");
  await page.waitForTimeout(200);
  const afterCancel = sent.slice(k).filter((x) => x.t === "in").map((x) => x.data).join("");
  check(`${at} Ask: Cancel closes it and pastes nothing; the keys typed next reach the pane`,
    !await page.evaluate(() => document.getElementById("askbox").open) && cancelFocus === "kbd" && afterCancel === "11" && !sent.slice(k).some((x) => x.t === "askno"), JSON.stringify({ cancelFocus, afterCancel }));
  // a narrow window: Ask under the [+] there too (the column's buttons give way)
  await page.setViewportSize({ width: 800, height: 900 });
  await page.waitForTimeout(300);
  await selectRow(page, "out 2500 lorem");
  await page.waitForTimeout(200);
  const narrow = await page.evaluate(() => { const r = (id) => document.getElementById(id).getBoundingClientRect();
    return { above: r("askfab").top >= r("markadd").bottom && r("askfab").top - r("markadd").bottom < 16, sameX: Math.abs(r("askfab").left - r("markadd").left) < 1,
      shown: !document.getElementById("askfab").hidden, sent: getComputedStyle(document.getElementById("sentfab")).visibility }; });
  check(`${at} Ask on a narrow window: just under the [+]`, narrow.shown && narrow.above && narrow.sameX && narrow.sent === "hidden", JSON.stringify(narrow));
  await page.evaluate(() => getSelection().removeAllRanges());
  await page.setViewportSize({ width: 1400, height: 900 });
  await page.waitForTimeout(300);

  // Send to Agent's Enter only to its pane, still in its agent: the agent ending while the dialog is
  // open hides the button and says so; another pane shown within the Enter's 0.2 s gets no Enter
  const agentLayout = [{ ...layout[0], items: [{ ...layout[0].items[0], agent: "claude", shell: false }, layout[0].items[1]] }];
  say({ t: "layout", groups: agentLayout });
  await page.waitForTimeout(150);
  await selectRow(page, "out 2500 lorem");
  await page.click("#askfab");
  await page.waitForSelector("#askbox[open]", { timeout: 3000 }).catch(() => {});
  const shown = await page.evaluate(() => !document.getElementById("asksend").hidden);
  say({ t: "layout", groups: layout });                               // the agent ended (another client's view)
  await page.waitForTimeout(150);
  const ended = await page.evaluate(() => ({ hidden: document.getElementById("asksend").hidden, said: document.getElementById("askerr").textContent, open: document.getElementById("askbox").open }));
  await page.keyboard.press("Escape");
  check(`${at} Ask: the agent ending while the dialog is open hides Send to Agent and says so`,
    shown && ended.hidden && /agent ended/.test(ended.said) && ended.open, JSON.stringify({ shown, ended }));
  say({ t: "layout", groups: agentLayout });
  await page.waitForTimeout(150);
  await selectRow(page, "out 2500 lorem");
  await page.click("#askfab");
  await page.fill("#askq", "switch away");
  k = sent.length;
  // the other pane is shown as the dialog closes, in the page: a round trip from here could outlast the 0.2 s under load
  await page.evaluate((id) => document.getElementById("askbox").addEventListener("close",
    () => document.querySelector(`#sessions .item[data-id="${id}"]`)?.click(), { once: true }), layout[0].items[1].id);
  await page.click("#asksend");
  await page.waitForFunction(() => !document.getElementById("askbox").open, null, { timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(400);
  const away = { keys: sent.slice(k).filter((x) => x.t === "in").map((x) => x.data), sub: sent.slice(k).some((x) => x.t === "sub" && x.id === layout[0].items[1].id),
    toast: await page.evaluate(() => document.getElementById("toast").textContent) };
  check(`${at} Ask's Send to Agent: another pane shown before the Enter gets none, and a toast says it was not pressed`,
    away.sub && away.keys.length === 1 && !away.keys.includes("\r") && /not pressed/.test(away.toast), JSON.stringify(away));
}
