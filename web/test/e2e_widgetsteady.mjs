// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-31 a table widget is drawn once and stays (on the fake bridge of e2e_widgets.mjs, whose messages
// play what terminumd sends as output scrolls): no frame shows a table's raw rows, also as it first
// appears or scrolls into the history or the reader scrolls to it, its <table> is the same element
// throughout, and a scan caught between the history's message and the screen's keeps its head.

import { check } from "./check.mjs";
import { line, open } from "./e2e_widgets.mjs";

const HEIGHT = 24;
const TABLE = ["┌───┬──────────┬──────┐", "│ # │ Server   │ Apps │", "│   │ name     │ all  │", "├───┼──────────┼──────┤",
  "│ 1 │ alpha    │   50 │", "├───┼──────────┼──────┤", "│ 2 │ beta     │    3 │", "└───┴──────────┴──────┘"];
const cursor = { r: [["$ ", null, null, 0], [" ", null, null, 256]], e: true };

/** Plays a terminal `HEIGHT` rows high: all its lines, what is on its screen and in its history. */
export function terminal(say, lines) {
  const state = { lines: [...lines], sent: Math.max(0, lines.length + 1 - HEIGHT) };
  const shown = () => [...state.lines.slice(state.sent).map((t) => line(t)), cursor];
  let last = shown().map((r) => JSON.stringify(r));
  /** The screen's message, as terminumd's: only the rows that changed. */
  state.draw = () => {
    const rows = shown(), now = rows.map((r) => JSON.stringify(r));
    say({ t: "screen", sid: "S1", full: false, n: rows.length, cols: 80, rows: rows.length, ch: rows.flatMap((r, i) => (now[i] === last[i] ? [] : [[i, r]])) });
    last = now;
  };
  /** `more` lines printed: the history's message, then the screen's, `gap` ms apart (0: together). */
  state.print = async (page, more, gap = 0) => {
    state.lines.push(...more);
    const to = Math.max(state.sent, state.lines.length + 1 - HEIGHT);
    if (to > state.sent) say({ t: "hist", mode: "append", sid: "S1", first: state.sent, oldest: 0, truncated: false, lines: state.lines.slice(state.sent, to).map((t) => line(t)) });
    state.sent = to;
    if (gap) await page.waitForTimeout(gap);
    state.draw();
  };
  return state;
}

// in the page: in each frame, right before it is painted (a ResizeObserver's turn, after every
// requestAnimationFrame), the rows of a table in sight as text, and whether the <table> changed
export function watch() {
  const raw = [], tables = new Set(), probe = document.createElement("div");
  probe.style.cssText = "position: fixed; left: -10px; top: 0; height: 1px; width: 1px;";
  document.body.append(probe);
  let frames = 0, odd = false;
  new ResizeObserver(() => {
    frames++;
    for (const el of document.querySelectorAll("#term .ln:not(.wg-host):not(.wg-hid)")) if (/^\s*[┌│├└+|]/.test(el.textContent) && el.getClientRects().length) { raw.push(el.textContent.slice(0, 30)); break; }
    for (const host of document.querySelectorAll("#term .ln.wg-host")) { const t = host.shadowRoot?.querySelector("table"); if (t) tables.add(t); }
  }).observe(probe);
  const tick = () => { odd = !odd; probe.style.width = odd ? "2px" : "1px"; requestAnimationFrame(tick); };
  requestAnimationFrame(tick);
  window.steady = () => ({ frames, raw: raw.length, first: raw[0] ?? null, tables: tables.size });
}
const steady = (page) => page.evaluate(() => window.steady());
const heads = (page) => page.evaluate(() => [...document.querySelectorAll("#term .ln.wg-host")].filter((h) => h.shadowRoot?.querySelector("table")).map((h) => ({
  rule: h.textContent.trim()[0], head: [...h.shadowRoot.querySelectorAll("th")].map((c) => c.textContent).join() })));
const whole = (page, rows, name, seen, rule = "┌", head = "#,Server name,Apps all") => {
  const good = rows.length === 1 && rows[0].rule === rule && rows[0].head === head;
  check(name, good && !seen.raw, JSON.stringify({ rows, raw: seen.raw, first: seen.first }));
};

