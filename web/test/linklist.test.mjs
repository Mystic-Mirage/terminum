// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The pane's links (AC-102): the count line, the filter and the time.
import { test } from "node:test";
import assert from "node:assert/strict";
import { countText, filtered, whenText } from "../static/linklist.js";

test("the count says how many, of how many, and past the limit", () => {
  assert.equal(countText(1, 1, 1), "1 link");
  assert.equal(countText(3, 3, 3), "3 links");
  assert.equal(countText(1, 3, 3), "1 of 3 links");
  assert.equal(countText(500, 500, 812), "500 of 812 links");
  assert.equal(countText(2, 500, 812), "2 of 500 links");
});

test("the filter takes the address, case aside", () => {
  const items = [{ u: "https://ci.example/run/7" }, { u: "https://git.example/mr/318" }];
  assert.deepEqual(filtered(items, "  "), items);
  assert.deepEqual(filtered(items, "MR/3"), [items[1]]);
  assert.deepEqual(filtered(items, "CI.EX"), [items[0]]);
  assert.deepEqual(filtered(items, "none"), []);
});

test("the time of the last mention: the hour today, the day this year, the year before", () => {
  const now = new Date(2026, 10, 3, 18, 0);
  assert.equal(whenText(new Date(2026, 10, 3, 9, 5).toISOString(), now), "09:05");
  assert.equal(whenText(new Date(2026, 10, 1, 23, 0).toISOString(), now), "Nov 1");
  assert.equal(whenText(new Date(2025, 0, 2).toISOString(), now), "Jan 2, 2025");
  assert.equal(whenText(null, now), "");
  assert.equal(whenText("not a time", now), "");
});
