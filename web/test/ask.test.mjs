// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What Ask puts (ask.js, AC-37): the selection as a numbered Markdown quote, compact, the question,
// typed into an agent's box when it fits, pasted otherwise.
import assert from "node:assert/strict";
import { test } from "node:test";
import { askSends, askText, boxRows, fitsTyped, sentCopy } from "../static/ask.js";
import { squeeze } from "../static/asked.js";

test("a Markdown quote numbered with four digits on its first line, a blank line, the question", () => {
  assert.equal(askText(7, "retry loop\nnever resets\n", "  why?  "), "> Quote #0007\n> retry loop\n> never resets\n\nwhy?");
  assert.ok(askText(12345, "x", "q").startsWith("> Quote #12345\n> x\n"), "more digits as they come");
});

test("compact: blank lines and line ends' spaces left out; tabs as spaces and no control characters (each would be a key typed)", () => {
  assert.equal(askText(2, "\n  Tests:   \n\n\n  - cuts  \n\n  - 416\n\n", "ok"), "> Quote #0002\n>   Tests:\n>   - cuts\n>   - 416\n\nok");
  assert.equal(askText(3, "a\tb\x1b[31mc\r", "q"), "> Quote #0003\n> a    b[31mc\n\nq");
});

test("a line ending in an @-word ends in a space: the agent's file list closes before the new line", () => {
  assert.equal(askText(4, "see @src/app.js\nnext", "q"), "> Quote #0004\n> see @src/app.js \n> next\n\nq");
});

test("typed into an agent's box with a blank line after it (its new lines are Ctrl+J), pasted when it does not fit", () => {
  assert.deepEqual(askSends("Q", { agent: true, typed: true }), [["keys", "Q\n\n"]]);
  assert.deepEqual(askSends("Q", { agent: true }), [["paste", "Q"], ["keys", "\n\n"]], "a long quote: pasted, folded by the agent");
  assert.deepEqual(askSends("Q", { agent: false, typed: true }), [["paste", "Q\n\n"]], "a shell: Ctrl+J would run the line");
});

test("Send to Agent: the text, then Enter typed a moment after (not a new line in a paste)", () => {
  assert.deepEqual(askSends("Q", { agent: true, now: true, typed: true }), [["keys", "Q"], ["enter", "\r"]]);
  assert.deepEqual(askSends("Q", { agent: true, now: true }), [["paste", "Q"], ["enter", "\r"]]);
});

test("the rows a text takes in an agent's box: its lines wrapped at the box's width", () => {
  assert.equal(boxRows("ab\n\n" + "x".repeat(20), 14), 1 + 1 + 2);
});

test("the quote as sent is told from the text asked about (asked.js marks the original only)", () => {
  // Claude Code shows the message as sent, indented and wrapped; the original stands in the agent's answer
  const sent = squeeze("❯ Quote #0002:\n  - Quote Start ----\n    «Daily runs failed\n  twice.»").out;
  const original = squeeze("  What is wrong\n\n  - Daily runs failed\n    twice.").out;
  const before = (s) => s.slice(Math.max(0, s.indexOf("Daily") - 40), s.indexOf("Daily"));
  assert.equal(sentCopy(before(sent)), true);
  assert.equal(sentCopy(before(original)), false);
  assert.equal(sentCopy(before(squeeze(askText(3, "Daily runs failed", "why?")).out)), true, "askText's own form");
  assert.equal(sentCopy(before(squeeze("❯ Quote #0003: «Daily runs failed\n  twice.»").out)), true, "as Claude Code shows it");
  assert.equal(sentCopy(before(squeeze("❯ > Quote #0004\n  > Daily runs failed twice.").out)), true, "0.4.9's Markdown quote as Claude Code shows it");
});

test("typed when the agent's box with it fits the pane, two rows kept; not half the pane any more", () => {
  const quote = askText(1, Array.from({ length: 10 }, (_, i) => `line ${i} of the agent's answer, forty chars`).join("\n"), "why?");
  assert.equal(boxRows(quote, 45), 23, "13 lines at a phone's 45 columns");
  assert.ok(fitsTyped(quote, { cols: 45, rows: 32, used: 5 }), "a 32-row pane: 23 + 5 + 2 rows");
  assert.ok(!fitsTyped(quote, { cols: 45, rows: 29, used: 5 }), "one row short");
  assert.ok(!fitsTyped(quote, { cols: 45, rows: 16, used: 5 }), "the pane a keyboard made short");
  assert.ok(!fitsTyped(quote, { cols: 45, rows: 32, used: 9 }), "a box holding text already");
  assert.ok(!fitsTyped(quote, { cols: 45, rows: 30, used: 0 }), "a box not found counts 6 rows");
  assert.ok(!fitsTyped("q", { cols: 0, rows: 0 }), "no size yet: pasted");
  assert.ok(!fitsTyped("q", undefined), "not measured: pasted");
});

