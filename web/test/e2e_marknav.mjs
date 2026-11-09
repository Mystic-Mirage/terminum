// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-33 and AC-98 for e2e_all.mjs: the bookmarks' pane as a navigator (its own by line, an agent's
// mark, a bell; the chosen one's content beside the list, ↑ ↓ to choose, Enter shows it in the
// terminal, Done and Delete with Undo for 5 s, the bar's Done, Quote in chat), and an agent's
// record: found where its call was printed (by its title), its text as Markdown.
import { check } from "./check.mjs";
import { LAYOUT, open } from "./e2e_widgets.mjs";
import { pressFab } from "./fake.mjs";

const line = (text) => ({ r: [[text, null, null, 0]], e: true });
const HIST = Array.from({ length: 400 }, (_, i) => line(`out ${i} lorem ipsum`));
HIST[300] = line('⏺ terminum - add_notes_for_user (MCP)(notes: [{"title": "Final report: retry loop fixed", "text": "…"}])');
const at0 = 1790000000;
const ITEMS = [
  { id: "A", sid: "S1", n: 200, text: "out 200 lorem ipsum", note: "second by line\nwith a **second** line", title: "One", at: at0 + 9 },
  { id: "B", sid: "S1", n: 100, text: "out 100 lorem ipsum", note: "first by line", title: "One", at: at0 + 1 },
  { id: "C", sid: "S1", n: 150, text: "out 150 lorem ipsum", note: "check the deploy", title: "One", at: at0 + 2, remind: true, due: true },
  { id: "D", sid: "S1", n: 160, text: "out 160 lorem ipsum", note: "settled", title: "One", at: at0 + 3, remind: true, resolved: at0 + 4 },
  { id: "R", sid: "S1", n: -1, note: "Final report: retry loop fixed", text: "**Fixed** the retry loop.\n\n- `src/retry.rs`\n- next: add a test",
    title: "One", at: at0 + 5, by: "claude", kind: "report" },
  { id: "Q", sid: "S1", n: -1, note: "Decide the API name", text: "fetch_all or list_all? See [the PR](https://example.com/pr/1).", title: "One", at: at0 + 6, by: "codex", remind: true },
];
const mod = process.platform === "darwin" ? "Meta" : "Control";

