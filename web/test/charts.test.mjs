// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The charts' scale (charts.js; terminum.md AC-151, AC-152): a "nice" top and its ticks.
import assert from "node:assert/strict";
import { test } from "node:test";
import { niceTicks } from "../static/charts.js";

test("AC-152 a nice top over the largest value, 1, 2 or 5 times a power of ten apart", () => {
  assert.deepEqual(niceTicks(87), { top: 100, ticks: [0, 50, 100] });
  assert.deepEqual(niceTicks(612), { top: 800, ticks: [0, 200, 400, 600, 800] });
  assert.deepEqual(niceTicks(3), { top: 3, ticks: [0, 1, 2, 3] });
  assert.deepEqual(niceTicks(0.4), { top: 0.4, ticks: [0, 0.1, 0.2, 0.3, 0.4] });
  assert.deepEqual(niceTicks(100), { top: 100, ticks: [0, 50, 100] }, "a round top stays");
});

test("AC-152 nothing to scale: one line at zero", () => {
  assert.deepEqual(niceTicks(0), { top: 1, ticks: [0] });
  assert.deepEqual(niceTicks(Number.NaN), { top: 1, ticks: [0] });
});
