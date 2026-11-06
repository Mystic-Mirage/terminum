// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-57 checks for e2e_panel.mjs: bookmarks in the whole web app (the real page files, under the
// page's own CSP) against a fake bridge (e2e_widgets.mjs's): [+] beside a selection, the
// dialog, the marks on the terminal's edge and in the session list, the way back into older
// lines not loaded, into a fold whose lines were never sent and into a rendered Markdown widget,
// lines that are gone, a closed session's bookmark, a refused save, a selection in a widget.
// Alone: node web/test/e2e_marks.mjs [chromium|webkit].

import { fileURLToPath } from "node:url";
import { pressFab, pressFoot, spyCopies } from "./fake.mjs";
import { check, cleanup, playwright, results } from "./check.mjs";
import { askChecks } from "./e2e_ask.mjs";
import { open } from "./e2e_widgets.mjs";

import { line, HISTORY, FOLDS, GONE, at0, ITEMS, LAYOUT, bridge, marked, inView, fromPane, lineColor, selectRow } from "./e2e_marksdata.mjs";

export async function webMarks(browser, name = "") {
  const at = `AC-57 web app${name}:`;
  const items = new Map(ITEMS.map((i) => [i.id, { ...i }]));
  const { page, errors, sent, say } = await open(browser, { history: HISTORY, first: 2000, folds: FOLDS, screen: ["done"], hook: bridge(items), layout: LAYOUT });
  say({ t: "marks", items: ITEMS, error: null });

  // a selection (a computer): Copy and [+] in the floating buttons' column (in one line with find and the sent history),
  // Copy level with the selection's first line; the column's other buttons hidden meanwhile
  await selectRow(page, "out 2500 lorem");
  const plus = await page.evaluate(() => {
    const r = (id) => document.getElementById(id).getBoundingClientRect(), add = r("markadd"), copy = r("markcopy");
    const s = [...getSelection().getRangeAt(0).getClientRects()].find((x) => x.height > 0);
    return { visible: !document.getElementById("markadd").hidden, lefts: ["findfab", "sentfab", "markcopy", "markadd"].map((id) => Math.round(r(id).left)),
      dy: Math.round(copy.top + copy.height / 2 - (s.top + s.height / 2)), order: copy.top < add.top,
      others: ["findfab", "sentfab", "marksfab"].map((id) => getComputedStyle(document.getElementById(id)).visibility) };
  });
  check(`${at} a selection gets Copy and [+] in the floating buttons' column, Copy level with its first line; the column's other buttons hidden`,
    plus.visible && new Set(plus.lefts).size === 1 && Math.abs(plus.dy) < 12 && plus.order && plus.others.every((v) => v === "hidden"), JSON.stringify(plus));
  const pair = await page.evaluate(() => {
    const c = document.getElementById("markcopy"), a = document.getElementById("markadd");
    const cr = c.getBoundingClientRect(), ar = a.getBoundingClientRect(), cs = getComputedStyle(c);
    return { shown: !c.hidden, above: cr.bottom <= ar.top && ar.top - cr.bottom < 16, sameX: Math.abs(cr.left - ar.left) < 1,
      gray: cs.backgroundColor, radius: getComputedStyle(a).borderTopLeftRadius, other: getComputedStyle(a).borderTopRightRadius };
  });
  check(`${at} a gray Copy button stands just above it; both are blobs, not circles`,
    pair.shown && pair.above && pair.sameX && /^rgb\((\d+), \1, |^rgb\(1[45]\d, 1[56]\d, 1[67]\d\)/.test(pair.gray) && pair.radius !== pair.other, JSON.stringify(pair));
  const gaps = await page.evaluate(() => { const r = (id) => document.getElementById(id).getBoundingClientRect();
    return [r("markadd").top - r("markcopy").bottom, r("askfab").top - r("markadd").bottom].map(Math.round); });
  check(`${at} a computer: Copy, [+] and Ask one gap (--sp-3, 12 px) apart`, gaps.every((g) => g === 12), JSON.stringify(gaps));
  await spyCopies(page);
  // Ask (AC-37): while text is selected, under the [+], on a pastel cornflower blue fill; the column stays in sight
  const asking = await page.evaluate(() => { const g = (id) => document.getElementById(id), r = (id) => g(id).getBoundingClientRect();
    return { shown: !g("askfab").hidden, under: r("askfab").top > r("markadd").bottom && r("askfab").top - r("markadd").bottom < 16 && Math.abs(r("askfab").left - r("markadd").left) < 1,
      icon: !!g("askfab").querySelector("svg") && !g("askfab").textContent.trim(), fill: getComputedStyle(g("askfab")).backgroundColor,
      marks: getComputedStyle(g("marksfab")).visibility };
  });
  check(`${at} Ask shows while text is selected: an icon just under the [+], a pastel cornflower blue; the column's other buttons hidden`,
    asking.shown && asking.under && asking.icon && /^(rgb\(1[6-8]\d, 1[89]\d, 2[34]\d\)|color\(srgb 0\.[67]\d* 0\.7\d* 0\.9\d*\))$/.test(asking.fill) && asking.marks === "hidden", JSON.stringify(asking));
  await page.click("#markcopy");
  await page.waitForTimeout(800);                                  // past the buttons' grace after a selection goes
  check(`${at} Copy copies the selected text and keeps the selection for a bookmark`,
    (await page.evaluate(() => window.copied)).some((c) => c.startsWith("out 2500 lorem ipsum")) && !await page.evaluate(() => document.getElementById("markadd").hidden)
    && (await page.evaluate(() => String(getSelection()))).includes("out 2500 lorem ipsum"),
    JSON.stringify(await page.evaluate(() => window.copied)));
  await page.click("#markadd");
  await page.waitForSelector("#markbox[open]");
  check(`${at} [+] opens the dialog with the selected text`, (await page.textContent("#marktext")).includes("out 2500 lorem ipsum"));
  const wide = await page.evaluate(() => ({ dlg: Math.round(document.getElementById("markbox").getBoundingClientRect().width),
    want: Math.round(Math.min(Math.max(480, document.getElementById("term").clientWidth * 0.8), innerWidth - 32)) }));
  check(`${at} the dialog is 80% of the terminal's width (480 px at least, the window's less 32 px at most)`, Math.abs(wide.dlg - wide.want) <= 1, JSON.stringify(wide));
  await page.fill("#marknote", "look again");
  await page.press("#marknote", "Meta+Enter");                   // as Save
  await page.waitForFunction(() => !document.getElementById("markbox").open, null, { timeout: 5000 }).catch(() => {});
  const mark = sent.find((m) => m.t === "mark" && !m.id);
  check(`${at} ⌘Enter saves: it sends the line, the text and the note; the dialog closes`,
    mark?.sid === "S1" && mark.n === 2500 && mark.text.startsWith("out 2500 lorem ipsum") && mark.note === "look again" && !await page.evaluate(() => document.getElementById("markbox").open), JSON.stringify(mark));
  const back = await page.evaluate(() => ({ add: document.getElementById("markadd").hidden, others: ["findfab", "sentfab"].map((id) => getComputedStyle(document.getElementById(id)).visibility) }));
  check(`${at} the selection gone, its buttons go and the column's own are back`, back.add && back.others.every((v) => v === "visible"), JSON.stringify(back));
  // its wavy line at once, where it stands, apart from the terminal's edge and the text
  const just = await page.evaluate(() => { const l = document.getElementById("markline"), t = document.getElementById("term"), r = l?.getBoundingClientRect(), tr = t.getBoundingClientRect();
    const row = [...document.querySelectorAll("#term .ln")].find((el) => el.textContent.includes("out 2500 lorem")), txt = row && document.createRange();
    if (txt) txt.selectNodeContents(row);
    const tx = txt?.getBoundingClientRect();
    return { shown: !!l && !l.hidden, text: l?.range?.toString().slice(0, 14), edge: r && Math.round(r.left - tr.left - t.clientLeft), gap: r && tx && Math.round(tx.left - r.right) }; });
  check(`${at} a bookmark just saved has its wavy line at once, 5 px from the terminal's edge and 5 px before the text`,
    just.shown && just.text === "out 2500 lorem" && just.edge === 5 && just.gap === 5, JSON.stringify(just));
  const orange = await lineColor(page);
  check(`${at} a bookmark's line is the bookmarks' orange`, orange.shown && orange.fill === orange.mark, JSON.stringify(orange));
  await page.waitForFunction(() => document.querySelector('#msgrail .dot[title*="look again"]'), null, { timeout: 3000 }).catch(() => {});   // drawn at the next frame
  const rail = await page.evaluate(() => [...document.querySelectorAll("#msgrail .dot.mark")].map((b) => ({ top: parseFloat(b.style.top), title: b.title })));
  check(`${at} the session's bookmarks are orange dots on the left strip (a wide window; none on the right edge), placed by their lines`,
    rail.length >= 4 && rail.at(-1).title.includes("Bookmark: look again") && rail[0].top < rail.at(-1).top && await page.evaluate(() => document.getElementById("markrail").hidden), JSON.stringify(rail));
  const rowsAt = await page.evaluate(() => ["S1", "S2"].map((id) => {
    const it = document.querySelector(`#sessions .item[data-id="${id}"]`), r = (sel) => it.querySelector(sel)?.getBoundingClientRect();
    return { strip: !!it.querySelector(".wave.mark"), text: Math.round(r(".txt").left), top: Math.round(r(".txt").top - it.getBoundingClientRect().top),
      state: Math.round(r(".st").left), stripIn: r(".wave.mark") ? Math.round(r(".wave.mark").left) >= Math.round(r(".st").right) && r(".wave.mark").right <= it.getBoundingClientRect().right : null };
  }));
  check(`${at} the session list shows an orange strip beside a session with bookmarks, its row laid out as the others`,
    rowsAt[0].strip && !rowsAt[1].strip && rowsAt[0].stripIn && rowsAt[0].text === rowsAt[1].text && rowsAt[0].top === rowsAt[1].top && rowsAt[0].state === rowsAt[1].state, JSON.stringify(rowsAt));

  // renaming the session (the pencil) keeps its bookmarks: they belong to the pane, which keeps its id
  say({ t: "layout", groups: [{ ...LAYOUT[0], items: [{ ...LAYOUT[0].items[0], title: "Renamed one" }, LAYOUT[0].items[1]] }] });
  await page.waitForFunction(() => document.querySelector('#sessions .item[data-id="S1"] .t')?.textContent === "Renamed one");
  await pressFab(page, "#marksfab");
  const renamed = await page.evaluate(() => ({
    rail: document.querySelectorAll("#msgrail .dot.mark").length, strip: !!document.querySelector('#sessions .item[data-id="S1"] .wave.mark'),
    fab: document.getElementById("marksfabn").textContent, listed: document.querySelectorAll("#markpane .mkitem").length,
    head: document.querySelector("#markpane h2")?.textContent }));
  await pressFab(page, "#marksfab");
  check(`${at} a renamed session keeps its bookmarks (marks, strip, count); outside projects its pane lists its own`,
    renamed.rail >= 4 && renamed.strip && renamed.fab === String(renamed.listed) && renamed.listed >= 6 && renamed.head === "Bookmarks and reminders of this session", JSON.stringify(renamed));

  // its count: round for one digit, an ellipse for two, clear of the drawn bookmark (its stroke's half too)
  const ring = await page.evaluate(() => {
    const n = document.getElementById("marksfabn"), b = n.getBoundingClientRect(), cs = getComputedStyle(n);
    const drawn = [...document.querySelectorAll("#marksfab svg path")].map((p) => p.getBoundingClientRect());
    const right = Math.max(...drawn.map((r) => r.right)), top = Math.min(...drawn.map((r) => r.top));
    const was = n.textContent;
    n.textContent = "12";
    const two = n.getBoundingClientRect();
    n.textContent = was;
    return { digits: was.length, w: b.width, h: b.height, radius: cs.borderTopLeftRadius, border: cs.borderTopWidth, clear: b.left - right, above: top - b.top, two: [two.width, two.height],
      inView: b.top >= 0 && b.right <= innerWidth, fill: cs.backgroundColor, edge: cs.borderTopColor };
  });
  check(`${at} the count is filled dark orange, no outline of another color (AC-33)`, ring.fill === "rgb(90, 47, 0)" && ring.edge === ring.fill, JSON.stringify(ring));
  check(`${at} the bookmarks' count is round for one digit, an ellipse for two, clear of the icon, inside the window`,
    ring.digits === 1 && Math.abs(ring.w - ring.h) < 0.5 && ring.radius === "50%" && ring.border === "1px" && ring.clear >= 0.5
      && ring.two[0] > ring.two[1] + 2 && ring.two[1] === ring.h && ring.inView, JSON.stringify(ring));

  // the way back: into older lines not loaded, found by its text (its line moved)
  await page.click('#msgrail .dot[title*="Bookmark: ask about the retry loop"]');
  await page.waitForFunction(() => document.getElementById("markline")?.range?.toString().includes("retry loop"), null, { timeout: 8000 }).catch(() => {});
  let m = await marked(page);
  const more = sent.find((x) => x.t === "more" && x.to !== undefined);
  check(`${at} a mark loads the older lines down to its text in one request and marks it in view, a third from the top`,
    more?.before === 2000 && more.to === 110 && m?.text === "the retry loop never resets" && inView(m) && Math.abs(m.top - m.h / 3) < 40, JSON.stringify({ more, m }));
  check(`${at} the bar shows its note`, (await page.textContent("#mbnote")) === "ask about the retry loop" && await page.isVisible("#markbar"));
  const before = sent.length;
  await page.click("#mbsend");
  await page.waitForTimeout(100);
  const pasted = sent.slice(before).filter((x) => x.t === "in").map((x) => x.data).join("");
  check(`${at} Quote in chat pastes the note and, under it, the text quoted into the input, no Enter`,
    pasted.includes("ask about the retry loop\r\r> ") && pasted.includes("the retry loop never resets") && !pasted.replace(/\x1b\[201~$/, "").endsWith("\r"), JSON.stringify(pasted));
  check(`${at} the text keeps its look; a wavy line on its left from its first row to its last marks it`, m?.line === true
    && await page.evaluate(() => !CSS.highlights?.has("bookmark") && /svg/.test(getComputedStyle(document.getElementById("markline")).maskImage || getComputedStyle(document.getElementById("markline")).webkitMaskImage)), JSON.stringify(m));

  // this session's bookmarks from the button at the terminal's top right: a toggle
  await pressFab(page, "#marksfab");
  const pane = await page.evaluate(() => ({ open: !document.getElementById("markpane").hidden,
    items: [...document.querySelectorAll("#markpane .mkitem .note")].map((n) => n.textContent),
    count: document.getElementById("marksfabn").textContent }));
  await page.mouse.click(30, 300);                                 // a press elsewhere closes it
  const outside = await page.evaluate(() => document.getElementById("markpane").hidden);
  await pressFab(page, "#marksfab");
  await pressFab(page, "#marksfab");
  const paneClosed = await page.evaluate(() => document.getElementById("markpane").hidden) && outside;
  check(`${at} the button at the top right opens this session's bookmarks, top to bottom; pressed again it closes them`,
    pane.open && paneClosed && pane.items[0] === "gone" && pane.items.includes("look again") && !pane.items.includes("closed one") && pane.count === String(pane.items.length) && pane.count !== "0",
    JSON.stringify({ pane, paneClosed }));

  // into a rendered Markdown widget, drawn only once it is near
  await page.click('#msgrail .dot[title*="Bookmark: rendered"]');
  await page.waitForFunction(() => { const r = document.getElementById("markline")?.range; return r?.startContainer?.getRootNode() instanceof ShadowRoot; }, null, { timeout: 8000 }).catch(() => {});
  m = await marked(page);
  await page.waitForTimeout(400);
  const later = await marked(page);
  check(`${at} text in a rendered Markdown widget is marked in its rendered text, in view, and stays put`,
    m?.shadow && /small tool that keeps bookmarks/.test(m.text) && inView(m) && Math.abs(later.top - m.top) < 2, JSON.stringify({ m, later }));

  // into a fold whose lines were never sent
  await fromPane(page, "folded secret line 515");
  await page.waitForFunction(() => document.getElementById("markline")?.range?.toString() === "folded secret line 515", null, { timeout: 8000 }).catch(() => {});
  m = await marked(page);
  check(`${at} from the list: a bookmark in a fold asks for its lines, opens it and marks the text`,
    sent.some((x) => x.t === "unfold" && x.n === 500) && m?.text === "folded secret line 515" && inView(m), JSON.stringify(m));
  // the list below the sessions has only those no session shows
  await pressFoot(page, "#marksbtn");
  const left = await page.evaluate(() => {
    const b = document.getElementById("marksbtn"), shown = [...b.querySelectorAll("svg")].filter((s) => s.getBoundingClientRect().width > 0);
    const color = (s) => getComputedStyle(s).color, more = b.querySelector(".more");
    return { items: [...document.querySelectorAll("#marklist .mkitem .note")].map((n) => n.textContent),
      count: document.getElementById("markcount").textContent, label: b.textContent.trim(),
      // a phone: one blob with both icons; a computer: the bookmark, then " / ", the bell, "Reminders"
      icons: shown.map((s) => s.classList.contains("two") ? "two" : s.classList.contains("bell") ? "bell" : "one"),
      colors: shown.map(color).concat(shown.flatMap((s) => [...s.querySelectorAll(".bell")].map(color))),
      mark: getComputedStyle(document.documentElement).getPropertyValue("--mark").trim(),
      bellFirst: !!more && getComputedStyle(more).display !== "none" ? more.childNodes[1]?.nodeName === "svg" && more.childNodes[2]?.textContent === "Reminders" : null };
  });
  await page.waitForTimeout(400);                                  // its opening settled
  const win = await page.evaluate(() => { const d = document.getElementById("marksdlg"), r = d.getBoundingClientRect();
    return { open: d.open, right: Math.round(innerWidth - r.right), tall: r.height > innerHeight * 0.8, wide: r.width >= 400, inNav: !!document.querySelector("#nav #marklist") }; });
  await page.keyboard.press("Escape");
  check(`${at} "Bookmarks / Reminders" opens a large window on the right (not in the session list); Esc closes it`,
    win.open && win.right <= 16 && win.tall && win.wide && !win.inNav && !await page.evaluate(() => document.getElementById("marksdlg").open), JSON.stringify(win));
  check(`${at} "Bookmarks / Reminders" below the sessions lists them all (the orphaned too), its count theirs`,
    left.items.length > 1 && left.items.includes("closed one") && left.count === String(left.items.length) && /^Bookmarks \/ Reminders/.test(left.label), JSON.stringify(left));
  check(`${at} its button: the bookmark orange, the bell pink, before "Reminders" on a computer, both in one blob on a phone`,
    left.colors.length === 2 && left.colors[0] !== left.colors[1] && (left.bellFirst === null ? left.icons.join() === "two" : left.icons.join() === "one,bell" && left.bellFirst), JSON.stringify(left));

  // search (AC-58): the magnifier, atop the buttons' column (the bookmarks' set apart below), opens a field; Find goes to the first match
  const at58 = at.replace("AC-57", "AC-58");
  const fabs = await page.evaluate(() => ["findfab", "splitfab", "sentfab", "marksfab"].map((id) => document.getElementById(id)).filter((b) => b.offsetHeight)
    .map((b) => !b.hidden && { left: Math.round(b.getBoundingClientRect().left), top: Math.round(b.getBoundingClientRect().top), h: Math.round(b.getBoundingClientRect().height) }));
  await pressFab(page, "#findfab");
  const focused = await page.evaluate(() => document.activeElement?.id);
  await page.fill("#findq", "alpha");
  await page.click('#findbox button[type="submit"]');
  const findShown = (text) => page.waitForFunction((text) => { const r = [...(CSS.highlights?.get("find") ?? [])][0]; return r && r.toString() === text && document.getElementById("findn").textContent.length; }, text, { timeout: 8000 }).catch(() => {});
  await findShown("alpha");
  const first = await page.evaluate(() => {
    const r = [...CSS.highlights.get("find")][0], t = document.getElementById("term").getBoundingClientRect(), b = r.getBoundingClientRect();
    return { n: document.getElementById("findn").textContent, row: r.startContainer.parentElement.closest(".ln")?.textContent, inView: b.top >= t.top && b.bottom <= t.bottom,
      marks: document.querySelectorAll("#findrail .fm").length, on: document.querySelectorAll("#findrail .fm.on").length };
  });
  const [mk, s1, s0] = [fabs.at(-1), fabs.at(-2), fabs.at(-3)];
  const column = fabs.every((b) => b && b.left === fabs[0].left) && fabs.every((b, i) => !i || b.top > fabs[i - 1].top)
    && mk.top - (s1.top + s1.h) > s1.top - (s0.top + s0.h);                                     // the colored ones set apart
  check(`${at58} search: the buttons stand in one column, the magnifier first and the bookmarks' set apart below; it opens a focused field, Find says how many and shows the first, older lines loaded`,
    column && focused === "findq" && first.n === "1 / 3" && first.row === "find alpha here" && first.inView && first.marks === 2 && first.on === 1,
    JSON.stringify({ fabs, focused, first }));
  await page.click("#findnext");
  await findShown("alpha");
  await page.click("#findnext");
  await page.waitForFunction(() => document.getElementById("findn").textContent === "3 / 3");
  await page.waitForTimeout(800);                                    // the pane idle: nothing marks it again
  const third = await page.evaluate(() => ({ n: document.getElementById("findn").textContent, text: [...(CSS.highlights.get("find") ?? [])][0]?.toString() }));
  await page.click("#findprev");
  await page.waitForFunction(() => document.getElementById("findn").textContent === "2 / 3");
  check(`${at58} search: next and the one before step through the matches, two on a row told apart, case aside`,
    third.n === "3 / 3" && third.text === "ALPHA", JSON.stringify(third));
  await page.fill("#findq", "gamma");
  await page.keyboard.press("Enter");
  await findShown("gamma");
  const wrapped = await page.evaluate(() => {
    const r = [...(CSS.highlights.get("find") ?? [])][0];
    return { n: document.getElementById("findn").textContent, text: r?.toString(), rows: r && r.startContainer.parentElement.closest(".ln") !== r.endContainer.parentElement.closest(".ln") };
  });
  check(`${at58} search: a match across a long row iTerm wrapped is found and marked over both rows`,
    wrapped.n === "1 / 1" && wrapped.text === "gamma" && wrapped.rows, JSON.stringify(wrapped));
  say({ t: "hist", mode: "reset", sid: "S1", first: 2000, oldest: 0, truncated: false, lines: HISTORY.slice(2000) });
  await page.waitForFunction(() => /history changed/.test(document.getElementById("findn").textContent), null, { timeout: 5000 }).catch(() => {});
  check(`${at58} search: the history numbered anew drops the matches and says so`,
    await page.evaluate(() => /history changed/.test(document.getElementById("findn").textContent) && document.getElementById("findrail").hidden));
  await page.fill("#findq", "zzzz");
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => document.getElementById("findn").textContent === "Not found", null, { timeout: 5000 }).catch(() => {});
  const none = await page.evaluate(() => ({ n: document.getElementById("findn").textContent, rail: document.getElementById("findrail").hidden }));
  await page.click("#findclose");
  const shut = await page.evaluate(() => ({ box: document.getElementById("findbox").hidden, rail: document.getElementById("findrail").hidden, hl: !!CSS.highlights?.get("find")?.size }));
  check(`${at58} search: nothing found says so; × closes the field, its marks and its highlight (the magnifier is out of sight while it is open)`,
    none.n === "Not found" && none.rail && shut.box && shut.rail && !shut.hl, JSON.stringify({ none, shut }));

  // a part of its text (rendered Markdown lacks a link's address); text not found says so
  await fromPane(page, "guide");
  await page.waitForFunction(() => /install steps/.test(document.getElementById("markline")?.range?.toString() ?? ""), null, { timeout: 8000 }).catch(() => {});
  m = await marked(page);
  check(`${at} found by a part of its text, that part is marked`, /for the install steps$/.test(m?.text ?? "") && !/https/.test(m.text) && inView(m), JSON.stringify(m));
  // longer than the 200 letters it is found by: the line covers it all, both paragraphs
  await fromPane(page, "long answer");
  await page.waitForFunction(() => /closing words of the answer$/.test(document.getElementById("markline")?.range?.toString() ?? ""), null, { timeout: 8000 }).catch(() => {});
  m = await marked(page);
  const lastRow = await page.evaluate(() => { const row = [...document.querySelectorAll("#term .ln")].find((el) => el.textContent.includes("closing words"));
    const r = row.getClientRects()[0], l = document.getElementById("markline").getBoundingClientRect(); return { rowBottom: Math.round(r.bottom), lineBottom: Math.round(l.bottom) }; });
  check(`${at} a bookmark longer than the letters it is found by: the wavy line covers its whole text, to its last row`,
    /^Long answer/.test(m?.text ?? "") && /closing words of the answer$/.test(m.text) && m.line && lastRow.lineBottom >= lastRow.rowBottom - 1, JSON.stringify({ m, lastRow }));
  await fromPane(page, "phantom");
  await page.waitForFunction(() => !document.getElementById("mbwhy").hidden, null, { timeout: 5000 }).catch(() => {});
  check(`${at} its text not on the line the bridge named: the bar says so`, /not found there/.test(await page.textContent("#mbwhy")));

  // the view is the user's again: the end of the output, another pane
  await page.click('#msgrail .dot[title*="Bookmark: ask about the retry loop"]');
  await page.waitForFunction(() => document.getElementById("markline")?.range?.toString().includes("retry loop"), null, { timeout: 8000 }).catch(() => {});
  await page.click("#bottom");
  say({ t: "screen", sid: "S1", full: false, n: 2, cols: 80, rows: 2, ch: [[0, line("after the jump")]] });
  await page.waitForFunction(() => [...document.querySelectorAll("#screen .ln")].some((el) => el.textContent === "after the jump"));
  await page.waitForTimeout(300);
  check(`${at} after the end-of-output button the view follows the output, not the bookmark`,
    await page.evaluate(() => { const t = document.getElementById("term"); return t.scrollHeight - t.scrollTop - t.clientHeight < 8; }));
  // a bookmark made on the screen, its rows then redrawn (Claude Code's /clear: no history reset): its line goes with its text
  await selectRow(page, "after the jump");
  await page.click("#markadd");
  await page.waitForSelector("#markbox[open]");
  await page.click('#markbox button[value="save"]');
  const lineOn = (ms) => page.waitForFunction(() => { const l = document.getElementById("markline"); return !l.hidden && l.range?.toString() === "after the jump"; }, null, { timeout: ms }).then(() => true, () => false);
  const onScreen = await lineOn(5000);
  say({ t: "screen", sid: "S1", full: false, n: 2, cols: 80, rows: 2, ch: [[0, line("")], [1, line("")]] });
  await page.waitForFunction(() => ![...document.querySelectorAll("#screen .ln")].some((el) => el.textContent === "after the jump"));
  await page.waitForTimeout(150);
  check(`${at} a bookmark's wavy line on the screen goes when its rows are redrawn without its text (a cleared screen)`,
    onScreen && await page.evaluate(() => document.getElementById("markline").hidden), JSON.stringify({ onScreen }));
  say({ t: "screen", sid: "S1", full: false, n: 2, cols: 80, rows: 2, ch: [[1, line("after the jump")]] });
  const returned = await lineOn(3000);
  check(`${at} its text back just under its line: the wavy line is back on it`, returned);
  await page.click('#sessions .item[data-id="S2"]');
  await page.waitForFunction(() => [...document.querySelectorAll("#screen .ln")].some((el) => el.textContent === "two"));
  check(`${at} a pane without bookmarks has no bookmarks button`, await page.evaluate(() => document.getElementById("marksfab").hidden));
  check(`${at} another pane: no bar, no mark from the last one`,
    await page.evaluate(() => document.getElementById("markbar").hidden && (document.getElementById("markline")?.hidden ?? true)));
  // in one project too, a session shows only its own (AC-33): the other's is not there, and in the
  // list below the sessions a press shows that session at its text
  say({ t: "projects", items: [{ id: "P", name: "Work", color: 0, host: null, dir: "~/p", members: ["S1", "S2"] }] });
  say({ t: "mark", item: { ...ITEMS[0], project: "P" } });
  await page.waitForTimeout(200);
  const notHere = await page.evaluate(() => document.getElementById("marksfab").hidden);
  await pressFoot(page, "#marksbtn");
  await page.waitForFunction(() => document.getElementById("marksdlg").open, null, { timeout: 3000 }).catch(() => {});
  await page.click('#marklist .mkitem:has-text("ask about the retry loop")');
  await page.waitForFunction(() => document.getElementById("markline")?.range?.toString().includes("retry loop"), null, { timeout: 8000 }).catch(() => {});
  m = await marked(page);
  check(`${at} a bookmark of another session of the project is not this one's; from the list below the sessions it shows its session and its text`, notHere && m?.text === "the retry loop never resets" && inView(m)
    && await page.evaluate(() => document.querySelector("#sessions .item.on")?.dataset.id === "S1"), JSON.stringify({ m, notHere }));
  say({ t: "projects", items: [] });                // its session closed: terminumd orphans its bookmark
  say({ t: "mark", item: { ...ITEMS[0], project: "P", orphaned: at0 + 10 } });
  await page.waitForFunction(() => !document.querySelector('#msgrail .dot[title*="Bookmark: ask about the retry loop"]'), null, { timeout: 3000 }).catch(() => {});   // the strip: at the next frame
  check(`${at} its session gone, the bookmark is orphaned: off the session, in the list below the sessions`,
    await page.evaluate(() => !document.querySelector('#msgrail .dot[title*="Bookmark: ask about the retry loop"]')), "");

  // lines that are gone, a session that closed
  await fromPane(page, "gone");
  await page.waitForFunction(() => !document.getElementById("mbwhy").hidden, null, { timeout: 5000 }).catch(() => {});
  check(`${at} lines no longer in the history: the bar says so and shows the text and the note`,
    (await page.textContent("#mbwhy")).startsWith(GONE) && (await page.textContent("#mbtext")) === "never printed anywhere" && (await page.textContent("#mbnote")) === "gone");
  await pressFoot(page, "#marksbtn");
  const closed = await page.evaluate(() => [...document.querySelectorAll("#marklist .mkitem")].find((b) => b.textContent.includes("closed one"))?.querySelector(".closed")?.textContent);
  await page.click('#marklist .mkitem:has-text("closed one")');
  check(`${at} an orphaned bookmark (its session closed) is listed as orphaned; picked, the window closes and its text shows`,
    closed === "orphaned" && !await page.evaluate(() => document.getElementById("marksdlg").open) && /Its session is gone/.test(await page.textContent("#mbwhy")) && (await page.textContent("#mbtext")) === "from a session that closed");

  // Delete asks again; a save the Mac refuses keeps the dialog with why
  await page.click("#mbdel");
  check(`${at} Delete pressed once only asks`, !sent.some((x) => x.t === "unmark"));
  await page.click("#mbdel");
  await page.waitForFunction(() => document.getElementById("markbar").hidden, null, { timeout: 3000 }).catch(() => {});
  check(`${at} … pressed again it deletes and the bar closes`, sent.some((x) => x.t === "unmark" && x.id === "E") && await page.evaluate(() => document.getElementById("markbar").hidden));
  await selectRow(page, "out 2600 lorem");
  await page.click("#markadd");
  await page.fill("#marknote", "FAIL");
  await page.click('#markbox button[value="save"]');
  await page.waitForSelector("#markerr:not([hidden])", { timeout: 3000 }).catch(() => {});
  check(`${at} a save the Mac refuses keeps the dialog open and says why`,
    /at most/.test(await page.textContent("#markerr")) && await page.evaluate(() => document.getElementById("markbox").open));
  await page.click('#markbox button[value="cancel"]');
  check(`${at} Cancel closes it`, !await page.evaluate(() => document.getElementById("markbox").open));
  // a reminder made the same way: its line is the reminders' pink (it was the bookmarks' orange)
  await selectRow(page, "out 2600 lorem");
  await page.click("#markadd");
  await page.click('#markkind button[data-kind="reminder"]');
  await page.fill("#marknote", "check this later");
  await page.press("#marknote", "Control+Enter");                // Ctrl+Enter saves as ⌘Enter
  await page.waitForFunction(() => !document.getElementById("markbox").open, null, { timeout: 5000 }).catch(() => {});
  const pink = await lineColor(page);
  check(`${at} a reminder just saved (Ctrl+Enter) has its wavy line in the reminders' pink`, pink.shown && pink.fill === pink.remind, JSON.stringify(pink));
  // brought into view from its mark on the strip, after a bookmark's: pink again, the bookmark's orange
  await page.click("#mbclose").catch(() => {});
  await page.waitForFunction(() => document.querySelector('#msgrail .dot[title*="check this later"]'), null, { timeout: 3000 }).catch(() => {});
  await page.click('#msgrail .dot[title*="look again"]');
  await page.waitForFunction(() => document.getElementById("markline")?.range?.toString().includes("out 2500"), null, { timeout: 8000 }).catch(() => {});
  const shownMark = await lineColor(page);
  await page.click('#msgrail .dot[title*="check this later"]');
  await page.waitForFunction(() => document.getElementById("markline")?.range?.toString().includes("out 2600"), null, { timeout: 8000 }).catch(() => {});
  const shownRemind = await lineColor(page);
  check(`${at} brought into view, a bookmark's line is orange and a reminder's pink`,
    shownMark.fill === shownMark.mark && shownRemind.shown && shownRemind.fill === shownRemind.remind, JSON.stringify({ shownMark, shownRemind }));
  await page.click("#mbclose").catch(() => {});

  // a selection in a widget's rendered text, by a mouse drag (WebKit keeps a script's range in a
  // shadow root out of the document's selection); the widget is drawn once near the view
  await page.click('#msgrail .dot[title*="Bookmark: rendered"]');
  await page.waitForFunction(() => { const r = document.getElementById("markline")?.range; return r?.startContainer?.getRootNode() instanceof ShadowRoot; }, null, { timeout: 8000 }).catch(() => {});
  await page.click("#mbclose");
  const box = await page.evaluate(() => {
    const host = [...document.querySelectorAll("#term .ln.wg-host")].find((h) => h.shadowRoot.querySelector('.wg[data-kind="markdown"] p'));
    host.scrollIntoView({ block: "center" });
    getSelection().removeAllRanges();
    const r = host.shadowRoot.querySelector('.wg[data-kind="markdown"] p').getBoundingClientRect();
    return { x: r.left + 1, y: (r.top + r.bottom) / 2, to: r.left + Math.min(r.width - 2, 400) };
  });
  await page.mouse.move(box.x, box.y);
  await page.mouse.down();
  await page.mouse.move(box.to, box.y, { steps: 8 });
  await page.mouse.up();
  const picked = await page.waitForSelector("#markadd:not([hidden])", { timeout: 3000 }).then(() => true, () => false);
  say({ t: "screen", sid: "S1", full: false, n: 2, cols: 80, rows: 2, ch: [[0, line("more output")]] });
  await page.waitForSelector("#paused:not([hidden])", { timeout: 3000 }).catch(() => {});
  check(`${at} while text in a widget is selected the terminal pauses (its lines stay as selected)`,
    await page.evaluate(() => !document.getElementById("paused").hidden && ![...document.querySelectorAll("#screen .ln")].some((el) => el.textContent === "more output")));
  if (picked) await page.click("#markadd");
  const fromWidget = picked && await page.textContent("#marktext");
  check(`${at} text selected in a rendered widget gets [+] too, with its rendered text`, /^A small tool/.test(fromWidget ?? ""), String(fromWidget));
  if (picked) await page.click('#markbox button[value="cancel"]');

  await askChecks({ page, sent, say, at, layout: LAYOUT, selectRow });   // the dialog (e2e_ask.mjs)
  check(`${at} no page error or CSP violation`, !errors.length && !(await page.evaluate(() => window.csp)).length, JSON.stringify([errors, await page.evaluate(() => window.csp)]));
  await page.close();
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const pw = await playwright();
  const engine = process.argv[2] ?? "chromium";
  const browser = await pw[engine].launch();
  try { await webMarks(browser, engine === "chromium" ? "" : ` (${engine})`); } finally { await browser.close(); cleanup(); }
  const failed = results.filter((r) => !r.ok).length;
  console.log(`${results.length - failed}/${results.length} passed`);
  process.exit(failed ? 1 : 0);
}
