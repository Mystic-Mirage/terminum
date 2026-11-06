// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-52 checks for e2e_panel.mjs on the whole web app (the real page files, app.js included)
// with the bridge played by a fake WebSocket: the profile bars, reminders of sessions waiting
// for an answer, focus, links with their underline and modifier key, long separators, View
// going back to the terminal, and files dropped on the page (the rest is in e2e_appscreen.mjs).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { check } from "./check.mjs";
import { webAppScreen } from "./e2e_appscreen.mjs";
import { webViewPark } from "./e2e_viewpark.mjs";

const STATIC = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../static");
const ORIGIN = "http://web.test";
const TYPES = { html: "text/html", css: "text/css", svg: "image/svg+xml", js: "text/javascript" };

const item = (id, title, state, profile = "Default") =>
  ({ id, title, sub: ["claude", "~/p"], index: "", host: null, profile, agent: "claude", shell: false, state });
const layout = (asking, also = "done") => [{ kind: "window", label: "Window 1", where: "", host: null, wid: "W1", new: "New tab", pool: "",
  items: [item("S1", "Shown", "working"), item("S2", "Asking", asking), item("S3", "Also asking", also, "Work")] }];
const run = (text, flags = 0) => [text, null, null, flags];
const SCREEN = [
  { r: [run("See https://devbox.example/docs and src/mod.rs:42 or ~/notes.md shots/row.png")], e: true },
  { r: [run("─".repeat(80))], e: true },
  { r: [run("─".repeat(10))], e: true },
  { r: [run("❯ "), run(" ", 256)], e: true },
];
const THEME = { bg: "#1e1e1e", fg: "#d4d4d4", cursor: "#ffffff", cursorText: "#000000", bold: "#ffffff", useBold: false,
  ansi: Array.from({ length: 16 }, () => "#808080"), font: null, size: null, dark: true };

