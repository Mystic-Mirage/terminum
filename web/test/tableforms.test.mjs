// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-31 a brute force over how a drawn table begins and ends: every family of rules, every way its
// top and bottom may be there or lost, with every kind of line before and after it. Each form is
// read whole, printed line by line and cut between the history and the screen at every line; a
// model of the form says what comes out. A form with no whole rule is text; nothing else is.
import { test } from "node:test";
import assert from "node:assert/strict";
import { forms, oneTable, play } from "./tableforms.mjs";
import { scan } from "../static/outparse.js";

const rows = (lines) => lines.map((text) => ({ text, eol: true, styled: false }));
const shape = (r) => ({ from: r.from, to: r.to, head: r.table.head, rows: r.table.rows });
const tables = (found) => found.filter((r) => r.kind === "table").map(shape);

// Failures are collected: one line per form, the first few shown.
function verdict(name, bad, count, least) {
  assert.ok(count >= least, `${name}: ${count} forms checked, expected at least ${least}`);
  assert.deepEqual(bad.slice(0, 8), [], `${name}: ${bad.length} of ${count} forms wrong`);
}
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

test("AC-31 brute force: every form read whole is its table, at its lines, or text", () => {
  const bad = [];
  let count = 0;
  for (const f of forms()) {
    count++;
    const { found, waiting } = scan(rows(f.lines));
    const got = tables(found);
    if (!same(got, f.want)) bad.push({ form: f.name, got, want: f.want });
    if (!same(waiting !== null, f.waits)) bad.push({ form: f.name, waiting, wants: f.waits });
    for (const r of found) if (r.kind !== "table") bad.push({ form: f.name, other: r.kind });
  }
  verdict("whole", bad, count, 10000);
});

test("AC-31 brute force: printed line by line, no line shows a wrong table", () => {
  const bad = [];
  let count = 0;
  for (const f of forms()) {
    count++;
    for (let k = 1; k < f.lines.length; k++) {
      for (const r of tables(scan(rows(f.lines.slice(0, k))).found)) {
        // a table of the form, as far as printed: its head (none yet: an ASCII rule over its first
        // part, that part its one row) and its first rows, the last perhaps not all of it
        const of = f.want.find((w) => w.from === r.from) ?? (r.from === f.of.from ? f.of : null);
        const rowsFine = r.rows.every((row, i) => i === r.rows.length - 1 || same(row, of.rows[i]));
        const fine = of && r.to <= of.to && (r.head === null ? of.head === null || (r.rows.length === 1 && same(r.rows[0], of.head)) : same(r.head, of.head) && rowsFine);
        if (!fine) bad.push({ form: f.name, at: k, got: r, of });
      }
    }
  }
  verdict("line by line", bad, count, 10000);
});

test("AC-31 brute force: wherever the history and the screen meet, as often as scans come, each table comes out once, whole", () => {
  const bad = [];
  let count = 0;
  for (const f of forms()) {
    count++;
    for (let cut = 1; cut < f.lines.length; cut++) {
      const got = play(f.lines, [cut]);
      if (!same(got, f.want)) bad.push({ form: f.name, cut, got, want: f.want });
    }
    const each = play(f.lines, f.lines.map((_, i) => i + 1));
    if (!same(each, f.want)) bad.push({ form: f.name, each, want: f.want });
  }
  verdict("cut", bad, count, 10000);
});

