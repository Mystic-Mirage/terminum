// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-92 the session list's one order of projects and tmux groups.
import { test } from "node:test";
import assert from "node:assert/strict";
import { arranged, moved } from "../static/reorder.js";

test("AC-92 a move among the parts drawn leaves the others where they are", () => {
  assert.deepEqual(moved(["a", "b", "c", "d", "e"], ["d", "b"]), ["a", "d", "c", "b", "e"], "a filter shows b and d: d up");
  assert.deepEqual(moved(["a", "b", "c"], ["c", "a", "b"]), ["c", "a", "b"], "all drawn: as drawn");
  assert.deepEqual(moved(["a", "old", "b"], ["a", "new", "old", "b"]), ["a", "new", "old", "b"], "a rename: the new name beside the old");
  assert.deepEqual(moved(["a", "gone-from-list"], ["a"]), ["a", "gone-from-list"]);
});

const P = [{ id: "a" }, { id: "b" }], G = [{ wid: "x" }, { wid: "y" }], ids = (l) => l.map((p) => p.id).join();

test("AC-92 with no order the projects stand first, the groups as given", () => {
  assert.equal(ids(arranged(G, P)), "p:a,p:b,g:x,g:y");
});
test("AC-92 an order mixes them, a group above a project", () => {
  assert.equal(ids(arranged(G, P, ["g:y", "p:b", "g:x", "p:a"])), "g:y,p:b,g:x,p:a");
});
test("AC-92 one not named: a project after the last project, a group at the end; a part gone is skipped", () => {
  assert.equal(ids(arranged(G, P, ["p:a", "g:x", "g:gone", "p:gone"])), "p:a,p:b,g:x,g:y");
  assert.equal(ids(arranged(G, [{ id: "new" }], ["g:x", "g:y"])), "p:new,g:x,g:y");
});
