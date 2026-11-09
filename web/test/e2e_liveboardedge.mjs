// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What breaks a board's flow, against a real private terminumd and tmux (e2e_live.mjs; terminum-kanban.md
// AC-136, AC-133): an agent that stops, runs out of limits or dies with its task open; a card ordered
// and straight back, answers out of order, a drag under another's change, the network or terminumd
// gone in an order, an edit over another's, a file dropped beside, odd text. Each probe stands alone.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { boardKit, nap } from "./boardkit.mjs";
import { check as report, ranToItsEnd, slack, until } from "./check.mjs";

const AGENT = new Set(["agent_stops_with_task_open", "agent_out_of_limits", "agent_process_dies", "pane_closes"]);

export async function liveBoardEdge(page, { base, tmux, work, bin, home, app, stop, start }) {
  let tag = "AC-133";
  const check = (name, ok, extra) => report(`${tag} ${name}`, ok, extra);
  const { j, newPane, keyOf, hook, endPanes, card, open } = boardKit(page, { base, tmux, work, bin, home, app });
  const make = async (title, more = {}) => (await j("/api/tasks", "POST", { title, ...more })).id;
  const st = async (id) => (await j(`/api/tasks/${id}`)).status;
  const colOf = (id) => page.evaluate((s) => document.querySelector(s)?.closest(".col")?.dataset.status ?? null, card(id));
  const isOpen = (sel) => page.evaluate((s) => document.querySelector(s).open, sel);
  const drag = (id, column) => page.dragAndDrop(card(id), `.col[data-status="${column}"]`);
  /** Within its column: `id` dropped just over `other`; a read of the column may be rebuilding it. */
  const before = async (id, other) => { await nap(300); await page.dragAndDrop(card(id), card(other), { targetPosition: { x: 20, y: 3 } }); };
  const colIds = () => page.evaluate(() => [...document.querySelectorAll('.col[data-status="TODO"] .card')].map((c) => c.dataset.id).join());
  const above = async (a, b) => { const ids = (await colIds()).split(","); return ids.includes(a) && ids.includes(b) && ids.indexOf(a) < ids.indexOf(b); };
  const probes = {
    async rapid_order_and_back() {
      const a = await make("drag and back"), b = await make("the other one"); await open();   // new on top: b over a
      let first = true;
      await page.route("**/api/tasks/*", async (r) => { if (r.request().method() === "PATCH" && first) { first = false; await nap(700); } await r.continue(); });
      await before(a, b); await before(b, a);
      await nap(1600); await page.unroute("**/api/tasks/*");
      check("user orders a card and straight back: the last drop stands", await until(async () => await above(b, a)), await colIds());
      await open();
      check("  and terminumd has it so", await above(b, a), await colIds());
    },
    async optimistic_order() {
      const a = await make("seen at once"), b = await make("over it"); await open();
      await page.route("**/api/tasks/*", async (r) => { if (r.request().method() === "PATCH") await nap(900); await r.continue(); });
      await before(a, b);
      check("a dropped card stands in its new place at once, not after terminumd answers", await until(() => above(a, b), 300), await colIds());
      await nap(1300); await page.unroute("**/api/tasks/*");
    },
    async no_move_by_a_drag() {
      const id = await make("dropped on another column"); await open();
      await drag(id, "DONE"); await nap(300);
      check("a drop on another column moves nothing", await st(id) === "TODO" && await colOf(id) === "TODO", `${await st(id)} ${await colOf(id)}`);
      check("  and nothing is said or left lit", !(await page.isVisible("#toast")) && await page.evaluate(() => !document.querySelector(".col.over") && !document.querySelector(".card.lifted")));
    },
    async drag_survives_a_change() {
      // the drag's own events: Playwright's WebKit carries no drag through a pause of the mouse
      const id = await make("held while others change"), stay = await make("stays under it"), other = await make("another"); await open();
      await page.evaluate((s) => { window.dt = new DataTransfer(); window.held = document.querySelector(s); window.held.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: window.dt })); }, card(id));
      await j(`/api/tasks/${other}`, "PATCH", { status: "FAILED" }); await nap(500);
      check("another's change during a drag leaves the card held", await page.evaluate(() => window.held.isConnected && window.held.classList.contains("lifted")));
      await page.evaluate((s) => {
        const col = document.querySelector('.col[data-status="TODO"]'), r = document.querySelector(s).getBoundingClientRect(), at = { bubbles: true, cancelable: true, dataTransfer: window.dt, clientX: r.left + 40, clientY: r.top + 2 };
        col.dispatchEvent(new DragEvent("dragover", at)); col.dispatchEvent(new DragEvent("drop", at)); window.held.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: window.dt }));
      }, card(stay));
      check("  and the drag goes on to its drop", await until(() => above(id, stay)), await colIds());
      check("  and what changed meanwhile shows after it", await until(async () => await colOf(other) === "FAILED"), await colOf(other));
    },
    async refused_order_comes_back() {
      const a = await make("refused its place"), b = await make("stands over it"); await open();
      await page.route("**/api/tasks/*", (r) => (r.request().method() === "PATCH" ? r.fulfill({ status: 400, contentType: "application/json", body: JSON.stringify({ error: "Refused for this check." }) }) : r.continue()));
      await before(a, b);
      check("a refused order says why", await until(async () => /Refused for this check/.test(await page.textContent("#toast"))), await page.textContent("#toast"));
      check("  and the card is back in its place", await until(() => above(b, a)), await colIds());
      await page.unroute("**/api/tasks/*");
    },
    async network_drop_on_order() {
      const a = await make("net drop"), b = await make("over the net drop"); await open();
      await page.route("**/api/tasks/*", (r) => (r.request().method() === "PATCH" ? r.abort() : r.continue()));
      await before(a, b);
      check("an order lost to the network is said", await until(async () => (await page.isVisible("#toast")) && (await page.textContent("#toast")).length > 0), "no toast");
      check("  and the card is back", await until(() => above(b, a)), await colIds());
      await page.unroute("**/api/tasks/*");
    },
    async server_restart() {
      const id = await make("over a restart"); await open();
      await stop();
      await nap(600);
      check("terminumd gone: the board says so", await until(async () => (await page.isVisible("#boardbanner")) && /terminum|out of date|not running/i.test(await page.textContent("#boardbanner")), 8000), await page.textContent("#boardbanner"));
      await start();
      check("  back: the board recovers by itself, the banner goes", await until(async () => !(await page.isVisible("#boardbanner")), 15000), await page.textContent("#boardbanner"));
      await j(`/api/tasks/${id}`, "PATCH", { status: "FAILED" });
      check("  and hears changes again", await until(async () => await colOf(id) === "FAILED", 8000), await colOf(id));
    },
    async meta_read_lost() {
      // its projects and filters lost to the network, as WebKit lost them once on CI: never unsaid by the columns' read
      await page.route("**/api/tasks/meta", (r) => r.abort());
      const columns = page.waitForResponse((r) => r.url().includes("/api/tasks?"));
      await page.goto(`${base}/kanban/?severity=P0`); await columns; await nap(300);
      check("its projects and filters not read: the board says so, after its columns came too", await page.isVisible("#boardbanner") && /out of date/.test(await page.textContent("#boardbanner")), await page.textContent("#boardbanner"));
      await page.unroute("**/api/tasks/meta");
      check("  read again by themselves: the banner goes, the filters show", await until(async () => !(await page.isVisible("#boardbanner")) && await page.isVisible("#clearfilters")), await page.textContent("#boardbanner"));
    },
    async reads_what_changed() {
      const id = await make("heard with no read"), p2 = await page.context().newPage();
      let toPage, columns = 0, windows = 0;
      const tell = (m) => toPage.send(JSON.stringify(m));
      await p2.routeWebSocket(/\/ws$/, (ws) => {
        const far = ws.connectToServer();
        ws.onMessage((m) => far.send(m));
        far.onMessage((m) => ws.send(m));
        toPage = ws;
      });
      p2.on("request", (r) => {
        const at = new URL(r.url()).pathname;
        if (at === "/api/tasks") columns++;
        else if (at === `/api/tasks/${id}`) windows++;
      });
      const counted = () => `columns ${columns}, window ${windows}`;
      await p2.goto(`${base}/kanban/${id}`);
      await p2.waitForSelector("#taskbox[open]");
      await nap(1000);   // the first word of the projects has been read
      columns = windows = 0;
      tell({ t: "taskchange", id, history: true }); await nap(500);
      check("a line of a task's history alone reads no column", columns === 0, counted());
      check("  and the window showing it reads it again", windows > 0, counted());
      columns = 0;
      tell({ t: "taskchange", id });
      check("a task's own change reads the columns", await until(() => columns > 0, 2000), counted());
      await p2.close();
    },
    async stale_answer() {
      const id = await make("newest wins"); await open();
      let held = true;
      await page.route("**/api/tasks?*", async (r) => { if (held && r.request().url().includes("status=DONE")) { held = false; await nap(1200); } await r.continue(); });
      await j(`/api/tasks/${id}`, "PATCH", { severity: "P1" });   // a load, whose DONE answer is held
      await nap(300);
      await j(`/api/tasks/${id}`, "PATCH", { status: "DONE" });   // a newer load shows it in DONE
      await nap(1800); await page.unroute("**/api/tasks?*");
      check("an older answer never stands over a newer one", await colOf(id) === "DONE", await colOf(id));
    },
    async slow_open_of_two_tasks() {
      const a = await make("first opened"), b = await make("second opened"); await open();
      let hold = true;
      await page.route(`**/api/tasks/${a}`, async (r) => { if (hold) { hold = false; await nap(900); } await r.continue(); });
      await page.click(card(a)); await nap(50);
      await page.evaluate((s) => document.querySelector(s).click(), card(b));
      await nap(1300); await page.unroute(`**/api/tasks/${a}`);
      const title = await page.textContent("#tasktitle");
      check("a slow window opened first does not replace the one opened last", title.startsWith(`${b} `) && page.url().includes(b), `${title} ${page.url()}`);
      await page.click("#taskclose");
    },
    async enter_in_title() {
      await open(); await page.click("#newtask"); await page.fill("#newtitle", "made by Enter"); await page.press("#newtitle", "Enter");
      await nap(500);
      const made = await until(async () => (await j("/api/tasks?q=made%20by%20Enter")).tasks?.length === 1, 1500);
      check("Enter in the title creates the task", made, `box open: ${await isOpen("#newbox")}`);
      if (await isOpen("#newbox")) await page.click("#newdiscard");
    },
    async double_create() {
      await open(); await page.click("#newtask"); await page.fill("#newtitle", "double create");
      await page.evaluate(() => { const b = document.querySelector("#newcreate"); b.click(); b.click(); }); await nap(800);
      check("a double press makes one task", (await j("/api/tasks?q=double%20create")).tasks?.length === 1);
    },
    async discard_asks() {
      await open(); await page.click("#newtask"); await page.fill("#newtitle", "typed for a while"); await page.click("#newdiscard"); await nap(300);
      const asked = await isOpen("#confirmbox");
      check("Discard of typed text asks first", asked);
      if (asked) await page.keyboard.press("Escape"); await nap(200);
      if (await isOpen("#newbox")) { await page.fill("#newtitle", ""); await page.click("#newdiscard"); await nap(200); if (await isOpen("#confirmbox")) await page.click("#confirmok"); }
    },
    async create_during_upload() {
      await open(); await page.click("#newtask"); await page.fill("#newtitle", "while it uploads");
      await page.route("**/api/task-files*", async (r) => { await nap(900); await r.continue(); });
      await page.setInputFiles("#newpick", { name: "late.txt", mimeType: "text/plain", buffer: Buffer.from("late") });
      await nap(100);
      const disabled = await page.evaluate(() => document.querySelector("#newcreate").disabled);
      await page.evaluate(() => document.querySelector("#newcreate").click());
      await nap(1600); await page.unroute("**/api/task-files*");
      const got = (await j("/api/tasks?q=while%20it%20uploads")).tasks?.[0];
      check("Create while a file uploads waits, the file on the task", disabled || got?.files === 1, JSON.stringify({ disabled, files: got?.files }));
      if (await isOpen("#newbox")) { await page.click("#newcreate"); await nap(300); }
    },
    async edit_conflict() {
      const id = await make("two editors", { text: "first words" }); await page.goto(`${base}/kanban/${id}`); await page.waitForSelector("#taskbox[open]"); await nap(300);
      await page.click('.tasktext button:has-text("Edit")'); await page.fill(".tasktext textarea", "my long edit");
      await j(`/api/tasks/${id}`, "PATCH", { text: "someone else's words" });
      await page.click('.tasktext button:has-text("Save")'); await nap(500);
      const text = (await j(`/api/tasks/${id}`)).text;
      const asked = await isOpen("#confirmbox");
      check("saving over another's edit asks, nothing lost", asked && text === "someone else's words", JSON.stringify({ asked, text }));
      if (asked) await page.keyboard.press("Escape"); await nap(150); await page.click("#taskclose").catch(() => {});
    },
    async unsaved_edit_asks() {
      const id = await make("an edit not saved", { text: "words" }); await page.goto(`${base}/kanban/${id}`); await page.waitForSelector("#taskbox[open]"); await nap(300);
      await page.click('.tasktext button:has-text("Edit")'); await page.fill(".tasktext textarea", "half a thought");
      await page.keyboard.press("Escape"); await nap(300);
      check("Esc with an edit not saved asks", await isOpen("#confirmbox"));
      await page.keyboard.press("Escape"); await nap(300);
      check("  and Cancel keeps it", await isOpen("#taskbox") && await page.inputValue(".tasktext textarea") === "half a thought");
      await page.click(".tasktext button:has-text('Cancel')"); await nap(200); await page.click("#taskclose"); await nap(200);
    },
    async toast_over_window() {
      const id = await make("a refusal in a window"); await page.goto(`${base}/kanban/${id}`); await page.waitForSelector("#taskbox[open]"); await nap(300);
      await page.route(`**/api/tasks/${id}`, (r) => (r.request().method() === "PATCH" ? r.abort() : r.continue()));
      await page.selectOption('#taskbody .fieldgrid label:has-text("Severity") select', "P0"); await nap(400); await page.unroute(`**/api/tasks/${id}`);
      const seen = await page.evaluate(() => { const t = document.querySelector("#toast"); return { pop: t.matches(":popover-open"), text: t.textContent, hidden: t.hidden }; });
      check("a refusal in a window is said over it, and says what is wrong", seen.pop && !seen.hidden && /does not answer/.test(seen.text), JSON.stringify(seen));
      await page.click("#taskclose"); await nap(200);
    },
    async enter_on_the_run_key() {
      const id = await make("Enter on its key"); await open();
      await page.focus(`${card(id)} .crun`); await page.keyboard.press("Enter"); await nap(400);
      const seen = await page.evaluate(() => ({ run: document.querySelector("#runbox").open, win: document.querySelector("#taskbox").open }));
      check("Enter on a card's run key opens the run window alone", seen.run && !seen.win, JSON.stringify(seen));
      await page.keyboard.press("Escape"); await nap(200); await page.evaluate(() => document.querySelector("#taskbox").close());
    },
    async file_dropped_beside() {
      await open();
      const stayed = await page.evaluate(async () => {
        const dt = new DataTransfer(); dt.items.add(new File(["x"], "dropped.txt", { type: "text/plain" }));
        const over = new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: dt }), drop = new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: dt });
        document.querySelector("#board").dispatchEvent(over); document.querySelector("#board").dispatchEvent(drop);
        await new Promise((r) => setTimeout(r, 700));
        return { prevented: drop.defaultPrevented, open: document.querySelector("#newbox").open, files: document.querySelector("#newfiles").textContent };
      });
      check("a file dropped on the board never leaves the page: it starts a new task", stayed.prevented && stayed.open && stayed.files.includes("dropped.txt"), JSON.stringify(stayed));
      if (stayed.open) { await page.fill("#newtitle", ""); await page.fill("#newtext", ""); await page.click("#newdiscard"); await nap(300); if (await isOpen("#confirmbox")) await page.click("#confirmok"); await nap(200); }
    },
    async bidi_title() {
      const id = await make("safe‮evil name");
      check("a title loses what hides or reorders it, whoever writes it", (await j(`/api/tasks/${id}`)).title === "safeevil name", JSON.stringify((await j(`/api/tasks/${id}`)).title));
    },
    async long_words_phone() {
      const id = await make("x".repeat(190), { text: `${"y".repeat(400)}

\`\`\`
${"z".repeat(400)}
\`\`\`` });
      await page.setViewportSize({ width: 390, height: 844 });
      await page.goto(`${base}/kanban/${id}`); await page.waitForSelector("#taskbox[open]"); await nap(500);
      const m = await page.evaluate(() => ({ page: document.documentElement.scrollWidth - innerWidth, box: document.querySelector("#taskbox").scrollWidth - document.querySelector("#taskbox").clientWidth, body: document.querySelector("#taskbody").scrollWidth - document.querySelector("#taskbody").clientWidth, h: document.querySelector("#taskbox").getBoundingClientRect().bottom - innerHeight }));
      check("long words stay inside a phone's window, sideways nothing", m.page <= 0 && m.box <= 1 && m.body <= 1 && m.h <= 0, JSON.stringify(m));
      await page.setViewportSize({ width: 1400, height: 900 }); await page.click("#taskclose"); await nap(200);
    },
    async human_moves_while_agent_finishes() {
      const pane = newPane("agentrace"), key = await keyOf(pane);
      const id = await make("human says DONE, agent finishes", { session: key }); await j(`/api/tasks/${id}`, "PATCH", { status: "IN PROGRESS" });
      await Promise.all([j(`/api/tasks/${id}`, "PATCH", { status: "DONE" }), j(`/api/tasks/${id}`, "PATCH", { status: "TODO" }), j(`/api/tasks/${id}`, "PATCH", { status: "DONE" })]);
      const a = await j(`/api/tasks/${id}`);
      check("moves at once leave one status and all of it in the history", a.code === 200 && a.history.filter((e) => e.kind === "move").length >= 2, JSON.stringify(a.history?.map((e) => e.text)));
    },
    async link_gone_session() {
      const r = await j("/api/tasks", "POST", { title: "linked to nothing", session: "tmux:local:terminum:%999" });
      check("a session that is gone cannot be linked", r.code >= 400 && /session/i.test(r.error ?? ""), JSON.stringify(r).slice(0, 160));
    },
    async deep_link_missing() {
      await page.goto(`${base}/kanban/TASK-99999`); await page.waitForSelector("#board .col"); await nap(500);
      check("a task that does not exist is said, and the address goes back to the board", await until(async () => /No task/.test(await page.textContent("#toast")) && new URL(page.url()).pathname === "/kanban/", 4000), `${await page.textContent("#toast")} ${page.url()}`);
    },
    async back_closes_window() {
      const id = await make("back closes it"); await open(); await page.click(card(id)); await page.waitForSelector("#taskbox[open]");
      await page.goBack(); await nap(500);
      check("Back closes the window and stays on the board", new URL(page.url()).pathname === "/kanban/" && !(await isOpen("#taskbox")), page.url());
      await open();
    },
    async project_gone_in_form() {
      const p = (await j("/api/task-projects", "POST", { name: `Gone ${Date.now() % 10000}` })).id; await open();
      await page.click("#newtask"); await page.fill("#newtitle", "to a gone project"); await page.selectOption("#newproj", p);
      // the project goes on another page
      await page.evaluate(async (id) => { const s = new WebSocket(`ws://${location.host}/ws`, (await import("/signin.js")).protocols()); await new Promise((d) => { s.onopen = d; }); s.send(JSON.stringify({ t: "project", action: "delete", project: id })); await new Promise((d) => setTimeout(d, 300)); s.close(); }, p);
      await nap(900);
      const offered = await page.evaluate((id) => [...document.querySelector("#newproj").options].some((o) => o.value === id), p);
      check("a project deleted meanwhile leaves the open form's picker", !offered, "still offered");
      await page.click("#newcreate"); await nap(500);
      const open_ = await isOpen("#newbox"), made = (await j("/api/tasks?q=to%20a%20gone%20project")).tasks?.length ?? 0;
      check("  and nothing typed is lost", made === 1 || (open_ && await page.inputValue("#newtitle") === "to a gone project"), JSON.stringify({ open_, made }));
      if (open_) await page.click("#newclose");
    },
    async html_is_text() {
      const id = await make("<img src=x onerror=\"window.pwned=1\"><b>bold</b>", { text: "<script>window.pwned=2</script>\n\n[x](javascript:window.pwned=3)" });
      await j(`/api/tasks/${id}/comments`, "POST", { text: "<img src=x onerror=\"window.pwned=4\">" });
      await open(); await page.click(card(id)); await page.waitForSelector("#taskbox[open]"); await nap(500);
      const bad = await page.evaluate(() => ({ p: window.pwned, imgs: document.querySelectorAll(".card img, #tasktitle img, #taskbody img").length, js: [...document.querySelectorAll("#taskbody a")].filter((a) => /^javascript:/i.test(a.getAttribute("href") ?? "")).length }));
      check("HTML in a title, text or comment stays text", !bad.p && !bad.imgs && !bad.js, JSON.stringify(bad));
      await page.click("#taskclose");
    },
    async huge_text() {
      await open(); await page.click("#newtask"); await page.fill("#newtitle", "too long a text");
      await page.evaluate(() => { const t = document.querySelector("#newtext"); t.value = "x".repeat((1 << 20) + 1); t.dispatchEvent(new Event("input", { bubbles: true })); });
      await page.click("#newcreate"); await nap(800);
      check("over 1 MiB is refused in place, the window stays", (await page.textContent("#newerror")).includes("1 MiB") && await isOpen("#newbox"), await page.textContent("#newerror"));
      await page.evaluate(() => { const t = document.querySelector("#newtext"); t.value = ""; }); await page.fill("#newtitle", ""); await page.click("#newclose").catch(() => {});
    },
    async pickers() {
      await open(); await page.click("#fproject summary"); await page.click("#fseverity summary"); await nap(100);
      check("one picker is open at a time", await page.evaluate(() => document.querySelectorAll(".pick[open]").length) === 1, String(await page.evaluate(() => document.querySelectorAll(".pick[open]").length)));
      await page.mouse.click(700, 600); await nap(100);
      check("a press elsewhere closes it", await page.evaluate(() => document.querySelectorAll(".pick[open]").length) === 0);
      await page.click("#fseverity summary"); await page.keyboard.press("Escape"); await nap(100);
      check("Esc closes it", await page.evaluate(() => document.querySelectorAll(".pick[open]").length) === 0);
      await page.evaluate(() => document.querySelectorAll(".pick[open]").forEach((d) => { d.open = false; }));
    },
    async odd_find() {
      await open(); await page.fill("#find", `100%_'"\\`); await nap(600);
      check("Find with % _ ' \" \\ is plain text, no error", !(await page.isVisible("#boardbanner")), await page.textContent("#boardbanner"));
      await page.fill("#find", ""); await nap(400);
    },
    async scroll_kept() {
      for (let i = 0; i < 30; i++) await make(`filler ${i}`, { project: "default" });
      await open();
      await page.evaluate(() => { document.querySelector('.col[data-status="TODO"] .cards').scrollTop = 400; });
      const other = await make("arrives elsewhere"); await j(`/api/tasks/${other}`, "PATCH", { status: "DONE" }); await nap(600);
      const top = await page.evaluate(() => document.querySelector('.col[data-status="TODO"] .cards').scrollTop);
      check("a column's scroll stays when another task changes", top >= 300, String(top));
    },
    async made_under_filter() {
      await open("?severity=P0");
      await page.click("#newtask"); await page.fill("#newtitle", "hidden by the filter"); await page.selectOption("#newsev", "P3"); await page.click("#newcreate"); await nap(600);
      check("a task a filter hides is said when made", /filter/i.test(await page.textContent("#toast")), await page.textContent("#toast"));
    },
    async comment_refused_keeps() {
      const id = await make("comment kept"); await page.goto(`${base}/kanban/${id}`); await page.waitForSelector("#taskbox[open]"); await nap(300);
      await page.fill(".newcomment textarea", "long thought");
      await page.route(`**/api/tasks/${id}/comments`, (r) => r.abort());
      await page.click('.newcomment button:has-text("Close")'); await nap(400); await page.unroute(`**/api/tasks/${id}/comments`);
      check("a comment lost to the network stays typed, the window open", await isOpen("#taskbox") && await page.inputValue(".newcomment textarea") === "long thought");
      await page.reload(); await page.waitForSelector("#taskbox[open]"); await nap(300);
      check("  and survives a reload", await page.inputValue(".newcomment textarea") === "long thought");
      await page.fill(".newcomment textarea", ""); await page.click("#taskclose");
    },
    async agent_stops_with_task_open() {
      const pane = newPane("agentstop"), key = await keyOf(pane);
      hook(pane, "SessionStart", { source: "startup" }); hook(pane, "UserPromptSubmit", { prompt: "go" });
      const id = await make("agent stops mid task", { session: key }); await j(`/api/tasks/${id}`, "PATCH", { status: "IN PROGRESS" }); await open();
      check("a working agent's bar pulses on its card", await until(() => page.isVisible(`${card(id)} .pbar.busy`)), await page.textContent(card(id)));
      hook(pane, "Stop", { last_assistant_message: "I did half of it." });
      check("an agent that stopped with its task open is said on the card", await until(async () => /stopped|not finished/i.test(await page.textContent(card(id))), 5000), await page.textContent(card(id)));
      const h = (await j(`/api/tasks/${id}`)).history;
      check("  and in the task's history, once", await until(async () => (await j(`/api/tasks/${id}`)).history.filter((e) => /stopped/i.test(e.text ?? "")).length === 1), JSON.stringify(h.map((e) => e.text)));
    },
    async agent_out_of_limits() {
      const pane = newPane("agentlimit"), key = await keyOf(pane);
      hook(pane, "SessionStart", { source: "startup" }); hook(pane, "UserPromptSubmit", { prompt: "go" });
      const id = await make("agent runs out of tokens", { session: key }); await j(`/api/tasks/${id}`, "PATCH", { status: "IN PROGRESS" }); await open();
      hook(pane, "StopFailure", { error: "rate_limit" });
      check("an agent out of limits is said on the card, with why", await until(async () => /rate|limit/i.test(await page.textContent(card(id))), 5000), await page.textContent(card(id)));
      check("  and in its history", await until(async () => (await j(`/api/tasks/${id}`)).history.some((e) => /rate|limit/i.test(e.text ?? ""))), "");
      check("  and a session Dispatcher did not start keeps its status", await st(id) === "IN PROGRESS", await st(id));
    },
    async agent_process_dies() {
      const pane = newPane("agentdies"), key = await keyOf(pane);
      hook(pane, "SessionStart", { source: "startup" }); hook(pane, "UserPromptSubmit", { prompt: "go" });
      const id = await make("agent crashes, the shell remains", { session: key }); await j(`/api/tasks/${id}`, "PATCH", { status: "IN PROGRESS" }); await open();
      hook(pane, "SessionEnd", { reason: "other" });
      check("an agent whose session ended with its task open is said on the card", await until(async () => /ended|stopped|no agent/i.test(await page.textContent(card(id))), 5000), await page.textContent(card(id)));
    },
    async pane_closes() {
      const pane = newPane("paneclose"), key = await keyOf(pane);
      const id = await make("its pane closes", { session: key }); await j(`/api/tasks/${id}`, "PATCH", { status: "IN PROGRESS" }); await open();
      tmux("kill-pane", "-t", pane);
      check("a closed pane is said, the status stands", await until(async () => /session closed/.test(await page.textContent(card(id))), 8000) && await st(id) === "IN PROGRESS", await page.textContent(card(id)));
    },
    async two_tabs_same_card() {
      const id = await make("two tabs"); await open();
      const p2 = await page.context().newPage(); await p2.goto(`${base}/kanban/`); await p2.waitForSelector(card(id));
      await p2.evaluate((t) => fetch(`/api/tasks/${t}`, { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ status: "FAILED" }) }), id);
      check("a change on one page shows on the other", await until(async () => await colOf(id) === "FAILED"), await colOf(id));
      await p2.close();
    },
    async dragleave_cleaned() {
      const id = await make("drag cancelled"); await open();
      const box = await (await page.$(card(id))).boundingBox(), to = await (await page.$('.col[data-status="FAILED"]')).boundingBox();
      await page.mouse.move(box.x + 30, box.y + 30); await page.mouse.down(); await page.mouse.move(to.x + 30, to.y + 80, { steps: 6 }); await page.keyboard.press("Escape"); await page.mouse.up(); await nap(200);
      check("a cancelled drag leaves no lit column and no lifted card", await page.evaluate(() => !document.querySelector(".col.over") && !document.querySelector(".card.lifted")));
    },
  };
  for (const [name, f] of Object.entries(probes)) {
    tag = AGENT.has(name) ? "AC-136" : name === "meta_read_lost" ? "AC-105" : "AC-133";
    await nap(200);                                // a window closed just now goes Back in its own time
    try { await f(); } catch (e) { ranToItsEnd(e, `${tag} ${name} ran to its end`); await page.unroute("**/*").catch(() => {}); }
    await page.evaluate(() => { for (const d of document.querySelectorAll("dialog[open]")) d.close(); }).catch(() => {});
  }
  // this test's panes go, their agents' states with them: what follows measures a quiet page
  endPanes();
  // the checks after these stand on the session page
  await page.goto(`${base}/sessions/`);
  await page.waitForSelector("#term", { state: "attached" });
}

