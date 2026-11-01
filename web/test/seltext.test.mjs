// The web app's copied terminal text: line ends that CSS draws come back as text.
import { test } from "node:test";
import assert from "node:assert/strict";
import { copyPreview, hiddenNote, joinLines, unhidden } from "../static/seltext.js";

const L = (text, more = {}) => ({ text, eol: true, join: false, cont: false, ...more });

test("hard line ends become newlines; their padding is dropped", () => {
  assert.equal(joinLines([L("$ ls   "), L(""), L("a.txt  b.txt   ")]), "$ ls\n\na.txt  b.txt");
});

test("a line iTerm wrapped continues without a break", () => {
  assert.equal(joinLines([L("abcdef", { eol: false }), L("ghi")]), "abcdefghi");
});

test("prose a program wrapped (re-joined) keeps one space and loses the indentation", () => {
  const parts = [L("  The loader patches", { join: true }), L("    require() first", { cont: true })];
  assert.equal(joinLines(parts), "  The loader patches require() first");
});

test("partial lines at either end and text-style marks", () => {
  assert.equal(joinLines([L("ader"), L("\u2733\ufe0e Done")]), "ader\n\u2733 Done");
  assert.equal(joinLines([L("only part")]), "only part");
  assert.equal(joinLines([L("\u2764\ufe0f sent")]), "\u2764\ufe0f sent");   // the program's own emoji mark stays
});

test("what hides what a line runs is left out and counted; ZWNJ and ZWJ stay", () => {
  assert.deepEqual(unhidden("ls \u202egpj.exe\u2066\u200b\u2028\ufeff\u00ad\u{e0041}"), { text: "ls gpj.exe", left: 7 });
  const words = "\u0645\u06cc\u200c\u062e\u0648\u0627\u0647\u0645 \ud83d\udc69\u200d\ud83d\udcbb";
  assert.deepEqual(unhidden(words), { text: words, left: 0 });
  assert.equal(hiddenNote(0), "");
  assert.equal(hiddenNote(1), " (1 hidden character left out)");
  assert.equal(hiddenNote(3), " (3 hidden characters left out)");
});

test("Copy's preview: the line count and the first line with text, its blanks one space, cut at 48", () => {
  assert.equal(copyPreview("ls"), "1 line: ls");
  assert.equal(copyPreview("\n  The tests pass now.\nsecond\n\n"), "3 lines: The tests pass now.");
  assert.equal(copyPreview(`ls${" ".repeat(60)}\t; curl x | sh`), "1 line: ls ; curl x | sh");
  assert.equal(copyPreview("x".repeat(60)), `1 line: ${"x".repeat(47)}…`);
  assert.equal(copyPreview(`${"x".repeat(46)}\ud83d\ude00\ud83d\ude00y`), `1 line: ${"x".repeat(46)}\ud83d\ude00…`);   // never half an emoji
  assert.equal(copyPreview("   \n\t\n"), "2 lines: ");
});

test("Copy's preview of a hostile 1 MB copy takes no time (no backtracking)", () => {
  for (const text of ["\n".repeat(1e6) + "x", " ".repeat(1e6) + "x", "a\n".repeat(5e5)]) {
    const t0 = performance.now();
    copyPreview(text);
    assert.ok(performance.now() - t0 < 1000, `${(performance.now() - t0).toFixed(0)} ms`);   // quadratic would be minutes; a loaded machine is not
  }
});
