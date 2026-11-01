// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Browser end-to-end checks of the Files panel against a private terminumd and a private tmux
// server (e2e_harness.mjs): signs in as `terminum open` does, opens the panel at /fb/ pinned to
// a pane as the page names it, drives it with Playwright (Chromium), and changes files on disk
// behind its back.
//
//   node test/e2e_panel.mjs            (from ui/; needs `npm run build` and terminumd built)
//
// Needs a browser once: npx playwright-core install chromium-headless-shell

import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { slack } from "../../web/test/check.mjs";
import { manyPanels } from "./e2e_sockets.mjs";
import { layoutTools } from "./e2e_layout.mjs";
import { panelReview } from "./e2e_panel_review.mjs";
import { diagrams, embedTools, hostOffer, panelSort, viewerTools } from "./e2e_panel_more.mjs";
import {
  base, bringBack, capture, check, cleanup, clearLine, PANE_A, PANE_B, panelUrl, PORT, restartServer, results, SB, signIn, stopServer, waitFor, within, wsKey,
} from "./e2e_harness.mjs";
import { panelMedia } from "./e2e_panel_media.mjs";
import { panelDrop } from "./e2e_panel_drop.mjs";
import { panelDocs } from "./e2e_panel_docs.mjs";
import { NO_SIGNALS } from "./process-stop.mjs";

// playwright-core is a devDependency; PLAYWRIGHT_CORE may point at another copy
const { chromium } = await import(process.env.PLAYWRIGHT_CORE ?? "playwright-core");

const browser = await chromium.launch(NO_SIGNALS);   // signals end the run through the harness's cleanup
const context = await browser.newContext({ viewport: { width: 520, height: 900 } });
const page = await context.newPage();
const SECRET = await signIn(page).catch(async (e) => { await cleanup(); throw e; });
const H = (extra = {}) => ({ "X-TM-Session": SECRET, ...extra });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
page.on("dialog", (d) => { errors.push("dialog: " + d.message()); void d.dismiss(); });
const toastText = () => page.textContent("#toast");
const outside = [], blocked = [];
const external = (u) => !u.startsWith(`http://127.0.0.1:${PORT}`) && !u.startsWith(`blob:http://127.0.0.1:${PORT}/`) && !/^(data|about):/.test(u);
// a CSP-blocked load still emits "request" and then fails with "csp": only finished ones left the machine
page.on("requestfinished", (r) => { if (external(r.url())) outside.push(r.url()); });
page.on("requestfailed", (r) => { if (external(r.url()) && r.failure()?.errorText === "csp") blocked.push(r.url()); });
await page.goto(panelUrl());
const row = (p) => `#tree .row[data-p="${SB}/${p}"]`;
const focusInput = async () => {
  await page.waitForSelector(".inline-edit.on input");
  await page.waitForFunction(() => document.activeElement?.matches(".inline-edit input"));
};

