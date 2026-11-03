// History held while the terminal is frozen (audit #3): appends merge, lines past the limit go.
import { test } from "node:test";
import assert from "node:assert/strict";
import { queueHist } from "../static/histq.js";

const app = (first, n, oldest = 0) => ({ mode: "append", first, oldest, truncated: false, lines: Array.from({ length: n }, (_, i) => ({ k: first + i })) });
const fill = (msgs) => msgs.reduce(queueHist, []);

test("contiguous appends merge into one message with the newest limit", () => {
  const q = fill([app(10, 2), app(12, 3, 1)]);
  assert.equal(q.length, 1);
  assert.equal(q[0].first, 10);
  assert.deepEqual(q[0].lines.map((l) => l.k), [10, 11, 12, 13, 14]);
  assert.equal(q[0].oldest, 1);
});

test("an overlapping append replaces the lines it sends again", () => {
  const q = fill([app(10, 3), { ...app(11, 3), lines: [{ k: "new11" }, { k: 12 }, { k: 13 }] }]);
  assert.deepEqual(q[0].lines.map((l) => l.k), [10, "new11", 12, 13]);
});

test("a long hold keeps at most one limit's worth of lines", () => {
  const limit = 500, msgs = [];
  for (let n = 0; n < 1201; n++) msgs.push(app(n, 1, Math.max(0, n + 1 - limit)));
  const q = fill(msgs);
  assert.equal(q.length, 1);
  assert.equal(q[0].lines.length, limit);
  assert.equal(q[0].first, 701);
  assert.equal(q[0].first + q[0].lines.length, 1201);   // the screen's top is unchanged
});

test("a gap or a prepend is kept apart; an append wholly below the limit goes when a later one follows", () => {
  const pre = { mode: "prepend", first: 0, oldest: 0, lines: [{ k: 0 }] };
  let q = fill([app(10, 2), pre, app(20, 1)]);
  assert.deepEqual(q.map((m) => m.mode), ["append", "prepend", "append"]);
  q = fill([app(10, 2), app(20, 1, 15)]);
  assert.deepEqual(q.map((m) => m.first), [20]);
});

test("the last append keeps its end even when all its lines are below the limit", () => {
  const q = fill([app(10, 2, 50)]);
  assert.equal(q.length, 1);
  assert.equal(q[0].lines.length, 0);
  assert.equal(q[0].first, 12);
});
