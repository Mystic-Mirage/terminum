// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-52 / AC-04 / AC-05 / AC-39 / AC-07 checks of the web app that e2e_app.mjs runs on its page and
// fake bridge, after its dropped files: a coding agent's input box and its footer behind a button on a
// phone, empty rows below the screen, a full-screen program wider than the view, the arrows on the
// alternate screen, a tab's other panes as children (and their Clone), and a page out of focus or hidden.

import { check, playLook } from "./check.mjs";

/** ctx: e2e_app.mjs's page, the messages it sent, tell (a message from the bridge), $eval, and its
 *  layout, item and run builders. */
export async function webAppScreen({ page, sent, tell, $eval, layout, item, run }) {
  // a coding agent's input box as a panel; its footer behind a button on a phone
  const note = "✔ Update installed · Restart to update";
  const screenOf = (input, below) => [{ r: [run("output")], e: true }, { r: [run("     (ctrl+b to run in background)")], e: true },
    { r: [run(" ".repeat(80 - note.length) + note)], e: true }, { r: [run(" ".repeat(30) + "deep(nested, code)")], e: true },
    { r: [run("━".repeat(40) + " 4.2/4.2 MB 12.3 MB/s eta 0:00:00")], e: true },
    { r: [run([..."─".repeat(48) + " Compromised Node.js packages ─"])], e: true },   // cells, as the bridge sends box drawing
    { r: [run("❯ " + input), run(" ", 256)], e: true }, { r: [run("─".repeat(80))], e: true }, ...below.map((t) => ({ r: [run(t)], e: true }))];
  const STATUS = ["  ~/p Opus 5.5 ctx:55%", "  ⏵⏵ bypass permissions on"];
  const send = (rows) => tell({ t: "screen", sid: "S1", full: true, n: rows.length, cols: 80, rows: rows.length, ch: rows.map((l, i) => [i, l]) });
  // the page draws at its own pace: each frame is waited for until it is on the page
  const shownText = (t) => page.waitForFunction((t) => document.getElementById("screen").textContent.includes(t), t, { timeout: 3000 }).catch(() => {});
  send(screenOf("", STATUS));                                    // idle: its status lines are learnt
  await shownText("ctx:55%");
  send(screenOf("hi", ["  ~/p Opus 5.5 ctx:61%", "  ⏵⏵ bypass permissions on"]));
  await shownText("ctx:61%");
  await page.waitForTimeout(100);
  const shown = (sel) => $eval((q) => [...document.querySelectorAll(q)].map((e) => getComputedStyle(e).display !== "none"), sel);
  const box = await $eval(() => ({ has: document.getElementById("term").classList.contains("hasbox"),
    inbox: [...document.querySelectorAll("#screen .blk.inbox .ln")].map((l) => l.textContent.trim().slice(0, 12)),
    foot: document.querySelectorAll("#screen .blk.foot .ln").length }));
  check("AC-52 web app: an agent's input box, with its rules, is one block and its status lines another",
    box.has && box.inbox.length === 3 && box.inbox[1] === "❯ hi" && box.foot === 2, JSON.stringify(box));
  const kept = await $eval(() => ({ bar: document.querySelectorAll(".ln.labelled").length, deep: [...document.querySelectorAll("#screen .ln")].find((l) => l.textContent.includes("deep(")).classList.contains("ralign") === false }));
  check("AC-52 web app: a progress bar is no titled rule, and a deeply indented line keeps its indent", kept.bar === 1 && kept.deep, JSON.stringify(kept));
  const phone = { foot: await shown("#screen .blk.foot .ln"), fab: await shown("#footfab"), hint: await shown(".ln.keyhint"),
    inbox: await $eval(() => [...document.querySelectorAll(".inbox .ln")].filter((l) => getComputedStyle(l).display !== "none").map((l) => l.textContent.trim())),
    lead: await shown(".ln.ralign .lead") };
  check("AC-52 web app: on a phone the box shows only its input, the footer waits behind a button, key hints and a notice's padding go",
    phone.inbox.join() === "❯ hi" && phone.foot.join() === "false,false" && phone.fab[0] && !phone.hint[0] && !phone.lead[0], JSON.stringify(phone));
  const tall = await $eval(() => { const b = document.querySelector(".blk.inbox"), lh = parseFloat(getComputedStyle(document.getElementById("term")).fontSize) * 1.25;
    return { h: b.clientHeight, min: 2 * lh }; });
  check("AC-52 web app: on a phone the input panel is at least two rows high", tall.h >= tall.min, JSON.stringify(tall));
  // the pane resized (the hot keys opened): Claude Code's redraw leaves the cursor on a status line; it stays behind ⓘ
  const parked = screenOf("hi", ["  ~/p Opus 5.5 ctx:62%", "  ⏵⏵ bypass permissions on"]).map((row) => ({ ...row, r: row.r.filter((c) => c[3] !== 256) }));
  const st0 = parked.findIndex((row) => row.r[0]?.[0].includes("ctx:62%"));
  parked[st0] = { r: [run("  ~/p "), run("O", 256), run("pus 5.5 ctx:62%")], e: true };
  send(parked);
  await shownText("ctx:62%");                                    // this frame drawn (only its numbers differ: a known status line)
  await page.waitForTimeout(100);
  const parkedFoot = { foot: await shown("#screen .blk.foot .ln"), cur: await $eval(() => !!document.querySelector("#screen .blk.inbox .cur")) };
  check("AC-52 web app: on a phone a cursor Claude Code left on its status line (after a resize) is drawn in the box, the status lines stay behind the button",
    parkedFoot.foot.join() === "false,false" && parkedFoot.cur, JSON.stringify(parkedFoot));
  send(screenOf("hi", ["  ~/p Opus 5.5 ctx:61%", "  ⏵⏵ bypass permissions on"]));
  await shownText("ctx:61%");
  // typed into while it works: lines it shows only then (a shell, its tokens) wait behind ⓘ too
  send(screenOf("Ис", ["  ~/p Opus 5.5 ctx:62%", "  ⏵⏵ bypass permissions on · 1 shell", "  756643 tokens"]));
  await shownText("756643");
  await page.waitForTimeout(100);
  const working = await shown("#screen .blk.foot .ln");
  check("AC-52 web app: on a phone the footer Claude Code shows while it works and is typed into waits behind the button too",
    working.join() === "false,false,false", JSON.stringify(working));
  send(screenOf("hi", ["  ~/p Opus 5.5 ctx:61%", "  ⏵⏵ bypass permissions on"]));
  await shownText("ctx:61%");
  await page.click("#footfab");
  check("AC-52 web app: the button shows the footer", (await shown("#screen .blk.foot .ln")).join() === "true,true" && await $eval(() => document.getElementById("footfab").getAttribute("aria-pressed") === "true"));
  await page.click("#footfab");
  send(screenOf("/", ["  /help   Get help with using Claude Code", "  /model  Set the AI model", ...STATUS]));
  await shownText("/model");
  await page.waitForTimeout(100);
  const menu = await shown("#screen .blk.foot .ln");
  check("AC-52 web app: a \"/\" list below the input stays in sight; only the learnt status lines wait behind the button",
    menu.join() === "true,true,false,false", JSON.stringify(menu));
  await page.setViewportSize({ width: 1400, height: 800 });
  check("AC-52 web app: with a mouse on a wide screen the footer stays, with no button", !(await shown("#screen .blk.foot .ln")).includes(false) && !(await shown("#footfab"))[0]);

  // after a clear the prompt stands at the top: the empty rows below it do not fill the view
  const cleared = [{ r: [run("$ "), run(" ", 256)], e: true }, ...Array.from({ length: 30 }, () => ({ r: [], e: true })),
    { r: [["   ", null, 4, 0]], e: true }, ...Array.from({ length: 5 }, () => ({ r: [], e: true }))];   // a coloured row counts as drawn
  send(cleared);
  await page.waitForTimeout(150);
  const spare = await $eval(() => {
    const t = document.getElementById("term"); t.scrollTop = t.scrollHeight;
    const rows = [...document.querySelectorAll("#screen .ln")], cur = document.querySelector("#screen .cur").getBoundingClientRect(), v = t.getBoundingClientRect();
    return { shown: rows.filter((l) => getComputedStyle(l).display !== "none").length, inView: cur.top >= v.top && cur.bottom <= v.bottom };
  });
  check("AC-52 web app: empty rows below the screen's last drawing and the cursor are left out, so the prompt is in sight",
    spare.shown === 32 && spare.inView, JSON.stringify(spare));

  // a new terminal: an empty screen first, then the shell's prompt near its top
  const blank = () => ({ r: [], e: true });
  send([{ r: [run("Last login: today")], e: true }, { r: [run(" ", 256)], e: true }, ...Array.from({ length: 62 }, blank)]);
  await page.waitForTimeout(100);
  tell({ t: "screen", sid: "S1", full: false, n: 64, cols: 80, rows: 64, ch: [[1, blank()], [2, { r: [run("~ via v3.13")], e: true }], [3, { r: [run("at 15:51 ❯ "), run(" ", 256)], e: true }]] });
  await page.waitForTimeout(150);
  const fresh = await $eval(() => {
    const t = document.getElementById("term"), v = t.getBoundingClientRect(), cur = document.querySelector("#screen .cur").getBoundingClientRect();
    return { shown: [...document.querySelectorAll("#screen .ln")].filter((l) => getComputedStyle(l).display !== "none").length, inView: cur.top >= v.top && cur.bottom <= v.bottom };
  });
  check("AC-52 web app: a new terminal shows its prompt, not the empty rows below it, also when the prompt comes later",
    fresh.shown === 4 && fresh.inView, JSON.stringify(fresh));

  // a full-screen program (mc) in a pane wider than the view: its rows stay rows, scaled to the width
  const mc = Array.from({ length: 6 }, (_, i) => ({ r: [run("│" + `file-${i}.json`.padEnd(58) + "│" + "4.0K".padStart(58) + "│", 0)], e: false }));
  const draw = (alt, cols = 120) => tell({ t: "screen", sid: "S1", full: true, n: mc.length, cols, rows: mc.length, alt, ch: mc.map((l, i) => [i, l]) });
  const layoutNow = () => $eval(() => {
    const t = document.getElementById("term"), right = t.getBoundingClientRect().right, rows = [...document.querySelectorAll("#screen .ln")];
    const lh = parseFloat(getComputedStyle(t).lineHeight);
    // a row's text inside the view (re-flowed, the spaces padding it hang past the edge unseen)
    const textRight = (l) => { const r = document.createRange(); let max = 0;
      for (const w = document.createTreeWalker(l, NodeFilter.SHOW_TEXT); w.nextNode();) {
        const n = w.currentNode, i = n.data.trimEnd().length;
        if (i) { r.setStart(n, i - 1); r.setEnd(n, i); max = Math.max(max, r.getBoundingClientRect().right); }
      }
      return max; };
    return { full: t.classList.contains("fullscreen"), oneRow: rows.every((l) => l.getBoundingClientRect().height < lh * 1.5),
      inside: rows.every((l) => textRight(l) <= right + 1) };
  });
  draw(true);
  await page.waitForTimeout(150);
  const full = await layoutNow();
  draw(false);
  await page.waitForTimeout(150);
  const flowing = { ...await layoutNow(), px: await $eval(() => document.getElementById("term").style.fontSize), reflow: await $eval(() => document.getElementById("term").classList.contains("reflow")) };
  check("AC-04 web app: a full-screen program wider than the view keeps its rows, scaled to the width; the normal screen after it re-flows to the view at the chosen size",
    full.full && full.oneRow && full.inside && !flowing.full && !flowing.oneRow && flowing.inside && flowing.px === "" && flowing.reflow, JSON.stringify({ full, flowing }));
  draw(true, 80);                                               // a pane that fits the view: its exact grid (AC-05)
  await page.waitForTimeout(150);
  const narrow = { ...await layoutNow(), px: await $eval(() => document.getElementById("term").style.fontSize) };
  check("AC-05 web app: a full-screen program in a pane that fits the view shows as its grid at the chosen size",
    narrow.full && narrow.oneRow && narrow.px === "", JSON.stringify(narrow));
  // its arrows are application cursor keys (mc takes no key after an ESC [ B); a shell's as before
  const arrows = async () => {
    const from = sent.length;
    await $eval(() => document.getElementById("kbd").focus());
    for (const k of ["ArrowDown", "ArrowLeft", "Shift+ArrowUp"]) await page.keyboard.press(k);
    await page.waitForTimeout(100);
    return sent.slice(from).filter((m) => m.t === "in").map((m) => m.data);
  };
  const inApp = await arrows();
  draw(false, 80);
  await page.waitForTimeout(150);
  const inShell = await arrows();
  check("AC-52 web app: on the alternate screen (mc, vim) the arrows go as ESC O, with a modifier as ESC [ 1;m; off it as ESC [",
    JSON.stringify(inApp) === JSON.stringify(["\x1bOB", "\x1bOD", "\x1b[1;2A"]) && JSON.stringify(inShell) === JSON.stringify(["\x1b[B", "\x1b[D", "\x1b[1;2A"]),
    JSON.stringify({ inApp, inShell }));

  // a tab's other panes (a coding agent's subagents) go under its first one, as children
  const pane = (id, title, n) => ({ ...item(id, title, "working"), index: "3", pane: n });
  tell({ t: "layout", groups: [{ ...layout("done")[0], items: [item("S1", "Shown", "working"), pane("P1", "Orange notes", 1),
    pane("P2", "pragmatic", 2), pane("P3", "general-purpose", 3), item("S4", "Deploy", "done")] }] });
  await page.waitForTimeout(150);
  const tree = await $eval(() => ({
    top: [...document.querySelectorAll("#sessions .grp > ul > li > .item")].map((b) => b.dataset.id),
    kids: [...document.querySelectorAll('#sessions .item[data-id="P1"] ~ .kids .item.child')].map((b) => b.dataset.id),
    kidDrags: document.querySelectorAll("#sessions .kids [data-drag]").length,   // a pane under its window does not move alone
    kidIdx: [...document.querySelectorAll("#sessions .kids .idx")].map((i) => i.textContent).join(""),
    indent: document.querySelector("#sessions .kids .item").getBoundingClientRect().left - document.querySelector('#sessions .item[data-id="P1"]').getBoundingClientRect().left }));
  const shownMarks = await $eval(() => ({ states: [...document.querySelectorAll("#sessions .item")].map((b) => b.querySelector(".state")?.classList[1] ?? "-").join(),
    busy: document.querySelectorAll("#sessions .item .pbar.busy").length, where: document.querySelector('#sessions .item[data-id="P1"] .idx').title }));
  check("AC-39 web app: a working agent's bar pulses, with no hourglass; a done one neither; AC-01: a window's number names its host on hover",
    shownMarks.states === "-,-,-,-,-" && shownMarks.busy === 4 && shownMarks.where === "This machine", JSON.stringify(shownMarks));
  await page.fill("#filter", "pragmatic");
  const flat = await $eval(() => [...document.querySelectorAll("#sessions .item")].map((b) => b.dataset.id + (b.classList.contains("child") ? "*" : "")));
  await page.fill("#filter", "");
  check("AC-52 web app: a tab's other panes go under its first one, shifted in, with no tab number, not draggable alone; a filter shows them flat",
    tree.top.join() === "S1,P1,S4" && tree.kids.join() === "P2,P3" && !tree.kidDrags && !tree.kidIdx && tree.indent > 10 && flat.join() === "P2",
    JSON.stringify({ tree, flat }));
  // the panes under it have no Clone: the pointer over one shows none, over the window's own row its
  // Clone shows on that row's middle (it stood on the block's middle, among the panes)
  const cloneAt = async (id) => {
    const r = await page.$(`#sessions .item[data-id="${id}"]`).then((b) => b.boundingBox());
    await page.mouse.move(r.x + 40, r.y + r.height / 2);
    await page.waitForTimeout(50);
    return $eval(() => { const li = document.querySelector('#sessions .item[data-id="P1"]').parentElement, c = li.querySelector(":scope > .clone"), b = c?.getBoundingClientRect(), i = li.querySelector(":scope > .item").getBoundingClientRect();
      return c && { shown: getComputedStyle(c).opacity, mid: Math.round(b.top + b.height / 2 - (i.top + i.height / 2)), kids: li.querySelectorAll(".kids .clone").length }; });
  };
  const [overKid, overOwn] = [await cloneAt("P2"), await cloneAt("P1")];
  check("AC-07 web app: a pane under its window has no Clone, the pointer over it shows the window's none; over the window's row its Clone is on that row's middle",
    overKid?.shown === "0" && overKid.kids === 0 && overOwn?.shown !== "0" && Math.abs(overOwn.mid) <= 1, JSON.stringify({ overKid, overOwn }));

  // on a computer a page out of focus is not the active page but keeps its stream; only a hidden
  // one pauses it; the Files frame's focus is the page's
  const flow = async (focused, ev, hidden = false) => {
    const before = sent.length;
    await $eval(playLook, [focused, hidden, ev]);
    await $eval(() => new Promise((r) => setTimeout(r, 0)));   // after the page's own deferred look (its blur waits a turn)
    await page.waitForTimeout(50);
    return sent.slice(before).filter((m) => m.t === "pause" || m.t === "resume").map((m) => m.t === "pause" ? (m.hidden ? "hidden" : "away") : m.t).join();
  };
  const away = await flow(false, "blur"), inFrame = await flow(true, "blur"), still = await flow(true, "focus");
  const again = await flow(false, "blur"), hid = await flow(false, "visibilitychange", true), back = await flow(false, "visibilitychange");
  const returned = await flow(true, "focus");
  check("AC-37 web app: on a computer a page out of focus says it is away (its stream goes on), a hidden one pauses it, focus resumes; focus in the Files frame is the page's",
    away === "away" && inFrame === "resume" && still === "" && again === "away" && hid === "hidden" && back === "away" && returned === "resume",
    JSON.stringify({ away, inFrame, still, again, hid, back, returned }));
}
