// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
import { test } from "node:test";
import assert from "node:assert/strict";
import { ageText, etaLine, pctText, rowTitle, sourceText, statusIcon } from "../static/plans.js";

const now = 1_800_000_000;
const plan = { done: 3, total: 7, pct: 42.857, eta: null, updated: now - 120, stale: false, partial: false };

test("status icons, unknown as pending", () => {
  assert.deepEqual(["completed", "in_progress", "pending", "blocked"].map(statusIcon), ["✓", "▶", "○", "!"]);
  assert.equal(statusIcon("deleted"), "○");
  assert.equal(statusIcon(undefined), "○");
});

test("percent is terminumd's, rounded", () => {
  assert.equal(pctText(plan), "43%");
  assert.equal(pctText({ pct: 0 }), "0%");
  assert.equal(pctText({ pct: 100 }), "100%");
});

test("ages", () => {
  assert.equal(ageText(now, now), "just now");
  assert.equal(ageText(now + 30, now), "just now");          // a clock ahead never goes negative
  assert.equal(ageText(now - 59, now), "just now");
  assert.equal(ageText(now - 60, now), "1 min ago");
  assert.equal(ageText(now - 3599, now), "59 min ago");
  assert.equal(ageText(now - 7200, now), "2 h ago");
  assert.equal(ageText(now - 86400 * 2, now), "2 d ago");
});

test("row without an ETA", () => {
  assert.equal(rowTitle(plan, now), "3 of 7 · updated 2 min ago");
  assert.equal(pctText(plan), "43%");
  assert.equal(etaLine(plan), "");
});

test("row with ETA, partial and stale", () => {
  const p = { ...plan, eta: now + 2700, partial: true, stale: true };
  const t = rowTitle(p, now);
  assert.match(t, /^3 of 7 · ETA ~\d\d:\d\d · updated 2 min ago · partial · stale$/);
  assert.match(etaLine({ ...p, eta_given: now }), /^~\d\d:\d\d \(agent's estimate, given \d\d:\d\d\)$/);
  assert.match(etaLine(p), /^~\d\d:\d\d \(agent's estimate\)$/);
});

test("source", () => {
  assert.equal(sourceText("tool"), "from the agent's plan tool");
  assert.equal(sourceText("mcp"), "reported through terminum's MCP");
});

