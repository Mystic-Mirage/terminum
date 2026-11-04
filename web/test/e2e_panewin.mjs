// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The floating buttons' panes as windows (AC-33, AC-90, AC-91, AC-97) for e2e_all.mjs, on the fake
// terminumd at 1400×900: Bookmarks, Reminders, Sent and Plan each a window of one place and size, as
// View's (12 px under the terminal's top, 16 px over its end, 14 px left of the floating buttons, 820
// px at most), View's chrome, corners and shadow, its head (36 px, a 14 px 600 title, 24 px keys) and
// its Markdown type (16 px, line height 1.65, 22 px in); the actions' first at the left, the last at the right.
import { check, runningIn } from "./check.mjs";
import { group, item, line, openPage, pause, pressFab } from "./fake.mjs";

const SID = "tmux:local:terminum:%1";
const now = Math.floor(Date.now() / 1000);
const plan = { done: 1, total: 3, pct: 33, eta: null, updated: now - 60, stale: false, partial: false };
const LAYOUT = [group("main", [item(SID, "api", { shell: true, sub: ["claude", "~/work/api"], dir: "/Users/alex/work/api", plan, agent: "claude" })])];
const SCREEN = [line("out 1"), line("reminder text here"), { r: [["alex@mac api % ", null, null, 0], [" ", null, null, 256]], e: true }];
const MARKS = [{ id: "R1", sid: SID, key: SID, n: 1, text: "reminder text here", note: "check the retry", title: "api", host: null, at: now - 900, remind: true, due: true },
  { id: "B1", sid: SID, key: SID, n: 0, text: "out 1", note: "Need to fix it.", title: "api", host: null, at: now - 1200 }];
const SENT = { at: now - 600, text: "fix the tests", agent: "claude", title: "api", key: SID, line: 2 };
const PLAN = { plan: { title: "Unify the panes", doing: "Measuring", items: [{ text: "one", status: "completed" }, { text: "two", status: "in_progress" }],
  done: 1, total: 2, pct: 50, eta: null, eta_given: null, updated: now - 60, source: "mcp", partial: false, agent: "claude" }, hooks: true };
const PANES = [["Bookmarks", "#marksfab", "#markpane"], ["Reminders", "#remindfab", "#remindpane"], ["Sent", "#sentfab", "#sentpane"], ["Links", "#linksfab", "#linkspane"], ["Plan", "#planfab", "#planpane"]];

/** A pane's place against the terminal and the floating column, its look, its head and its type. */
const measure = (sel) => {
  const w = document.querySelector(sel), r = w.getBoundingClientRect(), t = document.getElementById("term").getBoundingClientRect();
  const col = document.getElementById("findfab").getBoundingClientRect(), cs = getComputedStyle(w);
  const head = w.querySelector(".pwhead"), title = head?.querySelector(".pwtitle"), keys = [...(head?.querySelectorAll("button.icon") ?? [])];
  const body = [...w.querySelectorAll(".pwbody")].find((b) => b.offsetHeight) ?? w.querySelector(".mkvbody")?.shadowRoot?.querySelector(".body"), acts = [...w.querySelectorAll(".pwacts button, .mkvacts button")].filter((b) => b.offsetHeight);
  const ts = title && getComputedStyle(title), box = acts[0]?.parentElement.getBoundingClientRect();
  const v = getComputedStyle(document.getElementById("filewin"));
  return { view: [v.backgroundColor, v.borderRadius, v.boxShadow].join(" | "), place: [r.left - t.left, r.top - t.top, col.left - r.right, t.bottom - r.bottom, r.width, r.height].map(Math.round),
    look: [cs.backgroundColor, cs.borderRadius, cs.boxShadow].join(" | "), head: head && Math.round(head.getBoundingClientRect().height),
    title: ts && `${ts.fontSize} ${ts.fontWeight} ${Math.round(title.getBoundingClientRect().left - r.left)}`, keys: keys.map((k) => Math.round(k.getBoundingClientRect().width)),
    lh: body ? Math.round(parseFloat(getComputedStyle(body).lineHeight) / parseFloat(getComputedStyle(body).fontSize) * 100) / 100 : null,
    size: body ? getComputedStyle(body).fontSize : null,   // the navigator's in its widget
    family: body ? getComputedStyle(body).fontFamily : null, term: getComputedStyle(document.documentElement).getPropertyValue("--size").trim(),
    acts: acts.length < 2 || (Math.abs(acts[0].getBoundingClientRect().left - box.left - parseFloat(getComputedStyle(acts[0].parentElement).paddingLeft)) < 1.5
      && Math.abs(box.right - parseFloat(getComputedStyle(acts[0].parentElement).paddingRight) - acts.at(-1).getBoundingClientRect().right) < 1.5) };
};

export async function webPaneWin(browser, name) {
  runningIn(`${name} 1400×900`);
  const f = await openPage(browser, { layout: LAYOUT, screen: SCREEN, hook: (m, say) => {
    if (m.t === "sentget") say({ t: "sentone", sid: SID, n: m.n, total: 1, item: SENT, host: "local", path: "/Users/alex/work/api" });
    if (m.t === "plan") say({ t: "plan", key: m.key, ...PLAN });
  } });
  const { page } = f;
  await page.waitForFunction(() => document.querySelectorAll("#screen .ln").length > 0, null, { timeout: 10000 });
  f.say({ t: "marks", items: MARKS, error: null });
  f.say({ t: "settings", page: { markdownSize: 13 } });     // the menu's Markdown size (prefs.js --md-size): not these windows'
  await pause(page, 500);
  const got = {};
  for (const [label, fab, pane] of PANES) {
    await pressFab(page, fab);
    await page.waitForFunction((s) => !document.querySelector(s).hidden, pane, { timeout: 5000 }).catch(() => {});
    await pause(page, 300);
    got[label] = await page.evaluate(measure, pane);
    await page.keyboard.press("Escape");
    await pause(page, 200);
  }
  const first = Object.values(got)[0];
  const [left, top, right, end, width] = first.place;
  check(`AC-33 AC-90 AC-91 AC-97 AC-154 [${name}] Bookmarks, Reminders, Sent, Links and Plan are one window, placed as View's and with its chrome, corners and shadow: 12 px under the terminal's top, 16 px over its end, 14 px left of the floating buttons, 820 px at most`,
    Object.values(got).every((g) => g.place.join() === first.place.join() && g.look === g.view) && top === 12 && end === 16 && right === 14 && left >= 16 && width <= 820, JSON.stringify(got));
  check(`AC-33 AC-90 AC-91 AC-97 AC-154 [${name}] each has View's head (36 px, its title 14 px 600 12 px in, 24 px keys) and the chat's type (the terminal's font at its size, line height 1.5, whatever the menu's Markdown size; the navigators' content too); the actions' first at the left, the last at the right`,
    Object.values(got).every((g) => g.head === 36 && g.title === "14px 600 12" && g.keys.length && g.keys.every((k) => k === 24) && (g.lh === 1.5 && g.size === g.term && /^TermFont/.test(g.family)) && g.acts),
    JSON.stringify(got));
  await page.close();
}
