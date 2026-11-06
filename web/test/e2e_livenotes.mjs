// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-33, AC-91, AC-98 against a real private terminumd and tmux (e2e_live.mjs): two panes of one
// project, each running the built `terminum mcp` as its agent. Each session's Plan pane and bookmarks'
// pane show only its own; an agent creates, reads, changes and deletes its session's records, the
// user's bookmark too, and no other session's; a turn's end puts up only that session's reminders; a
// project ungrouped leaves a session its records, a closed session orphans them.
import fs from "node:fs";
import path from "node:path";
import { check, shownPane, until } from "./check.mjs";

const rpc = (id, method, params) => JSON.stringify({ jsonrpc: "2.0", ...(id === null ? {} : { id }), method, ...(params ? { params } : {}) });
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

export async function liveNotes(page, { tmux, work, bin, home, app }) {
  const shown = await shownPane(page, { tmux, work });
  const row = (pane) => `#sessions .item[data-id$=":${pane}"]`;
  const made = [];
  /** A pane whose long-lived shell runs what it is given, as an agent's tools run in its pane. */
  const agentPane = async (name) => {
    const dir = fs.realpathSync(fs.mkdtempSync("/tmp/tmln-"));
    fs.mkdirSync(path.join(dir, "bin"));
    fs.symlinkSync(bin, path.join(dir, "bin/terminum"));
    fs.writeFileSync(path.join(dir, "loop.sh"), [
      `export TM_HOME='${home}' TM_APP_DIR='${app}' PATH='${dir}/bin':"$PATH"`, `cd '${dir}'`, `echo "selectable words of ${name}"`,
      "while :; do if [ -f step.sh ]; then mv step.sh run.sh; sh run.sh > run.out 2>&1; echo $? > done; fi; sleep 0.1; done", ""].join("\n"));
    const pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", name, "-c", work, `sh ${dir}/loop.sh`);
    made.push({ pane, dir });
    await page.waitForSelector(row(pane), { timeout: 5000 });
    const inPane = async (script) => {
      fs.rmSync(path.join(dir, "done"), { force: true });
      fs.writeFileSync(path.join(dir, "step.tmp"), script);
      fs.renameSync(path.join(dir, "step.tmp"), path.join(dir, "step.sh"));
      for (let i = 0; i < 150 && !fs.existsSync(path.join(dir, "done")); i++) await wait(100);
      if (!fs.existsSync(path.join(dir, "done"))) throw new Error(`the pane did not run: ${script.slice(0, 80)}`);
      return fs.readFileSync(path.join(dir, "run.out"), "utf8");
    };
    /** One tool call through `terminum mcp`: {text, error}. */
    const tool = async (name, args) => {
      const lines = [rpc(1, "initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "e2e", version: "1" } }), rpc(null, "notifications/initialized"),
        rpc(2, "tools/call", { name, arguments: args })];
      await inPane(`terminum mcp > mcp.out <<'EOF'\n${lines.join("\n")}\nEOF\n`);
      const m = fs.readFileSync(path.join(dir, "mcp.out"), "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)).find((x) => x.id === 2);
      return { text: m?.result?.content?.[0]?.text ?? JSON.stringify(m), error: m?.result?.isError === true };
    };
    return { pane, dir, inPane, tool };
  };
  const added = (text) => Object.fromEntries([...text.matchAll(/(?:bookmark|reminder) (\w+) "([^"]+)"/g)].map((m) => [m[2], m[1]]));
  const stored = () => { try { return JSON.parse(fs.readFileSync(path.join(app, "web-bookmarks.json"), "utf8")).items; } catch { return []; } };
  const rec = (id) => stored().find((i) => i.id === id) ?? null;
  const show = async (pane) => {
    await page.click(row(pane), { position: { x: 12, y: 8 } });
    await page.waitForFunction((s) => document.querySelector(s)?.getAttribute("aria-current") === "true", row(pane), { timeout: 5000 });
    await page.waitForTimeout(300);
  };
  const paneIds = () => page.evaluate(() => [...document.querySelectorAll("#markpane .mknav li")].map((l) => l.dataset.id).sort().join());
  const sorted = (...ids) => [...ids].sort().join();
  const count = () => page.evaluate(() => (document.getElementById("marksfab").hidden ? null : document.getElementById("marksfabn").textContent));

  const a = await agentPane("notes-a"), b = await agentPane("notes-b");
  try {
    for (const [pane, make] of [[a.pane, "Notes"], [b.pane, null]]) {
      await page.click(row(pane), { button: "right" });
      await page.click('#rowmenu button[data-action="project"]');
      if (make) {
        await page.click('#rowmenu button[data-action="newproject"]');
        await page.waitForSelector("#namebox[open]", { timeout: 3000 });
        await page.fill("#namefield", make);
        await page.press("#namefield", "Enter");
      } else await page.click('#rowmenu button:has-text("Notes")');
      await page.waitForSelector(`#sessions section.project ${row(pane).replace("#sessions ", "")}`, { timeout: 5000 });
    }

    // create: the same title in the other session is its own record
    const fromA = added((await a.tool("add_notes_for_user", { notes: [{ title: "Same title", text: "text of a" }, { title: "A asks", text: "a question", remind: true }] })).text);
    const madeB = await b.tool("add_notes_for_user", { notes: [{ title: "Same title", text: "text of b" }, { title: "B asks", text: "b question", remind: true }, { title: "B later", text: "b later", remind: true }] });
    const fromB = added(madeB.text);
    check("AC-98 live: the same title in another session of the project is kept, its own record", !madeB.error && !!fromB["Same title"] && fromB["Same title"] !== fromA["Same title"], madeB.text);

    // the owner's path: the Plan pane, "show them"
    await show(a.pane);
    await page.click("#curplan");
    check("AC-98 live: the Plan pane of a session of the project counts its own records", await until(async () => (await page.textContent("#planbody .plkept").catch(() => "")) === "2 kept for you by the agent: show them"));
    await page.click("#planbody .plkept button");
    check("AC-98 live: \"show them\" lists only that session's", await until(async () => (await paneIds()) === sorted(fromA["Same title"], fromA["A asks"])));
    check("AC-33 live: the pane is headed by the session", (await page.textContent("#markpane h2")) === "Bookmarks and reminders of this session", await page.textContent("#markpane h2"));
    await page.keyboard.press("Escape");
    await show(b.pane);
    check("AC-33 live: the other session counts its own", (await count()) === "3", String(await count()));

    // the user's bookmark in that session, made on the page
    const line = await page.evaluate(() => {
      const r = [...document.querySelectorAll("#screen .ln")].find((l) => l.textContent.includes("selectable words")).getBoundingClientRect();
      return { x: r.left + 4, x2: r.left + 120, y: r.top + r.height / 2 };
    });
    await page.mouse.move(line.x, line.y); await page.mouse.down(); await page.mouse.move(line.x2, line.y, { steps: 4 }); await page.mouse.up();
    await page.waitForSelector("#markadd:not([hidden])", { timeout: 4000 });
    await page.click("#markadd");
    await page.fill("#marknote", "my own bookmark");
    await page.click('#markbox button[value="save"]');
    check("AC-33 live: the user's bookmark is kept", await until(() => stored().some((i) => i.note === "my own bookmark" && !i.by)));
    const mine = stored().find((i) => i.note === "my own bookmark").id;

    // read
    const list = await b.tool("get_notes_for_user", {});
    const seen = list.error ? [] : JSON.parse(list.text);
    check("AC-98 live: get_notes_for_user lists its session's, the agent's and the user's, none of the other session",
      seen.map((r) => r.id).sort().join() === sorted(fromB["Same title"], fromB["B asks"], fromB["B later"], mine) && seen.find((r) => r.id === mine).by === "user", list.text);
    const whole = await b.tool("get_notes_for_user", { id: fromB["Same title"] });
    check("AC-98 live: with an id, that record whole", !whole.error && JSON.parse(whole.text).text === "text of b", whole.text);
    const foreign = await b.tool("get_notes_for_user", { id: fromA["Same title"] });
    check("AC-98 live: another session's is not read", foreign.error && foreign.text.startsWith("No record"), foreign.text);

    // update and delete: only its own, the user's too
    const renamed = await b.tool("update_notes_for_user", { id: mine, title: "renamed by the agent" });
    check("AC-98 live: the agent changes the user's bookmark of its session", !renamed.error && rec(mine)?.note === "renamed by the agent", renamed.text);
    const refused = await b.tool("delete_notes_for_user", { ids: [fromB["B asks"], fromA["Same title"]] });
    check("AC-98 live: an id of another session refuses the whole call", refused.error && refused.text.startsWith("No record") && !!rec(fromB["B asks"]) && !!rec(fromA["Same title"]), refused.text);
    const gone = await b.tool("delete_notes_for_user", { ids: [mine, fromB["B asks"]] });
    check("AC-98 live: the user's bookmark and a reminder are deleted for good", !gone.error && !rec(mine) && !rec(fromB["B asks"]), gone.text);
    check("AC-98 live: the page drops them at once", await until(async () => (await count()) === "2"));

    // a turn's end
    const base = { session_id: "s-notes", cwd: work, prompt_id: "p1", permission_mode: "default" };
    fs.writeFileSync(path.join(a.dir, "start.json"), JSON.stringify({ ...base, hook_event_name: "UserPromptSubmit", prompt: "go" }));
    fs.writeFileSync(path.join(a.dir, "stop.json"), JSON.stringify({ ...base, hook_event_name: "Stop", stop_hook_active: false }));
    await a.inPane("terminum hook claude < start.json; sleep 1; terminum hook claude < stop.json");
    check("AC-91 live: a turn's end puts up its session's reminder", await until(() => rec(fromA["A asks"])?.due === true));
    await wait(500);
    check("AC-91 live: not another session's of its project", !rec(fromB["B later"])?.due, JSON.stringify(rec(fromB["B later"])));

    // the project ungrouped, then a session closed
    await page.click(`#sessions section[data-pid]:has(.item[data-id$=":${a.pane}"]) > h2`, { button: "right" });
    await page.click('#rowmenu button[data-action="ungroup"]');
    check("AC-33 live: the project goes", await until(() => !fs.readFileSync(path.join(app, "projects.json"), "utf8").includes("\"Notes\"")));
    await wait(1000);
    check("AC-33 live: its sessions stay open, their records are not orphaned", !rec(fromB["B later"])?.orphaned && !rec(fromA["A asks"])?.orphaned, JSON.stringify(rec(fromB["B later"])));
    check("AC-33 live: the shown session still counts its own", await until(async () => (await count()) === "2"));
    tmux("kill-pane", "-t", b.pane);
    made.splice(made.findIndex((m) => m.pane === b.pane), 1);
    fs.rmSync(b.dir, { recursive: true, force: true });
    check("AC-33 live: a closed session's records are orphaned", await until(() => !!rec(fromB["B later"])?.orphaned, 10000));
    check("AC-33 live: the other session's are not", !rec(fromA["A asks"])?.orphaned, JSON.stringify(rec(fromA["A asks"])));
  } finally {
    await page.evaluate((k) => document.querySelector(`#sessions .item[data-id$=":${k}"]`)?.click(), shown).catch(() => {});
    for (const { pane, dir } of made) { try { tmux("kill-pane", "-t", pane); } catch { /* its pane ended */ } fs.rmSync(dir, { recursive: true, force: true }); }
  }
  await page.click("#term");
}
