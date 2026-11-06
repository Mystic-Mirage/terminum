// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// More checks for e2e_panel.mjs: the page's View (`/fb/?view=`), the panel inside a page that
// receives its files as messages (AC-70), a host's files offer (AC 83), the tree's Sorting (AC-52).

import fs from "node:fs";
import { base, check, SB, within } from "./e2e_harness.mjs";

/** AC-70 the View alone: renders the file, no tree, no secret in its URL, a path bar. */
export async function viewerTools(context, base, SB, errors) {
  const vp = await context.newPage();
  await vp.setViewportSize({ width: 1200, height: 800 });
  vp.on("pageerror", (e) => errors.push("viewer: " + e.message));
  await vp.goto(`${base}/fb/?view=${encodeURIComponent(SB + "/README.md")}`);
  await within("AC-70 the View renders the file", (ms) => vp.waitForSelector(".md-body h1", { timeout: ms }));
  check("AC-70 the View has no tree", !(await vp.isVisible("#tree")));
  const barText = () => vp.textContent("#pathbar .path");
  check("AC-70 its path bar shows the full path", (await barText()) === `${SB}/README.md`, await barText());
  check("AC-70 its title is the full path", (await vp.title()) === `${SB}/README.md`, await vp.title());
  await vp.click('.md-body a:text("guide")');
  await within("AC-70 the path bar follows the active tab", (ms) => vp.waitForFunction((p) => document.querySelector("#pathbar .path")?.textContent === p, `${SB}/docs/guide.md`, { timeout: ms }));
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: base });
  await vp.click("#pathbar button");
  await vp.waitForTimeout(200);
  const clip = await vp.evaluate(() => navigator.clipboard.readText());
  check("AC-70 copy puts the full path on the clipboard", clip === `${SB}/docs/guide.md`, clip);
  check("AC-70 copy says so", (await vp.textContent("#toast")) === `Copied ${SB}/docs/guide.md`, await vp.textContent("#toast"));
  await vp.close();
}

/** AC-54 a ```mermaid fence is drawn as its diagram (an image), its source a click away; one
 *  Mermaid cannot draw keeps its source with Mermaid's message; other fences are highlighted. */
export async function diagrams(context, base, SB, errors) {
  const vp = await context.newPage();
  await vp.setViewportSize({ width: 1200, height: 800 });
  vp.on("pageerror", (e) => errors.push("diagram: " + e.message));
  vp.on("dialog", (d) => { errors.push("diagram dialog: " + d.message()); void d.dismiss(); });
  await vp.goto(`${base}/fb/?view=${encodeURIComponent(SB + "/docs/diagram.md")}`);
  await within("AC-54 a mermaid fence is drawn as a diagram", (ms) => vp.waitForSelector(".md-diagram img.diagram", { timeout: ms }));
  const img = await vp.$eval(".md-diagram img.diagram", (i) => ({ svg: i.src.startsWith("data:image/svg+xml;charset=utf-8,"), w: i.naturalWidth, shown: !i.hidden }));
  check("AC-54 the diagram is an SVG image at its size, its source hidden",
    img.svg && img.w > 0 && img.shown && (await vp.$eval(".md-diagram pre", (p) => p.hidden)), JSON.stringify(img));
  await vp.click(".md-diagram-toggle");
  const src = await vp.$eval(".md-diagram", (f) => ({ pre: !f.querySelector("pre").hidden, img: f.querySelector("img").hidden, text: f.querySelector("pre").textContent, button: f.querySelector("button").textContent }));
  check("AC-54 Source shows the fence's text, Diagram goes back", src.pre && src.img && src.text.startsWith("flowchart LR") && src.button === "Diagram", JSON.stringify(src));
  await within("AC-54 a fence Mermaid cannot draw keeps its source with its message", (ms) => vp.waitForSelector(".md-diagram-error + pre", { timeout: ms }));
  check("AC-54 the message is Mermaid's", (await vp.textContent(".md-diagram-error")).startsWith("Mermaid: "), await vp.textContent(".md-diagram-error"));
  check("AC-54 another fence is highlighted", !!(await vp.$('pre[data-lang="rust"] code span[class*="tok-"]')));
  await within("AC-54 a hostile fence (an HTML label, a click handler) is drawn as an image only", (ms) => vp.waitForFunction(() => document.querySelectorAll(".md-diagram img.diagram").length === 2, null, { timeout: ms }));
  await vp.click(".md-diagram:nth-of-type(2) img.diagram").catch(() => {});
  await vp.waitForTimeout(300);
  check("AC-54 nothing in a diagram runs: no dialog, no element of its label in the page", !errors.some((e) => e.startsWith("diagram dialog")) && !(await vp.$(".md-body img[src='x']")), errors.join(" | "));
  await vp.close();
}

