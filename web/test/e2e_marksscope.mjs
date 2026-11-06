// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-33, AC-91, AC-98 against a fake terminumd (fake.mjs): with a session selected only its own
// bookmarks and reminders show, on every path to them: the bookmarks' button and pane, the bell, its
// toast, the rows' wavy lines; another session of its project shows nothing of them. Only Bookmarks /
// Reminders below the sessions lists every session's. The Plan pane, from the header and from a row's
// percent, links only the shown session's tasks (terminum-kanban.md AC-113), its records not counted.
import { check, runningIn } from "./check.mjs";
import { drawn, group, item, openPage, pause, pressFab, SIZES } from "./fake.mjs";

const [A, B, C, D] = ["%1", "%2", "%3", "%4"].map((p) => `tmux:local:terminum:${p}`);
const rowPlan = { done: 1, total: 2, pct: 50, updated: 1790000000 };
const layout = () => [group("main", [item(A, "api", { plan: rowPlan }), item(B, "web", { index: "2", plan: rowPlan }),
  item(C, "docs", { index: "3" }), item(D, "empty", { index: "4" })])];
const PROJECT = { id: "P", name: "Work", color: 0, host: null, dir: "~/work", members: [A, B, D] };
const rec = (id, sid, extra = {}) => ({ id, sid, key: sid, n: -1, text: `${id} text`, note: `note ${id}`, title: "t", host: null, at: 1790000000, project: "P", ...extra });
const agent = { by: "claude" };
const TASKS = { [A]: ["T-1", "T-2"], [B]: ["T-3", "T-4", "T-5", "T-6"] };
const ITEMS = [rec("a1", A, agent), rec("a2", A, { ...agent, remind: true, due: true }), rec("a3", A, { n: 3 }),
  rec("b1", B, agent), rec("b2", B, agent), rec("b3", B, agent), rec("b4", B, { ...agent, remind: true, resolved: 1790000001 }),
  rec("c1", C, { project: undefined }), rec("c2", C, { project: undefined, remind: true })];

const ids = (page, pane) => page.evaluate((p) => [...document.querySelectorAll(`#${p} .mknav li`)].map((l) => l.dataset.id).join(), pane);
const heading = (page, pane) => page.textContent(`#${pane} h2`);
const show = async (page, key) => {
  await page.evaluate((k) => document.querySelector(`#sessions .item[data-id="${k}"]`).click(), key);
  await page.waitForFunction((k) => document.querySelector("#sessions .item.on")?.dataset.id === k, key, { timeout: 3000 });
  await pause(page, 200);
};
const buttons = (page) => page.evaluate(() => ({ marks: document.getElementById("marksfab").hidden ? null : document.getElementById("marksfabn").textContent,
  bell: document.getElementById("remindfab").hidden ? null : document.getElementById("remindfabn").textContent, filled: document.getElementById("remindfab").classList.contains("due"),
  label: document.getElementById("marksfab").getAttribute("aria-label") }));
// a row's wavy lines
const waves = (page) => page.evaluate(() => Object.fromEntries([...document.querySelectorAll("#sessions .item")].map((r) =>
  [r.dataset.id.split(":").pop(), `${r.querySelector(".wave.mark") ? "m" : ""}${r.querySelector(".wave.remind") ? "r" : ""}`])));
const toast = (page) => page.evaluate(() => (document.getElementById("toast").hidden ? "" : document.getElementById("toast").textContent));
const links = (page) => page.evaluate(() => [...document.querySelectorAll("#planbody .plkept a.linkish")].map((a) => `${a.textContent}=${a.getAttribute("href")}`).join());
const planOpen = (page) => page.waitForFunction(() => document.querySelector("#planbody .plkept"), null, { timeout: 3000 });

