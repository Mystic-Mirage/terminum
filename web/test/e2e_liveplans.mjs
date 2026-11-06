// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-97, AC-98 against a real private terminumd and tmux server (e2e_live.mjs): in a pane of its
// own a long-lived shell (the pane's program stays the same, as an agent's does) runs the built
// binary as `terminum hook claude` with Claude Code's own PostToolUse shapes and as `terminum mcp`
// fed JSON-RPC lines; the row, the header and the Plan pane follow; the records the agent keeps for
// the user (AC-98) are its bookmarks and reminders, settled with ✓ through terminumd and kept across
// its restart; outside tmux, for a pane terminumd does not know and with terminumd stopped the tools
// answer their errors. Then a phone shows the plan.
import fs from "node:fs";
import path from "node:path";
import { check, within } from "./check.mjs";
import { ownTmp } from "./testrun.mjs";

const SESSION = "s-live";
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

/** Claude Code's hook payloads (shapes of a real run), session "s-live", in folder `cwd`. */
function events(cwd) {
  const base = { session_id: SESSION, cwd, prompt_id: "p-live-1", permission_mode: "default" };
  const post = (tool_name, tool_input, tool_response, n) => ({ ...base, hook_event_name: "PostToolUse", tool_name, tool_input, tool_response, tool_use_id: `toolu_live${n}`, duration_ms: 12 });
  const create = (id, subject) => post("TaskCreate", { subject, description: `Task ${subject}` }, { task: { id, subject } }, id);
  const update = (id, from, to, n) => post("TaskUpdate", { taskId: id, status: to }, { success: true, taskId: id, updatedFields: ["status"], statusChange: { from, to } }, n);
  return [{ ...base, hook_event_name: "UserPromptSubmit", prompt: "plan the live check" },
    create("1", "alpha"), create("2", "beta"), create("3", "gamma"),
    update("1", "pending", "in_progress", 4), update("1", "in_progress", "completed", 5), update("2", "pending", "in_progress", 6)];
}

