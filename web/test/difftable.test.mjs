// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A Markdown table in Claude Code's diff of an edit (difftable.js, AC-31).
import assert from "node:assert/strict";
import { test } from "node:test";
import { diffTable } from "../static/difftable.js";

const L = (...t) => t.map((text) => ({ text }));

test("numbered diff rows of a table: its header, the rows, removed and added marked", () => {
  const t = diffTable(L("      5   | Field | Value |", "      6   |---|---|", "      7 - | Version | 0.11.1 |", "      7 + | Version | 0.12.0 |", "      8   | Date | 2026-10-19 |", "    ..."), 0);
  assert.deepEqual(t.head, ["Field", "Value"]);
  assert.deepEqual(t.rows, [["Version", "0.11.1"], ["Version", "0.12.0"], ["Date", "2026-10-19"]]);
  assert.deepEqual(t.marks, ["-", "+", ""]);
  assert.equal(t.to, 4, "up to the hunk's end");
});

test("rows without the header shown, an escaped pipe, one row only", () => {
  const t = diffTable(L("  110 - | AC-04 | [SHOULD / P1] | a \\| b |", "  110 + | AC-04 | [SHOULD / P1] | The pane |"), 0);
  assert.equal(t.head, null);
  assert.deepEqual(t.rows[0], ["AC-04", "[SHOULD / P1]", "a | b"]);
  assert.equal(diffTable(L("  12   | one row |", "  13   text"), 0), undefined, "one row is no table");
  assert.equal(diffTable(L("  12   plain code", "  13   more"), 0), undefined);
});

// `git diff` of a Markdown table in an agent's tool output: the mark (+, - or a space) in one
// column right before the "|", rows cut at the width going on in the next lines from that column
const GIT = ["     @@ -13,6 +13,7 @@ Change log:", "",
  "      | Version | Date | Change |", "      |---|---|---|",
  "     +| 1.21.0 | 2026-11-01 | Owner requests: signs of automation —",
  "     (WebDriver, driver globals and elements, an a",
  "      | 1.20.0 | 2026-11-01 | Owner requests: the server answers",
  "     every text the server words",
  "     -| 1.19.0 | 2026-10-31 | Ready for translation. |",
  "      | 1.18.0 | 2026-10-31 | Visitors counted |",
  "     @@ -128,7 +129,7 @@ Open questions:",
  "      ## Next heading"];

test("git diff rows of a table: the header from its context lines, cut rows joined, marked", () => {
  const t = diffTable(L(...GIT), 2);
  assert.deepEqual(t.head, ["Version", "Date", "Change"]);
  assert.deepEqual(t.rows, [
    ["1.21.0", "2026-11-01", "Owner requests: signs of automation — (WebDriver, driver globals and elements, an a"],
    ["1.20.0", "2026-11-01", "Owner requests: the server answers every text the server words"],
    ["1.19.0", "2026-10-31", "Ready for translation."],
    ["1.18.0", "2026-10-31", "Visitors counted"]]);
  assert.deepEqual(t.marks, ["+", "", "-", ""]);
  assert.equal(t.to, 9, "ends at the hunk's next head");
});

test("git diff: a context line after a cut row is no part of it; a table nothing changed in is a plain one", () => {
  const t = diffTable(L("    | a | b |", "    |---|---|", "   +| 1 | cut sh", "    ## Next heading", "   +| 2 | x |"), 0);
  assert.deepEqual(t.rows, [["1", "cut sh"]], "the context line ends the table");
  assert.equal(t.to, 2);
  assert.equal(diffTable(L("     | a | b |", "     |---|---|", "     | 1 | 2 |", "     | 3 | 4 |"), 0), undefined, "no row added or removed");
  assert.equal(diffTable(L("   +| a | b |", "  +| c | d |"), 0), undefined, "marks in two columns: one row each");
});
