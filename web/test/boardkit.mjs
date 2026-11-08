// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What the board's live checks share: terminumd's routes from the page, panes of their own and an
// agent's hooks and tools in them, by the built binary under its command's name.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { until } from "./check.mjs";
import { ownTmp } from "./testrun.mjs";

export const nap = (ms) => new Promise((r) => setTimeout(r, ms));
export const rpc = (id, method, params) => JSON.stringify({ jsonrpc: "2.0", ...(id === null ? {} : { id }), method, ...(params ? { params } : {}) });
/** How a client opens `terminum mcp`, before its calls. */
export const mcpOpening = [rpc(1, "initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "e2e", version: "1" } }), rpc(null, "notifications/initialized")];

export function boardKit(page, { base, tmux, work, bin, home, app }) {
  const cli = path.join(ownTmp("tmb-"), "terminum");
  fs.symlinkSync(bin, cli);
  const j = (p, method = "GET", body) => page.evaluate(async ([p, method, body]) => { const r = await fetch(p, { method, headers: method === "GET" ? undefined : { "Content-Type": "application/json" }, body: method === "GET" ? undefined : JSON.stringify(body ?? {}) }); return { code: r.status, ...(await r.json().catch(() => ({}))) }; }, [p, method, body]);
  const panes = [];
  const newPane = (name) => {
    const pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", name).trim();
    panes.push(pane);
    return pane;
  };
  const keyOf = async (pane) => { let k; await until(async () => (k = (await j("/api/tasks/meta")).sessions.find((s) => s.key.endsWith(`:${pane}`))?.key)); return k; };
  const envOf = (pane) => {
    const sock = tmux("display", "-p", "#{socket_path}"), pid = tmux("display", "-p", "#{pid}");
    return { ...process.env, TM_HOME: home, TM_APP_DIR: app, TMUX: `${sock},${pid},0`, TMUX_PANE: pane };
  };
  const hook = (pane, event, more = {}) => execFileSync(cli, ["hook", "claude"], { input: JSON.stringify({ session_id: `s-${pane}`, cwd: work, hook_event_name: event, ...more }), env: envOf(pane), encoding: "utf8", timeout: 8000 });
  /** One tool of `terminum mcp`, as the pane's agent calls it. */
  const tool = (pane, name, args) => {
    const lines = [...mcpOpening, rpc(2, "tools/call", { name, arguments: args })];
    return execFileSync(cli, ["mcp"], { input: `${lines.join("\n")}\n`, env: envOf(pane), encoding: "utf8", timeout: 8000 });
  };
  const endPanes = () => { for (const pane of panes.splice(0)) tmux("kill-pane", "-t", pane); };
  const card = (id) => `.card[data-id="${id}"]`;
  /** The board with its columns' first read drawn (a card or "No tasks" in each): on CI's runner
   *  WebKit's read outlasts any fixed wait, and a check then found no card. */
  const open = async () => {
    await page.goto(`${base}/kanban/`);
    await page.waitForFunction(() => { const lists = [...document.querySelectorAll(".col .cards")]; return lists.length > 0 && lists.every((l) => l.childElementCount > 0); });
    await nap(250);
  };
  /** Where the live test goes on: a session shown on the session page. */
  const backToSessions = async () => {
    await page.goto(`${base}/sessions/`);
    await page.waitForFunction(() => document.querySelectorAll("#sessions .item").length > 0, null, { timeout: 10000 });
    await page.click("#sessions .item");
    await page.waitForSelector("#term", { state: "visible", timeout: 5000 });
  };
  return { j, newPane, keyOf, hook, tool, endPanes, card, open, backToSessions, cli };
}