/** AS-112's tasks against §9's budgets: `node web/test/e2e_live.mjs board`, a run alone. */
export async function liveBoardScale(page, { base, app }) {
  execFileSync("python3", ["-I", path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../scripts/seed_board.py"), path.join(app, "kanban.sqlite")], { timeout: 120000 });
  const p95 = (l) => [...l].sort((a, b) => a - b)[Math.floor(l.length * 0.95)];
  const columns = ["TODO", "IN PROGRESS", "WAIT FOR HUMAN", "FAILED", "DONE"];
  const read = (q) => page.evaluate(async ([cols, q]) => {
    const at = performance.now();
    await Promise.all(cols.map((s, i) => fetch(`/api/tasks?status=${encodeURIComponent(s)}&limit=200${q}${i ? "" : "&counts=1"}`).then((r) => r.json())));
    return performance.now() - at;
  }, [columns, q]);
  await page.goto(`${base}/kanban/`);
  await page.waitForFunction(() => document.querySelectorAll(".card").length >= 800, null, { timeout: 30000 });
  const first = [], find = [];
  for (let i = 0; i < 9; i++) first.push(await read(""));
  for (let i = 0; i < 9; i++) find.push(await read(`&q=task%20${i}`));
  report("AC-105 §9: 5,000 open and 100,000 archived, the first page p95 < 300 ms", p95(first) < slack(300), Math.round(p95(first)));
  report("AC-111 §9: Find p95 < 300 ms", p95(find) < slack(300), Math.round(p95(find)));
  const id = await page.evaluate(() => document.querySelector('.col[data-status="TODO"] .card').dataset.id);
  const at = Date.now();
  await page.evaluate((t) => fetch(`/api/tasks/${t}`, { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ status: "FAILED" }) }), id);
  await page.waitForFunction((t) => document.querySelector(`.col[data-status="FAILED"] .card[data-id="${t}"]`), id, { timeout: 8000, polling: 16 });
  const moved = Date.now() - at;
  report(`AC-107 §9: a move shows with 1,000 cards drawn: ${moved} ms`, moved < slack(500), moved);
}
