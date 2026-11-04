// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-18, AC-57, AC-91 on a phone: Attach, the bookmarks, the reminders and the split stand in a
// row at the bottom left, under the input, at the height of the keyboard's buttons (find and the
// sent history stay at the top right); their panes open above them; Attach uploads several files
// and types their paths at the cursor; a computer has no Attach. The input's menu leaves Paste out
// when the clipboard is known empty (AC-03). The agent's input box has no
// frame on a phone; Files and View take the terminal's place whole. Alone: node web/test/e2e_all.mjs webPhoneRow.

import { check, runningIn } from "./check.mjs";
import { drawn, group, item, line, openPage, PHONE, WIDE, pressFab } from "./fake.mjs";

const SID = "tmux:local:terminum:%1", DIR = "/Users/alex/api";
const RULE = "─".repeat(60);
const MARKS = [{ id: "R1", sid: SID, key: SID, n: 1, text: "x", note: "check it", title: "api", host: null, dir: DIR, at: 1792000000, remind: true },
  { id: "M1", sid: SID, key: SID, n: 2, text: "y", note: "here", title: "api", host: null, dir: DIR, at: 1792000000 }];
const IDS = ["foldfab", "findfab", "splitfab", "sentfab", "linksfab", "attachfab", "marksfab", "remindfab", "fabs", "kbdbtn"];

async function opened(browser, device) {
  const f = await openPage(browser, { device, layout: [group("main", [item(SID, "api", { agent: "claude", shell: false, sub: ["2.1", "~/api"], dir: DIR })])] });
  await drawn(f.page);
  const rows = [line("⏺ Done."), line(""), line(RULE), { r: [["> fix the ", null, null, 0], [" ", null, null, 256]], e: true }, line(RULE), line("  ? for shortcuts")];
  f.say({ t: "screen", sid: SID, full: true, n: rows.length, cols: 60, rows: rows.length, ch: rows.map((r, i) => [i, r]) });
  f.say({ t: "marks", items: MARKS });
  await f.page.waitForTimeout(400);
  return f;
}
const boxes = (page) => page.evaluate((ids) => Object.fromEntries(ids.map((id) => {
  const el = document.getElementById(id), r = el.getBoundingClientRect();
  return [id, { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), mid: Math.round(r.top + r.height / 2), seen: r.width > 0 && getComputedStyle(el).visibility === "visible" }];
})), IDS);

