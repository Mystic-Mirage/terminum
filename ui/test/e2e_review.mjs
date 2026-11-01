// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Review regressions against a private terminumd (e2e_harness.mjs), with controlled responses
// to exercise races. A pane is shown by loading the panel at its URL, as the page does.
//
//   node test/e2e_review.mjs            (from ui/; needs `npm run build` and terminumd built)
import assert from "node:assert/strict";
import fs from "node:fs";
import { base, check, cleanup, panelUrl, results, SB, signIn, wsKey } from "./e2e_harness.mjs";
import { hostRegressions } from "./e2e_review_hosts.mjs";
const { chromium } = await import(process.env.PLAYWRIGHT_CORE ?? "playwright-core");
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 520, height: 900 } });
const SECRET = await signIn(page).catch(async (e) => { await cleanup(); throw e; });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
page.on("dialog", (d) => void d.accept());   // leaving a panel with unsaved edits for the next case
await page.addInitScript(() => {
  const Native = window.WebSocket;
  window.WebSocket = class extends Native { constructor(...args) { super(...args); window.reviewSocket = this; } };
});
const deferred = () => { let resolve; const promise = new Promise((r) => { resolve = r; }); return { promise, resolve }; };
const test = async (name, fn) => { try { await fn(); check(name, true); } catch (e) { check(name, false, e.stack); } };
const row = (p) => page.locator(`#tree .row[data-p="${p}"]`);
const text = async (s) => page.waitForFunction((s) => document.querySelector(".cm-content")?.textContent.includes(s), s);
const edit = async (s) => { await page.click(".cm-content"); await page.keyboard.press("ControlOrMeta+End"); await page.keyboard.type(s); };
async function workspace(key, root, tabs) {
  const url = `${base}/fb/api/workspace?key=${encodeURIComponent(key)}`;
  const headers = { "X-TM-Session": SECRET, "Content-Type": "application/json", Origin: base };
  for (let attempt = 0; attempt < 3; attempt++) {
    const pane = await (await fetch(url, { headers })).json();
    const r = await fetch(url, { method: "PUT", headers, body: JSON.stringify({ ...pane, root, tabs: tabs.map((path) => ({ path, view: "auto" })), active_tab: tabs.length ? 0 : null, expanded: [], selected: [], scroll: 0 }) });
    if (r.status === 409) continue; // a panel's pending autosave may race test fixture preparation
    assert.equal(r.status, 200, await r.text());
    return;
  }
  throw new Error("Could not prepare the test workspace after concurrent autosaves");
}
/** Shows pane `key` (on `host`) in `root` with `tabs` open: its workspace, then the panel at its URL. */
const focus = async (key, root, tabs, host) => { await workspace(wsKey(key, host ?? ""), root, tabs); await page.goto(panelUrl(key, root, host)); };
for (const [name, content] of Object.entries({ "crlf.py": "one\r\ntwo\r\n", "rename.py": "rename\n", "close.py": "close\n", "trash.py": "trash\n" })) fs.writeFileSync(`${SB}/${name}`, content);
fs.copyFileSync(`${SB}/docs/logo.png`, `${SB}/docs/100%.png`);
fs.copyFileSync(`${SB}/docs/logo.png`, `${SB}/docs/a#b?c.png`);
fs.writeFileSync(`${SB}/docs/percent.html`, '<h1>Percent image</h1><img src="100%.png">');
fs.writeFileSync(`${SB}/docs/encoded.md`, "![reserved](a%23b%3Fc.png)");
fs.mkdirSync(`${SB}/many`);
for (let i = 0; i < 1205; i++) fs.writeFileSync(`${SB}/many/f${String(i).padStart(4, "0")}.txt`, "x");

