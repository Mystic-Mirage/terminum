// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-18 on a phone: every dialog, pane and menu opens as a modal sheet centered on the visible
// screen, 16 px from the sides, fitting it (a tall one scrolls inside), the page shaded behind
// it, above its shade (its own rows take the press); a press on the shade or Esc closes it. On a computer a row's menu stays at the pointer.

import { check, runningIn } from "./check.mjs";
import { drawn, openPage, pause, PHONE, pressFab, WIDE, pressFoot } from "./fake.mjs";

// where `sel` stands against the visible screen, and whether something shades the page
const geo = (page, sel) => page.evaluate((sel) => {
  const el = document.querySelector(sel), r = el.getBoundingClientRect(), vw = innerWidth, vh = visualViewport.height, cs = getComputedStyle(el);
    // a shade: a modal dialog's ::backdrop, or what a corner's press would land on is a scrim
  const corner = document.elementFromPoint(4, Math.round(vh / 2));
  const shaded = el.matches("dialog:modal") ? getComputedStyle(el, "::backdrop").backgroundColor !== "rgba(0, 0, 0, 0)" : /^(sheetscrim|panescrim)$/.test(corner?.id);
  return { l: Math.round(r.left), r: Math.round(vw - r.right), t: Math.round(r.top), b: Math.round(vh - r.bottom), h: Math.round(r.height), vh: Math.round(vh),
    scrolls: el.scrollHeight > el.clientHeight + 1, shaded, shown: r.width > 0 && cs.visibility !== "hidden",
    on: el.contains(document.elementFromPoint(Math.round(r.left + r.width / 2), Math.round(r.top + Math.min(r.height / 2, 60)))) };   // its own press is its: above its shade
}, sel);
const centered = (g) => g.shown && Math.abs(g.l - g.r) <= 1 && Math.abs(g.l - 16) <= 1 && g.t >= 0 && g.b >= 0 && (Math.abs(g.t - g.b) <= 1 || g.h >= g.vh - 33) && g.shaded && g.on;
const shut = (page, sel) => page.evaluate((sel) => { const el = document.querySelector(sel); return el.hidden || (el.tagName === "DIALOG" && !el.open); }, sel);