export async function webApp(browser) {
  const page = await browser.newPage({ viewport: { width: 1400, height: 800 }, reducedMotion: "reduce" });   // windows at their place (e2e_motion checks the motion)
  const sent = [];
  let bridge = null;
  await page.addInitScript(() => { window.opened = []; window.open = (u) => { window.opened.push(u); return null; }; });
  await page.route(`${ORIGIN}/**`, (route) => {
    const p = new URL(route.request().url()).pathname;
    if (p === "/auth/check") return route.fulfill({ status: 204, body: "" });
    if (p === "/upload") return route.fulfill({ contentType: "application/json", body: JSON.stringify({ path: "/tmp/up/a b.txt" }) });
    if (p === "/blank.html") return route.fulfill({ contentType: "text/html", body: "<!doctype html><title>blank</title>" });
    if (p === "/fb/api/ls") {                    // the pane's files: src/mod.rs is there, ~/notes.md is not; shots/row.png under the agent's folder
      const q = new URL(route.request().url()).searchParams;
      const there = { "/Users/alex/p/src": ["mod.rs"], "/Users/alex/p/wt/shots": ["row.png"] }[q.get("path")] ?? [];
      const entries = there.filter((n) => !q.get("filter") || n.includes(q.get("filter"))).map((n) => ({ n, k: "f" }));
      return route.fulfill({ contentType: "application/json", body: JSON.stringify({ status: "ready", total: entries.length, writable: true, gen: 1, entries }) });
    }
    const f = path.join(STATIC, p === "/" ? "index.html" : path.normalize(p));
    if (!f.startsWith(STATIC) || !fs.existsSync(f)) return route.fulfill({ status: 404, body: "" });
    let body = fs.readFileSync(f);
    if (p === "/") body = body.toString().replace("__BUILD__", "test");
    return route.fulfill({ contentType: TYPES[f.split(".").pop()] ?? "application/octet-stream", body });
  });
  await page.routeWebSocket(`${ORIGIN.replace("http", "ws")}/ws`, (ws) => {
    bridge = ws;
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m));
      sent.push(msg);
      if (msg.t === "sub") {
        ws.send(JSON.stringify({ t: "theme", sid: msg.id, theme: THEME }));
        ws.send(JSON.stringify({ t: "hist", mode: "reset", sid: msg.id, first: 0, oldest: 0, truncated: false, lines: [] }));
        ws.send(JSON.stringify({ t: "screen", sid: msg.id, full: true, n: SCREEN.length, cols: 80, rows: SCREEN.length, ch: SCREEN.map((l, i) => [i, l]) }));
      }
      // the coding agent works in a worktree under the pane's folder (its hooks said so)
      if (msg.t === "files") ws.send(JSON.stringify({ t: "files", sid: "S1", key: "S1", cwd: "/Users/alex/p", home: "/Users/alex", host: null, agent: "/Users/alex/p/wt" }));
    });
    ws.send(JSON.stringify({ t: "build", id: "test" }));
    ws.send(JSON.stringify({ t: "layout", groups: layout("working") }));
  });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(`${ORIGIN}/`);
  await page.waitForFunction(() => document.querySelectorAll("#screen .ln").length === 4, null, { timeout: 10000 });
  const tell = (m) => bridge.send(JSON.stringify(m));
  const $eval = (fn, arg) => page.evaluate(fn, arg);

  // the profile bars: full for the shown session, breathing in the list while its agent works; the header's stands still
  const bars = await $eval(() => ({ on: [...document.querySelectorAll("#sessions .pbar.on")].map((b) => b.closest(".item").dataset.id),
    all: document.querySelectorAll("#sessions .pbar").length, row: !!document.querySelector('#sessions .item[data-id="S1"] .pbar.busy'),
    header: !!document.querySelector("#curbar .pbar.on") && getComputedStyle(document.querySelector("#curbar .pbar")).animationName === "none",
    dots: document.querySelectorAll("#sessions .pdot, #cursub .pdot").length }));
  check("AC-52 web app: a bar per session, full only for the shown one; the list's breathes while it works, the header's stands still",
    bars.all === 3 && bars.on.join() === "S1" && bars.row && bars.header && !bars.dots, JSON.stringify(bars));
  check("AC-52 web app: on a computer the chat takes the keys when it opens", await $eval(() => document.activeElement?.id === "kbd"));

  // reminders: the bridge sends a layout only when it changes; a session waiting 2.5 s with no
  // layout saying otherwise gets one; [x] and a click end it; it goes 2.5 s after waiting stops
  const cards = () => $eval(() => [...document.querySelectorAll("#asks .ask:not([hidden]) .what")].map((e) => e.textContent));
  const settle = () => page.waitForTimeout(2700);
  tell({ t: "layout", groups: layout("waiting") });
  await page.waitForTimeout(100);
  const soon = await cards();
  await settle();
  const later = await cards();
  check("AC-52 web app: a session that keeps waiting 2.5 s gets a reminder, not one that waited a moment",
    soon.length === 0 && later.join() === "Asking waits for your answer", JSON.stringify({ soon, later }));
  await page.click("#asks .ask .x");
  const closed = await cards();
  tell({ t: "layout", groups: layout("done") });
  await settle();
  tell({ t: "layout", groups: layout("waiting") });
  await settle();
  const back = await cards();
  check("AC-52 web app: a closed reminder stays closed, and comes back when that session asks again",
    closed.length === 0 && back.join() === "Asking waits for your answer", JSON.stringify({ closed, back }));
  tell({ t: "layout", groups: layout("waiting", "waiting") });
  await settle();
  const both = await cards();
  await page.click("#asks .ask .go");
  await page.waitForTimeout(100);
  const opened1 = sent.filter((m) => m.t === "sub").at(-1), left = await cards();
  check("AC-52 web app: a click on a reminder opens that session, and only its reminder goes",
    both.length === 2 && opened1?.id === "S2" && left.join() === "Also asking waits for your answer", JSON.stringify({ both, opened1, left }));
  tell({ t: "layout", groups: layout("waiting", "done") });
  await page.waitForTimeout(100);
  const stays = await cards();
  await settle();
  check("AC-52 web app: a reminder goes 2.5 s after its session stops waiting", stays.length === 1 && (await cards()).length === 0, JSON.stringify(stays));
  // a failed session reminds too (AC-39): "failed" in its words and its mark, and it goes with its failure
  const failing = layout("done", "failed");       // S2 is shown here: another session's
  failing[0].items[2].why = "rate_limit";
  tell({ t: "layout", groups: failing });
  await settle();
  const failedCard = await $eval(() => ({ what: document.querySelector("#asks .ask .what")?.textContent, mark: !!document.querySelector("#asks .ask .state.failed svg") }));
  check("AC-39 a failed session gets a reminder that says so and why, with the failed mark",
    failedCard.what === "Also asking failed: rate_limit" && failedCard.mark, JSON.stringify(failedCard));
  tell({ t: "layout", groups: layout("done") });
  await settle();
  await page.click("#sessions .item[data-id='S1']");
  await page.waitForFunction(() => document.querySelectorAll("#screen .ln").length === 4);
  // a waiting session's ? goes as soon as it is shown, and comes back only when it waits anew (AC-39)
  const asks = () => $eval(() => ({ row: !!document.querySelector('#sessions .item[data-id="S2"] .state.waiting'), head: !!document.querySelector("#curstate .state.waiting") }));
  tell({ t: "layout", groups: layout("done") });                     // its last wait (shown above) is over
  await page.waitForTimeout(100);
  tell({ t: "layout", groups: layout("waiting") });
  await page.waitForTimeout(100);
  const waitAsks = await asks();
  await page.click("#sessions .item[data-id='S2']");
  const shownAsks = await asks();
  tell({ t: "layout", groups: layout("waiting") });
  await page.waitForTimeout(100);
  const stillAsks = await asks();
  await page.click("#sessions .item[data-id='S1']");
  await page.waitForTimeout(100);
  const awayAsks = await asks();
  tell({ t: "layout", groups: layout("done") });
  await page.waitForTimeout(100);
  tell({ t: "layout", groups: layout("waiting") });
  await page.waitForTimeout(100);
  const anewAsks = await asks();
  check("AC-39 web app: a waiting session's ? goes at once when it is shown (row and header), stays gone while it waits, comes back when it waits anew",
    waitAsks.row && !shownAsks.row && !shownAsks.head && !stillAsks.row && !stillAsks.head && !awayAsks.row && anewAsks.row, JSON.stringify({ waitAsks, shownAsks, stillAsks, awayAsks, anewAsks }));
  tell({ t: "layout", groups: layout("done") });
  await page.waitForFunction(() => document.querySelectorAll("#screen .ln").length === 4);

  // links: a faint dotted underline; with a mouse they open with ⌘ (Ctrl elsewhere) only
  await page.waitForFunction(() => CSS.highlights?.get("lnk")?.size === 3, null, { timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(300);
  const marks = await $eval(() => [...(CSS.highlights?.get("lnk") ?? [])].map((r) => {
    const x = new Range(); x.setStart(r.startContainer, r.startOffset); x.setEnd(r.endContainer, r.endOffset); return x.toString();
  }));
  check("AC-52 web app: the addresses and the paths of files that are there are underlined, without changing the lines; a path not there is not; one under the coding agent's folder is (AC-35)",
    JSON.stringify(marks) === JSON.stringify(["https://devbox.example/docs", "src/mod.rs:42", "shots/row.png"]), JSON.stringify(marks));
  const at = await $eval(() => {
    const t = document.querySelector("#screen .ln span").firstChild, r = new Range();
    r.setStart(t, t.data.indexOf("devbox")); r.setEnd(t, t.data.indexOf("devbox") + 6);
    const b = r.getBoundingClientRect();
    return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
  });
  await page.mouse.click(at.x, at.y);
  const plain = await $eval(() => window.opened.length);
  const mod = process.platform === "darwin" ? "Meta" : "Control";
  await page.keyboard.down(mod);
  await page.mouse.click(at.x, at.y);
  await page.keyboard.up(mod);
  const opened = await $eval(() => window.opened);
  check("AC-52 web app: a plain click on an address opens nothing; with the modifier key it opens", plain === 0 && opened.length === 1
    && opened[0].startsWith("https://devbox.example/docs"), JSON.stringify(opened));
  const notes = await $eval(() => {
    const t = document.querySelector("#screen .ln span").firstChild, r = new Range(), i = t.data.indexOf("notes.md");
    r.setStart(t, i); r.setEnd(t, i + 3);
    const b = r.getBoundingClientRect();
    return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
  });
  const fileSrc = await $eval(() => document.getElementById("file").getAttribute("src"));
  await page.keyboard.down(mod);
  await page.mouse.click(notes.x, notes.y);
  await page.keyboard.up(mod);
  await page.waitForTimeout(300);
  check("AC-52 web app: a path whose file is not there opens nothing", await $eval(() => document.getElementById("file").getAttribute("src")) === fileSrc
    && await $eval(() => document.body.dataset.shown) === "term");
  // a path relative to the folder the coding agent works in (a worktree), not the pane's, opens from there (AC-35)
  const shot = await $eval(() => {
    const t = document.querySelector("#screen .ln span").firstChild, r = new Range(), i = t.data.indexOf("row.png");
    r.setStart(t, i); r.setEnd(t, i + 3);
    const b = r.getBoundingClientRect();
    return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
  });
  await page.keyboard.down(mod);
  await page.mouse.click(shot.x, shot.y);
  await page.keyboard.up(mod);
  const shotSrc = await page.waitForFunction(() => decodeURIComponent(document.getElementById("file").getAttribute("src") ?? "").includes("row.png") && document.getElementById("file").getAttribute("src"), null, { timeout: 3000 })
    .then((h) => h.jsonValue().then(decodeURIComponent), () => "not opened");
  check("AC-35 web app: a printed path under the coding agent's folder opens that file in View", shotSrc.includes("/Users/alex/p/wt/shots/row.png"), shotSrc);
  await page.click("#fileclose").catch(() => {});

  const rules = await $eval(() => [...document.querySelectorAll("#screen .ln.rule")].map((l) => [l.classList.contains("full"), Math.round(l.getBoundingClientRect().width)]));
  const termWidth = await $eval(() => { const t = document.getElementById("term"), cs = getComputedStyle(t);   // its text's width, inside the padding
    return t.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight); });
  check("AC-52 web app: a separator half of the pane's width or longer runs across the view; a short one stays text",
    rules[0][0] && !rules[1][0] && rules[0][1] >= termWidth - 2, JSON.stringify({ rules, termWidth }));

  // renaming hands the keys back to the chat
  await page.click("#rename");
  await page.fill("#renamename", "New name");
  await page.keyboard.press("Enter");
  check("AC-52 web app: after renaming, the chat has the keys again", await $eval(() => document.activeElement?.id === "kbd")
    && sent.some((m) => m.t === "rename" && m.name === "New name"));

  // View: closing its last file goes back to the terminal; the button below does too
  let views = 0;                                 // each opening a new address: WebKit may still show the last one
  const openView = () => $eval((n) => {
    const b = document.querySelector('button[data-view="file"]');
    b.disabled = false; b.click();
    document.getElementById("file").src = `/blank.html?${n}`;
  }, ++views);
  const emptied = async () => {
    await page.waitForFunction((n) => document.getElementById("file").contentWindow?.location.search === `?${n}`
      && document.getElementById("file").contentDocument?.readyState === "complete", views);
    await page.frame({ url: new RegExp(`blank\\.html\\?${views}$`) }).evaluate(() => parent.postMessage({ type: "fb-empty", path: "" }, location.origin));
    await page.waitForTimeout(100);
  };
  await openView();
  const wideOpen = await $eval(() => !document.getElementById("filewin").hidden);
  await emptied();
  check("AC-52 web app: closing View's last file closes its window over the terminal", wideOpen
    && await $eval(() => document.getElementById("filewin").hidden && document.querySelector('button[data-view="file"]').disabled));
  await page.setViewportSize({ width: 800, height: 800 });
  await page.waitForFunction(() => !document.body.classList.contains("wide"));   // the page took the new width (its change event comes later)
  await openView();
  const narrowOpen = await $eval(() => document.body.dataset.shown);
  await emptied();
  const afterEmpty = await $eval(() => document.body.dataset.shown);
  await openView();
  await page.click("#backterm");
  check("AC-52 web app: below 1400 px closing View's last file, or its Terminal button, shows the terminal",
    narrowOpen === "file" && afterEmpty === "term" && await $eval(() => document.body.dataset.shown) === "term", JSON.stringify({ narrowOpen, afterEmpty }));


  // files dropped on the page: uploaded, their paths pasted; the page stays
  const before = sent.length;
  await $eval(() => {
    const dt = new DataTransfer();
    dt.items.add(new File(["hello"], "a b.txt", { type: "text/plain" }));
    document.getElementById("term").dispatchEvent(new DragEvent("dragover", { dataTransfer: dt, bubbles: true, cancelable: true }));
    document.getElementById("term").dispatchEvent(new DragEvent("drop", { dataTransfer: dt, bubbles: true, cancelable: true }));
  });
  await page.waitForTimeout(400);
  const typed = sent.slice(before).filter((m) => m.t === "in").map((m) => m.data).join("");
  check("AC-52 web app: a file dropped on the terminal is uploaded and its quoted path pasted, without Enter",
    typed.includes("'/tmp/up/a b.txt'") && !typed.includes("\r") && page.url() === `${ORIGIN}/`, JSON.stringify(typed));
  // rows dragged from Files: their paths put at the input, nothing uploaded; another host's refused (AC-36)
  const dropRows = (data) => $eval((data) => {
    const dt = new DataTransfer();
    dt.setData("application/x-terminum-paths", data);
    for (const t of ["dragover", "drop"]) document.getElementById("term").dispatchEvent(new DragEvent(t, { dataTransfer: dt, bubbles: true, cancelable: true }));
  }, data);
  let mark = sent.length;
  await dropRows(JSON.stringify({ host: null, paths: ["/Users/alex/p/src", "/Users/alex/p/a b.md"] }));
  await page.waitForTimeout(300);
  const put = sent.slice(mark).filter((m) => m.t === "in").map((m) => m.data).join("");
  mark = sent.length;
  await dropRows(JSON.stringify({ host: "devbox", paths: ["/srv/x.md"] }));
  await page.waitForTimeout(300);
  const refusedDrop = { typed: sent.slice(mark).filter((m) => m.t === "in").length, toast: await $eval(() => document.getElementById("toast").textContent) };
  check("AC-36 web app: a folder and a file dragged from Files put their quoted paths at the input, no Enter; another host's are refused with a toast",
    put.includes("/Users/alex/p/src '/Users/alex/p/a b.md'") && !put.includes("\r") && refusedDrop.typed === 0 && refusedDrop.toast.includes("devbox"), JSON.stringify({ put, refusedDrop }));

  await webAppScreen({ page, sent, tell, $eval, layout, item, run });

  check("AC-52 web app: no page errors", !errors.length, errors.join(" | "));
  // View belongs to its session: another session's shows instead, the first one's waits as it was
  tell({ t: "layout", groups: layout("done") });
  await page.setViewportSize({ width: 1400, height: 800 });
  await page.waitForFunction(() => document.body.classList.contains("wide"));
  await page.click("#sessions .item[data-id='S1']");
  await openView();
  await page.waitForFunction((n) => document.getElementById("file").contentWindow?.location.search === `?${n}`, views);
  const mine = await $eval(() => document.getElementById("file").getAttribute("src"));
  await page.click("#sessions .item[data-id='S2']");
  const other = await $eval(() => ({ win: document.getElementById("filewin").hidden, off: document.querySelector('button[data-view="file"]').disabled,
    parked: document.querySelector("#filewin iframe[data-sid='S1']")?.getAttribute("src") ?? null }));
  await page.click("#sessions .item[data-id='S1']");
  const returnedView = await $eval(() => ({ win: document.getElementById("filewin").hidden, src: document.getElementById("file").getAttribute("src"),
    frames: document.querySelectorAll("#filewin iframe").length }));
  check("AC-70 web app: View belongs to its session: another session hides it, coming back shows it as it was",
    other.win && other.off && other.parked === mine && !returnedView.win && returnedView.src === mine && returnedView.frames === 1, JSON.stringify({ mine, other, returnedView }));
  await emptied();                                 // its last file closed: nothing waits for it
  await page.click("#sessions .item[data-id='S2']");
  await page.click("#sessions .item[data-id='S1']");
  const emptyBack = await $eval(() => ({ off: document.querySelector('button[data-view="file"]').disabled, win: document.getElementById("filewin").hidden,
    frames: document.querySelectorAll("#filewin iframe").length }));
  check("AC-70 web app: a View whose last file closed does not come back with its session", emptyBack.off && emptyBack.win && emptyBack.frames === 1,
    JSON.stringify(emptyBack));

  // a closed session's View goes with it, unless it holds unsaved edits
  await openView();
  await page.waitForFunction((n) => document.getElementById("file").contentWindow?.location.search === `?${n}`, views);
  await page.click("#sessions .item[data-id='S2']");
  const [win] = layout("done");
  const withoutS1 = [{ ...win, items: win.items.filter((it) => it.id !== "S1") }];
  const parkedNow = (dirty) => $eval((d) => { const f = document.querySelector("#filewin iframe[data-sid='S1']"); if (f && d !== null) f.contentWindow.fbBusy = () => d; return !!f; }, dirty);
  await parkedNow(true);
  tell({ t: "layout", groups: withoutS1 });
  await page.waitForTimeout(150);
  const keptDirty = await parkedNow(null);
  await parkedNow(false);
  tell({ t: "layout", groups: withoutS1 });
  await page.waitForTimeout(150);
  const goneClean = !(await parkedNow(null));
  check("AC-70 web app: a closed session's waiting View goes with it, but not while it holds unsaved edits", keptDirty && goneClean,
    JSON.stringify({ keptDirty, goneClean }));
  await webViewPark({ page, tell, win });
  await page.close();
}
