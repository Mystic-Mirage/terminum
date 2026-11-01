// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Regression checks against the actual Files panel and a private daemon/tmux (AC-59).
import assert from "node:assert/strict";
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { contrast } from "../../web/static/contrast.js";
import { base, check, cleanup, panelUrl, PANE_B, SB, signIn, wsKey } from "./e2e_harness.mjs";
import { NO_SIGNALS } from "./process-stop.mjs";

export async function panelReview(page) {
  const file = `${SB}/keyboard-cancel.txt`;
  fs.writeFileSync(file, "keep this synthetic file\n");
  const row = page.locator("#tree .row").filter({ has: page.locator('.name:text-is("keyboard-cancel.txt")') });
  await page.click("#refresh");
  await row.waitFor();
  for (const choice of ["cancel", "close"]) {
    await row.click();
    await page.focus("#tree");
    await page.keyboard.press("Meta+Backspace");
    await page.waitForSelector(".modal");
    if (choice === "cancel") {
      const colors = await page.evaluate(() => {
        const root = document.documentElement, before = root.getAttribute("style"), classes = root.className;
        const pairs = [];
        try {
          root.classList.add("themed");
          root.style.setProperty("--t-a1", "#dc322f");
          for (const bg of ["#322925", "#ffffff"]) {
            root.style.setProperty("--t-bg", bg);
            const style = getComputedStyle(document.querySelector(".modal .btn.primary.danger"));
            pairs.push([style.backgroundColor, style.color]);
          }
        } finally {
          root.className = classes;
          if (before === null) root.removeAttribute("style"); else root.setAttribute("style", before);
        }
        return pairs;
      });
      const hex = (rgb) => "#" + rgb.match(/\d+/g).slice(0, 3).map((n) => Number(n).toString(16).padStart(2, "0")).join("");
      const ratios = colors.map(([bg, fg]) => contrast(hex(bg), hex(fg)));
      check("AC-59 destructive confirmation text reaches 4.5:1 in dark and light themes", ratios.every((r) => r >= 4.5), ratios.join(", "));
    }
    if (choice === "cancel") {
      await page.keyboard.press("Tab");
      assert.equal(await page.evaluate(() => document.activeElement?.dataset.id), "cancel");
    } else await page.focus(".modal .mx");
    let requests = 0;
    const sent = (r) => { if (r.url() === `${base}/fb/api/fs/trash`) requests++; };
    page.on("request", sent);
    try {
      await page.keyboard.press("Enter");
      await page.waitForSelector(".modal", { state: "detached" });
      await page.waitForTimeout(150);
      const kept = fs.existsSync(file);
      check(`AC-59 Enter on focused ${choice} cancels Trash and keeps the file`, kept && requests === 0, `file=${kept}, trash requests=${requests}`);
      if (!kept) { fs.writeFileSync(file, "keep this synthetic file\n"); await page.click("#refresh"); await row.waitFor(); }
    } finally { page.off("request", sent); }
  }
  // Backdrop Escape and the primary action retain their standard behavior.
  await row.click(); await page.focus("#tree"); await page.keyboard.press("Meta+Backspace");
  await page.waitForSelector(".modal"); await page.keyboard.press("Escape");
  check("AC-59 Escape still cancels Trash", fs.existsSync(file));
  await row.click(); await page.focus("#tree"); await page.keyboard.press("Meta+Backspace");
  await page.waitForSelector(".modal"); await page.keyboard.press("Enter");
  await page.waitForFunction((p) => !document.querySelector(`#tree .row[data-p="${p}"]`), file);
  check("AC-59 Enter on focused primary still trashes the file", !fs.existsSync(file));
}

/** A restored folder outside the first page needs locate; collapse cancels that response. */
export async function expansionReview(page) {
  const root = `${SB}/src`, late = `${root}/zz-review-late`;
  const folders = [...Array.from({ length: 500 }, (_, i) => `${root}/review-${String(i).padStart(3, "0")}`), late];
  for (const dir of folders) fs.mkdirSync(dir);
  fs.writeFileSync(`${late}/inside.txt`, "synthetic child\n");
  const headers = { "X-TM-Session": await page.evaluate(() => localStorage.getItem("tm.session")) };
  const url = `${base}/fb/api/workspace?${new URLSearchParams({ key: wsKey(PANE_B), cwd: root })}`;
  const read = async () => (await page.request.get(url, { headers })).json();
  const put = async (pane) => { const r = await page.request.put(url, { headers, data: pane }); assert.ok(r.ok(), await r.text()); };
  const before = await read();
  await put({ ...before, root, expanded: [late], selected: [], tabs: [], active_tab: null });
  let release, sawLocate;
  const gate = new Promise((r) => { release = r; }), started = new Promise((r) => { sawLocate = r; });
  const match = (u) => { const q = new URL(u).searchParams; return q.get("path") === root && q.get("locate") === "zz-review-late"; };
  const hold = async (route) => {
    if (match(route.request().url())) { sawLocate(); await gate; }
    await route.continue();
  };
  await page.route("**/fb/api/ls?**", hold);
  try {
    await page.goto(panelUrl(PANE_B, root));
    await Promise.race([started, new Promise((_, reject) => setTimeout(() => reject(Error("no delayed locate request")), 5000))]);
    await page.click("#collapse");
    const answered = page.waitForResponse((r) => match(r.url()));
    release(); await answered;
    await page.waitForTimeout(500);
    await page.evaluate(() => { const el = document.getElementById("tree"); el.scrollTop = el.scrollHeight; });
    const last = page.locator(`#tree .row[data-p="${late}"]`);
    await last.waitFor();
    check("AC-68 a delayed locate after Collapse all leaves its folder closed", await last.locator(".chev.open").count() === 0);
  } finally {
    release(); await page.unroute("**/fb/api/ls?**", hold);
    const current = await read(); await put({ ...before, rev: current.rev, root });
    for (const dir of folders) fs.rmSync(dir, { recursive: true, force: true });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { chromium, webkit } = await import("playwright-core");
  try {
    for (const [name, engine] of Object.entries({ chromium, webkit })) {
      const browser = await engine.launch(NO_SIGNALS);
      try {
        for (const viewport of [{ width: 1400, height: 900 }, { width: 390, height: 844 }]) {
          const page = await browser.newPage({ viewport });
          await signIn(page); await page.goto(panelUrl());
          console.log(`${name} ${viewport.width}×${viewport.height}`);
          await panelReview(page);
          await expansionReview(page);
          await page.close();
        }
      } finally { await browser.close(); }
    }
  } finally { await cleanup(); }
  const { results } = await import("./e2e_harness.mjs");
  assert.ok(results.length && results.every(Boolean), "all private-panel regressions passed");
}
