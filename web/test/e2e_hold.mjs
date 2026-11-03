// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Against a fake terminumd (fake.mjs): an /api/hello that never answers counts as the server down
// (AC-14, proof.js's 5 s deadline) and the page connects once it answers; a burst of screen frames
// with nothing moved places the floating buttons no more (marks.js placeFab), a resize places them;
// a computer following the output keeps about 5000 history lines on the page (term.js trim).
import { check } from "./check.mjs";
import { drawn, line, openPage, pause } from "./fake.mjs";

const SID = "tmux:local:terminum:%1";
const SECRET = "b".repeat(64);
const banner = (page) => page.evaluate(() => { const e = document.getElementById("status"); return e.hidden ? "" : e.textContent; });

export async function webHold(browser) {
  // #14: the socket drops, /api/hello is held open: "not running" within RETRY + 5 s + a margin, then back
  const f = await openPage(browser, { secret: SECRET, stored: SECRET });
  await drawn(f.page);
  let held = 0;
  await f.page.route("**/api/hello*", () => { held++; });           // never answered
  const t0 = Date.now();
  f.sockets.at(-1).close();
  await f.page.waitForFunction(() => { const e = document.getElementById("status"); return !e.hidden && /not running/.test(e.textContent); },
    null, { timeout: 12000 }).catch(() => {});
  const took = Date.now() - t0, said = await banner(f.page);
  check("AC-14 an /api/hello that never answers: the page says the server is down within its retry delay plus 5 s",
    held >= 1 && /not running/.test(said) && took < 10000, JSON.stringify({ held, said, took }));
  const socks = f.sockets.length;
  await f.page.unroute("**/api/hello*");
  await f.page.waitForFunction(() => document.getElementById("status").hidden, null, { timeout: 12000 }).catch(() => {});
  check("AC-14 once /api/hello answers again the page connects and the line goes", f.sockets.length > socks && await banner(f.page) === "",
    JSON.stringify({ socks, now: f.sockets.length, said: await banner(f.page) }));
  await f.close();

  // #18: 200 frames in one tick with nothing moved place the buttons no more; a resize places them again
  const g = await openPage(browser, { screen: Array.from({ length: 30 }, (_, i) => line(`row ${i}`)) });
  await drawn(g.page);
  await pause(g.page, 400);
  const watch = () => g.page.evaluate(() => {
    window.__placed = 0;
    window.__mo?.disconnect();
    window.__mo = new MutationObserver((l) => { window.__placed += l.length; });
    window.__mo.observe(document.getElementById("marksfabn"), { childList: true });   // placeFab writes its count each run
  });
  await watch();
  for (let i = 0; i < 200; i++) g.say({ t: "screen", sid: SID, full: false, n: 30, cols: 80, rows: 30, alt: false, ch: [[i % 30, line(`row ${i % 30} again ${i}`)]] });
  await pause(g.page, 600);
  const burst = await g.page.evaluate(() => window.__placed);
  await watch();
  await g.page.setViewportSize({ width: 1200, height: 800 });
  await pause(g.page, 600);
  const resized = await g.page.evaluate(() => window.__placed);
  const col1 = await g.page.evaluate(() => { const t = document.getElementById("term").getBoundingClientRect(), b = document.getElementById("findfab").getBoundingClientRect();
    return { right: Math.round(t.right - b.right), visible: !document.getElementById("findfab").hidden }; });
  check("AC-18 200 screen frames with nothing moved: the floating buttons are not placed again", burst === 0, JSON.stringify({ burst }));
  check("AC-18 a window resize places the floating buttons again, still at the terminal's right", resized >= 1 && col1.visible && col1.right >= 0 && col1.right < 120,
    JSON.stringify({ resized, col1 }));
  // a pane switch (its "hist" reset and full screen): the column stays where it stood
  await g.page.click(`#sessions .item[data-id='tmux:local:terminum:%2']`);
  await pause(g.page, 600);
  const col2 = await g.page.evaluate(() => { const t = document.getElementById("term").getBoundingClientRect(), b = document.getElementById("findfab");
    return { right: Math.round(t.right - b.getBoundingClientRect().right), visible: !b.hidden }; });
  const sub = g.sent.filter((m) => m.t === "sub").at(-1)?.id;
  check("AC-18 after a pane switch the floating buttons stand in the same column", sub?.endsWith("%2") && col2.visible && col2.right === col1.right,
    JSON.stringify({ sub, col1, col2 }));
  await g.close();

  // #19: 8000 lines appended while following: a computer keeps about 5000 (every one before, each laid out per frame)
  const h = await openPage(browser);
  await drawn(h.page);
  const sid = h.sent.filter((m) => m.t === "sub").at(-1)?.id;
  for (let k = 0; k < 16; k++)
    h.say({ t: "hist", mode: "append", sid, first: k * 500, oldest: 0, truncated: false, lines: Array.from({ length: 500 }, (_, i) => line(`out ${k * 500 + i}`)) });
  await pause(h.page, 600);
  const kept = await h.page.evaluate(() => ({ lines: document.querySelectorAll("#hist .ln").length, last: [...document.querySelectorAll("#hist .ln")].at(-1)?.textContent }));
  check("AC-52 a computer following the output keeps 5000 to 6000 history lines on the page", kept.lines >= 5000 && kept.lines <= 6000 + 800 && kept.last === "out 7999",
    JSON.stringify(kept));
  check("web hold: no page errors", !f.errors.length && !g.errors.length && !h.errors.length, JSON.stringify([...f.errors, ...g.errors, ...h.errors]));
  await h.close();
}