/** AC-70 in a page around it (same origin), a file opens in the page's View: the panel posts
 *  `fb-open` (with `window` for ⌘-click) and opens no tab of its own. */
export async function embedTools(context, url, row) {
  const ep = await context.newPage();
  await ep.goto(new URL("/custom.css", url).href);   // any same-origin document will do
  await ep.evaluate((src) => {
    window.__msgs = [];
    addEventListener("message", (e) => window.__msgs.push(e.data));
    document.body.append(Object.assign(document.createElement("iframe"), { src, id: "files", style: "width:500px;height:800px" }));
  }, url);
  const frame = ep.frameLocator("#files");
  await frame.locator(row("README.md")).waitFor();
  await ep.waitForTimeout(500);                       // the pane's saved tabs are restored
  const tabs = await frame.locator(".tab").count();
  await frame.locator(row("README.md")).click();
  await frame.locator(row("README.md")).click({ modifiers: ["Meta"] });
  await ep.waitForTimeout(300);
  const msgs = await ep.evaluate(() => window.__msgs);
  const path = new URL(url).searchParams.get("cwd") + "/README.md";
  check("AC-70 a click asks the page to open the file in its View", msgs.some((m) => m.type === "fb-open" && m.path === path && m.host === null && !m.window), JSON.stringify(msgs));
  check("AC-70 ⌘-click asks for a window of its own", msgs.some((m) => m.type === "fb-open" && m.path === path && m.window === true), JSON.stringify(msgs));
  check("AC-70 … and the embedded panel opens no tab itself", (await frame.locator(".tab").count()) === tabs, `${tabs} tabs before`);
  await ep.close();
}

/** AC 83 a host's pane: Files offers to enable its files; a failure is loud. */
export async function hostOffer(context, url) {
  const hp = await context.newPage();
  await hp.goto(url);
  await within("AC 83 a host's pane offers to browse its files", (ms) => hp.waitForSelector('#offer.on button[data-a="enable"]', { timeout: ms }));
  check("AC 83 … naming the host", (await hp.textContent("#offer b")) === "devbox.example is a remote host.", await hp.textContent("#offer b"));
  check("AC-51 the badge says remote", (await hp.textContent("#mode")) === "remote");
  await within("AC-51 a host that is not connected says so", (ms) => hp.waitForFunction(() => document.getElementById("tree").textContent.includes("devbox.example is not connected"), null, { timeout: ms }));
  await hp.click('#offer button[data-a="enable"]');
  await within("AC 83 Enable on an unknown host fails loud", (ms) => hp.waitForFunction(() => document.getElementById("toast").textContent.includes("no host named devbox.example"), null, { timeout: ms }));
  await hp.click("#tree", { button: "right", position: { x: 40, y: 200 } });
  await hp.waitForSelector(".ctx .mi");
  check("AC 83 the menu can enable it later", !!(await hp.$('.ctx .mi:has-text("Browse Files of devbox.example")')));
  await hp.keyboard.press("Escape");
  await hp.close();
}

/** AC-52 Sorting › Recent First, Alphabetical, By Extension in the context menu, folders first in
 *  each, kept in the prefs; a finger held on a row opens the menu, a tap opens Sorting. */
