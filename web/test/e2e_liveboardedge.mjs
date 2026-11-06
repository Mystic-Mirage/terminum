// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What breaks a board's flow, against a real private terminumd and tmux (e2e_live.mjs; terminum-kanban.md
// AC-132, AC-133): an agent that stops, runs out of limits or dies with its task open; a card dragged
// and straight back, answers out of order, a drag under another's change, the network or terminumd
// gone in a move, an edit over another's, a file dropped beside, odd text. Each probe stands alone.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { check as report, slack, until } from "./check.mjs";
import { ownTmp } from "./testrun.mjs";

const nap = (ms) => new Promise((r) => setTimeout(r, ms));
const AGENT = new Set(["agent_stops_with_task_open", "agent_out_of_limits", "agent_process_dies", "pane_closes"]);

export async function liveBoardEdge(page, { base, tmux, work, bin, home, app, stop, start }) {
  let tag = "AC-133";
  const check = (name, ok, extra) => report(`${tag} ${name}`, ok, extra);
  // `terminum` by its name: the same binary, which its name makes the command
  const cli = path.join(ownTmp("tmb-"), "terminum");
  fs.symlinkSync(bin, cli);
  const j = (p, method = "GET", body) => page.evaluate(async ([p, method, body]) => { const r = await fetch(p, { method, headers: method === "GET" ? undefined : { "Content-Type": "application/json" }, body: method === "GET" ? undefined : JSON.stringify(body ?? {}) }); return { code: r.status, ...(await r.json().catch(() => ({}))) }; }, [p, method, body]);
  const make = async (title, more = {}) => (await j("/api/tasks", "POST", { title, ...more })).id;
  const st = async (id) => (await j(`/api/tasks/${id}`)).status;
  const card = (id) => `.card[data-id="${id}"]`;
  const colOf = (id) => page.evaluate((s) => document.querySelector(s)?.closest(".col")?.dataset.status ?? null, card(id));
  const isOpen = (sel) => page.evaluate((s) => document.querySelector(s).open, sel);
  const open = async () => { await page.goto(`${base}/kanban/`); await page.waitForSelector("#board .col"); await nap(250); };
  const drag = (id, column) => page.dragAndDrop(card(id), `.col[data-status="${column}"]`);
  const newPane = (name) => { tmux("new-window", "-d", "-n", name); return tmux("list-panes", "-a", "-F", "#{window_name} #{pane_id}").split("\n").find((l) => l.startsWith(`${name} `)).split(" ")[1]; };
  const keyOf = async (pane) => { let k; await until(async () => (k = (await j("/api/tasks/meta")).sessions.find((s) => s.key.endsWith(`:${pane}`))?.key)); return k; };
  const hook = (pane, event, more = {}) => {
    const sock = tmux("display", "-p", "#{socket_path}"), pid = tmux("display", "-p", "#{pid}");
    return execFileSync(cli, ["hook", "claude"], { input: JSON.stringify({ session_id: `s-${pane}`, cwd: work, hook_event_name: event, ...more }), env: { ...process.env, TM_HOME: home, TM_APP_DIR: app, TMUX: `${sock},${pid},0`, TMUX_PANE: pane }, encoding: "utf8", timeout: 8000 });
  };
  const probes = {
    async rapid_drag_and_back() {
      const id = await make("drag and back"); await open();
      let first = true;
      await page.route("**/api/tasks/*", async (r) => { if (r.request().method() === "PATCH" && first) { first = false; await nap(700); } await r.continue(); });
      await drag(id, "DONE"); await drag(id, "TODO");
      await nap(1600); await page.unroute("**/api/tasks/*");
      check("user drags to DONE and straight back: the last drop stands", await st(id) === "TODO", await st(id));
      check("  and the card stands where the last drop put it", await until(async () => await colOf(id) === "TODO"), await colOf(id));
    },
    async optimistic_move() {
      const id = await make("seen at once"); await open();
      await page.route("**/api/tasks/*", async (r) => { if (r.request().method() === "PATCH") await nap(900); await r.continue(); });
      await drag(id, "DONE");
      check("a dropped card stands in its new column at once, not after terminumd answers", await until(async () => await colOf(id) === "DONE", 300), await colOf(id));
      await nap(1300); await page.unroute("**/api/tasks/*");
    },
    async drag_survives_a_change() {
      // the drag's own events: Playwright's WebKit carries no drag through a pause of the mouse
      const id = await make("held while others change"), other = await make("another"); await open();
      await page.evaluate((s) => { window.dt = new DataTransfer(); window.held = document.querySelector(s); window.held.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: window.dt })); }, card(id));
      await j(`/api/tasks/${other}`, "PATCH", { status: "FAILED" }); await nap(500);
      check("another's change during a drag leaves the card held", await page.evaluate(() => window.held.isConnected && window.held.classList.contains("lifted")));
      await page.evaluate(() => {
        const col = document.querySelector('.col[data-status="DONE"]'), r = col.getBoundingClientRect(), at = { bubbles: true, cancelable: true, dataTransfer: window.dt, clientX: r.left + 40, clientY: r.top + 80 };
        col.dispatchEvent(new DragEvent("dragover", at)); col.dispatchEvent(new DragEvent("drop", at)); window.held.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: window.dt }));
      });
      check("  and the drag goes on to its drop", await until(async () => await st(id) === "DONE"), await st(id));
      check("  and what changed meanwhile shows after it", await until(async () => await colOf(other) === "FAILED"), await colOf(other));
    },
    async refused_move_comes_back() {
      const pane = newPane("busy"), key = await keyOf(pane);
      const a = await make("holds the session", { session: key }), b = await make("wants it too", { session: key });
      await j(`/api/tasks/${a}`, "PATCH", { status: "IN PROGRESS" }); await open();
      await drag(b, "IN PROGRESS");
      check("a refused move says why", await until(async () => /already works on/.test(await page.textContent("#toast"))), await page.textContent("#toast"));
      check("  and the card is back in its column", await until(async () => await colOf(b) === "TODO"), await colOf(b));
    },
    async network_drop_on_move() {
      const id = await make("net drop"); await open();
      await page.route("**/api/tasks/*", (r) => (r.request().method() === "PATCH" ? r.abort() : r.continue()));
      await drag(id, "DONE");
      check("a move lost to the network is said", await until(async () => (await page.isVisible("#toast")) && (await page.textContent("#toast")).length > 0), "no toast");
      check("  and the card is back", await until(async () => await colOf(id) === "TODO"), await colOf(id));
      await page.unroute("**/api/tasks/*");
    },
    async server_restart() {
      const id = await make("over a restart"); await open();
      await stop();
      await nap(600);
      check("terminumd gone: the board says so", await until(async () => (await page.isVisible("#boardbanner")) && /terminum|out of date|not running/i.test(await page.textContent("#boardbanner")), 8000), await page.textContent("#boardbanner"));
      await page.dragAndDrop(card(id), '.col[data-status="DONE"]').catch(() => {});
      await nap(400);
      await start();
      check("  back: the board recovers by itself, the banner goes", await until(async () => !(await page.isVisible("#boardbanner")), 15000), await page.textContent("#boardbanner"));
      await j(`/api/tasks/${id}`, "PATCH", { status: "FAILED" });
      check("  and hears changes again", await until(async () => await colOf(id) === "FAILED", 8000), await colOf(id));
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
      await page.selectOption("#taskbody select >> nth=1", "P0"); await nap(400); await page.unroute(`**/api/tasks/${id}`);
      const seen = await page.evaluate(() => { const t = document.querySelector("#toast"); return { pop: t.matches(":popover-open"), text: t.textContent, hidden: t.hidden }; });
      check("a refusal in a window is said over it, and says what is wrong", seen.pop && !seen.hidden && /does not answer/.test(seen.text), JSON.stringify(seen));
      await page.click("#taskclose"); await nap(200);
    },
    async enter_on_the_menu_key() {
      const id = await make("Enter on its key"); await open();
      await page.focus(`${card(id)} .cmove`); await page.keyboard.press("Enter"); await nap(400);
      const seen = await page.evaluate(() => ({ move: document.querySelector("#movebox").open, win: document.querySelector("#taskbox").open }));
      check("Enter on a card's menu key opens the menu alone", seen.move && !seen.win, JSON.stringify(seen));
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
      await page.goto(`${base}/kanban/T-99999`); await page.waitForSelector("#board .col"); await nap(500);
      check("a task that does not exist is said, and the address goes back to the board", /No task/.test(await page.textContent("#toast")) && new URL(page.url()).pathname === "/kanban/", `${await page.textContent("#toast")} ${page.url()}`);
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
      await page.goto(`${base}/kanban/?severity=P0`); await page.waitForSelector("#board .col"); await nap(200);
      await page.click("#newtask"); await page.fill("#newtitle", "hidden by the filter"); await page.selectOption("#newsev", "P3"); await page.click("#newcreate"); await nap(600);
      check("a task a filter hides is said when made", /filter/i.test(await page.textContent("#toast")), await page.textContent("#toast"));
    },
    async comment_refused_keeps() {
      const id = await make("comment kept"); await page.goto(`${base}/kanban/${id}`); await page.waitForSelector("#taskbox[open]"); await nap(300);
      await page.fill(".newcomment textarea", "long thought");
      await page.route(`**/api/tasks/${id}/comments`, (r) => r.abort());
      await page.click(".newcomment .primary"); await nap(400); await page.unroute(`**/api/tasks/${id}/comments`);
      check("a comment lost to the network stays typed, the window open", await isOpen("#taskbox") && await page.inputValue(".newcomment textarea") === "long thought");
      await page.reload(); await page.waitForSelector("#taskbox[open]"); await nap(300);
      check("  and survives a reload", await page.inputValue(".newcomment textarea") === "long thought");
      await page.fill(".newcomment textarea", ""); await page.click("#taskclose");
    },
    async agent_stops_with_task_open() {
      const pane = newPane("agentstop"), key = await keyOf(pane);
      hook(pane, "SessionStart", { source: "startup" }); hook(pane, "UserPromptSubmit", { prompt: "go" });
      const id = await make("agent stops mid task", { session: key }); await j(`/api/tasks/${id}`, "PATCH", { status: "IN PROGRESS" }); await open();
      check("a working agent's task says working", await until(async () => /working/.test(await page.textContent(card(id)))), await page.textContent(card(id)));
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
      check("  and a session Dispatcher did not start keeps its status: a human or its agent moves it", await st(id) === "IN PROGRESS", await st(id));
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
      check("  and a move of it no longer tells a session", (await j(`/api/tasks/${id}`, "PATCH", { status: "DONE" })).code === 200);
    },
    async two_tabs_same_card() {
      const id = await make("two tabs"); await open();
      const p2 = await page.context().newPage(); await p2.goto(`${base}/kanban/`); await p2.waitForSelector(card(id));
      await p2.dragAndDrop(card(id), '.col[data-status="FAILED"]');
      check("a move on one page shows on the other", await until(async () => await colOf(id) === "FAILED"), await colOf(id));
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
    tag = AGENT.has(name) ? "AC-132" : "AC-133";
    await nap(200);                                // a window closed just now goes Back in its own time
    try { await f(); } catch (e) { check(`${name} ran to its end`, false, e.message.split("\n").slice(0, 6).join(" | ")); await page.unroute("**/*").catch(() => {}); }
    await page.evaluate(() => { for (const d of document.querySelectorAll("dialog[open]")) d.close(); }).catch(() => {});
  }
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
