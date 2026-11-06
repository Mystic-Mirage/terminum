// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Layout guards (docs/UI-GUIDELINES.md §6) against a fake terminumd (fake.mjs), at a wide and a
// phone size: controls of one kind look alike, a primary button stands out from Cancel, a
// phone's targets are finger-sized, a long toast stays on the screen, waiting cards never cover
// the banners. They check computed styles and boxes, which behavior checks never look at.

import { check, runningIn } from "./check.mjs";
import { drawn, group, item, LAYOUT, openPage, pause, SIZES, pressFoot } from "./fake.mjs";

const HOST = group("devbox", [item("tmux:devbox:default:%1", "build", { host: "devbox" })],
  { host: "devbox", session: "terminum", state: "attached", others: 2, server: "devbox:default", attach: "ssh -t -- devbox \"tmux attach -t '=terminum'\"" });
const ASKING = group("asks", [item("tmux:local:terminum:%7", "claude", { agent: "claude", shell: false, state: "waiting" })]);
const AGENTS = group("agents", [item("tmux:local:terminum:%8", "Claude Code", { agent: "claude", shell: false }),
  item("tmux:local:terminum:%9", "Codex", { agent: "codex", shell: false, index: "2" }),
  item("tmux:local:terminum:%10", "OpenCode", { sub: ["opencode", "~/p"], shell: false, index: "3" }), item("tmux:local:terminum:%11", "OpenCode", { index: "4" })]);

/** Every visible button under `sel` smaller than `least` px tall: "id-or-class:height" (a closed
 *  dialog in Safari stays laid out, out of sight: motion.css). */
const small = (page, sel, least) => page.evaluate(([sel, least]) => [...document.querySelectorAll(sel)]
  .filter((b) => b.offsetParent && b.getClientRects().length && getComputedStyle(b).visibility === "visible")
  .map((b) => [b.id || b.className || b.textContent.trim().slice(0, 20), Math.round(b.getBoundingClientRect().height)])
  .filter(([, h]) => h < least).map(([n, h]) => `${n}:${h}`), [sel, least]);

