// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// holdLock of testrun.mjs (spec AC-104): one run at a time at what every run shares, so trees run checks at once.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { holdLock, ownTmp } from "./testrun.mjs";

const fresh = () => path.join(ownTmp("tm-holdlock-"), "lock");

test("the second waits until it is given back", async () => {
  const lock = fresh(), release = await holdLock(lock, 1000);
  let got = false;
  const second = holdLock(lock, 3000).then((r) => { got = true; return r; });
  await new Promise((resolve) => setTimeout(resolve, 400));
  assert.equal(got, false, "still held");
  release();
  let cleaned = false;
  (await second)();
  (await holdLock(lock, 1000, () => { cleaned = fs.existsSync(lock); }))();
  assert.equal(cleaned, true, "its cleanup runs while it is still held");
  assert.equal(fs.existsSync(lock), false);
});

test("one that stays taken gives null, and it stays", async () => {
  const lock = fresh(), release = await holdLock(lock, 1000);
  assert.equal(await holdLock(lock, 300), null);
  assert.equal(fs.existsSync(lock), true);
  release();
});

test("a holder that has died does not hold it", async () => {
  const lock = fresh(), child = spawn(process.execPath, ["-e", ""]);
  await new Promise((resolve) => child.once("exit", resolve));
  fs.mkdirSync(lock);
  fs.writeFileSync(path.join(lock, "pid"), String(child.pid));
  (await holdLock(lock, 1000))();
});
