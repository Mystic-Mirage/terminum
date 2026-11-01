// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Motion (docs/UI-GUIDELINES.md §2.8) against a fake terminumd (fake.mjs), with motion on: a
// window (a popover, a dialog) flies out of its button and back, a color changes over a moment; a pressed key sinks, a blob pulses, one under the pointer lifts and nods; the
// selected row settles once; nothing moves in the terminal; with reduced motion nothing moves.
// The other checks run with reduced motion, so they measure boxes at their place.

import { check, runningIn, slack } from "./check.mjs";
import { drawn, openPage, pause, WIDE, pressFoot } from "./fake.mjs";

/** The running animations whose target is under `sel`, as "tag#id.class:name". */
// what moves: running, of some length (one of no length only waits, as the list's foot before it folds), not a loop
const moving = (page, sel) => page.evaluate((sel) => document.getAnimations()
  .filter((a) => a.playState === "running" && a.effect?.target?.closest?.(sel) && a.effect.getTiming().iterations !== Infinity && Number(a.effect.getTiming().duration) > 0)
  .map((a) => { const t = a.effect.target; return `${t.tagName}#${t.id}.${t.className}:${a.animationName ?? a.transitionProperty}`; }), sel);

export async function webMotion(browser, name) {
  runningIn(`${name} 1400×900`);
  const f = await openPage(browser, { device: { ...WIDE, reducedMotion: "no-preference" } });
  const { page } = f;
  await drawn(page);

  // a window flies out of the button that opened it: from .3 and a blur, ~10% past its size and back on a
  // spring, its rows coming up one after another; it goes back into the button, fast. `trace` does `act`
  // and samples `sel` for `ms` from the moment it changes: its scale, opacity, blur, whether it is in sight
  // (a closed dialog in Safari stays laid out, out of sight: motion.css), and its first and last row's
  // opacity. Its own transitions and its rows' animations (all CSS: pop.js only sets the origin) are paused
  // and stepped through every 16 ms, so a busy browser's stalled frames never hide the spring or delay "gone".
  const trace = async (sel, ms, act) => {
    await page.evaluate(([sel, ms]) => { window.traced = new Promise((done) => {
      const el = document.querySelector(sel), out = [], rows = [el.firstElementChild, el.lastElementChild];
      const sample = (t) => { const s = getComputedStyle(el), op = (r) => parseFloat(getComputedStyle(r).opacity);
        out.push([parseFloat(s.scale === "none" ? "1" : s.scale), parseFloat(s.opacity), parseFloat(/blur\(([\d.]+)px/.exec(s.filter)?.[1] ?? "0"),
          s.display === "none" || s.visibility === "hidden" ? "gone" : "shown", t, op(rows[0]), op(rows[1])]); };
      const watch = new MutationObserver(() => {
        watch.disconnect();
        getComputedStyle(el).opacity;                     // its style flushed: its transitions begin
        const all = el.getAnimations({ subtree: true });
        all.forEach((a) => a.pause());
        for (let t = 0; ; t = Math.min(t + 16, ms)) { all.forEach((a) => { a.currentTime = t; }); sample(t); if (t === ms) break; }
        all.forEach((a) => a.finish());
        done(out);
      });
      watch.observe(el, { attributes: true });
      setTimeout(() => done(out), 3000);   // it never changed: the checks say so
    }); }, [sel, ms]);
    await act();
    return page.evaluate(() => window.traced);
  };
  /** Where `sel`'s origin is on the screen, at rest: [x, y]. */
  const origin = (sel) => page.evaluate((sel) => { const el = document.querySelector(sel), r = el.getBoundingClientRect(), [x, y] = getComputedStyle(el).transformOrigin.split(" ").map(parseFloat);
    return [Math.round(r.left + x), Math.round(r.top + y), Math.round(r.left + r.width / 2), Math.round(r.top + r.height / 2)]; }, sel);
  const pop = (t) => ({ first: t[0] ?? [], peak: Math.max(...t.map(([s]) => s)), low: Math.min(...t.map(([s]) => s)), last: t.at(-1) ?? [],
    gone: t.find(([, , , g]) => g === "gone")?.[4] ?? null, lag: t.some(([, , , , , a, b]) => a - b > 0.2), frames: t.length });
  const inn = await trace("#menu", 600, () => page.click("#menubtn"));
  const [ox, oy] = await origin("#menu"), btn = await page.$("#menubtn").then((b) => b.boundingBox()), menu = await page.$("#menu").then((b) => b.boundingBox());
  const out = await trace("#menu", 500, () => page.keyboard.press("Escape"));
  const [pin, pout] = [pop(inn), pop(out)];
  check("motion: a menu flies out of its button: from small, blurred and faded, ~10% past its size, then at rest in focus",
    pin.first[0] < 0.5 && pin.first[1] < 1 && pin.first[2] > 0 && pin.peak > 1.04 && pin.peak < 1.15 && pin.last[0] === 1 && pin.last[1] === 1 && pin.last[2] === 0,
    JSON.stringify(pin));
  const [mx0, my0] = [btn.x + btn.width / 2, btn.y + btn.height / 2];
  check("motion: it grows from its button's side: its origin the press, held to 40 px outside it",
    Math.abs(ox - Math.max(menu.x - 40, Math.min(menu.x + menu.width + 40, mx0))) <= 1 && Math.abs(oy - Math.max(menu.y - 40, Math.min(menu.y + menu.height + 40, my0))) <= 1,
    JSON.stringify({ origin: [ox, oy], btn, menu }));
  check("motion: its rows come up one after another", pin.lag && pin.last[5] === 1 && pin.last[6] === 1, JSON.stringify(inn.map((f) => f.slice(4))));
  check("motion: it goes back fast: shown while it goes, shrunk, blurred and faded, gone within 250 ms",
    out[0]?.[3] === "shown" && pout.low < 0.6 && out.some(([, , b]) => b > 0) && pout.gone !== null && pout.gone < 250 && pout.last[3] === "gone", JSON.stringify(pout));
  // a dialog no press opened (a second on) grows from its middle, the same way; its shade fades with it
  await pause(page, 1000);
  const din = await trace("#confirmbox", 500, () => page.evaluate(() => document.getElementById("confirmbox").showModal()));
  const [cx, cy, mx, my] = await origin("#confirmbox");
  const dout = await trace("#confirmbox", 400, () => page.evaluate(() => document.getElementById("confirmbox").close()));
  const [qin, qout] = [pop(din), pop(dout)];
  check("motion: a dialog pops in and out as a window does, from its middle when nothing pressed it open",
    qin.first[0] < 0.5 && qin.peak > 1.04 && qin.last[0] === 1 && Math.abs(cx - mx) <= 1 && Math.abs(cy - my) <= 1 && dout[0]?.[3] === "shown" && qout.low < 0.6 && qout.last[3] === "gone",
    JSON.stringify({ qin, qout, origin: [cx, cy, mx, my] }));
  // a color changes over a moment: a header's count turning the accent
  const shift = await page.evaluate(() => new Promise((done) => {
    const c = document.querySelector("#sessions h2 .count");
    c.classList.add("on");
    const at = [getComputedStyle(c).color];
    requestAnimationFrame(() => requestAnimationFrame(() => { at.push(getComputedStyle(c).color); setTimeout(() => { at.push(getComputedStyle(c).color); c.classList.remove("on"); done(at); }, 400); }));
  }));
  check("motion: a color changes over a moment, not at once", shift[0] !== shift[2] && shift[1] !== shift[2], JSON.stringify(shift));

  // a key sinks into the panel while pressed (its well), nothing scales or ripples; a blob pulses
  await page.click("#foottoggle");                 // the list's foot unfolds first (foot.js)
  await pause(page, 500);
  const box = await page.$("#newsessbtn").then((b) => b.boundingBox());
  await page.mouse.move(box.x + 20, box.y + 10);
  await page.mouse.down();
  await page.waitForFunction(() => /1px 3px/.test(getComputedStyle(document.getElementById("newsessbtn")).boxShadow), null, { timeout: 1000 }).catch(() => {});
  const press = await page.evaluate(() => { const b = document.getElementById("newsessbtn"), cs = getComputedStyle(b);
    return { shadow: cs.boxShadow, scale: cs.scale, mark: b.dataset.press ?? null, before: getComputedStyle(b, "::before").content }; });
  await page.mouse.up();
  await page.click("#newsesscancel");
  const fab = await page.$("#findfab").then((b) => b.boundingBox());
  // the pulse (320 ms) is counted as it starts, not sampled: a busy browser's frame can outlast it
  await page.evaluate(() => { window.pulses = 0; document.getElementById("findfab").addEventListener("animationstart", (e) => { if (e.pseudoElement === "::before") window.pulses++; }); });
  await page.mouse.click(fab.x + fab.width / 2, fab.y + fab.height / 2);
  await page.waitForFunction(() => window.pulses > 0, null, { timeout: slack(1000) }).catch(() => {});
  const pulse = await page.evaluate(() => ({ mark: document.getElementById("findfab").dataset.press, running: window.pulses }));
  await page.keyboard.press("Escape");
  await page.mouse.move(fab.x - 200, fab.y + 200);
  await pause(page, 400);
  await page.mouse.move(fab.x + fab.width / 2, fab.y + fab.height / 2);
  const nod = await page.evaluate(() => new Promise((done) => requestAnimationFrame(() =>
    done(document.getAnimations().filter((a) => a.animationName === "nod" && a.effect?.target?.closest("#findfab")).length))));
  await pause(page, 500);
  const lifted = await page.evaluate(() => { const cs = getComputedStyle(document.getElementById("findfab")); return { scale: cs.scale, translate: cs.translate }; });
  await page.mouse.move(fab.x - 200, fab.y + 200);
  check("motion: a blob under the pointer lifts 2 px and grows 12%, its icon nods once",
    nod === 1 && lifted.scale === "1.12" && lifted.translate === "0px -2px", JSON.stringify({ nod, lifted }));
  check("motion: a pressed key sinks into the panel (its well), nothing scales or ripples; a pressed blob pulses",
    /0px 1px 3px 0px/.test(press.shadow) && /inset/.test(press.shadow) && press.scale === "none" && press.mark === null && press.before === "none" && ["a", "b"].includes(pulse.mark) && pulse.running === 1,
    JSON.stringify({ press, pulse }));

  // the settle (320 ms) is noted as it starts, not sampled after the click: under load it can be over by then
  await page.evaluate(() => { window.settled = []; document.addEventListener("animationstart", (e) => {
    if (e.target.closest?.("#sessions .item.on") && parseFloat(getComputedStyle(e.target).animationDuration) > 0) window.settled.push(`${e.target.className}:${e.animationName}`); }); });
  await page.click("#sessions .item[data-id='tmux:local:terminum:%2']");
  const settle = await page.waitForFunction(() => window.settled.length && window.settled, null, { timeout: slack(1000) }).then((h) => h.jsonValue(), () => []);
  check("motion: the newly selected row settles once (its fill and its bar)", settle.length >= 1, JSON.stringify(settle));
  await pause(page, 400);

  await page.click("#term");
  await page.keyboard.type("ls -la");
  await page.mouse.click(300, 300);
  const inTerm = await moving(page, "#term, #kbd, .key");
  check("motion: nothing moves in the terminal, while typing or on a click there", !inTerm.length, JSON.stringify(inTerm));
  await f.close();

  const calm = await openPage(browser, { device: WIDE });
  await drawn(calm.page);
  await calm.page.click("#menubtn");
  await calm.page.keyboard.press("Escape");
  await pressFoot(calm.page, "#newsessbtn");
  await calm.page.click("#newsesscancel");
  const still = await moving(calm.page, "body");
  check("motion: with reduced motion nothing moves: no fade, no ripple, no settle", !still.length, JSON.stringify(still));
  check("motion: no page errors", !f.errors.length && !calm.errors.length, [...f.errors, ...calm.errors].join(" | "));
  await calm.close();
}
