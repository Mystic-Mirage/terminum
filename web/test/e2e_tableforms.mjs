// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-31 the brute force of tableforms.test.mjs on the real page, on the fake bridge of
// e2e_widgets.mjs: a sample of the forms of a table, loaded as history and printed in batches: each
// table is a widget with its head and rows, and every other line is in sight, in its place: no
// line lost, none left over but the screen's prompt.

import { check } from "./check.mjs";
import { line, open } from "./e2e_widgets.mjs";
import { terminal } from "./e2e_widgetsteady.mjs";
import { forms } from "./tableforms.mjs";

const STEP = 97;          // every so many forms: coprime with the matrix's sizes, so each kind of end meets each

/** The sample, one after another with a line between: {lines, tables (head, rows), text (the lines left outside them)}. */
function sample() {
  const lines = [], tables = [], text = [];
  let count = 0, n = 0;
  for (const f of forms()) {
    if (n++ % STEP || f.waits || /after:(nothing|blankEnd)$/.test(f.name)) continue;
    const base = lines.length;
    lines.push(...f.lines, `form ${count}`);
    const inside = (i) => f.want.some((w) => i >= w.from && i <= w.to);
    f.lines.forEach((t, i) => { if (!inside(i)) text.push(t); });
    text.push(`form ${count}`);
    tables.push(...f.want.map((w) => ({ head: w.head, rows: w.rows, from: base + w.from })));
    count++;
  }
  return { lines, tables, text, count };
}

// in the page: the tables of the widgets, in order, and the rows in sight outside them
const read = (page) => page.evaluate(() => {
  const cells = (row, tag) => [...row.querySelectorAll(tag)].map((c) => c.textContent);
  const rows = [...document.querySelectorAll("#term .ln")];
  return {
    tables: rows.filter((el) => el.classList.contains("wg-host") && !el.classList.contains("wg-raw")).map((el) => {
      const t = el.shadowRoot?.querySelector("table"), head = t ? cells(t, "th") : [];
      return { head: head.length ? head : null, rows: t ? [...t.tBodies[0].rows].map((r) => cells(r, "td")) : [] };
    }),
    text: rows.filter((el) => !el.classList.contains("wg-host") && !el.classList.contains("wg-hid") && el.getClientRects().length).map((el) => el.textContent.trimEnd()),
  };
});

function compare(at, got, want) {
  const bad = want.tables.flatMap((w, i) => (JSON.stringify([got.tables[i]?.head, got.tables[i]?.rows]) === JSON.stringify([w.head, w.rows]) ? [] : [{ i, want: w, got: got.tables[i] }]));
  check(`${at} each of the ${want.tables.length} tables is a widget with its head and rows`, got.tables.length === want.tables.length && !bad.length, JSON.stringify({ tables: got.tables.length, bad: bad.slice(0, 2) }));
  const text = want.text.map((t) => t.trimEnd()), off = text.findIndex((t, i) => got.text[i] !== t);
  check(`${at} every line outside them is in sight, in its place (${text.length})`, off < 0 && got.text.slice(text.length).every((t) => /^(\$|\S+@\S+ \S+ %)?$/.test(t.trim())), JSON.stringify({ rows: got.text.length, off, want: text[off], got: got.text[off], more: got.text.slice(text.length, text.length + 3) }));
}

export async function webTableForms(browser, name = "") {
  const at = `AC-31 web app${name}: the forms of a table,`;
  const s = sample();
  check(`${at} a sample of them`, s.count >= 100 && s.tables.length >= 100, `${s.count} forms, ${s.tables.length} tables, ${s.lines.length} lines`);

  // loaded with the page, as the history of a session opened
  const a = await open(browser, { history: s.lines.map((t) => line(t)), screen: [""] });
  await a.page.waitForFunction((n) => document.querySelectorAll("#term .ln.wg-host").length >= n, s.tables.length, { timeout: 20000 }).catch(() => {});
  await a.page.waitForTimeout(800);
  compare(`${at} loaded as history:`, await read(a.page), s);
  check(`${at} loaded as history: no page errors`, !a.errors.length, a.errors.join(" | "));
  await a.page.close();

  // printed in batches of 1 to 9 lines, scrolling through the screen
  const b = await open(browser, { history: [], screen: ["start"] });
  const tb = terminal(b.say, ["start"]);
  await b.page.waitForTimeout(150);
  for (let i = 0, size = 1; i < s.lines.length; i += size, size = size % 9 + 1) {
    await tb.print(b.page, s.lines.slice(i, i + size), i % 7 ? 0 : 20);
    await b.page.waitForTimeout(6);
  }
  await b.page.waitForTimeout(900);
  const got = await read(b.page);
  compare(`${at} printed in batches:`, { ...got, text: got.text.filter((t) => t !== "start") }, s);
  check(`${at} printed in batches: no page errors`, !b.errors.length, b.errors.join(" | "));
  await b.page.close();
}