try {
  await within("AC-51 the tree shows the pane's folder", (ms) => page.waitForSelector(row("app.py"), { timeout: ms }));
  await layoutTools(page);
  await panelReview(page);

  // AC-57 create file (opens in a tab), duplicate name, nested folder
  await page.click("#new-file"); await focusInput();
  await page.keyboard.type("hello.py"); await page.keyboard.press("Enter");
  await within("AC-57 new file created and opened", (ms) => page.waitForSelector('.tab.active .tname:text("hello.py")', { timeout: ms }));
  check("AC-57 file exists and is empty", fs.existsSync(`${SB}/hello.py`) && fs.statSync(`${SB}/hello.py`).size === 0);
  await page.click(row("README.md"));
  await page.click("#new-file"); await focusInput();
  await page.keyboard.type("hello.py"); await page.keyboard.press("Enter");
  await within("AC-57 duplicate name shows inline error", (ms) => page.waitForSelector(".inline-edit.bad", { timeout: ms }));
  check("AC-57 error text", (await page.textContent(".inline-edit .err")).includes("already exists"), await page.textContent(".inline-edit .err"));
  await page.keyboard.press("Escape");
  await page.click("#new-folder"); await focusInput();
  await page.keyboard.type("db/migrations"); await page.keyboard.press("Enter");
  await within("AC-57 nested folder created", (ms) => page.waitForSelector(row("db"), { timeout: ms }));
  check("AC-57 db/migrations on disk", fs.existsSync(`${SB}/db/migrations`));
  // a slow frame (a busy page): Escape, then New Folder at once; the left edit's blur does not cancel the new one
  await page.evaluate(() => { const raf = window.requestAnimationFrame; window.slowRaf = raf; window.requestAnimationFrame = (f) => setTimeout(() => raf(f), 300); });
  await page.click("#new-file"); await focusInput();
  await page.keyboard.press("Escape");
  await page.click("#new-folder");
  await page.waitForTimeout(600);
  const stillOn = await page.evaluate(() => !!document.querySelector(".inline-edit.on") && document.activeElement === document.querySelector(".inline-edit input"));
  await page.evaluate(() => { window.requestAnimationFrame = window.slowRaf; });
  check("AC-57 a new name's field opened right after another was left stays open (a slow frame)", stillOn);
  await page.keyboard.press("Escape");

  // AC-56 edit + save
  await page.click('.tab .tname:text("hello.py")');
  await page.waitForSelector(".cm-content");
  await page.click(".cm-content");
  await page.keyboard.type("import os\n");
  await within("AC-56 dirty marker", (ms) => page.waitForSelector(".tab.dirty", { timeout: ms }));
  await page.keyboard.press("ControlOrMeta+s");
  await within("AC-56 saved (marker cleared)", (ms) => page.waitForSelector(".tab.active:not(.dirty)", { timeout: ms }));
  check("AC-56 content on disk", fs.readFileSync(`${SB}/hello.py`, "utf8") === "import os\n", JSON.stringify(fs.readFileSync(`${SB}/hello.py`, "utf8")));

  // AC-58 rename with F2; the open tab follows
  await page.click(row("hello.py")); await page.focus("#tree");
  await page.keyboard.press("F2"); await focusInput();
  const pre = await page.evaluate(() => { const i = document.querySelector(".inline-edit input"); return i.value.slice(i.selectionStart, i.selectionEnd); });
  check("AC-58 F2 preselects the name without extension", pre === "hello", pre);
  await page.keyboard.type("greet"); await page.keyboard.press("Enter");
  await within("AC-58 tab follows rename", (ms) => page.waitForSelector('.tab .tname:text("greet.py")', { timeout: ms }));
  check("AC-58 renamed on disk", fs.existsSync(`${SB}/greet.py`) && !fs.existsSync(`${SB}/hello.py`));
  await page.click('.tab .tname:text("greet.py")');
  await within("AC-58 renamed tab shows content", (ms) => page.waitForFunction(() => document.querySelector(".cm-content")?.textContent.includes("import os"), null, { timeout: ms }));

  // AC-61 live refresh
  await page.click(row("app.py"));
  await page.waitForFunction(() => document.querySelector(".cm-content")?.textContent.includes("print(1)"));
  fs.appendFileSync(`${SB}/app.py`, "print(2)\n");
  await within("AC-61 clean tab reloads after external edit", (ms) => page.waitForFunction(() => document.querySelector(".cm-content")?.textContent.includes("print(2)"), null, { timeout: ms }));
  fs.writeFileSync(`${SB}/new.rs`, "");
  await within("AC-61 new file appears in tree", (ms) => page.waitForSelector(row("new.rs"), { timeout: ms }));
  fs.rmSync(`${SB}/new.rs`);
  await within("AC-61 deleted file disappears", (ms) => page.waitForSelector(row("new.rs"), { state: "detached", timeout: ms }));

  // AC-56 conflict: dirty tab + external change → banner → save → dialog → overwrite
  await page.click(".cm-content"); await page.keyboard.press("ControlOrMeta+End"); await page.keyboard.type("# mine");
  await page.waitForSelector(".tab.dirty");
  fs.appendFileSync(`${SB}/app.py`, "print(3)\n");
  await within("AC-61 dirty tab shows 'Changed on disk'", (ms) => page.waitForSelector('.banner:has-text("Changed on disk")', { timeout: ms }));
  await page.click(".cm-content"); await page.keyboard.press("ControlOrMeta+s");
  await within("AC-56 conflict dialog", (ms) => page.waitForSelector(".modal", { timeout: ms }));
  check("AC-56 dialog title", (await page.textContent(".modal .mtitle")) === "app.py changed on disk", await page.textContent(".modal .mtitle"));
  const before = fs.readFileSync(`${SB}/app.py`, "utf8");
  check("AC-56 disk untouched until chosen", before.includes("print(3)") && !before.includes("# mine"));
  await page.click(".modal button[data-id=overwrite]");
  await page.waitForSelector(".tab.active:not(.dirty)");
  check("AC-56 overwrite wrote my version", fs.readFileSync(`${SB}/app.py`, "utf8").endsWith("# mine"));

  // AC-56 edits typed while a save is in flight stay unsaved (the PUT is slowed down)
  await page.route("**/fb/api/file?*", async (route) => {
    if (route.request().method() === "PUT") await new Promise((r) => setTimeout(r, 600));
    await route.continue();
  });
  await page.click(".cm-content"); await page.keyboard.press("ControlOrMeta+End"); await page.keyboard.type("\n# a");
  await page.keyboard.press("ControlOrMeta+s");
  await page.waitForTimeout(150);
  await page.keyboard.type("\n# typed during save");
  await page.waitForTimeout(900);
  await page.unroute("**/fb/api/file?*");
  check("AC-56 edits during a save keep the tab dirty", !!(await page.$(".tab.active.dirty")));
  check("AC-56 the save wrote only what was sent", fs.readFileSync(`${SB}/app.py`, "utf8").endsWith("# a"));
  const puts = [], onPut = (r) => { if (r.method() === "PUT") puts.push(r.url().slice(-40)); };
  page.on("request", onPut);
  await page.click(".cm-content"); await page.keyboard.press("ControlOrMeta+s"); // the editor's own key, on Linux too
  await within("AC-56 the next save writes the rest", (ms) => page.waitForSelector(".tab.active:not(.dirty)", { timeout: ms }));
  if (await page.$(".tab.active.dirty")) console.log("     | still unsaved:", JSON.stringify({ puts, disk: fs.readFileSync(`${SB}/app.py`, "utf8").slice(-40), ...await page.evaluate(() => ({ modal: document.querySelector(".modal")?.textContent, banner: document.querySelector(".banner")?.textContent, toast: document.getElementById("toast")?.textContent, focus: `${document.activeElement?.tagName}.${document.activeElement?.className}` })) }));
  page.off("request", onPut);
  // a ⌘S pressed while a save is in flight saves again once it ends (AC-56)
  await page.route("**/fb/api/file?*", async (route) => {
    if (route.request().method() === "PUT") await new Promise((r) => setTimeout(r, 600));
    await route.continue();
  });
  await page.click(".cm-content"); await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type("\n# b"); await page.keyboard.press("ControlOrMeta+s");
  await page.waitForTimeout(150);
  await page.keyboard.type("\n# c"); await page.keyboard.press("ControlOrMeta+s");
  await within("AC-56 a save pressed during a save writes what was typed meanwhile", (ms) => page.waitForSelector(".tab.active:not(.dirty)", { timeout: ms }), 8000);
  await page.unrouteAll({ behavior: "wait" });
  check("AC-56 … all of it on disk", fs.readFileSync(`${SB}/app.py`, "utf8").endsWith("# b\n# c"), JSON.stringify(fs.readFileSync(`${SB}/app.py`, "utf8").slice(-30)));
  await page.waitForTimeout(900); // our own save's fs event must not raise "Changed on disk"
  check("AC-61 own save does not show 'Changed on disk'", !(await page.$('.banner:has-text("Changed on disk")')));

  await panelDocs(page, { row, SECRET, outside, blocked });   // AC-54, AC-67: Markdown and HTML documents

  // AC-36 a row dragged out carries its host and path (the page's terminal puts it at the input); a selected one, the selection
  const dragOf = (sel) => page.$eval(sel, (el) => {
    const dt = new DataTransfer();
    el.querySelector(".name").dispatchEvent(new DragEvent("dragstart", { dataTransfer: dt, bubbles: true, cancelable: true }));
    return { draggable: el.getAttribute("draggable"), paths: dt.getData("application/x-terminum-paths"), text: dt.getData("text/plain") };
  });
  const one = await dragOf(row("app.py"));
  await page.click(row("src")); await page.click(row("app.py"), { modifiers: ["Alt"] });   // ⌥ adds to the selection (⌘ on a file opens a window)
  // both selected and the tree redrawn (src's listing comes after its click): a row drawn anew carries no drag of the old
  await page.waitForFunction(() => document.querySelectorAll("#tree .row.sel").length === 2, null, { timeout: 3000 }).catch(() => {}); await page.waitForTimeout(300);
  const both = await dragOf(row("src"));
  check("AC-36 a tree row is draggable and carries its host and path; a selected row carries the selection, itself first; other places get plain text",
    one.draggable === "true" && one.paths === JSON.stringify({ host: null, paths: [`${SB}/app.py`] }) && one.text === `${SB}/app.py`
      && both.paths === JSON.stringify({ host: null, paths: [`${SB}/src`, `${SB}/app.py`] }) && both.text === `${SB}/src\n${SB}/app.py`, JSON.stringify({ one, both }));

  // AC-64 image tab
  await page.click(row("docs")); await page.click(row("docs/logo.png"));
  await within("AC-64 image tab shows size", (ms) => page.waitForFunction(() => /1 × 1/.test(document.querySelector(".imgmeta")?.textContent ?? ""), null, { timeout: ms }));
  // alone (no page around to show it full screen) a click toggles its own size and back
  await page.click(".imgview img");
  const actual = await page.$eval(".imgview", (v) => [v.classList.contains("actual"), getComputedStyle(v.querySelector("img")).maxWidth]);
  await page.click(".imgview img");
  check("AC-64 alone, a click on the picture shows it at its own size, another fits it again",
    actual[0] && actual[1] === "none" && !(await page.$eval(".imgview", (v) => v.classList.contains("actual"))), JSON.stringify(actual));

  await panelMedia(page, { row });   // AC-64 PDFs, videos, sounds; AC-94 data files
  await panelDrop(page, { row });    // AC-95 files dropped from Finder on the tree
  await panelSort(page, { row, H });  // AC-52 Sorting in the context menu, a finger held too

  // AC-55 per-pane memory: the page shows another pane, then this one again, then reloads
  await page.waitForTimeout(600);                      // the workspace write is debounced
  const expandedBefore = await page.$$eval("#tree .chev.open", (c) => c.length);
  await page.goto(panelUrl(PANE_B, `${SB}/src`));
  await within("AC-51 another pane: the tree is its folder", (ms) => page.waitForSelector(row("src/main.rs"), { timeout: ms }));
  check("AC-55 the other pane starts with no tabs", (await page.$$(".tab")).length === 0);
  for (const [how, go] of [["back", () => page.goto(panelUrl())], ["reload", () => page.reload()]]) {
    await go();
    await within(`AC-55 ${how}: tabs restored`, (ms) => page.waitForSelector('.tab .tname:text("greet.py")', { timeout: ms }));
    await page.waitForTimeout(500);
    check(`AC-55 ${how}: expanded folders restored`, (await page.$$eval("#tree .chev.open", (c) => c.length)) === expandedBefore, `${expandedBefore} open`);
  }
  // AC-51 / OQ-51 a cd: the page names the new folder; the tree re-roots there, the tabs stay
  await page.goto(panelUrl(PANE_A, `${SB}/docs`));
  await within("AC-51 a cd re-roots the tree", (ms) => page.waitForSelector(row("docs/guide.md"), { timeout: ms }));
  check("AC-55 … with nothing expanded, the tabs kept", !(await page.$("#tree .chev.open")) && !!(await page.$('.tab .tname:text("greet.py")')));
  await page.goto(panelUrl());
  await page.waitForSelector(row("src"));

  // AC-66 multi-select + AC-59 trash
  await page.click(row("a.log")); await page.click(row("c.log"), { modifiers: ["Shift"] });
  await page.waitForFunction(() => document.querySelectorAll("#tree .row.sel").length > 1, null, { timeout: 2000 }).catch(() => {});
  const selNames = await page.$$eval("#tree .row.sel", (r) => r.map((x) => x.dataset.p.split("/").pop()));
  check("AC-66 ⇧-click selects a range (a.log, app.py, b.log, c.log)", selNames.join() === "a.log,app.py,b.log,c.log", selNames.join());
  await page.focus("#tree"); await page.keyboard.press("Meta+Backspace");
  await page.waitForSelector(".modal");
  check("AC-59 confirm text", (await page.textContent(".modal .mtitle")) === "Move 4 items to Trash?", await page.textContent(".modal .mtitle"));
  await page.click(".modal button[data-id=trash]");
  await within("AC-59 items leave the tree", (ms) => page.waitForSelector(row("b.log"), { state: "detached", timeout: ms }));
  for (let i = 0; i < 20 && ["a", "b", "c"].some((f) => fs.existsSync(`${SB}/${f}.log`)); i++) await page.waitForTimeout(100);
  check("AC-59 moved off disk (to Trash)", !["a", "b", "c"].some((f) => fs.existsSync(`${SB}/${f}.log`)));

  // AC-63 filter
  await page.fill("#filter", "gree");
  await within("AC-63 filter narrows the tree", (ms) => page.waitForFunction(() => document.querySelectorAll("#tree .row[data-p]").length === 1, null, { timeout: ms }));
  await page.fill("#filter", "");

  // AC-69 file-type icons
  await page.waitForSelector(`${row("greet.py")} svg.ico`);   // app.py went to the Trash above
  const color = async (p) => (await page.waitForSelector(`${row(p)} svg.ico`, { state: "attached", timeout: 3000 })).getAttribute("style");
  check("AC-69 code icon in the code color", (await color("greet.py")) === "color:var(--k-code)", await color("greet.py"));
  check("AC-69 text icon in the doc color", (await color("d.log")) === "color:var(--k-doc)", await color("d.log"));
  check("AC-69 folders use the folder color", (await color("src")) === "color:var(--folder)");
  check("AC-69 tabs carry the file's icon", !!(await page.$(".tab svg.ico")));

  // AC-68 expand all, skipping heavy folders; ⌥→ / ⌥← / ⌥-click; collapse all
  await page.click("#collapse");
  await page.click("#expand");
  await within("AC-68 expand all opens nested folders", (ms) => page.waitForSelector(row("src/a/b"), { timeout: ms }));
  const said = await toastText();
  check("AC-68 heavy folders stay collapsed", !(await page.$(`${row("node_modules")} .chev.open`)) && !(await page.$(`${row(".git")} .chev.open`)));
  check("AC-68 toast names the skipped folders", /^Expanded \d+ folders · skipped 2 \((node_modules, \.git|\.git, node_modules)\)$/.test(said), said);
  await page.waitForTimeout(800);
  const wsNow = await (await fetch(`${base}/fb/api/workspace?key=${encodeURIComponent(wsKey(PANE_A))}`, { headers: H() })).json();
  check("AC-68 expanded folders are saved in the pane's workspace", wsNow.expanded?.includes(`${SB}/src/a/b`), `${wsNow.expanded?.length} saved`);
  await page.click("#collapse");
  await within("AC-68 collapse all", (ms) => page.waitForFunction(() => !document.querySelector("#tree .chev.open"), null, { timeout: ms }));
  await page.click(row("src"));                       // expands src one level and selects it
  await within("AC-68 after collapse all a folder opens one level", (ms) => page.waitForSelector(row("src/a"), { timeout: ms }));
  check("AC-68 … and not its old subfolders", !(await page.$(row("src/a/b"))));
  await page.focus("#tree"); await page.keyboard.press("Alt+ArrowRight");
  await within("AC-68 ⌥→ expands below the folder", (ms) => page.waitForSelector(row("src/a/b"), { timeout: ms }));
  await page.keyboard.press("Alt+ArrowLeft");
  await within("AC-68 ⌥← collapses it all", (ms) => page.waitForSelector(row("src/a"), { state: "detached", timeout: ms }));
  await page.click(`${row("src")} .chev`, { modifiers: ["Alt"] });
  await within("AC-68 ⌥-click on the arrow expands below", (ms) => page.waitForSelector(row("src/a/b"), { timeout: ms }));

  // AC-60 insert a path and cd into the shown tmux pane: typed, never run
  const menuItem = async (target, label) => {
    await page.click(target, { button: "right" });
    await page.click(`.ctx .mi:has-text("${label}")`);
  };
  await page.click(row("greet.py"));
  await menuItem(row("greet.py"), "Insert Path in Terminal");
  await within("AC-60 Insert Path types the relative path into the pane", (ms) => waitFor(() => /\$ greet\.py\s*$/m.test(capture(PANE_A)), ms));
  clearLine(PANE_A);
  await page.click(row("src"));
  await menuItem(row("src"), "Open Terminal Here");
  await within("AC-60 Open Terminal Here types a cd, without Enter", (ms) => waitFor(() => capture(PANE_A).includes(`$ cd ${SB}/src`), ms));
  check("AC-60 … nothing ran (still the prompt line)", !capture(PANE_A).includes("No such file"), capture(PANE_A).trim().split("\n").pop());
  clearLine(PANE_A);

  await viewerTools(context, base, SB, errors);          // AC-70 the page's View (`?view=`)
  await diagrams(context, base, SB, errors);             // AC-54 diagrams and highlighted fences
  await embedTools(context, panelUrl(), row);            // AC-70 files open in the page around

  // AC-70 a new panel starts from the latest split; an open one keeps its own
  const split = (p) => p.evaluate(() => getComputedStyle(document.getElementById("app")).getPropertyValue("--split").trim());
  const mine = await split(page);
  await fetch(base + "/fb/api/prefs", { method: "PUT", headers: H({ "Content-Type": "application/json" }), body: JSON.stringify({ hidden: true, split: 0.3 }) });
  const np = await context.newPage();
  await np.goto(panelUrl());
  await np.waitForSelector(row("src"));
  check("AC-70 a new panel takes the default split", (await split(np)) === "30%");
  check("AC-70 an open panel keeps its own split", (await split(page)) === mine, mine);
  // AC-65 hidden files toggle and the choice is remembered
  await np.click("#hidden");
  await within("AC-65 the eye hides dotfiles", (ms) => np.waitForSelector(row(".git"), { state: "detached", timeout: ms }));
  await np.reload(); await np.waitForSelector(row("src"));
  check("AC-65 … also after a reload", !(await np.$(row(".git"))));
  await np.click("#hidden");
  await within("AC-65 … and shows them again", (ms) => np.waitForSelector(row(".git"), { timeout: ms }));
  await np.close();

  // AC-16 a short restart: an amber dot, no error, and what changed meanwhile shows up
  await page.click("#expand"); await page.waitForSelector(row("src/a/b"));
  await page.waitForTimeout(800);                      // the workspace write is debounced
  const openBefore = await page.$$eval("#tree .chev.open", (c) => c.length);
  let noticed = "";
  const watchNotice = setInterval(() => void page.textContent("#notice.on", { timeout: 50 }).then((t) => { noticed ||= t ?? ""; }).catch(() => {}), 100);
  await restartServer(1200, () => fs.writeFileSync(`${SB}/during-gap.txt`, "x"));
  await within("AC-16 a file created during the gap appears", (ms) => page.waitForSelector(row("during-gap.txt"), { timeout: ms }));
  clearInterval(watchNotice);
  check("AC-16 no 'terminumd not running' for a short gap", !noticed, noticed);
  // AC-71 the pane's state survived the restart (workspaces.json)
  await page.reload();
  await within("AC-71 after a restart the tabs are back", (ms) => page.waitForSelector('.tab .tname:text("greet.py")', { timeout: ms }));
  await page.waitForTimeout(500);
  const openAfter = await page.$$eval("#tree .chev.open", (c) => c.length);
  check("AC-71 … and the expanded folders", openAfter === openBefore && openBefore > 0, `${openAfter} of ${openBefore}`);
  // a save while terminumd is away fails loud and keeps the edit (the tree, all open, scrolled to its row: it draws what shows)
  // the tree is virtual: scrolled down a screen at a time, each drawn before the next (from the top
  // again past the bottom), until the row is drawn
  const reach = async (p) => {
    for (const until = Date.now() + slack(10000); !(await page.$(row(p))) && Date.now() < until;) {
      await page.$eval("#tree", (t) => { t.scrollTop = t.scrollTop + t.clientHeight >= t.scrollHeight - 1 ? 0 : t.scrollTop + t.clientHeight * 0.8; });
      await page.waitForTimeout(50);
    }
  };
  await reach("greet.py");
  await page.click(row("greet.py"));
  await page.waitForSelector(".cm-content");
  await page.click(".cm-content"); await page.keyboard.type("# gap\n");
  await stopServer();
  await page.keyboard.press("ControlOrMeta+s");
  await within("AC-56 a save during the gap says so", (ms) => page.waitForFunction(() => document.getElementById("toast").textContent === "Not saved: server restarting — save again", null, { timeout: ms }));
  check("AC-56 … and the tab stays unsaved", !!(await page.$(".tab.active.dirty")));
  // an upgrade: the unsaved tab holds the reload back, the save lets it happen, in place
  await page.evaluate(() => { window.__before = 1; });
  await bringBack({ TM_BUILD_ID: "e2e-next" });
  await within("AC-16 a dirty panel waits for the save", (ms) => page.waitForFunction(() => document.getElementById("note").textContent.startsWith("Update ready"), null, { timeout: ms }));
  check("AC-16 … without reloading", (await page.evaluate(() => window.__before)) === 1);
  await page.keyboard.press("ControlOrMeta+s");
  await within("AC-16 after the save the panel reloads", (ms) => page.waitForFunction(() => window.__before === undefined, null, { timeout: ms }), 8000);
  await within("AC-16 … with its tabs and tree", (ms) => page.waitForSelector(`${row("src")}`, { timeout: ms }));
  // its tabs come back after its tree
  check("AC-16 … the edited tab is back", await page.waitForSelector('.tab .tname:text("greet.py")', { timeout: slack(5000) }).then(() => true, () => false));
  check("AC-16 the saved text is on disk", fs.readFileSync(`${SB}/greet.py`, "utf8").includes("# gap"));
  await page.evaluate(() => { window.__after = 1; });
  await page.waitForTimeout(2500);
  check("AC-16 one reload per build (a page that still differs does not loop)", (await page.evaluate(() => window.__after)) === 1);
  // what the panel signs its requests with: a credential for terminumd's boot, never the secret
  const creds = new Set();
  page.on("request", (r) => { const c = r.headers()["x-tm-session"]; if (c) creds.add(c); });
  await stopServer(); await bringBack(); await page.reload(); await page.waitForSelector(row("src"));

  // AC 09 another program on the port while terminumd is away: it never sees the sign-in,
  // whether it answers nothing useful or forges the proof; the panel comes back with its edits
  await reach("greet.py");
  // found and clicked in one go: the tree, its folders opening again after the reload, draws its rows
  // anew meanwhile, and a click that waits for the row to hold still can wait out its timeout
  await page.evaluate((sel) => document.querySelector(sel)?.click(), row("greet.py")); await page.waitForSelector(".cm-content");
  await page.click(".cm-content"); await page.keyboard.type("# kept through a squatter\n");
  for (const forge of [false, true]) {
    const seen = [];
    await stopServer();
    const squatter = http.createServer((req, res) => {
      seen.push(`${req.url} ${JSON.stringify(req.headers)}`);
      const body = forge && req.url.startsWith("/api/hello") ? JSON.stringify({ proof: "0".repeat(64) }) : "{}";
      res.writeHead(200, { "Content-Type": "application/json" }); res.end(body);
    });
    await new Promise((r) => squatter.listen(PORT, "127.0.0.1", r));
    await page.waitForTimeout(forge ? 4000 : 2500);
    if (forge) check("AC 09 … the panel says it cannot trust the port", (await page.textContent("#notice")).includes("another program holds its port"), await page.textContent("#notice"));
    await new Promise((r) => { squatter.close(r); squatter.closeAllConnections(); });
    await bringBack();
    check(`AC 09 a program on the port${forge ? " forging the proof" : ""} never gets the sign-in`, seen.length > 0 && !seen.some((x) => x.includes(SECRET)), `${seen.length} requests`);
    // had one of the panel's requests reached it before the panel noticed, it would hold a credential: the next terminumd refuses it
    const old = [...creds];
    const refused = await Promise.all(old.map((c) => fetch(`${base}/fb/api/ls?path=${encodeURIComponent(SB)}`, { headers: { "X-TM-Session": c } }).then((r) => r.status)));
    check("AC 09 … the panel's credentials so far are for a boot that is gone: the new terminumd refuses every one; none is the secret",
      old.length > 0 && refused.every((st) => st === 401) && old.every((c) => c.includes(".") && !c.includes(SECRET)), JSON.stringify({ n: old.length, refused }));
    await within("AC 09 … and the panel is back on terminumd", (ms) => page.waitForSelector("#dot:not(.off):not(.wait)", { timeout: ms }), 8000);
  }
  check("AC 09 … with the unsaved edit", (await page.textContent(".cm-content")).includes("# kept through a squatter"));

  // AC-53 only regular files are read; AC-56 a large text file saves, a body over the limit is 413
  const t0 = Date.now();
  const zero = await fetch(`${base}/fb/api/raw?path=/dev/zero`, { headers: H() });
  const query = await fetch(`${base}/fb/api/raw?path=${encodeURIComponent(`${SB}/README.md`)}&t=${SECRET}`);
  check("AC 09 the sign-in in an address signs nothing in", query.status === 401, String(query.status));
  check("AC-53 /dev/zero is refused at once", zero.status === 400 && Date.now() - t0 < 1000 && (await zero.json()).message === "Not a regular file");
  fs.writeFileSync(`${SB}/big.txt`, "x".repeat(9 << 20));
  const big = await (await fetch(`${base}/fb/api/file?path=${encodeURIComponent(`${SB}/big.txt`)}`, { headers: H() })).json();
  const put = (text) => fetch(`${base}/fb/api/file?path=${encodeURIComponent(`${SB}/big.txt`)}`, { method: "PUT",
    headers: H({ "Content-Type": "application/json", "If-Match": "*" }), body: JSON.stringify({ text }) });
  const saved = await put("y".repeat(9 << 20));
  check("AC-56 a 9 MB text file is editable and saves", big.writable && saved.status === 200, `${saved.status}`);
  // terminumd answers 413 and closes while the body still comes: on a busy machine the reset can
  // beat the answer to this side, so a reset with no answer is asked again (3 times at most)
  let huge = null;
  for (let i = 0; i < 3 && !huge; i++) {
    huge = await putRaw(`/fb/api/file?path=${encodeURIComponent(`${SB}/big.txt`)}`, JSON.stringify({ text: "\u0001".repeat(11 << 20) })).catch(() => null);
  }
  huge ??= { status: "reset 3 times", text: "" };
  check("AC-56 a body over the limit is refused as JSON", huge.status === 413 && !!JSON.parse(huge.text || "null")?.error, `${huge.status}`);
  fs.rmSync(`${SB}/big.txt`);
  const noSignin = await fetch(`${base}/fb/api/ls?path=${encodeURIComponent(SB)}`);
  check("AC 09 the Files API needs the sign-in", noSignin.status === 401);
  const gone = await fetch(`${base}/fb/api/ls?path=${encodeURIComponent(SB)}`, { headers: H({ "X-TM-Host": "nohost.example" }) });
  check("AC 82 an unknown host is never served from local files", gone.status === 404, `${gone.status}`);

  await hostOffer(context, panelUrl(PANE_A, SB, "devbox.example"));   // AC 83
  await manyPanels(browser, row, SECRET);                             // AC-61 many sockets

  // AC 09 a sign-in terminumd does not know: "Signed out", and no retry loop
  const ctx2 = await browser.newContext();
  await ctx2.addInitScript(() => localStorage.setItem("tm.session", "0".repeat(64)));
  const op = await ctx2.newPage();
  let calls = 0;
  op.on("request", () => calls++);
  await op.goto(panelUrl());
  await within("AC 09 an unknown sign-in says 'Signed out'", (ms) => op.waitForSelector('#notice.on:has-text("Signed out: run terminum open")', { timeout: ms }));
  const n = calls; await op.waitForTimeout(2500);
  check("AC 09 … no retry loop", calls - n <= 1, `${calls - n} requests in 2.5 s`);
  await ctx2.close();

  check("no page errors or alerts", errors.length === 0, errors.join("; "));
  await page.screenshot({ path: path.join(os.tmpdir(), "tm-e2e-panel.png") });
} finally {
  await browser.close();
  await cleanup();
}
const failed = results.filter((r) => !r).length;
console.log(failed ? `FAIL (${failed} of ${results.length})` : `PASS (${results.length} checks)`);
process.exit(failed ? 1 : 0);

/** A PUT whose answer may come before the body is sent (a 413): the answer counts, not the
 *  reset of the rest of the upload (fetch reports only the reset). Its own socket, never pooled:
 *  the reset can come after the answer ended, when the request no longer listens to the socket,
 *  and an error nobody listens to ends the run. */
function putRaw(p, body) {
  return new Promise((resolve, reject) => {
    const req = http.request(`${base}${p}`, { method: "PUT", agent: false, headers: H({ "Content-Type": "application/json", "If-Match": "*" }) }, (res) => {
      let text = ""; res.on("data", (d) => (text += d)); res.on("end", () => resolve({ status: res.statusCode, text }));
    });
    req.on("socket", (s) => s.on("error", () => {}));            // the request's own handler says what failed
    req.on("error", (e) => setTimeout(() => reject(e), 1000));   // a reset after the answer is ignored
    req.end(body);
  });
}