export async function webLook(browser, name) {
  for (const [size, device] of SIZES) {
    runningIn(`${name} ${size}`);
    const phone = size.startsWith("390");
    const f = await openPage(browser, { device, layout: [...LAYOUT, HOST, AGENTS] });
    const { page } = f;
    await drawn(page);

    const dialogs = await page.evaluate(() => [...document.querySelectorAll("dialog")].filter((d) => d.querySelector(".actions .primary")).map((d) => {
      const fill = (b) => getComputedStyle(b).backgroundColor;
      const primary = d.querySelector(".actions .primary"), other = d.querySelector(".actions button:not(.primary)");
      return [d.id, fill(primary), other && fill(other)];
    }));
    // what runs in a pane: a coding agent's mark (an image of web/static/agents), else the terminal icon; the header says it too
    await page.waitForFunction(() => [...document.querySelectorAll("#sessions .agmark img")].every((i) => i.complete), null, { timeout: 5000 }).catch(() => {});
    const marks = await page.evaluate(() => Object.fromEntries([...document.querySelectorAll("#sessions .item")].map((b) => {
      const m = b.querySelector(".head .agmark"), img = m?.querySelector("img");
      return [b.dataset.id.split(":").pop(), img ? `${img.getAttribute("src")}:${img.naturalWidth > 0}` : m?.querySelector("svg.lu") ? "terminal" : "-"];
    })));
    const header = await page.evaluate(() => document.querySelector("#curagent svg.lu, #curagent img")?.tagName);
    check("look: a row shows Claude Code's, Codex's or OpenCode's mark (by its program, not a shell's title), else the terminal icon; the header shows the shown pane's",
      marks["%8"] === "agents/claude.svg:true" && marks["%9"] === "agents/codex.svg:true" && marks["%10"] === "agents/opencode.svg:true"
        && marks["%11"] === "terminal" && marks["%1"] === "terminal" && !!header, JSON.stringify({ marks, header }));

    check("look: every dialog's primary button is filled apart from its Cancel", dialogs.length >= 3 && dialogs.every(([, p, c]) => c && p !== c),
      JSON.stringify(dialogs));
    // one kind (UI-GUIDELINES §5): every dialog and foot form has a title and × in its head, its fields
    // one look, Cancel before the primary; the primary in the hue of what opened it
    const kind = await page.evaluate(() => {
      const st = (el) => { const c = getComputedStyle(el); return [c.fontFamily, c.fontSize, c.paddingTop, c.paddingLeft, c.backgroundColor, c.borderRadius].join("|"); };
      const boxes = [...document.querySelectorAll("dialog.dlg, .footform")];
      const shown = boxes.map((b) => { const was = b.hidden; b.hidden = false; return was; });   // a hidden form computes too, but its fields must be laid out
      const out = {
        heads: boxes.filter((b) => !(b.querySelector(".dhead h2") && b.querySelector(".dhead .x"))).map((b) => b.id),
        fields: [...new Set([...document.querySelectorAll(":is(dialog.dlg, .footform) .field:not(.lead)")].map(st))],
        order: [...document.querySelectorAll(":is(dialog.dlg, .footform) .actions")].filter((a) => a.lastElementChild?.classList.contains("primary") === false).map((a) => a.parentElement.closest("[id]").id),
        ask: [getComputedStyle(document.querySelector("#askbox .primary")).backgroundColor, getComputedStyle(document.getElementById("askfab")).backgroundColor],
      };
      boxes.forEach((b, i) => { b.hidden = shown[i]; });
      return out;
    });
    check("look: every dialog and foot form has a title and × in its head; its fields look alike; Cancel, then the primary",
      kind.heads.length === 0 && kind.fields.length === 1 && kind.order.length === 0, JSON.stringify(kind));
    check("look: Ask's primary is the Ask button's blue", kind.ask[0] === kind.ask[1], kind.ask.join(" vs "));
    // one type in every dialog: its text, fields and buttons in the terminal's font at its size (a field of 16 px stood out)
    const type = await f.page.evaluate(() => [...document.querySelectorAll("dialog:not(#rowmenu)")].map((d) => {
      d.showModal();
      const cs = (e) => `${getComputedStyle(e).fontSize} ${getComputedStyle(e).fontFamily.split(",")[0]}`;
      const want = cs(d), odd = [...d.querySelectorAll("input:not([type=checkbox]), textarea, .actions button")].map((e) => [e.id || e.textContent.trim(), cs(e)]).filter(([, c]) => c !== want);
      d.close();
      return [d.id, want, odd];
    }));
    check("look: every dialog's fields and buttons share its type (the terminal's font, its size)", type.length >= 6 && type.every(([, w, odd]) => /TermFont/.test(w) && !odd.length),
      JSON.stringify(type));

    if (phone) { await page.click("#navbtn"); await pause(page, 300); }
    const head = await page.evaluate(() => [...document.querySelectorAll('#sessions .grp[data-wid="devbox/terminum"] h2 button:not(.toggle)')].map((b) => {
      const s = getComputedStyle(b);
      return [b.className, `${s.width} ${s.height} ${s.borderRadius} ${s.color}`];
    }));
    check("look: a group header's buttons (copy, import, New Window, ⋯) share one size, radius and color",
      head.length === 4 && head[0][0] === "new" && new Set(head.map(([, l]) => l)).size === 1, JSON.stringify(head));
    const counts = await page.evaluate(() => [...document.querySelectorAll("#sessions .grp h2 .count")].filter((c) => c.offsetParent).map((c) => Math.round(c.getBoundingClientRect().right)));
    check("look: the groups' counts end in one column, a header of four buttons among them", counts.length > 1 && Math.max(...counts) - Math.min(...counts) <= 1, JSON.stringify(counts));
    if (phone) {
      const list = await small(page, "#nav button:not(.item)", 40);
      await pressFoot(page, "#addhostbtn");
      const form = await small(page, "#addhost button", 40);
      await page.click("#addhostcancel");
      await page.click("#drawerclose");
      await pause(page, 300);
      const bar = await small(page, "#bar button:not(.seg button)", 40);
      await page.click("#menubtn");
      const menu = await small(page, "#menu .row, #menu button:not(.seg button)", 40);
      const segs = await small(page, ".seg button", 34);
      await page.keyboard.press("Escape");
      check("look: on a phone every button of the list, a form, the header and the menu is at least 40px tall (a segment 34)",
        !list.length && !form.length && !bar.length && !menu.length && !segs.length, JSON.stringify({ list, form, bar, menu, segs }));
      // the header's title row: ☰, the title block, ✎ and ⋯ on one axis, ✎ as large as ☰ and ⋯; 8 px above it, between the rows and under the views
      const row = await page.evaluate(() => {
        document.getElementById("rename").hidden = false;
        const r = (s) => document.querySelector(s).getBoundingClientRect(), mid = (b) => Math.round(b.top + b.height / 2);
        const bar = r("#bar"), nav = r("#navbtn"), cur = r("#current"), ren = r("#rename"), menu = r("#menubtn"), views = r("#views");
        return { mids: [nav, cur, ren, menu].map(mid), sizes: [nav, ren, menu].map((b) => Math.round(b.height)),
          above: Math.round(nav.top - bar.top), between: Math.round(views.top - nav.bottom), under: Math.round(bar.bottom - views.bottom),
          left: Math.round(nav.left - bar.left), aligned: Math.round(nav.left) === Math.round(views.left) && Math.round(menu.right) === Math.round(views.right),
          clear: ren.right <= menu.left - 8 && ren.left >= cur.left };
      });
      check("look: on a phone the header's title row is on one axis, its keys one size, 8 px above, between and under its rows",
        new Set(row.mids).size === 1 && new Set(row.sizes).size === 1 && row.above === 8 && row.between === 8 && row.under === 8 && row.aligned && row.clear,
        JSON.stringify(row));
    }

    const toast = await page.evaluate(() => {
      const t = document.getElementById("toast");
      t.textContent = "A long message from terminumd: ".repeat(5);
      t.hidden = false;
      const r = t.getBoundingClientRect();
      t.hidden = true;
      return { left: Math.round(r.left), right: Math.round(r.right), width: innerWidth };
    });
    check("look: a long toast wraps inside the screen", toast.left >= 0 && toast.right <= toast.width, JSON.stringify(toast));

    if (!phone) {
      f.say({ t: "layout", groups: [...LAYOUT, HOST, ASKING] });
      f.say({ t: "restore", report: { text: "Restored 2 sessions, 4 windows. ".repeat(3), sessions: 2, windows: 4, programs: [], errors: [], notes: [] } });
      await page.waitForSelector("#asks .ask:not([hidden])", { timeout: 6000 }).catch(() => {});
      const boxes = await page.evaluate(() => {
        const r = (id) => document.getElementById(id)?.getBoundingClientRect();
        const ask = document.querySelector("#asks .ask:not([hidden])")?.getBoundingClientRect(), banner = r("restored");
        const meets = (a, b) => !!(a && b && b.width && a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom);
        const term = r("term"), what = document.querySelector("#asks .ask:not([hidden]) .what"), icon = document.querySelector("#asks .ask:not([hidden]) .state svg");
        return { ask: !!ask, banner: meets(ask, banner), fabs: ["findfab", "splitfab", "sentfab", "attachfab", "marksfab", "remindfab"].filter((id) => meets(ask, r(id))),
          across: !!ask && Math.abs(ask.left - term.left) <= 1 && Math.abs(ask.right - term.right) <= 1 && ask.bottom <= term.top + 1 && ask.top >= banner.bottom - 1,
          oneLine: !!what && what.scrollHeight <= what.clientHeight + 1 && getComputedStyle(what).textOverflow === "ellipsis",
          yellow: !!icon && getComputedStyle(icon).color, shadow: ask && getComputedStyle(document.querySelector("#asks .ask:not([hidden])")).boxShadow };
      });
      check("AC-39 look: a waiting session's line spans the terminal's width at its top, under the banners, one line ending in an ellipsis, no card's shadow; it covers neither the banners nor a floating button",
        boxes.ask && !boxes.banner && !boxes.fabs.length && boxes.across && boxes.oneLine && boxes.shadow === "none", JSON.stringify(boxes));
    }
    await f.close();
  }
}
