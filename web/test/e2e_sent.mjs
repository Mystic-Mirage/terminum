// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Sent history (AC-90) and reminders (AC-91) against a fake terminumd (fake.mjs), at a wide and
// a phone size: Enter at a shell prompt tells what it sends before the key; the button under the
// split opens the last one as Markdown, ↑ asks for the one before; a reminder up makes the pale
// fuchsia button stand, its count filled, until it is pressed, its pane shows it as Markdown with Resolve; the
// dialog makes a reminder; the session list's strip is fuchsia for a session with one.

import fs from "node:fs";
import path from "node:path";
import { check, runningIn } from "./check.mjs";
import { drawn, group, item, line, openPage, pause, SIZES, pressFab, pressFoot } from "./fake.mjs";

const DIST = path.join(import.meta.dirname, "../../ui/dist");   // the renderers, as terminumd serves them at /fb/
const SID = "tmux:local:terminum:%1";
const API = "/Users/alex/work/api";
const LAYOUT = [group("main", [item(SID, "api", { shell: true, sub: ["zsh", "~/work/api"], dir: API }), item("tmux:local:terminum:%2", "docs", { index: "2", dir: "/Users/alex/work/docs" }),
  item("tmux:local:terminum:%3", "api tests", { index: "3", dir: API })])];
const SCREEN = [line("out 1"), line("reminder text here"), { r: [["alex@mac api % git status", null, null, 0], [" ", null, null, 256]], e: true }];
const REMINDER = { id: "R1", sid: SID, key: SID, n: 1, text: "reminder text here", note: "**check** the retry", title: "api", host: null, at: 1792000000, remind: true, due: true };
const ITEM = { at: 1792000000, text: "fix the **tests**", agent: "claude", title: "api", key: SID, line: 2 };