export async function webMarkNav(browser, name) {
  const at = `[${name}]`;
  // 900 px tall: half the pane holds the 5 entries of three lines each (where and when, title, text)
  const f = await open(browser, { history: HIST, first: 0, screen: ["done"], device: { viewport: { width: 1200, height: 900 } }, hook: (m, say) => {
    if (m.t !== "locate") return;
    const n = { A: 200, B: 100, C: 150, R: 300 }[m.id];
    say(n === undefined ? { t: "located", id: m.id, sid: "S1", n: null, why: "These lines are no longer in this session's history." }
      : { t: "located", id: m.id, sid: "S1", n, start: 0, size: m.id === "R" ? 26 : 12 });
  } });
  const { page, sent, say } = f;
  say({ t: "marks", items: ITEMS, error: null });
  await page.waitForFunction(() => !document.getElementById("marksfab").hidden, null, { timeout: 5000 }).catch(() => {});
  const fab = await page.evaluate(() => document.getElementById("marksfabn").textContent);
  // a reminder up (C) is said at the terminal's top for 4 s, once; another one up is said again (AC-91)
  const toast = () => page.evaluate(() => ({ text: document.getElementById("toast").textContent, shown: !document.getElementById("toast").hidden }));
  const said = await toast();
  await page.waitForTimeout(4300);
  const gone = await toast();
  say({ t: "marks", items: ITEMS.map((i) => ({ ...i })), error: null });
  await page.waitForTimeout(300);
  const repeat = await toast();
  say({ t: "marks", items: [...ITEMS, { id: "E", sid: "S1", n: 120, text: "out 120 lorem ipsum", note: "new up", title: "One", at: at0 + 10, remind: true, due: true }], error: null });
  await page.waitForTimeout(300);
  const fresh = await toast();
  say({ t: "marks", items: ITEMS, error: null });
  await page.waitForTimeout(300);
  check(`AC-91 ${at} reminders up are said at the terminal's top ("<where>: 2 reminders, 1 new") for 4 s, once each; a new one up is said again`,
    said.shown && /^.+: 2 reminders, 1 new$/.test(said.text) && !gone.shown && !repeat.shown && fresh.shown && /: 3 reminders, 2 new$/.test(fresh.text), JSON.stringify({ said, gone, repeat, fresh }));
  await page.click("#marksfab");
  await page.waitForSelector("#markpane .mknav li");
  const list = () => page.evaluate(() => [...document.querySelectorAll("#markpane .mknav li")].map((l) => ({ id: l.dataset.id, cls: l.className,
    agent: !!l.querySelector(".note .agmark img"), bell: !!l.querySelector(".note .mkbell"), title: l.querySelector(".note").textContent })));
  const rows = await list();
  check(`AC-33 ${at} the bookmarks' pane lists the session's bookmarks and open reminders together, by line, an agent's (not found yet) after by time; the button counts them`,
    rows.map((r) => r.id).join() === "B,C,A,R,Q" && fab === "5", JSON.stringify({ rows, fab }));
  const places = await page.evaluate(() => [...document.querySelectorAll("#markpane .mknav li")].map((l) => {
    const pos = l.querySelector(".mkpos"), meta = l.querySelector(".meta"), item = l.querySelector(".mkitem"), p = pos.getBoundingClientRect(), m = meta.getBoundingClientRect(), b = item.getBoundingClientRect();
    return { text: pos.textContent, right: Math.round(b.right - parseFloat(getComputedStyle(item).paddingRight) - p.right), line: Math.abs(p.bottom - m.bottom) < 2,
      type: getComputedStyle(pos).fontSize === getComputedStyle(meta).fontSize && getComputedStyle(pos).fontFamily === getComputedStyle(meta).fontFamily };
  }));
  check(`AC-33 ${at} each entry says its place and their count ("1 / 5") at the right of its where-and-when line, in its type`,
    places.map((p) => p.text).join() === "1 / 5,2 / 5,3 / 5,4 / 5,5 / 5" && places.every((p) => Math.abs(p.right) <= 1 && p.line && p.type), JSON.stringify(places));
  check(`AC-98 ${at} an agent's record has its agent's mark, a reminder a bell, a resolved reminder is not listed`,
    rows.find((r) => r.id === "R").agent && rows.find((r) => r.id === "Q").agent && rows.find((r) => r.id === "C").bell && !rows.some((r) => r.id === "D"), JSON.stringify(rows));
  const focus = await page.evaluate(() => document.activeElement?.id);
  check(`AC-33 ${at} on a computer the open pane has the keys`, focus === "markpane", String(focus));

  // the navigator: the first one shown at once; a press or ↓ shows another, nothing jumps; an agent's record as Markdown
  const view = () => page.evaluate(() => {
    const body = document.querySelector("#markpane .mkvbody")?.shadowRoot?.querySelector(".body");
    return { open: !document.getElementById("markpane").hidden, chosen: document.querySelector("#markpane li.chosen")?.dataset.id,
      title: document.querySelector("#markpane .mkvtitle")?.textContent, pre: body?.querySelector("pre")?.textContent.trim() ?? null,
      strong: body?.querySelector("strong")?.textContent ?? null, items: body ? body.querySelectorAll("li").length : 0,
      acts: [...document.querySelectorAll("#markpane .mkvacts button")].map((b) => b.textContent.trim()),
      // Show in terminal at the actions' left edge; Resolve and Quote in chat together at their right edge
      // the first at the left, the others together at the right (Convert to task between Resolve and Quote in chat)
      place: (() => { const box = document.querySelector("#markpane .mkvacts"), keys = box ? [...box.querySelectorAll("button")].map((b) => b.getBoundingClientRect()) : [];
        const [go, done] = keys, chat = keys.at(-1);
        if (keys.length < 3) return {}; const pad = parseFloat(getComputedStyle(box).paddingLeft), r = box.getBoundingClientRect();
        return { left: Math.abs(go.left - r.left - pad) < 1.5, right: Math.abs(r.right - parseFloat(getComputedStyle(box).paddingRight) - chat.right) < 1.5 && keys.slice(2).every((k, i) => k.left - keys[i + 1].right < 16) && done.left - go.right > 16 }; })(),
      wrap: body?.querySelector("pre") ? getComputedStyle(body.querySelector("pre")).whiteSpace : null,
      // the title, the list's titles and the record's text (its code too) in one size: the window's
      sizes: [document.querySelector("#markpane .mkvtitle"), document.querySelector("#markpane .mknav .note"), body?.querySelector("pre code")].map((e) => (e ? getComputedStyle(e).fontSize : null)),
      tall: Math.round(document.getElementById("term").getBoundingClientRect().height - document.getElementById("markpane").getBoundingClientRect().height) };
  });
  let from = sent.length;
  await page.waitForTimeout(400);
  const shown = await view();
  await page.click('#markpane li[data-id="A"] .mkitem');
  await page.keyboard.press("ArrowDown");
  await page.waitForTimeout(500);
  const next = await view();
  check(`AC-33 ${at} the pane, as tall as the terminal less 28 px, shows the first one's content at once beside the list (a bookmark: the terminal's text exactly, its long lines wrapped, in the title's and the list's size); a press or ↓ shows another, an agent's record as Markdown; nothing jumps; three actions`,
    shown.open && shown.chosen === "B" && shown.title === "first by line" && shown.pre === "out 100 lorem ipsum" && next.chosen === "R" && next.title.includes("Final report: retry loop fixed")
    && next.strong === "Fixed" && next.items === 2 && !sent.slice(from).some((m) => m.t === "locate") && shown.acts.join() === "Show in terminal,Resolve,Convert to task,Quote in chat"
    && shown.place.left && shown.place.right
    && shown.wrap === "pre-wrap" && shown.tall === 28 && shown.sizes.every((z) => z === shown.sizes[0]), JSON.stringify({ shown, next }));

  // the list on top, each its where and when on a line over its title (12 px: the list's meta and 1 px) and its text's start on one line; the view under it past the line
  const layout = () => page.evaluate(() => {
    const r = (s) => document.querySelector(s).getBoundingClientRect(), [list, grip, view] = ["#markpane .mknav", "#markpane .mkgrip", "#markpane .mkview"].map(r);
    const row = (id) => { const li = document.querySelector(`#markpane li[data-id="${id}"]`), note = li.querySelector(".note").getBoundingClientRect(), meta = li.querySelector(".meta").getBoundingClientRect(), snip = li.querySelector(".snip");
      return { snip: snip?.textContent ?? null, oneLine: snip ? snip.getBoundingClientRect().height < 24 : null, metaAbove: meta.bottom <= note.top + 1 && Math.abs(meta.left - note.left) < 1, metaSize: parseFloat(getComputedStyle(li.querySelector(".meta")).fontSize) }; };
    let saved = null; try { saved = localStorage.getItem("markShare"); } catch { /* none */ }
    const ul = document.querySelector("#markpane .mknav");
    return { fits: ul.scrollHeight <= ul.clientHeight + 1, stacked: list.bottom <= grip.top + 0.5 && grip.bottom <= view.top + 0.5 && Math.abs(list.left - view.left) < 1 && Math.abs(list.width - view.width) < 1,
      list: Math.round(list.height), view: Math.round(view.height), gripMid: Math.round(grip.top + grip.height / 2), gripX: Math.round(grip.left + grip.width / 2),
      chosen: document.querySelector("#markpane li.chosen")?.dataset.id, now: document.querySelector("#markpane .mkgrip").getAttribute("aria-valuenow"), saved,
      R: row("R"), A: row("A"), B: row("B") };
  });
  const half = await layout();
  check(`AC-33 ${at} the list on top, as tall as its 5 rows (under half), the view under it: each entry its time on a line over its title, its text's start on one line with no Markdown marks or breaks`,
    half.stacked && half.fits && half.list < half.view && half.now === "50" && half.R.snip === "Fixed the retry loop. src/retry.rs next: add a test" && half.R.oneLine
    && half.R.metaAbove && half.R.metaSize === 12 && half.A.snip === "out 200 lorem ipsum" && half.B.metaAbove, JSON.stringify(half));
  await page.mouse.move(half.gripX, half.gripMid);
  await page.mouse.down();
  await page.mouse.move(half.gripX, half.gripMid - 80, { steps: 4 });
  await page.mouse.up();
  const dragged = await layout();
  await page.keyboard.press("ArrowDown");                     // the pane kept the keys: ↓ still chooses
  const keys = await layout();
  await page.focus("#markpane .mkgrip");
  await page.keyboard.press("ArrowUp");                       // the focused line: ↑ moves it, the list's choice stays
  const stepped = await layout();
  await pressFab(page, "#marksfab");
  await pressFab(page, "#marksfab");
  await page.waitForSelector("#markpane .mknav li");
  const again = await layout();
  check(`AC-33 ${at} the line dragged 80 px up shares the height so, the keys stay the list's; ↑ on the focused line moves it 5%; kept when the pane opens again`,
    Math.abs(dragged.list - (half.list - 80)) <= 2 && !dragged.fits && dragged.chosen === "R" && keys.chosen !== "R"
    && Math.abs(Number(stepped.now) - (Number(dragged.now) - 5)) <= 1 && stepped.list < dragged.list && stepped.chosen === keys.chosen && Number(stepped.saved) * 100 - Number(stepped.now) < 0.6
    && Math.abs(again.list - stepped.list) <= 1, JSON.stringify({ dragged, keys, stepped, again }));
  await page.mouse.dblclick(again.gripX, again.gripMid);       // a double click: half and half again
  const reset = await layout();
  check(`AC-33 ${at} a double click on the line: half at most again, the list as tall as its rows`, reset.now === "50" && reset.fits && Math.abs(reset.list - half.list) <= 1, JSON.stringify(reset));
  await page.click('#markpane li[data-id="R"] .mkitem');

  // a note of two lines with Markdown: one line in the list and the title, whole above its text in the view
  await page.click('#markpane li[data-id="A"] .mkitem');
  await page.waitForTimeout(400);
  const noted = await page.evaluate(() => { const body = document.querySelector("#markpane .mkvbody").shadowRoot.querySelector(".body");
    return { row: document.querySelector('#markpane li[data-id="A"] .note').textContent, title: document.querySelector("#markpane .mkvtitle").textContent,
      strong: body.querySelector("p strong")?.textContent, pre: body.querySelector("pre")?.textContent.trim() }; });
  check(`AC-33 ${at} a note of two lines with Markdown: one line without its marks in the list and the title, whole as Markdown above the text in the view`,
    noted.row === "second by line with a second line" && noted.title === noted.row && noted.strong === "second" && noted.pre === "out 200 lorem ipsum", JSON.stringify(noted));
  await page.click('#markpane li[data-id="R"] .mkitem');

  // Enter: shown in the terminal, found by its title where its call was printed; the pane closes, the terminal has the keys
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => document.getElementById("markrail") && document.querySelector(".marked, .wavy, #term [data-marked]") !== undefined, null, { timeout: 3000 }).catch(() => {});
  await page.waitForTimeout(800);
  const rec = await page.evaluate(() => {
    const rows = [...document.querySelectorAll("#hist .ln, #screen .ln")], t = rows.find((r) => r.textContent.includes("add_notes_for_user"));
    const view = document.getElementById("term").getBoundingClientRect(), r = t?.getBoundingClientRect();
    return { inView: !!r && r.top >= view.top && r.bottom <= view.bottom, bar: document.getElementById("mbnote").textContent, why: document.getElementById("mbwhy").hidden,
      open: !document.getElementById("markpane").hidden, keys: document.activeElement?.id };
  });
  check(`AC-98 ${at} Enter (Show in terminal) on an agent's record closes the pane and brings the line where its call was printed into view`,
    sent.slice(from).some((m) => m.t === "locate" && m.id === "R") && rec.inView && rec.bar === "Final report: retry loop fixed" && rec.why && !rec.open && rec.keys === "kbd", JSON.stringify(rec));
  await page.click("#mbdone");
  const barUndo = await page.evaluate(() => document.getElementById("mbdone").textContent);
  await page.click("#mbdone");
  const barBack = await page.evaluate(() => document.getElementById("mbdone").textContent);
  check(`AC-33 ${at} the bar's ✓ Resolve turns into Undo, and back`, barUndo === "Undo" && barBack === "Resolve", JSON.stringify({ barUndo, barBack }));
  from = sent.length;
  await page.click("#mbsend");
  await page.waitForTimeout(200);
  const pasted = sent.slice(from).filter((m) => m.t === "in").map((m) => m.data).join("");
  check(`AC-98 ${at} the bar's Quote in chat pastes an agent's title and, under it, its text quoted`,
    pasted.includes("Final report: retry loop fixed") && pasted.includes("> **Fixed** the retry loop."), JSON.stringify(pasted));
  await page.click("#mbclose");

  // Done: at once in the list with Undo, terminumd told only after 5 s; Undo takes it back
  await page.click("#marksfab");
  await page.waitForSelector('#markpane li[data-id="A"]');
  from = sent.length;
  await page.click('#markpane li[data-id="A"] .mkitem');
  await page.click("#markpane .mkvdone");
  await page.waitForTimeout(300);
  const pending = await page.evaluate(() => { const l = document.querySelector('#markpane li[data-id="A"]'); return { pending: l.classList.contains("pending"), said: l.querySelector(".mkdonetext")?.textContent, undo: !!l.querySelector(".mkundo"), done: document.querySelector("#markpane .mkvdone").textContent }; });
  await page.click('#markpane li[data-id="A"] .mkundo');
  await page.waitForTimeout(5300);
  const undone = await page.evaluate(() => !document.querySelector('#markpane li[data-id="A"]').classList.contains("pending"));
  check(`AC-33 ${at} Done marks a bookmark deleted at once with Undo (in its row and in the view); Undo takes it back and nothing is sent`,
    pending.pending && pending.said === "Deleted" && pending.undo && pending.done === "Undo" && undone && !sent.slice(from).some((m) => m.t === "unmark"), JSON.stringify({ pending, undone, sent: sent.slice(from) }));
  from = sent.length;
  await page.click('#markpane li[data-id="C"] .mkitem');
  await page.click("#markpane .mkvdone");
  await page.click('#markpane li[data-id="A"] .mkitem');      // chosen: Delete on it
  await page.keyboard.press("Delete");
  await page.waitForTimeout(1000);
  const early = sent.slice(from).filter((m) => m.t === "unmark" || m.t === "remind").length;
  await page.waitForTimeout(4600);
  const told = sent.slice(from).filter((m) => m.t === "unmark" || m.t === "remind");
  check(`AC-33 ${at} after 5 s a reminder is resolved and a bookmark deleted (Done, and Delete on the chosen one); nothing before`,
    early === 0 && told.some((m) => m.t === "remind" && m.id === "C" && m.action === "resolve") && told.some((m) => m.t === "unmark" && m.id === "A"), JSON.stringify({ early, told }));
  for (const m of told) say(m.t === "unmark" ? { t: "unmark", id: m.id } : { t: "mark", item: { ...ITEMS.find((i) => i.id === m.id), due: false, resolved: at0 + 50 } });

  // the view's Quote in chat: the chosen one pasted quoted, the pane closed
  await page.waitForTimeout(300);
  await page.click('#markpane li[data-id="B"] .mkitem');
  from = sent.length;
  await page.click("#markpane .mkvacts .tochat");
  await page.waitForTimeout(200);
  const quoted = sent.slice(from).filter((m) => m.t === "in").map((m) => m.data).join("");
  const after = await page.evaluate(() => !document.getElementById("markpane").hidden);
  check(`AC-33 ${at} the view's Quote in chat pastes the note and its text quoted, no Enter; the pane closes`,
    quoted.includes("first by line") && quoted.includes("> out 100 lorem ipsum") && !after, JSON.stringify({ quoted, after }));

  // the reminders' pane: the same navigator of the reminders only; an agent's with its mark, its text as Markdown, its link in the theme's link color
  await page.click("#remindfab");
  await page.waitForSelector("#remindpane .mknav li");
  await page.click('#remindpane li[data-id="Q"] .mkitem');
  await page.waitForTimeout(500);
  const pane = await page.evaluate(() => {
    const body = document.querySelector("#remindpane .mkvbody")?.shadowRoot?.querySelector(".body"), a = body?.querySelector("a");
    const probe = (parent) => { const s = document.createElement("span"); s.style.color = "var(--link)"; parent.append(s); const c = getComputedStyle(s).color; s.remove(); return c; };
    // the page's own Markdown (the sent history, Open all) takes the same sheet
    const page = Object.assign(document.createElement("div"), { className: "markdown", innerHTML: '<a href="https://example.com/x">x</a>' });
    document.body.append(page);
    const out = { title: document.querySelector("#remindpane .pwtitle").textContent, ids: [...document.querySelectorAll("#remindpane .mknav li")].map((l) => l.dataset.id),
      mark: !!document.querySelector('#remindpane li[data-id="Q"] .note .agmark'), quote: !!body?.querySelector("blockquote, pre"), text: body?.textContent.trim(),
      link: a && getComputedStyle(a).color, want: probe(document.body), pageLink: getComputedStyle(page.querySelector("a")).color,
      acts: [...document.querySelectorAll("#remindpane .mkvacts button")].map((b) => b.textContent.trim()), grip: !!document.querySelector("#remindpane .mkgrip") };
    page.remove();
    return out;
  });
  const blue = ["rgb(0, 0, 238)", "rgb(85, 26, 139)", "rgb(0, 0, 255)"];
  // a layout while it is open (they come often while agents work): its rows not made again; Resolve here, Undo on the bookmarks' pane
  const rowBefore = await page.evaluate(() => { window.__row = document.querySelector('#remindpane li[data-id="Q"]'); return !!window.__row; });
  say({ t: "layout", groups: LAYOUT });
  await page.waitForTimeout(200);
  const kept = await page.evaluate(() => document.querySelector('#remindpane li[data-id="Q"]') === window.__row);
  await page.click("#remindpane .mkvdone");
  await page.keyboard.press("Escape");
  await pressFab(page, "#marksfab");
  await page.waitForSelector('#markpane li[data-id="Q"]');
  const shared = await page.evaluate(() => ({ pending: document.querySelector('#markpane li[data-id="Q"]').classList.contains("pending"), undo: !!document.querySelector('#markpane li[data-id="Q"] .mkundo') }));
  await page.click('#markpane li[data-id="Q"] .mkundo');
  const from2 = sent.length;
  await page.waitForTimeout(5300);
  check(`AC-91 ${at} a layout while the reminders' pane is open makes no row again; Resolve there is pending in the bookmarks' pane, whose Undo takes it back (nothing sent)`,
    rowBefore && kept && shared.pending && shared.undo && !sent.slice(from2).some((m) => m.t === "remind" && m.id === "Q"), JSON.stringify({ rowBefore, kept, shared }));
  await page.keyboard.press("Escape");
  check(`AC-91 AC-98 ${at} the reminders' pane is the bookmarks' navigator of the reminders only: an agent's with its mark, its text as Markdown (no quote), the same actions; a link in the theme's link color, in the widget and on the page, never the browser's blue or purple`,
    pane.title === "Reminders of this session" && pane.ids.join() === "Q" && pane.mark && !pane.quote && pane.text.includes("fetch_all or list_all?") && pane.grip
    && pane.acts.join() === "Show in terminal,Resolve,Convert to task,Quote in chat" && pane.link === pane.want && pane.pageLink === pane.want && !blue.includes(pane.link), JSON.stringify(pane));

  // the list emptied while the line is held (the last one resolved elsewhere): the pane closes; opened again, nothing is held
  await page.keyboard.press("Escape");                      // the reminders' pane closed
  await pressFab(page, "#marksfab");
  await page.waitForSelector("#markpane .mkgrip");
  const g = await page.evaluate(() => { const r = document.querySelector("#markpane .mkgrip").getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; });
  await page.mouse.move(g.x, g.y);
  await page.mouse.down();
  await page.mouse.move(g.x, g.y - 20, { steps: 2 });
  say({ t: "marks", items: [], error: null });
  await page.waitForTimeout(200);
  const emptied = await page.evaluate(() => document.getElementById("markpane").hidden);
  await page.mouse.up();
  say({ t: "marks", items: ITEMS, error: null });
  await page.waitForFunction(() => !document.getElementById("marksfab").hidden, null, { timeout: 5000 }).catch(() => {});
  await pressFab(page, "#marksfab");
  await page.waitForSelector("#markpane .mknav li");
  const back = await page.evaluate(() => ({ empty: document.querySelector("#markpane .empty").hidden, rows: document.querySelectorAll("#markpane .mknav li").length,
    grip: !!document.querySelector("#markpane .mkgrip")?.isConnected, dragging: document.querySelector("#markpane .mkcols").classList.contains("dragging"), sel: getComputedStyle(document.querySelector("#markpane .mkcols")).userSelect }));
  check(`AC-33 ${at} the list emptied while the line is held: the pane closes; filled and opened again, the list and the line are back, nothing held`,
    emptied && !back.dragging && back.empty && back.rows > 0 && back.grip && back.sel !== "none", JSON.stringify({ emptied, back }));
  check(`AC-33 ${at} no page errors`, f.errors.length === 0, f.errors.join(" | "));
  await page.close();

  // a phone: the sheet's line a finger's size, the view at least 200 px with its actions inside the sheet
  const p = await open(browser, { history: HIST, first: 0, screen: ["done"], device: { viewport: { width: 390, height: 844 }, isMobile: name === "chromium", hasTouch: true } });
  p.say({ t: "marks", items: ITEMS, error: null });
  await p.page.waitForFunction(() => !document.getElementById("marksfab").hidden, null, { timeout: 5000 }).catch(() => {});
  await pressFab(p.page, "#marksfab");
  await p.page.waitForSelector("#markpane .mknav li");
  await p.page.waitForTimeout(300);
  const phone = await p.page.evaluate(() => {
    const r = (s) => document.querySelector(s).getBoundingClientRect(), pane = r("#markpane"), acts = r("#markpane .mkvacts");
    return { sheet: "sheet" in document.getElementById("markpane").dataset, grip: Math.round(r("#markpane .mkgrip").height), view: Math.round(r("#markpane .mkview").height),
      acts: acts.top >= pane.top && acts.bottom <= pane.bottom + 0.5 };
  });
  check(`AC-33 ${at} phone: a sheet, its line 44 px high, its view at least 200 px with its actions inside it`,
    phone.sheet && phone.grip >= 44 && phone.view >= 200 && phone.acts && p.errors.length === 0, JSON.stringify({ phone, errors: p.errors }));
  // Show in terminal: the bar after the jump takes the terminal's width under ⟨…⟩, every key inside it (beside ⟨…⟩ Delete stood out of it)
  await p.page.evaluate(() => document.querySelector("#markpane .mkvgo").click());
  await p.page.waitForFunction(() => !document.getElementById("markbar").hidden, null, { timeout: 5000 }).catch(() => {});
  const bar = await p.page.evaluate(() => {
    const b = document.getElementById("markbar").getBoundingClientRect(), fold = document.getElementById("foldfab").getBoundingClientRect();
    const keys = [...document.querySelectorAll("#markbar button")].filter((k) => k.offsetHeight).map((k) => k.getBoundingClientRect());
    return { shown: b.height > 0, left: Math.round(b.left), right: Math.round(innerWidth - b.right), underFold: b.top >= fold.bottom, inside: keys.every((k) => k.left >= b.left - 0.5 && k.right <= b.right + 0.5),
      keys: Math.round(Math.min(...keys.map((k) => k.height))), quote: document.getElementById("mbsend").innerText.trim() };
  });
  check(`AC-33 ${at} phone: the bar after Show in terminal spans the terminal under ⟨…⟩, its keys inside it, 44 px, "Quote" for Quote in chat`,
    bar.shown && bar.left === 12 && bar.right === 12 && bar.underFold && bar.inside && bar.keys >= 44 && bar.quote === "Quote", JSON.stringify(bar));
  await p.page.close();
}