export async function webWidgetSteady(browser, name = "") {
  const at = `AC-31 web app${name}:`;
  const filler = (from, n) => Array.from({ length: n }, (_, i) => `line ${from + i}`);

  // printed whole on the screen: a widget in the very frame it appears
  const a = await open(browser, { history: [], screen: filler(0, 2) });
  await a.page.evaluate(watch);
  const ta = terminal(a.say, filler(0, 2));
  await ta.print(a.page, TABLE);
  await a.page.waitForTimeout(400);
  const first = await steady(a.page);
  check(`${at} a table printed whole is a widget in the frame it appears: no frame shows its rows`, first.tables === 1 && !first.raw, JSON.stringify(first));

  // output scrolls it off the screen, a line at a time, then in batches: never its rows, one <table>
  for (let i = 0; i < 30; i++) { await ta.print(a.page, filler(100 + i, 1)); await a.page.waitForTimeout(40); }
  for (let i = 0; i < 4; i++) { await ta.print(a.page, filler(200 + i * 5, 5)); await a.page.waitForTimeout(40); }
  await a.page.waitForTimeout(300);
  const off = await steady(a.page);
  check(`${at} scrolled into the history by output: no frame shows its rows`, !off.raw, JSON.stringify(off));
  check(`${at} and its <table> is drawn once`, off.tables === 1, `${off.tables}`);
  whole(a.page, await heads(a.page), `${at} and it keeps its head and top rule`, off);
  check(`${at} no page errors (printed, scrolled)`, !a.errors.length, a.errors.join(" | "));
  await a.page.close();

  // scrolled by two lines at a time, so a rule between rows stays the same row of the screen: hidden still
  const two = await open(browser, { history: [], screen: filler(0, 2) });
  await two.page.evaluate(watch);
  const t2 = terminal(two.say, filler(0, 2));
  await t2.print(two.page, TABLE);
  await two.page.waitForTimeout(100);
  for (let i = 0; i < 16; i++) { await t2.print(two.page, filler(100 + i * 2, 2)); await two.page.waitForTimeout(60); }
  await two.page.waitForTimeout(200);
  const stepped = await steady(two.page);
  check(`${at} scrolled two lines at a time, a rule that stays the same row: no frame shows it`, !stepped.raw && stepped.tables === 1, JSON.stringify(stepped));
  await two.page.close();

  // the screen's message a little after the history's, and a screen on its way as the next history comes
  const late = await open(browser, { history: [], screen: filler(0, 2) });
  await late.page.evaluate(watch);
  const t4 = terminal(late.say, filler(0, 2));
  await t4.print(late.page, TABLE);
  await late.page.waitForTimeout(100);
  await t4.print(late.page, filler(600, 14));
  await late.page.waitForTimeout(100);
  for (let i = 0; i < 10; i++) {
    await t4.print(late.page, filler(620 + i * 4, 2));
    await t4.print(late.page, filler(622 + i * 4, 2), 30);
    await late.page.waitForTimeout(30);
  }
  await late.page.waitForTimeout(200);
  const behind = await steady(late.page);
  check(`${at} the screen's message 30 ms after the history's: no frame shows its rows`, !behind.raw && behind.tables === 1, JSON.stringify(behind));
  whole(late.page, await heads(late.page), `${at} and it keeps its head`, behind);
  await late.page.close();

  // the same table in the history and redrawn in place below it, erased between: its own <table>, kept
  const again = await open(browser, { history: [], screen: filler(0, 2) });
  const t3 = terminal(again.say, filler(0, 2));
  await t3.print(again.page, TABLE);
  await t3.print(again.page, filler(500, HEIGHT));
  await again.page.waitForTimeout(200);
  await again.page.evaluate(watch);
  await t3.print(again.page, TABLE);
  await again.page.waitForTimeout(100);
  for (let i = 0; i < 6; i++) {
    t3.lines.splice(-TABLE.length, TABLE.length);
    t3.draw();
    await again.page.waitForTimeout(60);
    t3.lines.push(...TABLE);
    t3.draw();
    await again.page.waitForTimeout(60);
  }
  await again.page.waitForTimeout(100);
  const redrawn = await steady(again.page);
  check(`${at} the same table redrawn below its copy in the history: two <table>s, not one each time`, redrawn.tables === 2, JSON.stringify(redrawn));
  await again.page.close();

  // printed a row at a time (a slow link, a program's frames): out of sight until whole, one <table>
  const slow = await open(browser, { history: [], screen: filler(0, 2) });
  await slow.page.evaluate(watch);
  const t5 = terminal(slow.say, filler(0, 2));
  for (const row of TABLE) { await t5.print(slow.page, [row]); await slow.page.waitForTimeout(15); }
  await slow.page.waitForTimeout(200);
  const parts = await steady(slow.page);
  check(`${at} a table drawn a row at a time: no frame shows its rows`, parts.tables === 1 && !parts.raw, JSON.stringify(parts));
  await slow.page.close();

  // its last rule never comes: its rows show as text, nothing lost
  const cut = await open(browser, { history: [], screen: filler(0, 2) });
  await cut.page.evaluate(watch);
  const t6 = terminal(cut.say, filler(0, 2));
  await t6.print(cut.page, TABLE.slice(0, 5));
  await cut.page.waitForTimeout(350);
  const shownAfter = await cut.page.evaluate(() => [...document.querySelectorAll("#screen .ln:not(.wg-hid)")].filter((el) => /^[┌│├]/.test(el.textContent)).length);
  check(`${at} a table whose last rule does not come shows as text after a moment`, shownAfter === 5, `${shownAfter} rows`);
  await cut.page.close();

  // a Markdown table growing a row at a time: not a new <table> per row, whole once a line follows it
  const grow = await open(browser, { history: [], screen: filler(0, 2) });
  await grow.page.evaluate(watch);
  const t7 = terminal(grow.say, filler(0, 2));
  await t7.print(grow.page, ["| a | b |", "|---|---|"]);
  for (let i = 0; i < 8; i++) { await t7.print(grow.page, [`| ${i} | x |`]); await grow.page.waitForTimeout(40); }
  await t7.print(grow.page, ["done"]);
  await grow.page.waitForTimeout(200);
  const grown = await steady(grow.page);
  check(`${at} a Markdown table growing a row at a time gets one <table>, whole once a line follows it`, grown.tables === 1, JSON.stringify(grown));
  await grow.page.close();

  // a Markdown table at the end of the output: text for 500 ms, then a widget, and printed slowly, once
  const late2 = await open(browser, { history: [], screen: filler(0, 2) });
  await late2.page.evaluate(watch);
  const t10 = terminal(late2.say, filler(0, 2));
  await t10.print(late2.page, ["| a | b |", "|---|---|", "| 1 | x |"]);
  await late2.page.waitForTimeout(250);
  const early = await steady(late2.page);
  check(`${at} a Markdown table at the end of the output is no widget yet after 250 ms`, early.tables === 0, JSON.stringify(early));
  await late2.page.waitForTimeout(600);
  const pausedOn = await steady(late2.page);
  check(`${at} and it is one after 500 ms unchanged`, pausedOn.tables === 1, JSON.stringify(pausedOn));
  await late2.page.close();
  const slowly = await open(browser, { history: [], screen: filler(0, 2) });
  await slowly.page.evaluate(watch);
  const t11 = terminal(slowly.say, filler(0, 2));
  await t11.print(slowly.page, ["| a | b |", "|---|---|"]);
  for (let i = 0; i < 5; i++) { await t11.print(slowly.page, [`| ${i} | x |`]); await slowly.page.waitForTimeout(300); }
  await t11.print(slowly.page, ["done"]);
  await slowly.page.waitForTimeout(200);
  const once = await steady(slowly.page);
  check(`${at} a Markdown table printed a row every 300 ms: one <table>, not one per row`, once.tables === 1, JSON.stringify(once));
  await slowly.page.close();

  // an ASCII table cut right after its head rule: its head is never a row, no frame shows its rows
  const mysql = await open(browser, { history: [], screen: filler(0, 2) });
  await mysql.page.evaluate(watch);
  const t12 = terminal(mysql.say, filler(0, 2));
  await mysql.page.waitForTimeout(150);
  await t12.print(mysql.page, ["+----+------+", "| id | name |", "+----+------+"]);
  await mysql.page.waitForTimeout(40);
  await t12.print(mysql.page, ["|  1 | a    |", "+----+------+", "1 row in set"]);
  await mysql.page.waitForTimeout(300);
  const cutHead = await steady(mysql.page);
  check(`${at} an ASCII table cut right after its head rule: one <table>, no frame shows its rows`, cutHead.tables === 1 && !cutHead.raw, JSON.stringify(cutHead));
  whole(mysql.page, await heads(mysql.page), `${at} and its head is its head`, cutHead, "+", "id,name");
  await mysql.page.close();
  const bare = await open(browser, { history: [], screen: filler(0, 2) });
  await bare.page.evaluate(watch);
  const t13 = terminal(bare.say, filler(0, 2));
  await bare.page.waitForTimeout(150);
  await t13.print(bare.page, ["+---+---+", "| a | b |", "+---+---+"]);
  await bare.page.waitForTimeout(350);
  const stays = await steady(bare.page);
  check(`${at} an ASCII table with one part and nothing after it: a widget all the same, no frame shows its rows`, stays.tables === 1 && !stays.raw, JSON.stringify(stays));
  await bare.page.close();

  // a batch that takes it wholly into the history, none of its lines left on the screen
  const batch = await open(browser, { history: [], screen: filler(0, 2) });
  await batch.page.evaluate(watch);
  const t8 = terminal(batch.say, filler(0, 2));
  await batch.page.waitForTimeout(150);
  await t8.print(batch.page, filler(690, 1));                     // a scan just now: the next would wait
  await batch.page.waitForFunction(() => document.getElementById("screen").textContent.includes("line 690"));
  await t8.print(batch.page, [...TABLE, ...filler(700, HEIGHT + 5)]);
  await batch.page.waitForTimeout(300);
  const gone = await steady(batch.page);
  check(`${at} a batch that takes it straight into the history: no frame shows its rows`, gone.tables === 1 && !gone.raw, JSON.stringify(gone));
  await batch.page.close();

  // a scan caught between the history's message and the screen's: a batch of output, the history's
  // message ending inside the table, the screen still as it was (its first row no table's)
  const MYSQL = ["+----+------+", "| id | name |", "+----+------+", "|  1 | a    |", "|  2 | b    |", "+----+------+"];
  const PIPES = ["| Name | Count |", "|---|---|", "| a | 1 |", "| b | 2 |", "| c | 3 |", "after it"];
  for (const [table, what, ruleOf, headOf, cuts] of [[TABLE, "", "┌", "#,Server name,Apps all", [1, 2, 3, 4, 5]], [MYSQL, " (mysql)", "+", "id,name", [2, 3]], [PIPES, " (pipes)", "|", "Name,Count", [1, 2]]]) for (const cut of cuts) {
    const lines = filler(0, 10);
    const b = await open(browser, { history: [], screen: lines });
    const tb = terminal(b.say, lines);
    await b.page.waitForTimeout(200);
    await tb.print(b.page, table);
    await b.page.waitForTimeout(300);
    await tb.print(b.page, filler(300, cut + 23 - table.length), 150);   // scrolled: the 10 lines and `cut` of the table
    await b.page.waitForTimeout(300);
    whole(b.page, await heads(b.page), `${at} the history's message ending ${cut} lines into the table${what}, ahead of the screen's: its top line and whole head stay in the widget`, { raw: 0 }, ruleOf, headOf);
    await tb.print(b.page, filler(400, HEIGHT));
    await b.page.waitForTimeout(300);
    whole(b.page, await heads(b.page), `${at} and all of it in the history`, { raw: 0 }, ruleOf, headOf);
    await b.page.close();
  }

  // far up in the history, read by scrolling: drawn before it comes, nothing changes on the way
  const far = await open(browser, { history: [...filler(0, 400), ...TABLE, ...filler(500, 400)].map((t) => line(t)), screen: filler(900, 3) });
  await far.page.waitForFunction(() => document.querySelector("#term .ln.wg-host")?.shadowRoot.querySelector("table"), null, { timeout: 3000 }).catch(() => {});
  await far.page.evaluate(watch);
  await far.page.evaluate(() => {
    window.changes = 0;
    new MutationObserver((list) => { window.changes += list.filter((m) => [...m.addedNodes, ...m.removedNodes].some((n) => n.nodeType === 1)).length; }).observe(document.getElementById("hist"), { childList: true, subtree: true });
    const t = document.getElementById("term");
    return new Promise((resolve) => { let i = 0; const step = () => { t.scrollTop = t.scrollHeight * (1 - i / 40); if (++i > 40) resolve(); else requestAnimationFrame(step); }; step(); });
  });
  await far.page.waitForTimeout(400);
  const read = await steady(far.page), changes = await far.page.evaluate(() => window.changes);
  check(`${at} a table far up in the history is a widget before the reader comes to it`, read.tables === 1 && !read.raw, JSON.stringify(read));
  check(`${at} scrolling to it changes no row`, changes === 0, `${changes} changes`);
  await far.page.close();
}
