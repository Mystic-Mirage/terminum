// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Checks of sizing (AC-13), the pane as tmux has it (AC-04), full-screen programs (AC-05) and the
// mouse for them (AC-06) against a fake terminumd (fake.mjs), at a wide and a phone size: the
// size messages and when they go, the alternate screen, too small a view, a wider pane scaled, a
// pane sized for another screen, and the mouse reports of clicks, drags, the wheel and taps.

import { check, runningIn, slack, until } from "./check.mjs";
import { drawn, LAYOUT, line, openPage, pause, SIZES } from "./fake.mjs";

const SID = "tmux:local:terminum:%1";
const ROWS = Array.from({ length: 6 }, (_, i) => line(`${i}123456789012345678901234567890`));

export async function webScreen(browser, name) {
  for (const [size, device] of SIZES) {
    runningIn(`${name} ${size}`);
    const phone = size.startsWith("390");
    const f = await openPage(browser, { device });
    const { page, sent } = f;
    await drawn(page);
    await pause(page, 400);
    const sizes = () => sent.filter((m) => m.t === "size");
    const frame = (extra) => f.say({ t: "screen", sid: SID, full: true, n: ROWS.length, cols: 80, rows: ROWS.length, alt: false, ch: ROWS.map((l, i) => [i, l]), ...extra });
    // the grid at the chosen text size, measured apart from the page's own code
    const fits = () => page.evaluate(() => {
      const t = document.getElementById("term"), cs = getComputedStyle(t), px = parseFloat(cs.getPropertyValue("--size"));
      const probe = document.createElement("span");
      probe.textContent = "0".repeat(100); probe.style.cssText = `position:absolute;visibility:hidden;white-space:pre;font-size:${px}px`;
      t.append(probe); const cell = probe.getBoundingClientRect().width / 100; probe.remove();
      const w = t.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight), h = t.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
      return { cols: Math.floor(w / cell), rows: Math.floor(h / (px * 1.25)) };
    });

    const subAt = sent.findIndex((m) => m.t === "sub"), first = sizes()[0], want = await fits();
    check("AC-13 size: after opening a pane the page sends the grid that fits its terminal at the chosen text size",
      sent.indexOf(first) > subAt && first.typing === false && !("screen" in first) && Math.abs(first.cols - want.cols) <= 1
        && Math.abs(first.rows - want.rows) <= 1, JSON.stringify({ first, want }));
    // until the view's sizes settle (a slow browser lays the page out late), then type
    for (let n = -1; n !== sizes().length;) { n = sizes().length; await pause(page, 600); }
    const n0 = sizes().length;
    await page.evaluate(() => document.getElementById("kbd").focus());
    await page.keyboard.type("ab");
    await pause(page);
    const typing = sizes().slice(n0);
    check("AC-13 size: the first key after a pause sends the size as typing, once a second at most",
      typing.length === 1 && typing[0].typing === true, JSON.stringify(typing));
    await page.click("#menubtn");
    // two changes at once, the menu closed with no key (a second after the last, one sends the size as
    // typing); counted in the page: its timers keep their order however late they run, the sends reach us later
    const early = await page.evaluate(() => new Promise((done) => {
      const send = WebSocket.prototype.send;
      let n = 0;
      WebSocket.prototype.send = function (m) { if (JSON.parse(m).t === "size") n++; return send.call(this, m); };
      for (const id of ["bigger", "bigger", "menubtn"]) document.getElementById(id).click();
      setTimeout(() => { WebSocket.prototype.send = send; done(n); }, 150);
    }));
    await until(() => sizes().length >= n0 + 2, slack(8000));
    const bigger = sizes().at(-1);
    check("AC-13 size: a bigger text size sends the smaller grid, 300 ms after the last change",
      early === 0 && sizes().length === n0 + 2 && bigger.cols < first.cols && bigger.typing === false, JSON.stringify({ early, bigger }));

    // a full-screen program on the alternate screen: its grid alone, and back
    frame({ alt: true });
    await pause(page, 200);
    const on = await page.evaluate(() => ({ full: document.getElementById("term").classList.contains("fullscreen"),
      hist: getComputedStyle(document.getElementById("hist")).display, modes: document.querySelectorAll("[data-mode], [data-screen], #resize, #scrlabel").length,
      mark: ((m) => m.getBoundingClientRect().width > 0 && m.title)(document.getElementById("curscreen")) }));
    // a coding agent's full-screen TUI: an orange ! beside the mark says why to turn it off, on hover and on a press
    const why = () => page.evaluate(() => ((b) => ({ shown: b.getBoundingClientRect().width > 0, title: b.title, color: getComputedStyle(b).color }))(document.getElementById("curscreenwhy")));
    // a screen whose program shows the terminal's cursor; the ! changes once it held 400 ms (sizing.js)
    const cursorShown = () => frame({ alt: true, ch: ROWS.map((l, i) => [i, i ? l : { r: [[" ", null, null, 256]], e: true }]) });
    const held = () => pause(page, 600);
    const plain = await why();
    f.say({ t: "layout", groups: LAYOUT.map((g) => ({ ...g, items: g.items.map((it) => (it.id === SID ? { ...it, agent: "claude" } : it)) })) });
    await held();
    const agent = await why();
    if (agent.shown) await page.click("#curscreenwhy");
    const told = await page.textContent("#toast");
    // a toast stands over the terminal's top, under the header, not over the agent's input at the bottom
    const atTop = await page.evaluate(() => ((t, m) => ({ gap: Math.round(t.top - m.top), shown: t.height > 0 }))(document.getElementById("toast").getBoundingClientRect(), document.getElementById("term").getBoundingClientRect()));
    check("a toast shows at the terminal's top, under the header", atTop.shown && atTop.gap >= 0 && atTop.gap <= 24, JSON.stringify(atTop));
    // a program the agent starts in its pane (Claude Code's Ctrl+G editor) shows the terminal's cursor: no !
    cursorShown();
    await held();
    const editor = await why();
    frame({ alt: true });                                    // a redraw hides the cursor for a moment: the ! stays away
    await pause(page, 100);
    cursorShown();
    await held();
    const blip = await why();
    frame({ alt: true });
    await held();
    const back = await why();
    frame({ alt: false });
    await held();
    const off = await page.evaluate(() => ({ full: document.getElementById("term").classList.contains("fullscreen"), hist: getComputedStyle(document.getElementById("hist")).display,
      mark: document.getElementById("curscreen").getBoundingClientRect().width }));
    const agentOff = await why();
    f.say({ t: "layout", groups: LAYOUT });
    check("AC-05 a coding agent's full-screen TUI: an orange ! beside the mark, its why on hover and as a toast on a press; none for a program, its Ctrl+G editor (the terminal's cursor shown, a redraw's moment without it too) or off the alternate screen",
      !plain.shown && agent.shown && /fullscreen mode off/.test(agent.title) && agent.color === "rgb(255, 159, 26)" && told === agent.title && !editor.shown && !blip.shown && back.shown && !agentOff.shown,
      JSON.stringify({ plain, agent, told, editor, blip, back, agentOff }));
    check("AC-05 a full-screen program hides the history; no view modes, Screen mode or Fit to this screen are offered",
      on.full && on.hist === "none" && on.modes === 0, JSON.stringify(on));
    check("AC-05 off the alternate screen the history is back", !off.full && off.hist !== "none", JSON.stringify(off));
    check("AC-05 the header marks a full-screen program (\"Full screen mode\") while it runs, and not after", on.mark === "Full screen mode" && off.mark === 0, JSON.stringify({ on, off }));

    // the mouse for a full-screen program
    frame({ alt: true, mouse: true });
    await pause(page, 200);
    const at = (x, y) => page.evaluate(([x, y]) => {
      const t = document.querySelectorAll("#screen .ln")[y].querySelector("span").firstChild, r = new Range();
      r.setStart(t, x); r.setEnd(t, x + 1);
      const b = r.getBoundingClientRect();
      return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
    }, [x, y]);
    const mice = () => sent.filter((m) => m.t === "mouse").map((m) => `${m.action}:${m.button}@${m.x},${m.y}` + (m.count ? `x${m.count}` : ""));
    const stepsIn = (list) => list.reduce((n, e) => n + Number(e.match(/x(\d+)$/)?.[1] ?? 1), 0);
    const n1 = mice().length;
    if (!phone) {
      const a = await at(10, 1), b = await at(14, 1);
      await page.mouse.click(a.x, a.y);
      await page.mouse.move(a.x, a.y);
      await page.mouse.down();
      await page.mouse.move(b.x, b.y, { steps: 3 });
      await page.mouse.up();
      await page.mouse.click(a.x, a.y, { button: "right" });
      await page.mouse.move(a.x, a.y);
      await page.mouse.wheel(0, -120);
      await page.mouse.wheel(0, 120);
      await pause(page, 200);
      const got = mice().slice(n1);
      const shift = sent.filter((m) => m.t === "mouse")[n1];
      check("AC-06 mouse: a click, a drag, a right click and the wheel go to the program with 0-based cells",
        got.filter((e) => !e.startsWith("move")).join() === ["press:0@10,1", "release:0@10,1", "press:0@10,1", "release:0@14,1", "press:2@10,1",
          "release:2@10,1", "wheelup:0@10,1", "wheeldown:0@10,1"].join() && got.includes("move:0@14,1") && shift?.mods?.shift === false, JSON.stringify(got));
      await page.click("#filter");                  // the keys elsewhere, then a click on the program
      await page.mouse.click(a.x, a.y);
      const n5 = sent.filter((m) => m.t === "in").length;
      await page.keyboard.type("q");
      await pause(page);
      const keys = sent.filter((m) => m.t === "in").slice(n5).map((m) => m.data).join("");
      check("AC-06 mouse: a click on the program gives the terminal the keys (OpenCode took no typing)", keys === "q", JSON.stringify(keys));
      const n4 = mice().length;
      await page.keyboard.down("Shift");
      await page.mouse.click(a.x, a.y);
      await page.keyboard.up("Shift");
      await pause(page);
      check("AC-06 mouse: Shift held selects text instead (no press sent)", mice().length === n4, JSON.stringify(mice().slice(n4)));
      // a trackpad: 24 small events, 120 px in all, are a step a text line, sent as one message
      const n2 = mice().length;
      const lines = await page.evaluate(([x, y]) => {
        const term = document.getElementById("term");
        for (let i = 0; i < 24; i++) term.dispatchEvent(new WheelEvent("wheel", { deltaY: 5, deltaMode: 0, clientX: x, clientY: y, bubbles: true, cancelable: true }));
        return Math.floor(120 / parseFloat(getComputedStyle(term).lineHeight));
      }, [a.x, a.y]);
      await pause(page, 200);
      const pad = mice().slice(n2);
      check("AC-06 mouse: a trackpad's small wheel events are a step a text line, a frame's steps one message with their count",
        pad.length === 1 && pad[0].startsWith("wheeldown:0@10,1") && stepsIn(pad) === lines, JSON.stringify({ pad, lines }));
    } else {
      const a = await at(3, 2);
      await page.touchscreen.tap(a.x, a.y);
      await pause(page, 200);
      // a two-finger tap, a swipe and a long press, as touch events (WebKit makes no Touch objects)
      const touches = (steps) => page.evaluate(async (steps) => {
        const term = document.getElementById("term");
        for (const [type, pts, wait] of steps) {
          const list = pts.map(([x, y]) => ({ clientX: x, clientY: y, target: term }));
          const e = new Event(type, { bubbles: true, cancelable: true });
          Object.defineProperties(e, { touches: { value: type === "touchend" ? [] : list }, changedTouches: { value: list } });
          term.dispatchEvent(e);
          if (wait) await new Promise((r) => setTimeout(r, wait));
        }
      }, steps);
      const p = [a.x, a.y], q = [a.x + 30, a.y];
      await touches([["touchstart", [p]], ["touchstart", [p, q]], ["touchend", [], 50]]);
      await touches([["touchstart", [p]], ["touchmove", [[a.x, a.y - 40]]], ["touchmove", [[a.x, a.y - 80]]], ["touchend", []]]);
      await touches([["touchstart", [p], 600], ["touchend", []]]);
      await pause(page, 200);
      const got = mice().slice(n1);
      check("AC-06 touch: a tap is a click, a two-finger tap a right click, a swipe the wheel, a long press nothing",
        got.slice(0, 4).join() === "press:0@3,2,release:0@3,2,press:2@3,2,release:2@3,2" && stepsIn(got.slice(4)) >= 2
          && got.slice(4).every((e) => e.startsWith("wheeldown:0@3,2")), JSON.stringify(got));
    }
    frame({ alt: true, mouse: false });
    await pause(page);
    const n2 = mice().length, c = await at(5, 0);
    await page.mouse.click(c.x, c.y);
    await pause(page);
    check("AC-06 mouse: a program that did not ask for the mouse gets no events", mice().length === n2);

    // a pane wider than the view (one this page does not size, AC-89): on the normal screen
    // re-flowed at the chosen size; a full-screen program scaled to the view's width down to 7 px,
    // past that at the chosen size, scrolling sideways
    const wide = (cols, alt = true) => frame({ alt, mouse: false, cols, ch: ROWS.map((_, i) => [i, line(`${i}`.padEnd(cols, "x"))]) });
    const shown = () => page.evaluate(() => {
      const t = document.getElementById("term"), cs = getComputedStyle(t);
      return { px: parseFloat(cs.fontSize), size: parseFloat(cs.getPropertyValue("--size")), sideways: t.scrollWidth > t.clientWidth + 1 };
    });
    const look = async (cols, alt) => { wide(cols, alt); await pause(page, 200); return shown(); };
    f.say({ t: "layout", groups: [{ ...LAYOUT[0], attached: "default" }, ...LAYOUT.slice(1)] });
    await pause(page);
    const flowed = await look(phone ? 60 : 160, false), flowedHuge = await look(600, false);
    check("AC-04 a pane wider than the view on the normal screen re-flows to it at the chosen size, never scrolled sideways",
      [flowed, flowedHuge].every((s) => s.px === s.size && !s.sideways), JSON.stringify({ flowed, flowedHuge }));
    const scaled = await look(phone ? 60 : 160), huge = await look(600);
    check("AC-04 a full-screen program wider than the view is scaled to its width (7 px at least); wider still, the chosen size with sideways scrolling",
      scaled.px < scaled.size && scaled.px >= 7 && !scaled.sideways && huge.px === huge.size && huge.sideways, JSON.stringify({ scaled, huge }));
    f.say({ t: "layout", groups: LAYOUT });
    frame({ alt: true, mouse: false });

    // sized for another screen: said, and drawn as it is, until this page types
    f.say({ t: "sized", sid: SID, cols: 180, rows: 50, mine: false });
    await pause(page);
    const sized = await page.evaluate(() => ({ text: document.getElementById("sized").textContent, shown: !document.getElementById("sized").hidden }));
    await page.evaluate(() => document.getElementById("kbd").focus());
    await page.keyboard.type("x");
    await pause(page);
    check("AC-13 sized: another screen's size is said until this page types",
      sized.shown && sized.text === "Sized for another screen (180×50)" && await page.isHidden("#sized"), JSON.stringify(sized));
    f.say({ t: "sized", sid: SID, cols: 180, rows: 50, mine: true, error: "size too small" });
    await pause(page);
    const refused = await page.evaluate(() => !document.getElementById("sized").hidden && document.getElementById("sized").textContent);
    f.say({ t: "sized", sid: SID, cols: 40, rows: 20, mine: true });
    await pause(page);
    check("AC-13 sized: a refused resize is said with tmux's reason; this page's own size clears it",
      refused === "tmux did not resize this pane: size too small" && await page.isHidden("#sized"), JSON.stringify(refused));
    f.say({ t: "sized", sid: "tmux:devbox:default:%9", cols: 129, rows: 68, mine: false, error: "tmux exited" });
    await pause(page);
    check("AC-13 sized: an answer for a pane left meanwhile says nothing over this one", await page.isHidden("#sized"));

    // a sliver of a view (under 20×5: an animation, a drag) keeps the pane's last size (big
    // text first, its size settled; then a narrow view in one step); terminumd bounds the rest
    await page.click("#menubtn");
    for (let i = 0; i < 16; i++) await page.click("#bigger");
    await page.keyboard.press("Escape");
    await pause(page, 600);
    const n3 = sizes().length;
    await page.setViewportSize({ width: 300, height: device.viewport.height });
    await pause(page, 600);
    check("AC-13 size: a view under 20×5 keeps the pane's last size", sizes().length === n3, JSON.stringify({ more: sizes().slice(n3) }));
    check("AC-05 no page errors", !f.errors.length, f.errors.join(" | "));
    await f.close();
  }
  runningIn(name);
}