export async function webMarksScope(browser, name) {
  for (const [size, device] of SIZES) {
    const at = `(${name} ${size})`;
    runningIn(`${name} ${size}`);
    const f = await openPage(browser, { device, layout: layout(), hook: (m, say) => {
      if (m.t === "plan") say({ t: "plan", key: m.key, plan: null, hooks: true });
    } });
    const { page } = f;
    await drawn(page);
    f.say({ t: "projects", items: [PROJECT] });
    f.say({ t: "taskmarks", marks: TASKS });
    f.say({ t: "marks", items: ITEMS, error: null });
    await show(page, A);

    const inA = await buttons(page);
    check(`AC-33 ${at} the bookmarks' button counts the shown session's own, the bell its reminders, not another session's of its project`,
      inA.marks === "3" && inA.bell === "1" && inA.filled && inA.label === "Bookmarks and reminders of this session: 3", JSON.stringify(inA));
    const said = await toast(page);
    check(`AC-91 ${at} the toast of reminders up names the session, its own counted`, said === "api: 1 reminder, 1 new", said);
    const rows = await waves(page);
    check(`AC-33 ${at} the rows' wavy lines: each session's own kinds; a session of the same project with none has none`,
      rows["%1"] === "mr" && rows["%2"] === "m" && rows["%3"] === "mr" && rows["%4"] === "", JSON.stringify(rows));

    // the Plan pane: this session's tasks; its records by the bookmarks' own button
    await page.click("#curplan");
    await planOpen(page);
    const words = await links(page), hover = await page.getAttribute("#curplan", "title");
    check(`AC-113 (terminum-kanban.md) ${at} the Plan pane and the header's hover name this session's tasks alone`,
      words === "T-1=/kanban/T-1,T-2=/kanban/T-2" && hover.endsWith(" · 2 tasks"), JSON.stringify({ words, hover }));
    await page.keyboard.press("Escape");
    await pause(page, 150);
    await pressFab(page, "#marksfab");
    await pause(page, 200);
    check(`AC-98 ${at} the bookmarks' button lists only this session's`, (await ids(page, "markpane")) === "a3,a1,a2" && (await heading(page, "markpane")) === "Bookmarks and reminders of this session",
      `${await ids(page, "markpane")} | ${await heading(page, "markpane")}`);

    // what comes while the pane is open
    f.say({ t: "mark", item: rec("b9", B, agent) });
    await pause(page, 200);
    check(`AC-33 ${at} a record of another session of the project, coming while the pane is open, changes nothing`,
      (await ids(page, "markpane")) === "a3,a1,a2" && (await buttons(page)).marks === "3", await ids(page, "markpane"));
    f.say({ t: "mark", item: rec("a9", A, agent) });
    await pause(page, 200);
    check(`AC-33 ${at} one of this session comes into it`, (await ids(page, "markpane")) === "a3,a1,a2,a9" && (await buttons(page)).marks === "4", await ids(page, "markpane"));
    await page.keyboard.press("Escape");
    await pause(page, 150);

    // the bell's pane
    await pressFab(page, "#remindfab");
    await page.waitForSelector("#remindpane:not([hidden])", { timeout: 3000 });
    check(`AC-91 ${at} the bell's pane lists only this session's reminders`, (await ids(page, "remindpane")) === "a2" && (await heading(page, "remindpane")) === "Reminders of this session",
      `${await ids(page, "remindpane")} | ${await heading(page, "remindpane")}`);
    await page.keyboard.press("Escape");
    await pause(page, 150);

    // a reminder of another session up: nothing here; said when that session is shown
    f.say({ t: "mark", item: rec("c2", C, { project: undefined, remind: true, due: true }) });
    await pause(page, 200);
    check(`AC-91 ${at} another session's reminder up changes nothing on the shown one`, (await buttons(page)).bell === "1", JSON.stringify(await buttons(page)));
    await show(page, B);
    const inB = await buttons(page);
    check(`AC-33 ${at} another session of the project shown: its own count, no bell`, inB.marks === "4" && inB.bell === null, JSON.stringify(inB));
    await show(page, D);
    const inD = await buttons(page);
    check(`AC-33 ${at} a session of the project with none of its own: no button, no bell`, inD.marks === null && inD.bell === null, JSON.stringify(inD));
    check(`AC-113 (terminum-kanban.md) ${at} with no plan, no agent and no task of its own: no Plan button, the tasks of its project's others not counted`, !(await page.isVisible("#curplan")));
    f.say({ t: "taskmarks", marks: { ...TASKS, [D]: ["T-7"] } });
    await pause(page, 150);
    const alone = await page.getAttribute("#curplan", "title");
    check(`AC-113 (terminum-kanban.md) ${at} a task alone brings the Plan button, its hover the one task`, (await page.isVisible("#curplan")) && alone === "Open: 1 task", alone);
    f.say({ t: "taskmarks", marks: TASKS });
    await pause(page, 150);
    await show(page, C);
    const toldC = await toast(page);
    check(`AC-91 ${at} another session's reminder is said when that session is shown`, toldC === "docs: 1 reminder, 1 new", toldC);
    check(`AC-33 ${at} a session outside projects: its own`, (await buttons(page)).marks === "2", JSON.stringify(await buttons(page)));

    // a row's percent opens that session's Plan pane
    await page.evaluate((k) => document.querySelector(`#sessions .item[data-id="${k}"] .plpct`).click(), B);
    await planOpen(page);
    await pause(page, 200);
    const fromRow = await links(page);
    check(`AC-113 (terminum-kanban.md) ${at} a row's percent: that session's tasks`, fromRow === ["T-3", "T-4", "T-5", "T-6"].map((t) => `${t}=/kanban/${t}`).join(), fromRow);
    await page.keyboard.press("Escape");
    await pause(page, 150);
    await pressFab(page, "#marksfab");
    await pause(page, 200);
    check(`AC-98 ${at} and its bookmarks' button: that session's`, (await ids(page, "markpane")) === "b1,b2,b3,b9", await ids(page, "markpane"));
    await page.keyboard.press("Escape");
    await pause(page, 150);

    // another project now, or none: still its own
    await show(page, A);
    f.say({ t: "projects", items: [{ ...PROJECT, members: [B, D] }, { ...PROJECT, id: "Q", name: "Other", members: [A] }] });
    await pause(page, 200);
    check(`AC-33 ${at} its session moved to another project: its own stay`, (await buttons(page)).marks === "4", JSON.stringify(await buttons(page)));
    f.say({ t: "projects", items: [] });
    await pause(page, 200);
    check(`AC-33 ${at} its project deleted, the session open: its own stay`, (await buttons(page)).marks === "4", JSON.stringify(await buttons(page)));

    // the pane's switches: session, project, all; everything, reminders, bookmarks
    f.say({ t: "projects", items: [PROJECT] });
    await pause(page, 150);
    const pressed = (data) => page.evaluate((d) => document.querySelector(`#markpane .mkfilter [data-${d}][aria-pressed="true"]`)?.dataset[d], data);
    const pick = async (data, v) => { await page.click(`#markpane .mkfilter [data-${data}="${v}"]`); await pause(page, 120); };
    await pressFab(page, "#marksfab");
    await pause(page, 200);
    const sessionList = await ids(page, "markpane");
    check(`AC-33 ${at} the pane's switches: the session's own, everything`, sessionList === "a3,a1,a2,a9" && (await pressed("scope")) === "session" && (await pressed("kind")) === "all", sessionList);
    await pick("kind", "reminder");
    check(`AC-33 ${at} Reminders: its open reminders; the head says so`, (await ids(page, "markpane")) === "a2" && (await heading(page, "markpane")) === "Reminders of this session", await ids(page, "markpane"));
    await pick("kind", "bookmark");
    check(`AC-33 ${at} Bookmarks: its bookmarks`, (await ids(page, "markpane")) === "a3,a1,a9" && (await heading(page, "markpane")) === "Bookmarks of this session", await ids(page, "markpane"));
    await pick("kind", "all");
    await pick("scope", "project");
    const inProject = (await ids(page, "markpane")).split(",").sort().join();
    check(`AC-33 ${at} Project: every session's of the project, not another's; the head names it`, inProject === "a1,a2,a3,a9,b1,b2,b3,b9" && (await heading(page, "markpane")) === "Bookmarks and reminders of Work", inProject);
    const named = await page.evaluate(() => document.querySelector('#markpane .mknav li[data-id="b1"] .meta').textContent);
    check(`AC-33 ${at} another session's record says its session`, named.startsWith("web"), named);
    await pick("scope", "all");
    const inAll = (await ids(page, "markpane")).split(",");
    check(`AC-33 ${at} All: every session's; the head says so; the button's count stays the session's`, inAll.includes("c1") && inAll.includes("a1") && inAll.includes("b1") && (await heading(page, "markpane")) === "All bookmarks and reminders" && (await buttons(page)).marks === "4", inAll.join());
    await pick("kind", "reminder");
    check(`AC-33 ${at} All and Reminders: every session's open ones, a resolved one not`, (await ids(page, "markpane")).split(",").sort().join() === "a2,c2", await ids(page, "markpane"));
    await page.keyboard.press("Escape");
    await pause(page, 150);
    await pressFab(page, "#marksfab");
    await pause(page, 150);
    check(`AC-33 ${at} the choice stays when the button opens it again`, (await pressed("scope")) === "all" && (await pressed("kind")) === "reminder", `${await pressed("scope")} ${await pressed("kind")}`);
    await pick("kind", "all");
    // what became of a closed session's record, an orphaned one's own name
    f.say({ t: "mark", item: rec("z1", "tmux:local:terminum:%77", { orphaned: 1790000100, title: "old one" }) });
    f.say({ t: "mark", item: rec("z2", "tmux:local:terminum:%78", { title: "closed one" }) });
    f.say({ t: "mark", item: rec("z3", B, { orphaned: 1790000100, title: "reused id" }) });
    await pause(page, 200);
    const became = await page.evaluate(() => Object.fromEntries(["z1", "z2", "z3"].map((id) => { const m = document.querySelector(`#markpane .mknav li[data-id="${id}"] .meta`); return [id, m?.textContent]; })));
    check(`AC-33 ${at} All says what became of a session: orphaned, closed, by its last name when its pane id is another's now`, /^old one.*orphaned$/.test(became.z1) && /^closed one.*closed$/.test(became.z2) && /^reused id.*orphaned$/.test(became.z3), JSON.stringify(became));
    for (const id of ["z1", "z2", "z3"]) f.say({ t: "unmark", id });
    await pause(page, 120);
    // the session's own run out: under All the pane stays
    const own = ["a1", "a2", "a3", "a9"];
    for (const id of own) f.say({ t: "unmark", id });
    await pause(page, 200);
    check(`AC-33 ${at} under All the pane stays when the session's own run out`, await page.evaluate(() => !document.getElementById("markpane").hidden) && (await ids(page, "markpane")).includes("c1"), await ids(page, "markpane"));
    for (const id of own) f.say({ t: "mark", item: ITEMS.find((i) => i.id === id) ?? rec(id, A, agent) });
    await pause(page, 150);
    // another session's record: Quote in chat only into an agent's input (this session runs none)
    await page.click('#markpane .mknav li[data-id="b1"] .mkitem');
    await pause(page, 120);
    check(`AC-33 ${at} Quote in chat of another session's record is off where no coding agent takes it`, await page.evaluate(() => document.querySelector("#markpane .tochat").disabled));
    await page.keyboard.press("Escape");
    await pause(page, 150);
    await pressFab(page, "#marksfab");
    await pause(page, 150);
    await pick("scope", "all");
    await page.click('#markpane .mknav li[data-id="b1"] .mkitem');
    await page.click("#markpane .mkvgo");
    await page.waitForFunction((k) => document.querySelector("#sessions .item.on")?.dataset.id === k, B, { timeout: 3000 }).catch(() => {});
    check(`AC-33 ${at} Show in terminal on another session's record shows that session`, await page.evaluate((k) => document.querySelector("#sessions .item.on")?.dataset.id === k, B));
    await show(page, D);
    f.say({ t: "projects", items: [] });
    await pause(page, 120);
    await show(page, A);
    await pressFab(page, "#marksfab");
    await pause(page, 150);
    const off = await page.evaluate(() => document.querySelector('#markpane .mkfilter [data-scope="project"]').disabled);
    check(`AC-33 ${at} Project is off for a session in no project`, off);
    await pick("scope", "session");
    await page.keyboard.press("Escape");
    await pause(page, 150);
    // the wavy lines: at the row's right edge, past its program
    const edge = await page.evaluate((k) => { const r = document.querySelector(`#sessions .item[data-id="${k}"]`), w = r.querySelector(".waves").getBoundingClientRect(), s = r.querySelector(".st").getBoundingClientRect(), b = r.getBoundingClientRect(); return { right: b.right - w.right, past: w.left - s.right, tall: w.height > 20 }; }, A);
    check(`AC-33 ${at} a session's wavy lines stand at its row's right edge, past its state column`, edge.right >= 0 && edge.right <= 3 && edge.past >= 0 && edge.tall, JSON.stringify(edge));

    // the one place with every session's
    await page.evaluate(() => document.getElementById("marksbtn").click());   // a phone: in the closed drawer
    await page.waitForFunction(() => document.getElementById("marksdlg").open, null, { timeout: 3000 });
    await pause(page, 200);
    const all = await page.evaluate(() => [...document.querySelectorAll("#marklist .mkitem .note")].map((n) => n.textContent));
    check(`AC-33 ${at} Bookmarks / Reminders below the sessions lists every session's`, ["note a1", "note a3", "note a9", "note b1", "note c1"].every((n) => all.includes(n)), JSON.stringify(all));
    await page.keyboard.press("Escape");
    check(`no page error ${at}`, f.errors.length === 0, f.errors.join("; "));
    await f.context.close();
  }
}
