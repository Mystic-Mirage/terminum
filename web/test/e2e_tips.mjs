// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Tip of the day (AC-101, tips.js) against a fake terminumd (fake.mjs): on a computer one tip above
// the list's tools, on a card of its own (small text, small keys); Next, × until tomorrow, Turn off
// and Settings' switch; the next tip on a new day; none on a phone.
import { check, runningIn } from "./check.mjs";
import { drawn, openPage, pause, SIZES } from "./fake.mjs";
import { MEASURE } from "./e2e_planslook.mjs";

const state = async (page) => { await page.evaluate(MEASURE); return page.evaluate(() => {
  const tip = document.getElementById("tip"), b = tip.getBoundingClientRect(), foot = document.querySelector(".nav-foot").getBoundingClientRect();
  const nav = document.getElementById("nav").getBoundingClientRect();
  const keys = [...tip.querySelectorAll(".tipacts button")].map((k) => k.getBoundingClientRect().height);
  const off = document.getElementById("tipoff").getBoundingClientRect(), next = document.getElementById("tipnext").getBoundingClientRect();
  const sides = Math.abs(next.right - document.getElementById("tipclose").getBoundingClientRect().right) <= 1 && off.left < b.left + 20
    && off.top - document.getElementById("tiptext").getBoundingClientRect().bottom >= 8;
  return { shown: b.height > 0, text: document.getElementById("tiptext").textContent, icons: document.querySelectorAll("#tiptext svg, #tiptext .tipring").length,
    fill: getComputedStyle(tip).backgroundColor, navFill: getComputedStyle(document.getElementById("nav")).backgroundColor, size: getComputedStyle(document.getElementById("tiptext")).fontSize, keys, sides, contrast: window.__ratio(document.getElementById("tiptext")),
    aboveFoot: b.bottom <= foot.top + 0.5, inNav: b.left >= nav.left && b.right <= nav.right,
    on: document.getElementById("tipson").checked, stored: JSON.parse(localStorage.getItem("tip") ?? "null"), tips: localStorage.getItem("tips") };
}); };

const reload = async (page) => { await page.reload(); await drawn(page); await pause(page, 200); };

export async function webTips(browser, name) {
  for (const [size, device] of SIZES) {
    runningIn(`${name} ${size}`);
    const f = await openPage(browser, { device });
    const { page } = f;
    await drawn(page);
    await pause(page, 200);
    const first = await state(page);
    if (size !== "1400×900") {
      check("AC-101 no tip on a phone, Settings' switch on for a new browser", !first.shown && first.on, JSON.stringify(first));
      await f.close();
      continue;
    }
    check("AC-101 a new browser shows the first tip above the list's tools, on a card of its own fill, 12px text at 4.5:1 or more, 24px keys (Turn off left, Next right, 8px or more under the text)",
      first.shown && /normal TUI/.test(first.text) && first.fill !== "rgba(0, 0, 0, 0)" && first.fill !== first.navFill && first.contrast >= 4.5 && first.sides && first.size === "12px" && first.keys.every((h) => h === 24)
        && first.aboveFoot && first.inNav && first.on, JSON.stringify(first));
    await page.setViewportSize({ width: 1400, height: 500 });
    await pause(page, 100);
    const short = await state(page);
    await page.setViewportSize({ width: 1400, height: 900 });
    await pause(page, 100);
    check("AC-101 a window under 560 px tall keeps the list's room: no tip", !short.shown && (await state(page)).shown, JSON.stringify(short));
    await page.click("#tipnext");
    const next = await state(page);
    check("AC-101 Next shows the next tip and keeps it for today", next.shown && next.text !== first.text && next.stored.at === 1, JSON.stringify(next));
    for (let i = 1; i < 11; i++) await page.click("#tipnext");
    const icons = await state(page);
    check("AC-101 a tip draws its icons (the plan's ring)", icons.icons === 1 && /round button/.test(icons.text), JSON.stringify(icons));
    await page.click("#tipclose");
    const closed = await state(page);
    await reload(page);
    const reloaded = await state(page);
    check("AC-101 × hides it until tomorrow, the switch stays on", !closed.shown && closed.on && !reloaded.shown && reloaded.on, JSON.stringify({ closed, reloaded }));
    // the way back: Settings' "Show today's tip", there only while today's tip is closed
    await page.click("#menubtn");
    const offered = await page.isVisible("#tipshow");
    await page.click("#tipshow");
    const shownAgain = await state(page);
    const menuClosed = await page.evaluate(() => document.getElementById("menu").hidden);
    await page.click("#menubtn");
    const offeredAfter = await page.isVisible("#tipshow");
    await page.click("#menubtn");
    check("AC-101 Settings' Show today's tip brings back a tip closed with ×, and is gone while it shows", offered && shownAgain.shown && menuClosed && !offeredAfter,
      JSON.stringify({ offered, shown: shownAgain.shown, menuClosed, offeredAfter }));
    // a new day: the tip after the one shown last, × forgotten
    await page.evaluate(() => localStorage.setItem("tip", JSON.stringify({ day: "2000-01-01", at: 4, closed: "2000-01-01" })));
    await reload(page);
    const day = await state(page);
    check("AC-101 on a new day the next tip shows", day.shown && day.stored.at === 5 && /viewer/.test(day.text), JSON.stringify(day));
    // a tab left open: the next day comes when it is looked at again, × of the day before forgotten
    await page.click("#tipclose");
    await page.evaluate(() => { Date.prototype.toLocaleDateString = () => "2099-01-01"; window.dispatchEvent(new Event("focus")); });
    const open = await state(page);
    check("AC-101 a tab left open shows the next tip on a new day, when looked at again", open.shown && open.stored.at === 6 && open.stored.day === "2099-01-01", JSON.stringify(open));
    // a stored tip this page cannot read: the first one, nothing broken
    await page.evaluate(() => localStorage.setItem("tip", JSON.stringify({ day: "x", at: -1.5 })));
    await reload(page);
    const bad = await state(page);
    check("AC-101 a stored tip it cannot read starts again at the first", bad.shown && bad.stored.at === 0 && /normal TUI/.test(bad.text), JSON.stringify(bad));
    await page.click("#tipoff");
    const off = await state(page);
    await reload(page);
    const offLater = await state(page);
    await page.click("#menubtn");
    await page.click("#tipson");
    const back = await state(page);
    check("AC-101 Turn off hides it and turns Settings' switch off, kept; the switch turns it on again",
      !off.shown && !off.on && off.tips === "false" && !offLater.shown && !offLater.on && back.shown && back.on && back.tips === "true", JSON.stringify({ off, offLater, back }));
    check("AC-101 no page errors", f.errors.length === 0, f.errors.join(" | "));
    await f.close();
  }
}
