// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What the board's live checks share: terminumd's routes from the page, panes of their own and an
// agent's hooks and tools in them, by the built binary under its command's name.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { until } from "./check.mjs";
import { ownTmp } from "./testrun.mjs";

export function boardKit(page, { tmux, work, bin, home, app }) {
  const cli = path.join(ownTmp("tmb-"), "terminum");
  fs.symlinkSync(bin, cli);
  const j = (p, method = "GET", body) => page.evaluate(async ([p, method, body]) => { const r = await fetch(p, { method, headers: method === "GET" ? undefined : { "Content-Type": "application/json" }, body: method === "GET" ? undefined : JSON.stringify(body ?? {}) }); return { code: r.status, ...(await r.json().catch(() => ({}))) }; }, [p, method, body]);
  const panes = [];
  const newPane = (name) => {
    tmux("new-window", "-d", "-n", name);
    const pane = tmux("list-panes", "-a", "-F", "#{window_name} #{pane_id}").split("\n").find((l) => l.startsWith(`${name} `)).split(" ")[1];
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
    const rpc = (id, method, params) => JSON.stringify({ jsonrpc: "2.0", ...(id === null ? {} : { id }), method, ...(params ? { params } : {}) });
    const lines = [rpc(1, "initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "e2e", version: "1" } }), rpc(null, "notifications/initialized"), rpc(2, "tools/call", { name, arguments: args })];
    return execFileSync(cli, ["mcp"], { input: `${lines.join("\n")}\n`, env: envOf(pane), encoding: "utf8", timeout: 8000 });
  };
  const endPanes = () => { for (const pane of panes.splice(0)) tmux("kill-pane", "-t", pane); };
  return { j, newPane, keyOf, hook, tool, endPanes };
}
