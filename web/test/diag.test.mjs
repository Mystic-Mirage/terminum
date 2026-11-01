// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A pane's diagnostics (AC-100): the text shown names the build, the browser, the row and tmux's
// fields; an error is said, nothing shown; another pane's answer, or one nobody asked for, is ignored.
import { test } from "node:test";
import assert from "node:assert/strict";
import { CHECK, diagnostics, diagText } from "../static/diag.js";

const ENV = { ua: "Test/1.0", w: 1400, h: 900, dpr: 2 };
const ANSWER = { t: "diag", id: "tmux:local:terminum:%3", build: "abc1234-deadbeef", server: "this Mac",
  tmux: { pane_width: "80", pane_height: "24", alternate_on: "1", mouse_any_flag: "1", pane_current_command: "claude", window_zoomed_flag: "" } };

test("the text: build, browser and window, the row as listed, tmux's fields by name", () => {
  const t = diagText(ANSWER, { title: "Claude Code", agent: "claude", state: "working", host: null, dir: "/Users/alex/api", alt: true, sub: ["x"] }, ENV);
  const lines = t.split("\n");
  assert.equal(lines[0], "terminum diagnostics");
  assert.ok(t.includes("build: abc1234-deadbeef") && t.includes("pane: tmux:local:terminum:%3 (this Mac)"));
  assert.ok(t.includes("browser: Test/1.0") && t.includes("window: 1400×900 at 2×"));
  assert.ok(t.includes("  agent: claude") && t.includes("  state: working") && t.includes("  alt: true"));
  assert.ok(!t.includes("host:") && !t.includes("sub:") && !t.includes("/Users/alex"), "empty fields, others than the row's and the row's full folder are left out");
  assert.ok(t.includes("  alternate_on: 1") && t.includes("  pane_current_command: claude") && t.includes("  window_zoomed_flag: -"));
});

test("a row no longer in the list is said", () => {
  assert.ok(diagText(ANSWER, undefined, ENV).includes("  (not in the list)"));
});

test("only the answer asked for is shown, with what to check first; an error is said and shows nothing", () => {
  const sent = [], shown = [], said = [];
  globalThis.navigator ??= { userAgent: "node" };
  Object.assign(globalThis, { innerWidth: 390, innerHeight: 844, devicePixelRatio: 3 });
  const d = diagnostics({ send: (m) => sent.push(m), show: (...a) => shown.push(a), say: (t) => said.push(t), item: () => ({ title: "api" }) });
  d.seen(ANSWER);
  assert.equal(shown.length, 0, "nobody asked");
  d.open(ANSWER.id);
  assert.deepEqual(sent, [{ t: "diag", id: ANSWER.id }]);
  d.seen({ ...ANSWER, id: "tmux:local:terminum:%9" });
  assert.equal(shown.length, 0, "another pane's");
  d.seen(ANSWER);
  assert.equal(shown.length, 1);
  const [heading, meta, text] = shown[0];
  assert.equal(heading, "Diagnostics · api");
  assert.equal(meta, CHECK);
  assert.ok(meta.includes("folder, host and title") && text.includes("window: 390×844 at 3×"));
  d.seen(ANSWER);
  assert.equal(shown.length, 1, "answered once");
  d.open(ANSWER.id);
  d.seen({ t: "diag", id: ANSWER.id, error: "This pane has closed. Pick another one." });
  assert.deepEqual(said, ["No diagnostics: This pane has closed. Pick another one."]);
  assert.equal(shown.length, 1);
});