export async function webPhoneRow(browser, name) {
  runningIn(`${name} 390×844`);
  const f = await opened(browser, PHONE);
  const { page } = f;
  // folded: ⟨…⟩ at the top right where find stood, the keyboard's buttons at the bottom right; the rest out of sight
  const column = ["findfab", "splitfab", "sentfab", "linksfab", "attachfab", "marksfab", "remindfab"];
  const b = await boxes(page);
  check("AC-18 a phone: every floating button but the keyboard's folds under ⟨…⟩ at the terminal's top right",
    b.foldfab.seen && b.foldfab.x > 300 && b.foldfab.y < 200 && column.every((id) => !b[id].seen) && b.fabs.seen && b.kbdbtn.seen, JSON.stringify(b));
  await page.tap("#foldfab");
  await page.waitForTimeout(150);
  const u = await boxes(page);
  check("AC-18 AC-58 a phone: a tap on ⟨…⟩ shows find, the split, the sent history, Attach, the bookmarks and the reminders in a column under it",
    column.every((id, i) => u[id].seen && u[id].x === u.foldfab.x && u[id].y > (i ? u[column[i - 1]].y : u.foldfab.y)), JSON.stringify(u));
  await page.tap("#foldfab");                       // folds again
  await page.waitForTimeout(150);
  const frame = await page.evaluate(() => getComputedStyle(document.querySelector("#screen .blk.inbox")).borderTopWidth);
  check("AC-18 a phone: the agent's input box is a tint, no frame", frame === "0px", frame);

  // the panes open as sheets centered on the screen (AC-18); a press on their shade closes them
  for (const [fab, pane] of [["marksfab", "markpane"], ["remindfab", "remindpane"]]) {
    await pressFab(page, `#${fab}`, "tap");
    await page.waitForTimeout(250);
    const at = await page.evaluate(([pane]) => {
      const p = document.getElementById(pane).getBoundingClientRect();
      return { open: !document.getElementById(pane).hidden, mid: Math.round(p.left - (innerWidth - p.right)), inside: p.left >= 0 && p.right <= innerWidth && p.top >= 0 };
    }, [pane]);
    check(`AC-57 AC-18 a phone: ${fab}'s pane opens centered, on the screen`, at.open && Math.abs(at.mid) <= 1 && at.inside, JSON.stringify(at));
    await page.touchscreen.tap(4, 400);
    await page.waitForTimeout(150);
    check(`AC-18 a phone: a press on the shade closes ${fab}'s pane`, await page.evaluate((pane) => document.getElementById(pane).hidden, pane));
  }

  // Attach: two files uploaded, both paths typed at the cursor, no Enter
  const posted = [];
  await page.route("**/upload?*", (route) => {
    const name = new URL(route.request().url()).searchParams.get("name");
    posted.push(name);
    return route.fulfill({ contentType: "application/json", body: JSON.stringify({ path: `/Users/alex/api/${name}` }) });
  });
  const before = f.sent.length;
  const [chooser] = await Promise.all([page.waitForEvent("filechooser"), pressFab(page, "#attachfab", "tap")]);
  await chooser.setFiles([{ name: "a.png", mimeType: "image/png", buffer: Buffer.from("png") }, { name: "notes b.txt", mimeType: "text/plain", buffer: Buffer.from("hi") }]);
  await page.waitForFunction(() => document.getElementById("toast").hidden, null, { timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(200);
  const typed = f.sent.slice(before).filter((m) => m.t === "in").map((m) => m.data).join("");
  // one per line in the bracketed paste (AC-36): a new line there is no Enter
  check("AC-18 a phone: Attach takes several files, uploads each and types their paths at the cursor, one per line, without Enter",
    chooser.isMultiple() && JSON.stringify(posted) === JSON.stringify(["a.png", "notes b.txt"])
    && typed === "\x1b[200~/Users/alex/api/a.png\r'/Users/alex/api/notes b.txt'\x1b[201~", JSON.stringify({ posted, typed }));

  // the input's menu (a finger held still and lifted; a CDP touch screen) where the page may read the clipboard
  // unasked (a permission given): an empty one has no Paste; a full one's Paste pastes it
  if (name === "chromium") {
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    const cdp = await page.context().newCDPSession(page);
    const k = await page.evaluate(() => { const r = document.getElementById("kbd").getBoundingClientRect(); return { x: r.left + 60, y: r.top + r.height / 2 }; });
    const hold = async () => {
      await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [k] });
      await page.waitForTimeout(700);
      await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
      await page.waitForTimeout(400);                             // the lift's own click is not a choice (rowmenu.js)
      return page.evaluate(() => { const m = document.getElementById("rowmenu"); return m.open ? [...m.querySelectorAll("button[data-action]:not([data-action=x])")].map((b) => b.textContent) : null; });
    };
    await page.evaluate(() => navigator.clipboard.writeText(""));
    const empty = await hold();
    await page.keyboard.press("Escape");
    await page.evaluate(() => navigator.clipboard.writeText("hi there"));
    const from = f.sent.length, full = await hold();
    await page.click("#rowmenu button[data-action=paste]");
    await page.waitForTimeout(200);
    const pasted = f.sent.slice(from).filter((m) => m.t === "in").map((m) => m.data).join("");
    check("AC-03 a phone: the input's menu has no Paste with the clipboard known empty; with it full, Paste pastes it",
      JSON.stringify(empty) === JSON.stringify(["Undo"]) && JSON.stringify(full) === JSON.stringify(["Paste", "Undo"]) && pasted.includes("hi there"),
      JSON.stringify({ empty, full, pasted }));
  }

  // Files and View take the terminal's place whole, from under the tabs to the bottom (the
  // terminal's row kept half the height)
  const fill = async (view) => {
    await page.evaluate((v) => { const b = document.querySelector(`button[data-view="${v}"]`); b.disabled = false; b.click(); }, view);
    await page.waitForTimeout(250);
    return page.evaluate((id) => {
      const r = document.getElementById(id).getBoundingClientRect(), bar = document.getElementById("bar").getBoundingClientRect();
      return { top: Math.round(r.top - bar.bottom), bottom: Math.round(innerHeight - r.bottom), row: document.getElementById("termrow").getBoundingClientRect().height };
    }, view === "files" ? "files" : "filewin");
  };
  const filesAt = await fill("files"), viewAt = await fill("file");
  check("AC-18 a phone: Files and View fill the space under the tabs, none of it left to the terminal",
    [filesAt, viewAt].every((a) => Math.abs(a.top) <= 1 && a.bottom <= 1 && a.row === 0), JSON.stringify({ filesAt, viewAt }));
  await page.evaluate(() => document.querySelector('button[data-view="term"]').click());
  await page.waitForTimeout(250);

  // a selection in the terminal: its Copy, [+] and Ask stand where ⟨…⟩ stood, Copy first; ⟨…⟩ and the column out of sight (AC-37)
  await page.evaluate(() => { const r = document.createRange(); r.selectNodeContents(document.querySelector("#screen .ln")); getSelection().removeAllRanges(); getSelection().addRange(r); });
  await page.waitForTimeout(250);
  const sel = await page.evaluate(() => Object.fromEntries(["foldfab", "markcopy", "markadd", "askfab", "findfab"].map((id) => {
    const e = document.getElementById(id), r = e.getBoundingClientRect();
    return [id, { x: Math.round(r.left), y: Math.round(r.top), seen: !e.hidden && getComputedStyle(e).visibility === "visible" }];
  })));
  check("AC-18 AC-37 a phone: a selection's Copy, [+] and Ask stand in a column where ⟨…⟩ stood, Copy first; ⟨…⟩ and the folded column out of sight",
    sel.markcopy.seen && sel.markadd.seen && sel.askfab.seen && !sel.foldfab.seen && !sel.findfab.seen && sel.markcopy.x === sel.foldfab.x && sel.markcopy.y === sel.foldfab.y
    && sel.markadd.y > sel.markcopy.y && sel.askfab.y > sel.markadd.y && sel.markadd.x === sel.markcopy.x, JSON.stringify(sel));
  const gaps = await page.evaluate(() => { const r = (id) => document.getElementById(id).getBoundingClientRect();
    return [r("markcopy").top - r("term").top, r("markadd").top - r("markcopy").bottom, r("askfab").top - r("markadd").bottom].map(Math.round); });
  check("AC-37 a phone: Copy, [+] and Ask one gap (--sp-3, 12 px) apart, and the same gap above Copy", gaps.every((g) => g === 12), JSON.stringify(gaps));
  await page.evaluate(() => getSelection().removeAllRanges());
  await f.close();

  runningIn(`${name} 1400×900`);
  const w = await opened(browser, WIDE);
  const wb = await boxes(w.page);
  check("AC-18 a computer: no Attach (Upload on the hot keys, a drop); the bookmarks and reminders in the column at the top right",
    !wb.attachfab.seen && wb.marksfab.seen && wb.marksfab.x === wb.findfab.x && wb.remindfab.y > wb.marksfab.y && wb.marksfab.y < 300, JSON.stringify(wb));
  await w.close();
}