try {
  await page.goto(panelUrl());
  await row(`${SB}/app.py`).waitFor();
  await test("CRLF survives edit and save", async () => {
    await focus("review-crlf", SB, [`${SB}/crlf.py`]); await text("one");
    await edit("three"); await page.keyboard.press("ControlOrMeta+s");
    await page.locator(".tab.active:not(.dirty)").waitFor();
    assert.equal(fs.readFileSync(`${SB}/crlf.py`, "utf8"), "one\r\ntwo\r\nthree");
  });
  await test("Mod-S saves the renamed retained editor state", async () => {
    await focus("review-rename", SB, [`${SB}/rename.py`]); await text("rename");
    await row(`${SB}/rename.py`).click(); await page.focus("#tree"); await page.keyboard.press("F2");
    await page.locator(".inline-edit.on input").fill("renamed.py"); await page.keyboard.press("Enter");
    await page.locator('.tab.active .tname:text("renamed.py")').waitFor();
    await edit("after rename"); await page.keyboard.press("ControlOrMeta+s");
    await page.locator(".tab.active:not(.dirty)").waitFor();
    assert.equal(fs.readFileSync(`${SB}/renamed.py`, "utf8"), "rename\nafter rename");
  });
  await test("close after Save retains edits typed during the PUT", async () => {
    await focus("review-close", SB, [`${SB}/close.py`]); await text("close"); await edit("first edit");
    const started = deferred(), release = deferred();
    const slow = async (route) => { if (route.request().method() === "PUT") { started.resolve(); await release.promise; } await route.continue(); };
    await page.route("**/fb/api/file?*", slow);
    await page.click(".tab.active .close"); await page.click('.modal button[data-id="save"]');
    await started.promise; await edit(" later edit"); release.resolve();
    await page.waitForFunction(() => document.querySelector("#toast")?.textContent.includes("Saved close.py"));
    assert.equal(await page.locator(".tab.active.dirty").count(), 1);
    assert.ok((await page.textContent(".cm-content")).includes("later edit"));
    assert.equal(fs.readFileSync(`${SB}/close.py`, "utf8"), "close\nfirst edit");
    await page.unroute("**/fb/api/file?*", slow);
  });
  await test("request-level Trash failure preserves the dirty buffer", async () => {
    await focus("review-trash", SB, [`${SB}/trash.py`]); await text("trash"); await edit(" unsaved");
    const fail = (route) => route.abort(); await page.route("**/fb/api/fs/trash", fail);
    await row(`${SB}/trash.py`).click(); await page.focus("#tree"); await page.keyboard.press("Meta+Backspace");
    await page.click('.modal button[data-id="trash"]');
    await page.waitForFunction(() => document.querySelector("#toast")?.textContent.includes("terminumd not reachable"));
    assert.equal(await page.locator(".tab.active.dirty").count(), 1);
    assert.ok((await page.textContent(".cm-content")).includes("unsaved"));
    assert.ok(fs.existsSync(`${SB}/trash.py`)); await page.unroute("**/fb/api/fs/trash", fail);
  });
  await test("HTML percent filename renders without throwing", async () => {
    await focus("review-percent", SB, [`${SB}/docs/percent.html`]);
    await page.waitForFunction(() => document.querySelector("iframe")?.contentDocument?.querySelector("img")?.naturalWidth === 1);
  });
  await test("Markdown encoded hash/question image filename loads", async () => {
    await focus("review-image", SB, [`${SB}/docs/encoded.md`]);
    await page.waitForFunction(() => document.querySelector(".md-body img")?.naturalWidth === 1);
  });
  await test("End selects the unloaded destination before Delete", async () => {
    await focus("review-end", `${SB}/many`, []); await row(`${SB}/many/f0000.txt`).waitFor();
    await row(`${SB}/many/f0000.txt`).click(); await page.focus("#tree"); await page.keyboard.press("End");
    await row(`${SB}/many/f1204.txt`).waitFor();
    await page.waitForFunction((p) => document.querySelector(`.row[data-p="${p}"]`)?.classList.contains("sel"), `${SB}/many/f1204.txt`);
    await page.keyboard.press("Meta+Backspace");
    assert.ok((await page.textContent(".modal .mtitle")).includes("f1204.txt"));
    await page.click('.modal button[data-id="cancel"]');
  });
  await test("Shift-End pages sequentially and selects all 1205 rows", async () => {
    await page.focus("#tree"); await page.keyboard.press("Home");
    await page.waitForFunction(() => document.querySelector('.row[data-i="0"]')?.classList.contains("sel"));
    await page.keyboard.press("Shift+End");
    // asked from here: a browser may not sign in with the secret, only the panel's credential (AC-09)
    for (const end = Date.now() + 10000; ; await new Promise((r) => setTimeout(r, 100))) {
      const r = await fetch(`${base}/fb/api/workspace?key=${encodeURIComponent(wsKey("review-end"))}`, { headers: { "X-TM-Session": SECRET } });
      if ((await r.json()).selected.length === 1205) break;
      if (Date.now() > end) throw new Error("the selection did not reach 1205 rows in 10 s");
    }
  });
  await test("a late old page cannot rewind a refreshed listing", async () => {
    const host = "paged.example", started = deferred(), release = deferred();
    let generation = 1;
    const listing = async (route) => {
      if (route.request().headers()["x-tm-host"] !== host) return route.continue();
      const offset = Number(new URL(route.request().url()).searchParams.get("offset") ?? 0), gen = generation;
      if (gen === 1 && offset === 1000) { started.resolve(); await release.promise; }
      const total = gen === 1 ? 1205 : 700;
      const entries = Array.from({ length: Math.max(0, Math.min(500, total - offset)) }, (_, i) => ({ n: `f${String(offset + i).padStart(4, "0")}.txt`, k: "f", s: 1 }));
      await route.fulfill({ json: { status: "ready", writable: true, gen, total, entries } });
    };
    await page.route("**/fb/api/ls?*", listing);
    await focus("review-page-order", `${SB}/many`, [], host); await row(`${SB}/many/f0000.txt`).waitFor();
    await page.focus("#tree"); await page.keyboard.press("End"); await started.promise;
    generation = 2;
    await page.evaluate((host) => window.reviewSocket.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({ event: "rescan", data: { host } }) })), host);
    await page.waitForTimeout(150); release.resolve(); await page.waitForTimeout(150);
    await page.keyboard.press("Home"); await row(`${SB}/many/f0000.txt`).waitFor(); await page.keyboard.press("End");
    await row(`${SB}/many/f0699.txt`).waitFor();
    await page.waitForFunction((p) => document.querySelector(`.row[data-p="${p}"]`)?.classList.contains("sel"), `${SB}/many/f0699.txt`);
    assert.equal(await row(`${SB}/many/f1204.txt`).count(), 0);
    await page.unroute("**/fb/api/ls?*", listing);
  });
  await hostRegressions({ page, test, focus, text, edit, deferred, SB });
  check("review regressions have no uncaught page errors", errors.length === 0, errors.join("; "));
} finally {
  await browser.close(); await cleanup();
}
process.exit(results.every(Boolean) ? 0 : 1);
