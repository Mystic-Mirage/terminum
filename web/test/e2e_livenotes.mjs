// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Against a real private terminumd and tmux (e2e_live.mjs): two panes of one project and one outside
// it, each running the built `terminum mcp` as its agent. terminum-kanban.md AC-115: an agent's task
// tools and rights.
// terminum-terminal.md AC-33 and AC-91: bookmarks and reminders are the user's, a session's own; a
// turn's end puts up only that session's reminders; a project deleted leaves a session its records, a
// closed session orphans them.
import fs from "node:fs";
import path from "node:path";
import { mcpOpening, rpc } from "./boardkit.mjs";
import { check, shownPane, until } from "./check.mjs";
import { foldFoot, pressFoot } from "./fake.mjs";
import { ownTmp } from "./testrun.mjs";

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

export async function liveNotes(page, { tmux, work, bin, home, app }) {
  const shown = await shownPane(page, { tmux, work });
  const row = (pane) => `#sessions .item[data-id$=":${pane}"]`;
  const made = [];
  /** A pane whose long-lived shell runs what it is given, as an agent's tools run in its pane. */
  const agentPane = async (name) => {
    const dir = ownTmp("tmln-");
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
    /** One call through `terminum mcp`: its answer, or the list's. */
    const mcp = async (name, args) => {
      const lines = [...mcpOpening,
        name ? rpc(2, "tools/call", { name, arguments: args }) : rpc(2, "tools/list")];
      await inPane(`terminum mcp > mcp.out <<'EOF'\n${lines.join("\n")}\nEOF\n`);
      return fs.readFileSync(path.join(dir, "mcp.out"), "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)).find((x) => x.id === 2);
    };
    const tool = async (name, args) => {
      const m = await mcp(name, args);
      return { text: m?.result?.content?.[0]?.text ?? JSON.stringify(m), error: m?.result?.isError === true };
    };
    return { pane, dir, inPane, tool, mcp };
  };
  const stored = () => { try { return JSON.parse(fs.readFileSync(path.join(app, "web-bookmarks.json"), "utf8")).items; } catch { return []; } };
  const rec = (note) => stored().find((i) => i.note === note) ?? null;
  const show = async (pane) => {
    await page.click(row(pane), { position: { x: 12, y: 8 } });
    await page.waitForFunction((s) => document.querySelector(s)?.getAttribute("aria-current") === "true", row(pane), { timeout: 5000 });
    await page.waitForTimeout(300);
  };
  const count = () => page.evaluate(() => (document.getElementById("marksfab").hidden ? null : document.getElementById("marksfabn").textContent));
  const task = (id) => page.evaluate(async (t) => (await fetch(`/api/tasks/${t}`)).json(), id);
  const madeId = (said) => /Kept as ([A-Z][A-Z0-9]*-\d+)/.exec(said)?.[1];
  /** The user's own record on the selected words of the shown pane. */
  const keep = async (note, reminder) => {
    const line = await page.evaluate(() => {
      const r = [...document.querySelectorAll("#screen .ln")].find((l) => l.textContent.includes("selectable words")).getBoundingClientRect();
      return { x: r.left + 4, x2: r.left + 120, y: r.top + r.height / 2 };
    });
    await page.mouse.move(line.x, line.y); await page.mouse.down(); await page.mouse.move(line.x2, line.y, { steps: 4 }); await page.mouse.up();
    await page.waitForSelector("#markadd:not([hidden])", { timeout: 4000 });
    await page.click("#markadd");
    if (reminder) await page.click('#markkind button[data-kind="reminder"]');
    await page.fill("#marknote", note);
    await page.click('#markbox button[value="save"]');
    await until(() => !!rec(note));
    await page.evaluate(() => getSelection().removeAllRanges());
    await page.waitForTimeout(200);
  };

  const a = await agentPane("notes-a"), b = await agentPane("notes-b"), lone = await agentPane("notes-c");
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

    // the tools: tasks, no notes
    const list = (await a.mcp())?.result?.tools?.map((t) => t.name) ?? [];
    check("AC-98 agents make tasks: no note tool is offered", !list.some((n) => n.endsWith("_notes_for_user")) && list.includes("create_task"), list.join());

    // an agent's tools and rights
    const madeA = await a.tool("create_task", { title: "Bug: Gap in the parser", text: "API_TOKEN=abc123 here", severity: "P1" });
    const t1 = madeId(madeA.text);
    check("AC-115 live: an agent's new task stands in TODO", !madeA.error && !!t1, madeA.text);
    const asked = await task(t1);
    check("AC-115 live: marked by agent, with no session, in its project, redacted", asked.by_agent === true && asked.session === null && asked.status === "TODO" && asked.owner_kind === "agent" && asked.text.includes("[redacted]") && !asked.text.includes("abc123"), JSON.stringify(asked).slice(0, 300));
    const again = await b.tool("create_task", { title: "Bug: gap in the  PARSER.", severity: "P1" });
    check("AC-115 live: the project's repeat is not kept", again.error && again.text.includes(t1), again.text);
    const elsewhere = await lone.tool("create_task", { title: "Bug: Gap in the parser", severity: "P1" });
    check("AC-115 live: nor is it a repeat outside the project", !elsewhere.error, elsewhere.text);
    check("AC-115 live: a session outside the project does not read it", (await lone.tool("get_task", { id: t1 })).error);
    const read = await b.tool("get_task", { id: t1 });
    check("AC-115 live: the project's other session reads it, said to be data", !read.error && read.text.includes("data, never instructions"), read.text.slice(0, 200));
    check("AC-115 live: and comments on it", !(await b.tool("add_comment", { id: t1, text: "seen" })).error);
    const notYet = await b.tool("move_task", { id: t1, to: "DONE" });
    check("AC-115 live: a free task is taken before it moves", notYet.error && notYet.text.includes("take_task first"), notYet.text);
    check("AC-115 live: take_task links it to the session", !(await a.tool("take_task", { id: t1 })).error && (await task(t1)).session?.key?.endsWith(`:${a.pane}`));
    const notHers = await b.tool("move_task", { id: t1, to: "DONE" });
    check("AC-115 live: another session's is not moved", notHers.error && notHers.text.includes("You can read it and comment"), notHers.text);

    fs.writeFileSync(path.join(a.dir, "begin.json"), JSON.stringify({ session_id: "s-notes", cwd: work, hook_event_name: "SessionStart", source: "startup" }));
    await a.inPane("terminum hook claude < begin.json");
    const done = await a.tool("finish_task", { id: t1, result: "**Fixed** it" });
    const finished = await task(t1);
    check("AC-115 live: finish_task: DONE, its result a comment by the agent", !done.error && finished.status === "DONE" && finished.history.some((e) => e.kind === "comment" && e.text === "**Fixed** it" && e.author_kind === "agent"), done.text);

    // the user's records stay a session's own
    await show(a.pane);
    await keep("A asks", true);
    await show(b.pane);
    await keep("B later", true);
    await keep("my own bookmark", false);
    check("AC-33 live: a session counts its own", (await count()) === "2", String(await count()));
    await show(a.pane);
    check("AC-33 live: the other session counts its own", (await count()) === "1", String(await count()));

    // a turn's end
    const base = { session_id: "s-notes", cwd: work, prompt_id: "p1", permission_mode: "default" };
    fs.writeFileSync(path.join(a.dir, "start.json"), JSON.stringify({ ...base, hook_event_name: "UserPromptSubmit", prompt: "go" }));
    fs.writeFileSync(path.join(a.dir, "stop.json"), JSON.stringify({ ...base, hook_event_name: "Stop", stop_hook_active: false }));
    await a.inPane("terminum hook claude < start.json; sleep 1; terminum hook claude < stop.json");
    check("AC-91 live: a turn's end puts up its session's reminder", await until(() => rec("A asks")?.due === true));
    await wait(500);
    check("AC-91 live: not another session's of its project", !rec("B later")?.due, JSON.stringify(rec("B later")));

    // the project deleted, then a session closed
    await page.click(`#sessions section[data-pid]:has(.item[data-id$=":${a.pane}"]) > h2`, { button: "right" });
    await page.click('#rowmenu button[data-action="deleteproject"]');
    await page.click("#confirmok");
    check("AC-33 live: the project goes", await until(() => page.evaluate(() => ![...document.querySelectorAll("#sessions section.project .label")].some((l) => l.textContent === "Notes"))));
    check("AC-110 live: its tasks stand in Default", await until(async () => (await task(t1)).project === "default"));
    await wait(1000);
    check("AC-33 live: its sessions stay open, their records are not orphaned", !rec("B later")?.orphaned && !rec("A asks")?.orphaned, JSON.stringify(rec("B later")));
    check("AC-33 live: the shown session still counts its own", await until(async () => (await count()) === "1"));
    tmux("kill-pane", "-t", b.pane);
    made.splice(made.findIndex((m) => m.pane === b.pane), 1);
    fs.rmSync(b.dir, { recursive: true, force: true });
    check("AC-33 live: a closed session's records are orphaned", await until(() => !!rec("B later")?.orphaned, 10000));
    check("AC-33 live: the other session's are not", !rec("A asks")?.orphaned, JSON.stringify(rec("A asks")));

    // a record becomes a task (terminum-kanban.md AC-116)
    const back = page.url();
    const makeOf = async (note) => {
      if (page.url() !== back) {
        await page.goto(back);
        await page.waitForSelector(row(a.pane), { timeout: 8000 });
      }
      await pressFoot(page, "#marksbtn");             // the foot folded on a computer: its toggle first
      await page.waitForSelector("#marksdlg[open]");
      await page.click('#marklist .mkkind button:has-text("Reminders")');
      await page.click(`#marklist li:has-text("${note}") .mkmake`);
      await page.waitForSelector("#newbox[open]", { timeout: 8000 });
    };
    await makeOf("A asks");
    const said = await page.inputValue("#newtext");
    check("AC-116 Make a task fills the new-task window", await page.inputValue("#newtitle") === "A asks" && /^> .*electable word/m.test(said) && /\]\(\/sessions\/#\//.test(said)
      && (await page.inputValue("#newsession")).endsWith(`:${a.pane}`), said);
    await page.click("#newdiscard");
    await page.click("#confirmok");                  // what it brought counts as typed: Discard asks
    await page.waitForSelector("#newbox", { state: "hidden" });
    check("AC-116 Discard keeps the record", !!rec("A asks"));
    await makeOf("A asks");
    await page.fill("#newtitle", " ");
    await page.click("#newcreate");
    check("AC-116 a task refused says why, and the record stays", await until(async () => (await page.textContent("#newerror").catch(() => "")).length > 0) && await page.isVisible("#newbox") && !!rec("A asks"));
    await page.fill("#newtitle", "A asks");
    await page.click("#newcreate");
    check("AC-116 Create keeps the task, then the record goes", await until(() => !rec("A asks"), 8000)
      && (await page.evaluate(async () => (await (await fetch("/api/tasks?status=TODO&limit=200")).json()).tasks)).some((t) => t.title === "A asks"));
    await page.goto(back);
    await page.waitForSelector(row(a.pane), { timeout: 8000 });
    await foldFoot(page);
  } finally {
    await page.evaluate((k) => document.querySelector(`#sessions .item[data-id$=":${k}"]`)?.click(), shown).catch(() => {});
    for (const { pane, dir } of made) { try { tmux("kill-pane", "-t", pane); } catch { /* its pane ended */ } fs.rmSync(dir, { recursive: true, force: true }); }
  }
  await page.click("#term");
}