export async function panelSort(page, { row, H }) {
  const dir = `${SB}/sorted`, now = Date.now() / 1000;
  fs.mkdirSync(`${dir}/adir`, { recursive: true });
  fs.mkdirSync(`${dir}/zdir`);
  for (const [name, ago] of [["b.txt", 10], ["a.md", 300], ["C.rs", 100], ["Makefile", 200], ["adir", 5], ["zdir", 1000]]) {
    if (!name.endsWith("dir")) fs.writeFileSync(`${dir}/${name}`, name);
    fs.utimesSync(`${dir}/${name}`, now - ago, now - ago);
  }
  const order = () => page.$$eval(`#tree .row[data-p^="${dir}/"]`, (rows) =>
    rows.sort((a, b) => a.dataset.i - b.dataset.i).map((r) => r.dataset.p.split("/").pop()).join(" "));
  const shows = async (want) => {                     // true once the rows are in that order, else the order they are in
    for (const end = Date.now() + 5000; Date.now() < end; await page.waitForTimeout(100)) if ((await order()) === want) return true;
    return order();
  };
  await page.waitForSelector(row("sorted"));
  await page.click(row("sorted"));
  const byName = "adir zdir a.md b.txt C.rs Makefile";
  const first = await shows(byName);
  await page.click(row("sorted/a.md"), { button: "right" });
  await page.hover('.ctx .mi:has-text("Sorting")');
  await page.waitForSelector(".ctx + .ctx, .ctx ~ .ctx");
  const sub = await page.$$eval(".ctx", (m) => m.length === 2 && [...m[1].querySelectorAll(".mi")].map((i) => `${i.getAttribute("aria-checked") === "true" ? "✓" : ""}${i.textContent.replace("✓", "")}`).join(", "));
  await page.click('.ctx .mi:has-text("By Extension")');
  const byExt = await shows("adir zdir Makefile a.md C.rs b.txt");
  const saved = await (await fetch(`${base}/fb/api/prefs`, { headers: H() })).json();
  check("AC-52 Sorting › in the context menu: Recent First, Alphabetical (checked), By Extension; By Extension orders by it, folders first, names without one first; kept in the prefs",
    first === true && sub === "Recent First, ✓Alphabetical, By Extension" && byExt === true && saved.sort === "ext", JSON.stringify({ first, sub, byExt, saved }));
  // a finger held on a row: the menu, kept when the finger lifts; a tap on Sorting opens it
  const touch = (type) => page.$eval(row("sorted/b.txt"), (el, type) => {
    const r = el.getBoundingClientRect(), at = { clientX: r.left + 30, clientY: r.top + 5, bubbles: true, pointerType: "touch", isPrimary: true };
    el.dispatchEvent(new PointerEvent(type, at));
    if (type === "pointerup") el.dispatchEvent(new MouseEvent("click", at));
  }, type);
  await touch("pointerdown");
  await page.waitForTimeout(700);
  await touch("pointerup");
  const held = await page.$$eval(".ctx", (m) => m.length);
  await page.click('.ctx .mi:has-text("Sorting")');
  await page.click('.ctx .mi:has-text("Recent First")');
  const recent = await shows("adir zdir b.txt C.rs Makefile a.md");
  const chosen = await page.$eval("#tree .row.sel", (r) => r.dataset.p.split("/").pop()).catch(() => null);
  check("AC-52 a finger held on a row opens the menu (its lift no click), a tap on Sorting its choices; Recent First: folders, then files, newest first",
    held === 1 && recent === true && chosen === "b.txt", JSON.stringify({ held, recent, chosen }));
  await page.click(row("sorted/a.md"), { button: "right" });
  await page.hover('.ctx .mi:has-text("Sorting")');
  await page.keyboard.press("ArrowRight"); // keys in the tree (AC-62)
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  const keyed = await shows(byName);
  check("AC-52 → opens Sorting from the keyboard, ↓ ↩ chooses Alphabetical again", keyed === true, String(keyed));
  await page.click(row("sorted"));
  fs.rmSync(dir, { recursive: true });
}
