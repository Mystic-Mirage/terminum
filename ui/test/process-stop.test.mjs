// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { stopChild } from "./process-stop.mjs";

async function child(onTerm) {
  const process = spawn(globalThis.process.execPath, ["-e", `process.on('SIGTERM',()=>{${onTerm}});console.log('ready');setInterval(()=>{},1000)`], { stdio: ["ignore", "pipe", "pipe"] });
  await once(process.stdout, "data");
  return process;
}

test("shutdown waits for graceful exit before cleanup can continue", async () => {
  const process = await child("setTimeout(()=>process.exit(0),100)");
  try {
    const at = performance.now();
    await stopChild(process);
    assert.equal(process.exitCode, 0);
    assert.ok(performance.now() - at >= 80);
  } finally { await stopChild(process, 10); }
});

test("shutdown bounds a child ignoring SIGTERM and accepts an already signaled exit", async () => {
  const process = await child("");
  try {
    await stopChild(process, 50);
    assert.equal(process.signalCode, "SIGKILL");
    await stopChild(process, 10);
  } finally { await stopChild(process, 10); }
});

/** A run that took endOnSignal's teardown: it writes "ended" to `out` after 100 ms, once. */
async function run(out, ...args) {
  const helper = new URL("./process-stop.mjs", import.meta.url).href;
  const code = `import fs from "node:fs"; import { endOnSignal } from ${JSON.stringify(helper)};
    const teardown = endOnSignal(() => new Promise((r) => setTimeout(() => { fs.appendFileSync(${JSON.stringify(out)}, "ended\\n"); r(); }, 100)));
    if (process.argv[1] === "finally") { try {} finally { await teardown(); await teardown(); } }
    else { console.log("ready"); setInterval(() => {}, 1000); }`;
  return spawn(globalThis.process.execPath, ["--input-type=module", "-e", code, ...args], { stdio: ["ignore", "pipe", "pipe"] });
}

for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  test(`${sig} runs the teardown to its end, then exits as the signal would`, async () => {
    const out = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "tmps-")), "out");
    const child = await run(out);
    try {
      await once(child.stdout, "data");
      child.kill(sig);
      const [code] = await once(child, "exit");
      assert.equal(code, 128 + os.constants.signals[sig]);
      assert.equal(fs.readFileSync(out, "utf8"), "ended\n");
    } finally { await stopChild(child, 10); fs.rmSync(path.dirname(out), { recursive: true, force: true }); }
  });
}

test("a teardown called again from finally runs once", async () => {
  const out = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "tmps-")), "out");
  try {
    const [code] = await once(await run(out, "finally"), "exit");
    assert.equal(code, 0);
    assert.equal(fs.readFileSync(out, "utf8"), "ended\n");
  } finally { fs.rmSync(path.dirname(out), { recursive: true, force: true }); }
});