test("AC-31 brute force: the owner's table, its top lost by the terminal, a wrapped row first", () => {
  const lines = ["  Heading", "  │ Comes in parts (a slow link)       │ Rows stay hidden for 150 ms, then the text shows, nothing is  │",
    "  │                                    │ lost.                                                         │",
    "  ├────────────────────────────────────┼───────────────────────────────────────────────────────────────┤",
    "  │ A repeated rule flashed            │ The old region no longer shows it.                            │",
    "  │ as text                            │                                                               │",
    "  ├────────────────────────────────────┼───────────────────────────────────────────────────────────────┤",
    "  │ Far tables stayed text             │ Drawn at once.                                                │",
    "  └────────────────────────────────────┴───────────────────────────────────────────────────────────────┘", "", "  After it."];
  const t = oneTable(lines);
  assert.deepEqual([t.from, t.to, t.head], [1, 8, null]);
  assert.deepEqual(t.rows, [["Comes in parts (a slow link)", "Rows stay hidden for 150 ms, then the text shows, nothing is lost."],
    ["A repeated rule flashed as text", "The old region no longer shows it."], ["Far tables stayed text", "Drawn at once."]]);
});

test("AC-31 an ASCII table cut right after its head rule is not whole: its head is no row, and it waits", () => {
  const a = ["text", "+---+---+", "| a | b |", "+---+---+"];
  const cut = scan(rows(a));
  assert.equal(cut.waiting, 1, "out of sight meanwhile");
  assert.equal(cut.found[0].table.open, true, "it may go on");
  const go = scan(rows([...a, "| 1 | 2 |", "+---+---+"]));
  assert.deepEqual([go.found[0].table.head, go.found[0].table.rows, go.found[0].table.open, go.waiting], [["a", "b"], [["1", "2"]], false, null], "whole with its last rule and a head");
  const after = scan(rows([...a, "done"]));
  assert.deepEqual([after.found[0].table.open, after.waiting], [false, null], "a line after the rule closes it");
});

test("AC-31 what may still grow says so: a Markdown table at the end, a table with no bottom rule", () => {
  const pipes = ["| a | b |", "|---|---|", "| 1 | 2 |"];
  assert.equal(scan(rows(pipes)).found[0].table.open, true);
  assert.equal(scan(rows([...pipes, ""])).found[0].table.open, false);
  const lost = ["┌───┬───┐", "│ a │ b │", "├───┼───┤", "│ 1 │ 2 │", "next line"];
  const s = scan(rows(lost));
  assert.deepEqual([s.found[0].to, s.found[0].table.head, s.found[0].table.open, s.waiting], [3, ["a", "b"], true, null]);
  const whole = scan(rows(["┌───┬───┐", "│ a │ b │", "├───┼───┤", "│ 1 │ 2 │", "└───┴───┘"]));
  assert.equal(whole.found[0].table.open, false);
});

test("AC-31 no table, whatever the ends: a tree, a panel, one column, rows alone, a line of a box off the grid", () => {
  for (const lines of [["├── src", "│   └── x.js", "└── y"], ["╭────╮", "│ hi │", "╰────╯"], ["┌───┐", "│ a │", "├───┤", "│ b │", "└───┘"],
    ["│ a │ b │", "│ c │ d │", "text"], ["┌───┬───┐", "│ a │ b │", "│ c   d │", "└───┴───┘", "text"], ["┌───┬───┐", "text", "more"], ["├───┼───┤", "text", "more"], ["+---+---+", "text", "more"],
    ["│ a │ b │", "├───┼───┼───┤", "│ c │ d │", "text"], ["| a | b |", "| c | d |", "text"],
    ["┌───┬───┐", "│ a │ b │", "│ c   d │", "│ e │ f │", "├───┼───┤", "│ g │ h │", "└───┴───┘", "text"]]) {
    assert.deepEqual(tables(scan(rows(lines)).found), [], lines.join(" / "));
  }
});

test("AC-31 a line of text in the middle of a table: a table on each side of it, no row left out", () => {
  const got = tables(scan(rows(["┌───┬───┐", "│ a │ b │", "├───┼───┤", "│ 1 │ 2 │", "a line", "│ 3 │ 4 │", "├───┼───┤", "│ 5 │ 6 │", "└───┴───┘"])).found);
  assert.deepEqual(got, [{ from: 0, to: 3, head: ["a", "b"], rows: [["1", "2"]] }, { from: 5, to: 8, head: ["3", "4"], rows: [["5", "6"]] }]);
});