export async function webSent(browser, name) {
  for (const [size, device] of SIZES) {
    runningIn(`${name} ${size}`);
    const f = await openPage(browser, { device, layout: LAYOUT, screen: SCREEN, hook: (m, say) => {
      if (m.t === "sentget") say({ t: "sentone", sid: SID, n: m.n, total: 2, item: m.n === 0 ? ITEM : { ...ITEM, text: "ls -la", agent: undefined, key: undefined, line: undefined }, host: "local", path: "/Users/alex/work/api" });
      // where a sent message stands (AC-90): "ls -la" and "out 1" are found, the newest is gone
      if (m.t === "locate" && m.text) say(/ls -la|out 1/.test(m.text) ? { t: "located", id: m.id, sid: SID, n: 0, start: 0, size: 3 } : { t: "located", id: m.id, sid: SID, n: null, why: "These lines are no longer in this session's history." });
      if (m.t === "sentmarks") say({ t: "sentmarks", sid: SID, items: [{ n: 0, text: "gone from here", prompt: true }, { n: 0, text: "out 1", prompt: true }], ends: [{ n: 2, text: "alex@mac api % git status" }] });
    } });
    const { page, sent } = f;
    await page.route("**/fb/**", (route) => {
      const file = path.join(DIST, path.normalize(new URL(route.request().url()).pathname.slice(4)));
      return file.startsWith(DIST) && fs.existsSync(file) ? route.fulfill({ contentType: "text/javascript", body: fs.readFileSync(file) }) : route.fulfill({ status: 404, body: "" });
    });
    await drawn(page);
    await pause(page, 300);

    await page.focus("#kbd");
    const n = sent.length;
    await page.keyboard.press("Enter");
    await pause(page, 150);
    const after = sent.slice(n).map((m) => m.t);
    const told = sent.slice(n).find((m) => m.t === "sent");
    check("sent: Enter at a shell prompt tells the command after the prompt and the line it began on, before the key", told?.text === "git status" && told.agent === ""
      && Number.isInteger(told.line) && told.line >= 2 && after.indexOf("sent") < after.indexOf("in"), JSON.stringify(sent.slice(n)));

    const at = await page.evaluate(() => { const r = (id) => document.getElementById(id).getBoundingClientRect(), s = r("sentfab"), p = r("findfab");
      return { below: s.top >= p.bottom, aligned: Math.abs(s.left - p.left) < 1, shown: !document.getElementById("sentfab").hidden }; });
    check("sent: its button stands under find (a phone's split is in the row at the bottom left)", at.shown && at.below && at.aligned, JSON.stringify(at));
    const menu = await page.evaluate(() => { const c = (id) => { const r = document.getElementById(id).getBoundingClientRect(); return r.left + r.width / 2; }; return Math.round(c("menubtn") - c("findfab")); });
    check("the header's ⋯ stands right over the floating buttons' column (its middle on the magnifier's)", Math.abs(menu) <= 1, String(menu));
    await pressFab(page, "#sentfab");
    await page.waitForFunction(() => document.querySelector("#spbody strong")?.textContent === "tests", null, { timeout: 5000 }).catch(() => {});
    const shown = await page.evaluate(() => ({ pos: document.getElementById("sppos").textContent, where: document.getElementById("spwhere").textContent,
      md: document.querySelector("#spbody strong")?.textContent, newer: document.getElementById("spnewer").disabled, inView: (() => { const r = document.getElementById("sentpane").getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth; })() }));
    check("sent: the last one as Markdown, where, 1 of 2; newer is off; the pane fits the view", shown.md === "tests" && shown.pos === "1 of 2"
      && shown.where.replace(/\u200e/g, "") === "/Users/alex/work/api" && shown.newer && shown.inView, JSON.stringify(shown));
    // modal: Tab and Shift+Tab go round its buttons, never to the terminal's input behind it, and a key typed reaches no pane
    const typedFrom = sent.length, inside = [];
    for (const key of [...Array(12).fill("Tab"), ...Array(4).fill("Shift+Tab")]) {
      await page.keyboard.press(key);
      inside.push(await page.evaluate(() => document.getElementById("sentpane").contains(document.activeElement)));
    }
    await page.keyboard.type("x");
    await pause(page, 150);
    check("sent: its pane keeps the keys: Tab and Shift+Tab stay in it, nothing typed reaches the pane behind it",
      inside.every(Boolean) && !sent.slice(typedFrom).some((m) => m.t === "in"), JSON.stringify({ inside, typed: sent.slice(typedFrom).filter((m) => m.t === "in") }));
    await page.click("#spolder");
    await page.waitForFunction(() => document.querySelector("#spbody code")?.textContent.includes("ls -la"), null, { timeout: 5000 }).catch(() => {});
    const older = await page.evaluate(() => ({ code: document.querySelector("#spbody code")?.textContent, pos: document.getElementById("sppos").textContent }));
    check("sent: ↑ the one before; a command as a shell block", older.code?.includes("ls -la") && older.pos === "2 of 2" && sent.some((m) => m.t === "sentget" && m.n === 1), JSON.stringify(older));
    // Jump to message: another pane's (no line) is looked for nearest the end; found, the pane closes
    let k = sent.length;
    await page.click("#spjump");
    await pause(page, 400);
    const asked = sent.slice(k).find((m) => m.t === "locate");
    check("sent: Jump to message asks terminumd for its text (no line: kept without one); found, the pane closes",
      asked?.text === "ls -la" && asked.n === undefined && await page.evaluate(() => document.getElementById("sentpane").hidden), JSON.stringify(asked));
    await pressFab(page, "#sentfab");
    await page.waitForFunction(() => document.querySelector("#spbody strong")?.textContent === "tests", null, { timeout: 5000 }).catch(() => {});
    k = sent.length;
    await page.click("#spjump");
    await page.waitForFunction(() => !document.getElementById("spwhy").hidden && document.getElementById("spwhy").classList.contains("err"), null, { timeout: 3000 }).catch(() => {});
    const gone = await page.evaluate(() => ({ why: document.getElementById("spwhy").textContent, open: !document.getElementById("sentpane").hidden }));
    check("sent: Jump to message of this pane's own goes with its line; not there, the pane says so and stays open",
      sent.slice(k).some((m) => m.t === "locate" && m.n === 2 && m.text === "fix the **tests**") && gone.open && /no longer in this session's history/.test(gone.why), JSON.stringify(gone));
    k = sent.length;
    await page.click("#spinsert");
    await pause(page, 150);
    const inserted = sent.slice(k).filter((m) => m.t === "in").map((m) => m.data).join("");
    check("sent: Insert into chat pastes the message as it was sent, no Enter; the pane closes; its button looks as a reminder's",
      inserted.includes("fix the **tests**") && !inserted.replace(/\x1b\[201~$/, "").endsWith("\r") && await page.evaluate(() => document.getElementById("sentpane").hidden), JSON.stringify(inserted));

    // a reminder up: the button stands with its count filled until pressed, never blinking (the owner); its pane: Markdown, Resolve
    f.say({ t: "marks", items: [REMINDER, { ...REMINDER, id: "B1", remind: false, due: false, note: "a bookmark" }] });
    await pause(page, 300);
    await page.emulateMedia({ reducedMotion: "no-preference" });       // the others run with reduced motion: no blink anywhere
    const up = await page.evaluate(() => { const b = document.getElementById("remindfab"), n = document.getElementById("remindfabn");
      return { shown: !b.hidden, due: b.classList.contains("due"), n: n.textContent, touch: matchMedia("(hover: none)").matches,
        blinks: /remind/.test(getComputedStyle(b).animationName), filled: getComputedStyle(n).backgroundColor !== getComputedStyle(document.body).backgroundColor }; });
    await page.emulateMedia({ reducedMotion: "reduce" });
    check("reminders: one up shows the button with its count (reminders only) filled, blinking nowhere",
      up.shown && up.due && up.n === "1" && !up.blinks && up.filled, JSON.stringify(up));
    const look = await page.evaluate(() => { const g = (id) => document.getElementById(id), r = (id) => g(id).getBoundingClientRect(), cs = (id) => getComputedStyle(g(id));
      return { marks: !g("marksfab").hidden, under: r("remindfab").top > r("marksfab").bottom && Math.abs(r("remindfab").left - r("marksfab").left) < 1,   // a phone's too, in the folded column
        size: r("remindfab").width === r("marksfab").width, shadow: cs("remindfab").boxShadow === cs("marksfab").boxShadow,
        fill: cs("remindfab").backgroundColor === cs("findfab").backgroundColor, pink: cs("remindfab").color !== cs("findfab").color }; });
    check("reminders: the bell stands under the bookmarks' button (a phone's in the folded column too), as large, the same shadow, no glow; the plain buttons' fill, the bell itself pink",
      look.marks && look.under && look.size && look.shadow && look.fill && look.pink, JSON.stringify(look));
    // the marks on the right edge (a wide window): the message sent (white) and, on one line, the bookmark and the reminder (pink over orange)
    await page.waitForFunction(() => document.querySelectorAll("#msgrail .dot").length >= 2, null, { timeout: 3000 }).catch(() => {});
    const strip = await page.evaluate(() => { const g = (id) => document.getElementById(id), r = g("msgrail").getBoundingClientRect(), t = g("term").getBoundingClientRect();
      const bar = g("msgrail").querySelector(".dot"), cs = bar && getComputedStyle(bar, "::before");
      return { shown: !g("msgrail").hidden && r.width > 0, edge: Math.round(r.left - (t.left + g("term").clientLeft + g("term").clientWidth - 18)), bar: cs && [cs.width, cs.height, cs.borderRadius, cs.opacity, cs.scale].join(),
        right: g("markrail").hidden,
        gap: bar && Math.round(bar.getBoundingClientRect().left + bar.offsetWidth / 2 - 2 - (t.left + g("term").clientLeft + g("term").clientWidth - parseFloat(getComputedStyle(g("term")).paddingRight))),
        dots: [...document.querySelectorAll("#msgrail .dot")].map((d) => d.className.replace("dot ", "")), titles: [...document.querySelectorAll("#msgrail .dot")].map((d) => d.title) }; });
    if (size === "1400×900") {
      check("the right edge's marks: a bookmark's rounded bar each, white where a message was sent, one pink where a bookmark and a reminder meet, a pale blue one where the agent's answer ended; no bookmarks' marks of their own; the text stops 6 px before them",
        strip.shown && Math.abs(strip.edge) <= 1 && strip.bar === "4px,14px,2px,1,none" && strip.gap === 6 && strip.right && strip.dots.join() === "sent,remind,end" && /Reminder: \*\*check\*\* the retry/.test(strip.titles[1]) && /Bookmark: a bookmark/.test(strip.titles[1]), JSON.stringify(strip));
      const k = sent.length;
      await page.click("#msgrail .dot.sent");
      await pause(page, 300);
      check("the right edge's marks: a bar of two messages asks for the first, not found there, then the next, near its line, as an agent's prompt",
        JSON.stringify(sent.slice(k).filter((m) => m.t === "locate").map((m) => [m.text, m.n, m.prompt])) === '[["gone from here",0,true],["out 1",0,true]]', JSON.stringify(sent.slice(k)));
      const k3 = sent.length;
      f.say({ t: "endnew", key: "tmux:local:terminum:%2" });           // another pane's answer: nothing to ask
      f.say({ t: "endnew", key: SID });                                // this pane's: its places again
      await pause(page, 600);
      check("the right edge's marks: an answer ended in this pane asks for the places again, one in another pane does not",
        sent.slice(k3).filter((m) => m.t === "sentmarks").length === 1, JSON.stringify(sent.slice(k3).map((m) => m.t)));
      // Settings' Side marks: off hides them (and asks terminumd no more), kept in this browser; on by default
      const onAtFirst = await page.evaluate(() => document.getElementById("sidemarks").checked);
      await page.click("#menubtn");
      await page.click('label:has(#sidemarks)');
      await page.keyboard.press("Escape");
      await pause(page, 200);
      const off = await page.evaluate(() => ({ rail: getComputedStyle(document.getElementById("msgrail")).display, kept: localStorage.getItem("sideMarks") }));
      const k2 = sent.length;
      await page.evaluate(() => document.dispatchEvent(new Event("sidemarks")));   // what would ask terminumd again
      await pause(page, 500);
      const asked = sent.slice(k2).some((m) => m.t === "sentmarks");
      await page.click("#menubtn");
      await page.click('label:has(#sidemarks)');
      await page.keyboard.press("Escape");
      await page.waitForFunction(() => getComputedStyle(document.getElementById("msgrail")).display !== "none", null, { timeout: 3000 }).catch(() => {});
      const back = await page.evaluate(() => getComputedStyle(document.getElementById("msgrail")).display);
      check("Settings' Side marks: on by default; off hides the right edge's marks, is kept in this browser and asks terminumd no more; on brings them back",
        onAtFirst && off.rail === "none" && off.kept === "false" && !asked && back !== "none", JSON.stringify({ onAtFirst, off, asked, back }));
    } else check("the right edge's message marks: none on a phone (the bookmarks' own marks there)", !strip.shown, JSON.stringify(strip));
    await pressFab(page, "#remindfab");
    await page.waitForSelector("#remindpane .mknav li", { timeout: 5000 }).catch(() => {});
    await pause(page, 400);
    const pane = await page.evaluate(() => ({ title: document.querySelector("#remindpane .pwtitle")?.textContent, note: document.querySelector("#remindpane .mknav .note")?.textContent,
      at: document.querySelector("#remindpane .mknav .meta")?.textContent, text: document.querySelector("#remindpane .mkvbody")?.shadowRoot?.querySelector("pre")?.textContent.trim(),
      acts: [...document.querySelectorAll("#remindpane .mkvacts button")].map((b) => b.textContent.trim()), grip: !!document.querySelector("#remindpane .mkgrip"),
      due: document.getElementById("remindfab").classList.contains("due") }));
    check("reminders: pressed, it is marked no more; its pane the bookmarks' navigator (AC-91): the reminder on top, its note without Markdown marks, \"up now\", its text under the line, Show in terminal, Resolve, Quote in chat",
      pane.title === "Reminders of this session" && pane.note === "check the retry" && pane.at === "up now" && pane.text === "reminder text here" && pane.grip
      && pane.acts.join() === "Show in terminal,Resolve,Quote in chat" && !pane.due, JSON.stringify(pane));
    await page.click("#remindpane .mkvdone");
    const early = sent.some((m) => m.t === "remind" && m.id === "R1");
    await pause(page, 5300);
    check("reminders: Resolve tells terminumd after its 5 s of Undo", !early && sent.some((m) => m.t === "remind" && m.id === "R1" && m.action === "resolve"));
    const before = sent.length;
    const looks = await page.evaluate(() => { const cs = (e) => { const s = getComputedStyle(e); return [s.height, s.backgroundColor, s.fontSize, s.borderRadius].join(); };
      return [cs(document.querySelector("#remindpane .mkvacts .tochat")), cs(document.getElementById("spinsert")), cs(document.getElementById("mbsend"))]; });
    check("Quote in chat (a reminder, a bookmark's bar) looks as the sent history's Insert into chat", new Set(looks).size === 1, JSON.stringify(looks));
    await page.click("#remindpane .mkvacts .tochat");
    await pause(page, 100);
    const pasted = sent.slice(before).filter((m) => m.t === "in").map((m) => m.data).join("");
    check("reminders: Quote in chat pastes its note and, under it, its text quoted into the input, no Enter; the pane closes",
      pasted.includes("**check** the retry\r\r> reminder text here") && !/reminder text here\r$/.test(pasted.replace(/\x1b\[201~$/, ""))
      && await page.evaluate(() => document.getElementById("remindpane").hidden), JSON.stringify(pasted));
    if (size === "1400×900") {
      const strip = await page.evaluate((id) => [...document.querySelectorAll(`#sessions .item[data-id="${id}"] .wave`)].map((w) => w.className), SID);
      check("reminders: a wavy orange and a wavy pink line in the session list, side by side", strip.join() === "wave mark,wave remind", strip.join());
      // the dialog: Reminder sends remind
      await page.evaluate(() => { const row = [...document.querySelectorAll("#term .ln")].find((el) => el.textContent.includes("out 1"));
        const r = document.createRange(); r.selectNodeContents(row); getSelection().removeAllRanges(); getSelection().addRange(r); });
      await page.waitForSelector("#markadd:not([hidden])", { timeout: 3000 }).catch(() => {});
      await page.click("#markadd");
      await page.waitForSelector("#markbox[open]");
      await page.click('#markkind [data-kind="reminder"]');
      const title = await page.textContent("#marktitle");
      await page.click('#markbox button[value="save"]');
      await pause(page, 150);
      const mark = sent.find((m) => m.t === "mark" && !m.id);
      check("reminders: the dialog's Reminder makes one", title === "Reminder" && mark?.remind === true, JSON.stringify({ title, mark }));
      f.say({ t: "marked", ref: mark?.ref, id: "N1" });       // the Mac kept it: the dialog closes
      await page.waitForFunction(() => !document.getElementById("markbox").open, null, { timeout: 3000 }).catch(() => {});

      // a session shows only its own (AC-33, AC-91): not another session's of its project, of another
      // project or outside projects; an orphaned one (its session gone) and one of a host out of reach
      // only below the list
      const b = (id, sid, project, note, extra = {}) => ({ id, sid, key: sid, n: 1, text: `${note} text`, note, title: "t", host: null, at: 1792000000, project, ...extra });
      f.say({ t: "projects", items: [{ id: "P", name: "Work", color: 0, host: null, dir: API, members: [SID, "tmux:local:terminum:%3"] },
        { id: "Q", name: "Docs", color: 1, host: null, dir: "/Users/alex/work/docs", members: ["tmux:local:terminum:%2"] }] });
      f.say({ t: "marks", items: [b("P0", SID, "P", "of this session"), b("P1", "tmux:local:terminum:%3", "P", "from the tests session"), b("P3", "tmux:local:terminum:%8", "P", "from a closed session"),
        b("P2", "tmux:local:terminum:%2", undefined, "docs only"),
        b("Q1", "tmux:local:terminum:%2", "Q", "of the docs project"), b("H1", "tmux:devbox:default:%1", undefined, "host one", { host: "devbox" }),
        b("O1", "tmux:local:terminum:%9", "P", "orphan one", { orphaned: 1792000100 }), b("R2", "tmux:local:terminum:%9", undefined, "orphan reminder", { remind: true, orphaned: 1792000100 })] });
      await pause(page, 300);
      await pressFab(page, "#marksfab");
      await pause(page, 200);
      const pane = await page.evaluate(() => [...document.querySelectorAll("#markpane .mkitem")].map((i) => `${i.querySelector(".note").textContent}${i.querySelector(".closed") ? ":" + i.querySelector(".closed").textContent : ""}`));
      const head = await page.textContent("#markpane h2");
      await page.keyboard.press("Escape");
      await pressFoot(page, "#marksbtn");
      await pause(page, 200);
      const list = await page.evaluate(() => [...document.querySelectorAll("#marklist .mkitem")].map((i) => `${i.querySelector(".note").textContent}${i.querySelector(".closed") ? ":" + i.querySelector(".closed").textContent : ""}`));
      const where = await page.evaluate(() => document.querySelector("#marklist .mkitem .meta").textContent);
      await page.click('#marklist .mkkind button:text-is("Reminders")');
      const rem = await page.evaluate(() => [...document.querySelectorAll("#marklist .mkitem .note")].map((n) => n.textContent));
      await page.click("#marklist .mkall");
      await page.waitForFunction(() => document.getElementById("markdoc").open && document.querySelector("#markdocbody h3"), null, { timeout: 5000 }).catch(() => {});
      const docOpen = await page.evaluate(() => ({ open: document.getElementById("markdoc").open, h: [...document.querySelectorAll("#markdocbody h3")].map((h) => h.textContent),
        full: document.getElementById("markdoc").getBoundingClientRect().height > innerHeight * 0.8 }));
      await page.keyboard.press("Escape");                            // the document, then the list's window
      await page.keyboard.press("Escape");
      check("projects: a session shows only its own, none of another session of its project, of another project or outside it",
        pane.join() === "of this session" && head === "Bookmarks and reminders of this session", JSON.stringify({ pane, head }));
      check("projects: the list below the sessions holds them all, each with its project, out of reach or orphaned said; Reminders lists the open reminders",
        list.join() === "of this session,from the tests session,from a closed session:out of reach,docs only,of the docs project,host one:out of reach,orphan one:orphaned"
          && where.startsWith("Work · ") && rem.join() === "orphan reminder", JSON.stringify({ list, where, rem }));
      check("projects: Open all shows them as one document, full screen", docOpen.open && docOpen.h.join() === "orphan reminder" && docOpen.full, JSON.stringify(docOpen));
      // the bell's pane is the shown session's: another session shown, it closes (it stayed, over the other session, at the window's corner)
      f.say({ t: "marks", items: [REMINDER] });
      await pause(page, 300);
      await pressFab(page, "#remindfab");
      await page.waitForSelector("#remindpane:not([hidden])", { timeout: 3000 }).catch(() => {});
      const before = await page.isVisible("#remindpane");
      const shaded = await page.evaluate(() => !document.getElementById("panescrim")?.hidden);
      await page.mouse.click(5, 5);                                   // outside the pane: on its shade
      await pause(page, 200);
      const closed = await page.evaluate(() => document.getElementById("remindpane").hidden && document.getElementById("panescrim").hidden);
      await pressFab(page, "#remindfab");
      await page.waitForSelector("#remindpane:not([hidden])", { timeout: 3000 }).catch(() => {});
      await page.keyboard.press("Escape");                           // the keys are the pane's while it is open
      await pause(page, 200);
      const esc = await page.evaluate(() => document.getElementById("remindpane").hidden);
      await pressFab(page, "#remindfab");
      await page.waitForSelector("#remindpane:not([hidden])", { timeout: 3000 }).catch(() => {});
      f.say({ t: "marks", items: [] });                               // its reminders gone (another session's shown): it closes
      await pause(page, 400);
      check("reminders: the bell's pane is modal: the page shaded behind it, a press outside or Esc closes it, and it closes when its reminders go",
        before && shaded && closed && esc && await page.evaluate(() => document.getElementById("remindpane").hidden), JSON.stringify({ before, shaded, closed, esc }));
    }
    await f.context.close();
  }
  // a split's embedded page shows its shell alone: no floating buttons
  runningIn(`${name} 1400×900`);
  const e = await openPage(browser, { layout: LAYOUT, screen: SCREEN, url: `/?embed=${encodeURIComponent(SID)}` });
  await drawn(e.page);
  e.say({ t: "marks", items: [{ ...REMINDER, sid: SID }] });
  await pause(e.page, 300);
  const fabs = await e.page.evaluate(() => ["findfab", "splitfab", "sentfab", "marksfab", "remindfab"]
    .filter((id) => { const b = document.getElementById(id); return getComputedStyle(b).display !== "none" && b.getBoundingClientRect().width > 0; }));
  check("a split's embedded page shows none of the floating buttons", !fabs.length, fabs.join());
  // a selection there: its Copy and [+] stand at the terminal's right, top down, never over the text at its left (no Ask there)
  await e.page.evaluate(() => {
    const row = [...document.querySelectorAll("#screen .ln")].find((l) => l.textContent.includes("reminder text"));
    const walk = document.createTreeWalker(row, NodeFilter.SHOW_TEXT);
    let n; while ((n = walk.nextNode()) && !n.data.includes("reminder"));
    const r = document.createRange(); r.setStart(n, n.data.indexOf("reminder")); r.setEnd(n, n.data.indexOf("reminder") + 8);
    getSelection().removeAllRanges(); getSelection().addRange(r);
  });
  await pause(e.page, 300);
  const sel = await e.page.evaluate(() => {
    const t = document.getElementById("term").getBoundingClientRect();
    return ["markcopy", "markadd"].map((id) => { const b = document.getElementById(id), r = b.getBoundingClientRect();
      return { id, shown: !b.hidden && r.width > 0, right: Math.round(t.right - r.right), left: Math.round(r.left - t.left), top: Math.round(r.top) }; });
  });
  check("a split's embedded page: a selection's Copy and [+] stand at the terminal's right, one under another, not over its text",
    sel.every((b) => b.shown && b.right >= 8 && b.right <= 80 && b.left > 100) && sel[0].top < sel[1].top, JSON.stringify(sel));
  await e.context.close();
}