const rpc = (id, method, params) => JSON.stringify({ jsonrpc: "2.0", ...(id === null ? {} : { id }), method, ...(params ? { params } : {}) });
const call = (id, name, args) => rpc(id, "tools/call", { name, arguments: args });
/** "17:40", the local time `at` (unix seconds), as terminumd's localtime_r says it. */
const hhmm = (at) => { const d = new Date(at * 1000); return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`; };

export async function livePlans(page, { tmux, work, bin, home, app, stop, start, signIn, name }) {
  const dir = ownTmp("tmlp-");
  fs.mkdirSync(path.join(dir, "bin"));
  fs.symlinkSync(bin, path.join(dir, "bin/terminum"));        // run by the name `terminum`, as installed
  events(work).forEach((e, i) => fs.writeFileSync(path.join(dir, `ev${i}.json`), JSON.stringify(e)));
  // the private app dir and home by name: never the installed terminumd's socket
  fs.writeFileSync(path.join(dir, "loop.sh"), [
    `export TM_HOME='${home}' TM_APP_DIR='${app}' PATH='${dir}/bin':"$PATH"`, `cd '${dir}'`,
    "while :; do if [ -f step.sh ]; then mv step.sh run.sh; sh run.sh > run.out 2>&1; echo $? > done; fi; sleep 0.1; done", ""].join("\n"));
  const back = await page.evaluate(() => document.querySelector('#sessions .item[aria-current="true"]')?.dataset.id);
  const paneId = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", "plans", "-c", work, `sh ${dir}/loop.sh`);
  /** Runs `script` in the pane's shell; its output. */
  const inPane = async (script) => {
    fs.rmSync(path.join(dir, "done"), { force: true });
    fs.writeFileSync(path.join(dir, "step.tmp"), script);
    fs.renameSync(path.join(dir, "step.tmp"), path.join(dir, "step.sh"));
    for (let i = 0; i < 150 && !fs.existsSync(path.join(dir, "done")); i++) await wait(100);
    if (!fs.existsSync(path.join(dir, "done"))) throw new Error(`the pane did not run: ${script.slice(0, 80)}`);
    return fs.readFileSync(path.join(dir, "run.out"), "utf8");
  };
  const mcp = async (lines, prefix = "") => {
    await inPane(`${prefix}terminum mcp > mcp.out <<'EOF'\n${lines.join("\n")}\nEOF\n`);
    return Object.fromEntries(fs.readFileSync(path.join(dir, "mcp.out"), "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)).map((m) => [m.id, m]));
  };
  const textOf = (m) => m?.result?.content?.[0]?.text ?? JSON.stringify(m);
  const row = () => page.locator("#sessions .item", { hasText: "plans" }).last();
  const rowInfo = () => row().evaluate((r) => ({ id: r.dataset.id, pct: r.querySelector(".plpct")?.textContent ?? null,
    fill: r.querySelector(".pltrack")?.style.getPropertyValue("--pct") ?? null }));
  const paneState = () => page.evaluate(() => {
    const b = document.getElementById("planbody"), all = (s) => [...b.querySelectorAll(s)];
    return { open: !document.getElementById("planpane").hidden, items: all(".plitem").map((li) => `${li.querySelector(".plmark").textContent} ${li.querySelector(".pltext").textContent} ${li.className}`),
      doing: b.querySelector(".pldoing")?.textContent ?? null, meta: all(".plmeta").map((e) => e.textContent), kept: b.querySelector(".plkept")?.textContent ?? null };
  });
  // the session's bookmarks' pane: its entries, an agent's with its mark (AC-33, AC-98)
  const marksState = () => page.evaluate(() => [...document.querySelectorAll("#markpane .mknav li")].map((l) => ({ id: l.dataset.id, agent: !!l.querySelector(".note .agmark"),
    remind: l.classList.contains("remind"), title: l.querySelector(".note").textContent })));
  const until = async (what, ok, ms = 8000) => {
    let last;
    for (let t = 0; t < ms; t += 150) { last = await what(); if (ok(last)) return last; await wait(150); }
    throw new Error(JSON.stringify(last));
  };
  try {
    check("AC-97 a pane of the private tmux server runs the agent's stand-in", /^%\d+$/.test(paneId), paneId);
    await within("AC-97 the pane's shell answers", () => inPane("echo ready"));
    // 1. Claude Code's hooks: a prompt, three TaskCreate, TaskUpdate 1 → completed, 2 → in progress
    const hooked = await inPane(`for f in ev*.json; do terminum hook claude < "$f" || echo "hook failed $?"; done; echo hooks-done`);
    check("AC-97 `terminum hook claude` takes each event silently and exits 0", hooked.trim() === "hooks-done", hooked);
    await within("AC-97 the row shows 33% over a track filled to 33% (1 of 3 completed)", () => until(rowInfo, (r) => r.pct === "33%" && r.fill === "33%"));
    const info = await rowInfo();
    check("AC-97 the row's id is the pane's key", info.id === `tmux:local:${tmux("display", "-p", "-t", paneId, "#{socket_path}").split("/").pop()}:${paneId}`, JSON.stringify(info));
    await row().click();
    await within("AC-97 the shown session's progress circle says 33%, its Plan button's hover 1 of 3 · updated just now", () => until(() => page.evaluate(() => [document.getElementById("planfabt").textContent, document.getElementById("curplan").title].join(" | ")), (t) => t === "33% | Plan: 1 of 3 · updated just now"));
    await page.click("#curplan");
    await within("AC-97 the Plan pane lists alpha ✓, beta ▶, gamma ○ from the agent's plan tool", () => until(paneState, (s) => s.open
      && s.items.join("|") === "✓ alpha plitem completed|▶ beta plitem in_progress|○ gamma plitem pending" && s.meta.includes("from the agent's plan tool")));

    // 2. terminum mcp in the same pane: the tool plan wins over the MCP items; doing, ETA, notes kept
    const before = Math.floor(Date.now() / 1000);
    const a = await mcp([
      rpc(1, "initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "e2e", version: "1" } }),
      rpc(null, "notifications/initialized"), rpc(2, "tools/list"),
      call(3, "update_session_plan", { items: [{ text: "one", status: "completed" }, { text: "two", status: "pending" }], doing: "Checking the live page", eta_minutes: 30 }),
      call(4, "add_notes_for_user", { notes: [{ title: "Parser drops a BOM", text: "<b>parser</b> drops a BOM in `src/parse.rs`", kind: "bug", file: "src/parse.rs" },
        { title: "Decide the paging size", text: "50 or 100 rows a page?", kind: "question", remind: true }] }),
      call(5, "get_session_plan", {})]);
    const etas = [hhmm(before + 1800), hhmm(Math.floor(Date.now() / 1000) + 1800)];
    check("AC-97 initialize answers terminum's server, the version asked, and its instructions",
      a[1]?.result?.protocolVersion === "2025-06-18" && a[1]?.result?.serverInfo?.name === "terminum" && /Keep a plan/.test(a[1]?.result?.instructions ?? ""), JSON.stringify(a[1]));
    check("AC-97 tools/list names the tools; a notification gets no answer", (a[2]?.result?.tools ?? []).map((t) => t.name).join() === "update_session_plan,get_session_plan,add_notes_for_user,get_notes_for_user,update_notes_for_user,delete_notes_for_user"
      && Object.keys(a).sort().join() === "1,2,3,4,5", Object.keys(a).join());
    const t3 = textOf(a[3]), eta = /ETA ~(\d\d:\d\d)\.$/.exec(t3)?.[1];
    check("AC-97 update_session_plan with a built-in plan keeps doing and the ETA (now + 30 min), not its items",
      a[3]?.result?.isError === false && /^Doing and ETA stored; items follow your own plan tool \(1 of 3 completed\), so the items sent were not taken, ETA ~\d\d:\d\d\.$/.test(t3) && etas.includes(eta), `${t3} ${etas}`);
    const ids = /^Kept for the user: bookmark (\w+) "Parser drops a BOM"; reminder (\w+) "Decide the paging size" \(it comes up at each turn end until the user resolves it\)\.$/.exec(textOf(a[4]));
    check("AC-98 add_notes_for_user answers a bookmark's id and a reminder's, saying when the reminder comes up", a[4]?.result?.isError === false && !!ids, textOf(a[4]));
    check("AC-97 get_session_plan answers the stored plan as [x] [>] [ ] lines, doing and ETA",
      textOf(a[5]).startsWith(`[x] alpha\n[>] beta\n[ ] gamma\nDoing: Checking the live page\nETA: ~${eta}\n`), textOf(a[5]));
    await within("AC-97 the Plan button's hover shows the agent's ETA: 1 of 3 · ETA ~hh:mm", () => until(() => page.evaluate(() => document.getElementById("curplan").title), (t) => (t ?? "").startsWith(`Plan: 1 of 3 · ETA ~${eta} · updated just now`)));
    const [bug, later] = [ids?.[1], ids?.[2]];
    await within("AC-98 the open pane asks again: doing, the ETA as the agent's estimate, and the two records it kept", () => until(paneState, (s) => s.doing === "Checking the live page"
      && s.meta.some((m) => m.startsWith(`ETA ~${eta} (agent's estimate, given `)) && s.kept === "2 kept for you by the agent: show them"));
    await page.click("#planbody .plkept button");
    await within("AC-98 \"show them\" opens the session's bookmarks' pane: the bookmark and the reminder with the agent's mark", () => until(marksState, (m) => m.length === 2
      && m.some((e) => e.id === bug && e.agent && !e.remind && e.title === "Parser drops a BOM") && m.some((e) => e.id === later && e.agent && e.remind)));
    const stored = () => { try { return JSON.parse(fs.readFileSync(path.join(app, "web-bookmarks.json"), "utf8")).items; } catch { return null; } };
    const rec = (id) => (stored() ?? []).find((i) => i.id === id) ?? null;
    check("AC-98 terminumd keeps them in web-bookmarks.json as the agent's, the reminder a reminder", rec(bug)?.by === "claude" && rec(bug)?.kind === "bug" && rec(later)?.remind === true,
      JSON.stringify([rec(bug), rec(later)]));
    for (const id of [bug, later]) { await page.click(`#markpane li[data-id="${id}"] .mkitem`); await page.click("#markpane .mkvdone"); }
    await within("AC-33 ✓ settles them through terminumd after 5 s: the bookmark deleted, the reminder resolved", () => until(() => ({ b: rec(bug), r: rec(later) }),
      ({ b, r }) => b === null && !!r?.resolved, 9000));
    await page.keyboard.press("Escape");

    // 3. refused: outside tmux, a pane terminumd does not know, terminumd stopped
    const get = [rpc(1, "initialize", {}), call(2, "get_session_plan", {})];
    const out = await mcp(get, "env -u TMUX -u TMUX_PANE ");
    check("AC-97 outside tmux each tool answers the not-in-a-pane error", out[2]?.result?.isError === true && textOf(out[2]) === "This session does not run in a terminum pane; do not call terminum's tools again.", textOf(out[2]));
    const unknown = await mcp(get, "TMUX_PANE=%99999 ");
    check("AC-97 in a pane terminumd does not know the same error", unknown[2]?.result?.isError === true && /does not run in a terminum pane/.test(textOf(unknown[2])), textOf(unknown[2]));
    await page.keyboard.press("Escape");
    check("AC-97 the Plan pane is closed (\"show them\" closed it)", (await paneState()).open === false);
    await stop();
    const down = await mcp(get);
    check("AC-97 with terminumd stopped each tool answers it is not running", down[2]?.result?.isError === true && textOf(down[2]) === "terminum is not running; go on without it.", textOf(down[2]));
    const quiet = await inPane(`terminum hook claude < ev0.json; echo "exit $?"`);
    check("AC-97 with terminumd stopped the hook still exits 0, silent", quiet.trim() === "exit 0", quiet);
    await start();
    await within("AC-97 after a restart the row keeps its plan and the pane its plan; the resolved reminder stays resolved", async () => {
      await page.waitForFunction(() => !document.getElementById("status").innerText.includes("not running") && document.querySelectorAll("#sessions .item").length > 0, null, { timeout: 8000 });
      await until(rowInfo, (r) => r.pct === "33%");
      await row().click({ position: { x: 12, y: 8 } });  // its title's side, not the percent (which opens the pane)
      await until(() => page.evaluate(() => [document.getElementById("planfabt").textContent, document.getElementById("curplan").title].join(" | ")), (t) => (t ?? "").startsWith("33% | Plan: 1 of 3 · ETA ~"));
      if (!(await paneState()).open) await page.click("#curplan");
      await until(paneState, (s) => s.open && s.doing === "Checking the live page" && s.kept === "Nothing kept for you by the agent.");
    });
    await page.keyboard.press("Escape");

    // 4. a phone: the shown session's progress and the Plan pane as a sheet under the header
    const phone = await page.context().browser().newContext({ viewport: { width: 390, height: 844 }, isMobile: name === "chromium", hasTouch: true });
    try {
      const p = await phone.newPage();
      await signIn(p);
      await p.evaluate((k) => document.querySelector(`#sessions .item[data-id="${k}"]`).click(), info.id);
      await within("a phone: the progress circle says 33%, the Plan button's hover the ETA", () => p.waitForFunction(() => document.getElementById("planfabt").textContent === "33%"
        && /^Plan: 1 of 3 · ETA ~/.test(document.getElementById("curplan").title), null, { timeout: 8000 }));
      await p.click("#curplan");
      await within("a phone: the Plan pane is full width under the header with the three items, its buttons a finger's size", () => p.waitForFunction(() => {
        const pane = document.getElementById("planpane"), r = pane.getBoundingClientRect(), bar = document.getElementById("bar").getBoundingClientRect();
        const small = [...pane.querySelectorAll("button")].filter((b) => b.offsetParent).some((b) => { const q = b.getBoundingClientRect(); return q.width < 44 || q.height < 44; });
        return !pane.hidden && Math.round(r.left) === 0 && Math.round(r.width) === 390 && Math.abs(r.top - bar.bottom) <= 1 && pane.querySelectorAll(".plitem").length === 3 && !small
          && document.documentElement.scrollWidth <= innerWidth;
      }, null, { timeout: 8000 }));
    } finally { await phone.close(); }
  } finally {
    // the session shown before comes back, then the pane goes (its loop with it: this test's own)
    await page.evaluate((k) => document.querySelector(`#sessions .item[data-id="${k}"]`)?.click(), back).catch(() => {});
    tmux("kill-pane", "-t", paneId);                   // its loop ends with the pane (this test's own)
    fs.rmSync(dir, { recursive: true, force: true });
  }
  await page.click("#term");
}