export async function webSheets(browser, name) {
  runningIn(`${name} 390×844`);
  let f = await openPage(browser, { device: PHONE });
  let { page } = f;
  await drawn(page);
  await pause(page, 300);

  // a dialog: centered by CSS alone (a modal <dialog> has its ::backdrop)
  await page.evaluate(() => document.getElementById("confirmbox").showModal());
  let g = await geo(page, "#confirmbox");
  check("AC-18 phone: a dialog is centered, 16 px from the sides, the page shaded", centered(g), JSON.stringify(g));
  await page.keyboard.press("Escape");
  check("AC-18 phone: Esc closes the dialog", await shut(page, "#confirmbox"));

  // the header's ⋯ menu
  await page.click("#menubtn");
  await pause(page, 200);
  g = await geo(page, "#menu");
  check("AC-18 phone: the ⋯ menu is a centered sheet, the page shaded", centered(g), JSON.stringify(g));
  let n = f.sent.length;
  await page.mouse.click(4, 422);
  await pause(page, 150);
  check("AC-18 phone: a press on the shade closes the ⋯ menu and reaches nothing under it", await shut(page, "#menu") && f.sent.length === n, JSON.stringify(f.sent.slice(n)));

  // a row's menu (the list's contextmenu; a finger's long press opens the same)
  await page.evaluate(() => { const it = document.querySelector("#sessions .item:not(.child)"); it.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 20, clientY: 700 })); });
  await pause(page, 200);
  g = await geo(page, "#rowmenu");
  check("AC-18 phone: a row's menu is centered, not at the finger, the page shaded", centered(g), JSON.stringify(g));
  await page.keyboard.press("Escape");
  await pause(page, 100);

  // a list foot's form: New session
  const navOpen = () => page.evaluate(() => document.getElementById("app").classList.contains("nav-open"));
  if (!(await navOpen())) await page.click("#navbtn");
  await pause(page, 300);
  await pressFoot(page, "#newsessbtn");
  await pause(page, 200);
  g = await geo(page, "#newsess");
  check("AC-18 phone: a list form (New session) is a centered sheet over its shade", centered(g), JSON.stringify(g));
  n = f.sent.length;
  const before = await page.evaluate(() => document.querySelector("#sessions .item.sel, #sessions .item[aria-current]")?.dataset.id ?? null);
  await page.mouse.click(4, 700);
  await pause(page, 150);
  const after = await page.evaluate(() => document.querySelector("#sessions .item.sel, #sessions .item[aria-current]")?.dataset.id ?? null);
  check("AC-18 phone: a press on the shade closes the form; no row under it is picked, nothing is sent", await shut(page, "#newsess") && f.sent.length === n && before === after,
    JSON.stringify({ sent: f.sent.slice(n), before, after }));
  if (await navOpen()) await page.click("#scrim", { position: { x: 380, y: 400 } }).catch(() => page.keyboard.press("Escape"));
  await pause(page, 300);

  // the bookmarks' pane: a bookmark of the shown session fed as terminumd sends them
  f.say({ t: "marks", items: [{ id: "B1", sid: "tmux:local:terminum:%1", key: null, n: 1, text: "a.txt b.txt", note: "", title: "api", host: null, at: 1792000000 }], error: null });
  await pause(page, 300);
  if (await page.evaluate(() => !document.getElementById("marksfab").hidden)) {   // folded under ⟨…⟩ (fold.js)
    await pressFab(page, "#marksfab");
    await pause(page, 200);
    g = await geo(page, "#markpane");
    check("AC-18 phone: the bookmarks' pane is a centered sheet over its shade", centered(g), JSON.stringify(g));
    n = f.sent.length;
    await page.mouse.click(4, 422);
    await pause(page, 150);
    check("AC-18 phone: a press on the shade closes the bookmarks' pane, nothing sent", await shut(page, "#markpane") && f.sent.length === n, JSON.stringify(f.sent.slice(n)));
  } else check("AC-18 phone: the bookmarks' button shows (to open its pane)", false);

  // the sent history's pane
  if (await page.evaluate(() => !document.getElementById("sentfab").hidden)) {   // folded under ⟨…⟩ (fold.js)
    await pressFab(page, "#sentfab");
    await pause(page, 200);
    g = await geo(page, "#sentpane");
    check("AC-18 phone: the sent pane is a centered sheet over its shade", centered(g), JSON.stringify(g));
    await page.mouse.click(4, 422);
    await pause(page, 150);
    check("AC-18 phone: a press on the shade closes the sent pane", await shut(page, "#sentpane"));
  } else check("AC-18 phone: the sent button shows (to open its pane)", false);

  // Esc closes a sheet that is no <dialog>, and the keys are in it while it is open
  await page.click("#menubtn");
  await pause(page, 200);
  const focusIn = await page.evaluate(() => document.getElementById("menu").contains(document.activeElement));
  await page.keyboard.press("Escape");
  await pause(page, 100);
  check("AC-18 phone: the ⋯ menu as a sheet takes the keys, and Esc closes it", focusIn && await shut(page, "#menu"), JSON.stringify({ focusIn }));
  // a sheet over a sheet: the bookmarks' pane, then the ⋯ menu; the menu closed, the pane is still shaded and a press there closes it
  await pressFab(page, "#marksfab");
  await pause(page, 200);
  await page.evaluate(() => document.getElementById("menubtn").click());
  await pause(page, 200);
  await page.keyboard.press("Escape");
  await pause(page, 150);
  const under = await geo(page, "#markpane");
  await page.mouse.click(4, 422);
  await pause(page, 150);
  check("AC-18 phone: a sheet over a sheet closed, the one under it keeps its shade; a press on it closes that one",
    await shut(page, "#menu") && centered(under) && await shut(page, "#markpane"), JSON.stringify(under));

  // a short visible height (the on-screen keyboard up): a tall sheet fits and scrolls inside
  await page.setViewportSize({ width: 390, height: 300 });
  await pause(page, 300);
  await page.click("#menubtn");
  await pause(page, 200);
  g = await geo(page, "#menu");
  check("AC-18 phone, 300 px high: the ⋯ menu fits the visible screen and scrolls inside", centered(g) && g.scrolls && g.h <= g.vh - 32, JSON.stringify(g));
  await page.mouse.click(4, 150);
  await page.context().close();

  runningIn(`${name} 1400×900`);
  f = await openPage(browser, { device: WIDE });
  page = f.page;
  await drawn(page);
  await pause(page, 300);
  await page.click("#sessions .item:not(.child)", { button: "right", position: { x: 30, y: 10 } });
  await pause(page, 200);
  const at = await page.evaluate(() => { const m = document.getElementById("rowmenu"), r = m.getBoundingClientRect(), it = document.querySelector("#sessions .item:not(.child)").getBoundingClientRect();
    return { sheet: "sheet" in m.dataset, dx: Math.round(r.left - it.left - 30), dy: Math.round(r.top - it.top - 10) }; });
  check("AC-18 computer: a row's menu stays at the pointer (no sheet)", !at.sheet && Math.abs(at.dx) <= 1 && Math.abs(at.dy) <= 1, JSON.stringify(at));
  await page.keyboard.press("Escape");
  await page.click("#menubtn");
  await pause(page, 150);
  const m = await page.evaluate(() => { const r = document.getElementById("menu").getBoundingClientRect(), b = document.getElementById("menubtn").getBoundingClientRect();
    return { sheet: "sheet" in document.getElementById("menu").dataset, right: Math.round(b.right - r.right), gap: Math.round(r.top - b.bottom) }; });
  check("AC-18 computer: the settings menu stays under its button, no sheet", !m.sheet && m.right === 0 && m.gap === 6, JSON.stringify(m));
  await page.context().close();
}
