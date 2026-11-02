// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
import assert from "node:assert/strict";
import { test } from "node:test";
import { load } from "./bundle.mjs";

Object.defineProperty(globalThis, "localStorage", { value: { getItem: () => null }, configurable: true });
const { Tree } = await load("src/tree.ts");
const node = (path) => ({ path, children: new Map(), pages: new Map(), pending: new Set() });

function deferredExpansion(parentPath = "/sandbox") {
  const root = node("/sandbox"), parent = parentPath === root.path ? root : node(parentPath);
  const tree = Object.assign(Object.create(Tree.prototype), {
    root, nodes: new Map([[root.path, root], [parent.path, parent]]), expanded: new Set(parent === root ? [] : [parent.path]),
    epoch: 0, expandRun: 0, selectionRun: 0, edit: {}, cb: { changed() {} }, touch() {}, load: async () => {},
  });
  let resolve;
  tree.locate = () => new Promise((r) => { resolve = r; });
  const path = `${parent.path}/sub`, pending = tree.expand(path);
  return { tree, parent, path, finish: async () => { resolve(0); await pending; } };
}

test("a delayed locate cannot reopen a folder after Collapse all", async () => {
  const { tree, parent, finish } = deferredExpansion();
  tree.collapseAll();
  await finish();
  assert.deepEqual([...tree.expanded], []);
  assert.equal(parent.children.size, 0);
});

test("a delayed child expansion cannot outlive collapsing its parent", async () => {
  const { tree, parent, finish } = deferredExpansion("/sandbox/parent");
  tree.collapse(parent.path);
  await finish();
  assert.deepEqual([...tree.expanded], []);
  assert.equal(parent.children.size, 0);
});

test("a delayed normal expansion still opens the folder", async () => {
  const { tree, parent, path, finish } = deferredExpansion();
  await finish();
  assert.deepEqual([...tree.expanded], [path]);
  assert.equal(parent.children.get("sub").node.path, path);
});

test("New File with a selection whose folder cannot open here starts in the root", async () => {
  for (const kept of ["/elsewhere/notes.txt", "/sandbox/closed/deep/a.txt"]) {
    const root = node("/sandbox"), started = [];
    const tree = Object.assign(Object.create(Tree.prototype), {
      root, nodes: new Map([[root.path, root]]), expanded: new Set(), selected: new Set([kept]), cursor: -1, rows: new Map(),
      epoch: 0, expandRun: 0, selectionRun: 0, edit: { start: (e) => started.push(e) }, cb: { changed() {} }, touch() {}, load: async () => {},
      locate: async () => null,
    });
    await tree.startCreate(tree.targetDir(), "file");
    assert.deepEqual(started, [{ mode: "file", dir: "/sandbox" }], kept);
  }
});
