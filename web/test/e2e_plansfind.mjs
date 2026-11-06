// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The Plan pane found before a plan (AC-97) against a fake terminumd, at both sizes: a coding
// agent's row without a plan shows the header's Plan button and no progress circle; "Plan and Tasks…" in a row's menu
// and the header ⋯ menu, for any session; the pane without a plan says the agent sent none; a plan whose
// agent exited is marked finished. Alone: node web/test/e2e_all.mjs webPlansFind.
import { check, runningIn } from "./check.mjs";
import { drawn, group, item, openPage, pause, SIZES } from "./fake.mjs";

const A = "tmux:local:terminum:%1", B = "tmux:local:terminum:%2", C = "tmux:local:terminum:%3";
const layout = () => [group("main", [item(A, "agent", { agent: "claude", shell: false }), item(B, "shell", { index: "2" }),
  item(C, "exited", { index: "3" })])];
const q = (page, fn, arg) => page.evaluate(fn, arg);
const paneText = (page) => q(page, () => document.getElementById("planpane").hidden ? null : document.getElementById("planbody").textContent);

export async function webPlansFind(browser, name) {
  for (const [size, device] of SIZES) {
    runningIn(`${name} ${size}`);
    const now = Math.floor(Date.now() / 1000);
    const finished = { title: "Old work", doing: null, items: [{ text: "done item", status: "completed" }], done: 1, total: 1, pct: 100,
      eta: null, eta_given: null, updated: now - 600, source: "tool", partial: false, agent: "claude", finished: true };
    const f = await openPage(browser, { device, layout: layout(), hook: (m, say) => {
      if (m.t === "plan") say({ t: "plan", key: m.key, plan: m.key === C ? finished : null, notes: [] });
    } });
    const { page } = f;
    await drawn(page);
    await pause(page, 200);

    const head = await q(page, () => { const b = document.getElementById("curplan"), r = b.getBoundingClientRect();
      return { hidden: b.hidden, title: b.title, w: r.width, ring: document.getElementById("planfab").hidden }; });
    check("AC-97 a coding agent's row without a plan shows the header's Plan button (no plan yet), no progress circle", !head.hidden && head.title === "Plan: none yet" && head.w > 0
      && head.ring, JSON.stringify(head));

    await page.click("#curplan");
    await pause(page, 300);
    const empty = await paneText(page);
    check("AC-97 the Plan pane without a plan says the agent sent none, then that the session has no task (terminum-kanban.md AC-113)", empty === "No plan: the agent has not sent one for this session.TasksNo task in this session.", empty);
    await page.keyboard.press("Escape");
    await pause(page, 200);

    // a row's menu, on a session without an agent: it is shown, its pane opens
    await page.$eval(`#sessions .item[data-id="${B}"]`, (el) => { const r = el.getBoundingClientRect();
      el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 40, clientY: r.top + 10 })); });
    await page.waitForSelector("#rowmenu[open]");
    const entry = await q(page, () => document.querySelector('#rowmenu button[data-action="plan"]')?.textContent);
    check("AC-97 a row's menu has Plan and Tasks… for any session", entry === "Plan and Tasks…", entry);
    await page.click('#rowmenu button[data-action="plan"]');
    await pause(page, 300);
    const viaRow = await q(page, (b) => ({ asked: null, hidden: document.getElementById("curplan").hidden, shown: document.querySelector(`.item[data-id="${b}"]`)?.getAttribute("aria-current") }), B);
    const asked = f.sent.filter((m) => m.t === "plan").map((m) => m.key);
    check("AC-97 Plan and Tasks… shows the row's session and opens its Plan pane (no header button without an agent)",
      /^No plan: /.test(await paneText(page) ?? "") && asked.at(-1) === B && viaRow.hidden, JSON.stringify({ ...viaRow, asked }));
    const inView = await q(page, () => { const r = document.getElementById("planpane").getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth + 1 && r.top >= 0 && r.width > 200; });
    check("AC-97 the pane opened from a menu stands in the window", inView, String(inView));
    await page.keyboard.press("Escape");
    await pause(page, 200);

    // the header ⋯ menu, on the session whose agent exited: its last plan, finished
    await q(page, (c) => document.querySelector(`.item[data-id="${c}"]`).click(), C);
    await pause(page, 300);
    await page.click("#menubtn");
    await pause(page, 200);
    const menuItem = await q(page, () => { const b = document.getElementById("planmenu"); return !b.hidden && b.getBoundingClientRect().height > 0 ? b.textContent : null; });
    check("AC-97 the header ⋯ menu has Plan and Tasks…", /^Plan and Tasks…/.test(menuItem ?? ""), menuItem);
    await page.click("#planmenu");
    await pause(page, 300);
    const fin = await paneText(page);
    const rowPct = await q(page, (c) => !!document.querySelector(`.item[data-id="${c}"] .plpct`), C);
    check("AC-97 a plan whose agent exited shows marked Finished session (agent exited hh:mm), its items kept",
      /^Finished session \(agent exited \d\d:\d\d\)Old work1 of 1 done100%Steps✓done item/.test(fin ?? ""), fin);
    check("AC-97 the row shows no percent for a finished plan", !rowPct, String(rowPct));
    await page.keyboard.press("Escape");

    const scroll = await q(page, () => document.documentElement.scrollWidth - innerWidth);
    check("no sideways page scroll (plans found)", scroll <= 0, String(scroll));
    check("no page errors (plans found)", !f.errors.length, f.errors.join(" | "));
    await f.close();
  }
  runningIn(name);
}
