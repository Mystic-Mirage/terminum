#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Coding agents' progress on a host (terminum-remote.md AC-99) end to end against a private
terminumd, with this Mac playing the host through scripts/fake_ssh.py (a private tmux server,
a fake home with a stub `claude` and a ~/.codex folder). Checks: agents switched on (the
per-agent lines, the plugin and Codex entries, the link socket), a hook and `terminum-host mcp`
from a host pane reach that host's row (plan, doing, ETA, the prompt kept under the host) and the Mac's board (a task),
a pane of another tmux with the same socket name is not filed, a refused forward, agents off,
and agents with files off past the helper's 90 s rule (the longest wait here: about 95 s).

    python3 scripts/e2e_remote_agents.py   (needs terminumd built: cd terminumd && cargo build)
"""
import glob
import json
import re
import os
import sqlite3
import stat
import subprocess
import sys
import time
from urllib.parse import urlencode

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e2e_ws  # noqa: E402
from e2e_ws import Terminumd, check, summary, tmux_on, unix_request, wait_layout  # noqa: E402
from e2e_remote import api  # noqa: E402
from testrun import free_port, own  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
PORT, LOCAL_SOCKET, REMOTE_SOCKET = free_port(), own("tm-test-alocal"), own("tm-test-aremote")
STUB = """#!/bin/sh
echo "$*" >> "$HOME/claude-argv"
case "$*" in
  "plugin install"*) touch "$HOME/.claude-plugin-on" ;;
  "plugin uninstall"*) rm -f "$HOME/.claude-plugin-on" ;;
  "plugin list"*) [ -f "$HOME/.claude-plugin-on" ] && echo "terminum@terminum-local" ;;
esac
exit 0
"""


def events(cwd, session):
    base = {"session_id": session, "cwd": cwd, "prompt_id": "p-host-1", "permission_mode": "default"}
    post = lambda tool, ti, tr, n: {**base, "hook_event_name": "PostToolUse", "tool_name": tool, "tool_input": ti,  # noqa: E731
                                    "tool_response": tr, "tool_use_id": f"toolu_host{n}", "duration_ms": 12}
    create = lambda i, s: post("TaskCreate", {"subject": s, "description": f"Task {s}"}, {"task": {"id": i, "subject": s}}, i)  # noqa: E731
    update = lambda i, a, b, n: post("TaskUpdate", {"taskId": i, "status": b},  # noqa: E731
                                     {"success": True, "taskId": i, "updatedFields": ["status"], "statusChange": {"from": a, "to": b}}, n)
    return [{**base, "hook_event_name": "UserPromptSubmit", "prompt": "plan the host check"},
            create("1", "alpha"), create("2", "beta"), create("3", "gamma"),
            update("1", "pending", "in_progress", 4), update("1", "in_progress", "completed", 5), update("2", "pending", "in_progress", 6)]


def mcp_lines(n0=1):
    call = lambda i, name, args: {"jsonrpc": "2.0", "id": i, "method": "tools/call", "params": {"name": name, "arguments": args}}  # noqa: E731
    return [{"jsonrpc": "2.0", "id": n0, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "e2e", "version": "1"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            call(n0 + 1, "update_session_plan", {"items": [{"text": "one", "status": "completed"}], "doing": "Checking the host", "eta_minutes": 30}),
            call(n0 + 2, "create_task", {"title": "host parser drops a BOM", "text": "The host's parser drops a BOM."})]


def in_pane(sock, target, cmd, done, timeout=20):
    """Type `cmd` into `target` of the private tmux `sock` and wait for `done` in its screen."""
    tmux_on(sock, "send-keys", "-t", target, "-l", cmd)
    tmux_on(sock, "send-keys", "-t", target, "Enter")
    end = time.time() + timeout
    while time.time() < end:
        out = subprocess.run(["tmux", "-L", sock, "capture-pane", "-p", "-J", "-S", "-200", "-t", target], capture_output=True, text=True).stdout
        if done in out.replace(cmd, ""):
            return out
        time.sleep(0.3)
    return out


def host_rec(t, name="devbox"):
    hs = unix_request(t.sock, "GET", "/hosts")[1].get("hosts", [])
    return next((h for h in hs if h.get("name") == name), {})


def until(fn, pred, timeout):
    end, v = time.time() + timeout, None
    while time.time() < end:
        v = fn()
        if pred(v):
            return v
        time.sleep(0.3)
    return v


def hooks_known(sock, target, evdir, hb):
    """Whether terminum remembers the pane's hooks: get_session_plan through `terminum-host mcp`
    says HOOKS_OFF once its hook state is dropped."""
    call = {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "get_session_plan", "arguments": {}}}
    open(os.path.join(evdir, "get.in"), "w").write(json.dumps(mcp_lines()[0]) + "\n" + json.dumps(mcp_lines()[1]) + "\n" + json.dumps(call) + "\n")
    out = os.path.join(evdir, "get.out")
    if os.path.exists(out):
        os.unlink(out)
    done = f"get-done-{time.time():.0f}"   # the screen still shows the marker of an earlier call
    in_pane(sock, target, f"{hb} mcp < get.in > get.out 2>&1; echo {done}", done)
    answers = [json.loads(x) for x in open(out) if x.startswith("{")]
    said = next((a for a in answers if a.get("id") == 2), {}).get("result", {})
    return said.get("isError") is False and "hooks are not running" not in json.dumps(said)


def agents_only(t, home, hb, pane):
    """AC-99 "a link in use stays up with files off"; beside it, a helper with its stdin open that
    nothing talks to: it must exit by the same 90 s rule. A flap fails at once, not at the end."""
    st, body = unix_request(t.sock, "POST", "/hosts/files-remove", {"name": "devbox"})
    check("setup: files off", st == 200, body)
    st, body = unix_request(t.sock, "POST", "/hosts/agents-on", {"name": "devbox"})
    up = until(lambda: host_rec(t), lambda h: h.get("agents") == "up", 15)
    check("setup: agents on, files off, the link up", st == 200 and up.get("agents") == "up" and up.get("files") is None, (body, up))
    evdir = os.path.join(home, "proj")
    in_pane(REMOTE_SOCKET, pane, f'cd {evdir}; {hb} hook claude < ev0.json; echo "hook-rc=$?"', "hook-rc=")
    check("AC-99 agents only: the pane's hook state is known", hooks_known(REMOTE_SOCKET, pane, evdir, hb), open(os.path.join(evdir, "get.out")).read()[-400:])
    alog = os.path.join(home, ".terminum", "logs", "agent.log")
    lines = lambda: open(alog).read()   # noqa: E731
    starts = lines().count('event="start"')
    lone_log = os.path.join(t.root, "lone.log")
    lone = subprocess.Popen([e2e_ws.BIN, "--agent", "--socket", os.path.join(t.root, "l", "s"), "--log", lone_log],
                            env={**t.env, "HOME": home}, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    lone.stdin.write(b"0123456789abcdef0123456789abcdef\n")
    lone.stdin.flush()
    began = time.time()
    try:
        while time.time() - began < 95 or lone.poll() is None:
            if "no Mac connected" in lines() or host_rec(t).get("agents") != "up" or time.time() - began > 110:
                break
            time.sleep(1)
        took = time.time() - began
        check(f"AC-99 agents only: the helper stays past 90 s with no event stream ({took:.0f} s)",
              took >= 95 and "no Mac connected" not in lines() and lines().count('event="start"') == starts, lines()[-300:])
        check("AC-99 agents only: `terminum hosts` still says up", host_rec(t).get("agents") == "up", host_rec(t))
        until(lambda: host_rec(t), lambda h: h.get("agents") == "up", 5)
        check("AC-99 agents only: the hook state survives", hooks_known(REMOTE_SOCKET, pane, evdir, hb), open(os.path.join(evdir, "get.out")).read()[-400:])
        lone_said = open(lone_log).read() if os.path.exists(lone_log) else ""
        check("AC-82 a helper nothing connects to still exits after 90 s", lone.poll() is not None and "no Mac connected for 90 s" in lone_said, (lone.poll(), lone_said[-300:]))
    finally:
        if lone.poll() is None:
            lone.kill()
        lone.wait()


def main():
    t = Terminumd(PORT, LOCAL_SOCKET)
    home = os.path.join(t.root, "rh")
    os.makedirs(os.path.join(home, ".local", "bin"))
    os.makedirs(os.path.join(home, ".codex"))
    os.makedirs(os.path.join(home, "proj"))
    stub = os.path.join(home, ".local", "bin", "claude")
    open(stub, "w").write(STUB)
    os.chmod(stub, stat.S_IRWXU)
    pids = os.path.join(t.root, "ssh-pids")
    t.env.update(TM_SSH=os.path.join(HERE, "fake_ssh.py"), FAKE_SSH_HOME=home, FAKE_SSH_TMUX_SOCKET=REMOTE_SOCKET, FAKE_SSH_PIDS=pids)
    os.system(f"tmux -L {REMOTE_SOCKET} kill-server 2>/dev/null")
    other = None
    try:
        t.start()
        ws = t.page(t.signin())
        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "devbox", "recipe": "alex@devbox.example", "files": True})
        check("setup: the host is added with files", st == 200, body)
        groups = wait_layout(ws, lambda gs: any(g.get("host") == "devbox" and g["items"] for g in gs), 10) or []
        item = next((g["items"][0] for g in groups if g.get("host") == "devbox" and g["items"]), {})
        key = item.get("id", "")
        until(lambda: host_rec(t), lambda h: h.get("files") == "enabled", 20)

        # 2. agents on
        st, body = unix_request(t.sock, "POST", "/hosts/agents-on", {"name": "devbox"})
        msg = (body or {}).get("message") or ""
        print("    agents on:", json.dumps(body.get("message") if body else body))
        argv = open(os.path.join(home, "claude-argv")).read() if os.path.exists(os.path.join(home, "claude-argv")) else ""
        check("AC-99 agents on: Claude installed through its own CLI", st == 200 and "claude: installed" in msg.lower()
              and "plugin install terminum@terminum-local" in argv, (msg, argv))
        check("AC-99 agents on: Codex installed or needs trust", "codex: installed" in msg.lower() or "trust" in msg.lower(), msg)
        hb = os.path.join(home, ".terminum", "bin", "terminum-host")
        check("AC-99 ~/.terminum/bin/terminum-host is there", os.path.exists(hb), hb)
        plug = {os.path.basename(p): open(p).read() for p in glob.glob(os.path.join(home, ".terminum", "hooks", "claude", "**", "*.json"), recursive=True, include_hidden=True)}
        check("AC-99 the Claude plugin's .mcp.json and hooks.json run terminum-host, PostToolBatch synchronous",
              "terminum-host" in plug.get(".mcp.json", "") and "terminum-host" in plug.get("hooks.json", "")
              and "PostToolBatch" in plug.get("hooks.json", ""), list(plug))
        cfg = open(os.path.join(home, ".codex", "config.toml")).read() if os.path.exists(os.path.join(home, ".codex", "config.toml")) else ""
        chk = open(os.path.join(home, ".codex", "hooks.json")).read() if os.path.exists(os.path.join(home, ".codex", "hooks.json")) else ""
        check("AC-99 Codex: the MCP block and hooks.json entries run terminum-host", "terminum-host" in cfg and "terminum-host" in chk, (cfg[:200], chk[:200]))
        up = until(lambda: host_rec(t), lambda h: str(h.get("agents", "")).startswith("up"), 10)
        check("AC-99 `terminum hosts` shows agents up within seconds", str(up.get("agents", "")).startswith("up"), up.get("agents"))
        socks = glob.glob(os.path.join(home, ".terminum", "run", "mac-*.sock"))
        live = False
        if socks:
            import socket
            s = socket.socket(socket.AF_UNIX)
            try:
                s.connect(socks[0])
                live = True
            except OSError:
                pass
            s.close()
        check("AC-99 the host's ~/.terminum/run/mac-<id>.sock answers", live, socks)

        # 3. hook + mcp from a host pane
        pane = item.get("id", "").rsplit(":", 1)[-1]
        evdir = os.path.join(home, "proj")
        for i, ev in enumerate(events(evdir, "s-host")):
            open(os.path.join(evdir, f"ev{i}.json"), "w").write(json.dumps(ev))
        open(os.path.join(evdir, "mcp.in"), "w").write("".join(json.dumps(m) + "\n" for m in mcp_lines()))
        # one program plays the agent, its hooks and its MCP server its children, as Claude Code
        # runs them: run from the shell, each would be the pane's program for a moment and the
        # list could see the program change (the plan archived, AC-97); it waits for a key
        open(os.path.join(evdir, "agent.sh"), "w").write(
            f'for f in ev*.json; do {hb} hook claude < "$f" || echo "hook-failed $?"; done; echo hooks-done\n'
            f"{hb} mcp < mcp.in > mcp.out 2>&1; echo mcp-done\nread _\n")
        out = in_pane(REMOTE_SOCKET, pane, f"cd {evdir}; sh agent.sh", "mcp-done")
        check("AC-99 `terminum-host hook claude` in a host pane exits 0 silently", "hooks-done" in out and not re.search(r"hook-failed \d", out), out[-400:])
        answers = [json.loads(x) for x in open(os.path.join(evdir, "mcp.out")) if x.startswith("{")] if os.path.exists(os.path.join(evdir, "mcp.out")) else []
        init = next((a for a in answers if a.get("id") == 1), {})
        check("AC-99 terminum-host mcp: initialize answers with instructions", bool(init.get("result", {}).get("instructions")), answers[:1])
        check("AC-99 terminum-host mcp: the two tool calls succeed", all(next((a for a in answers if a.get("id") == i), {}).get("result", {}).get("isError") is False for i in (2, 3)),
              [a.get("result", {}).get("content") for a in answers[1:]])
        ws.send({"t": "plan", "key": key})
        plan = ws.until(lambda m: m["t"] == "plan" and m.get("key") == key, 10) or {}
        p = plan.get("plan") or {}
        check("AC-99 the host's row has the plan 1 of 3, doing, an ETA",
              key.startswith("tmux:devbox:default:%") and p.get("done") == 1 and p.get("total") == 3 and p.get("doing") == "Checking the host"
              and p.get("eta"), plan)
        db = sqlite3.connect(f"file:{os.path.join(t.app, 'kanban.sqlite')}?mode=ro", uri=True)
        try:
            tasks = db.execute("SELECT title, by_agent FROM tasks").fetchall()
        finally:
            db.close()
        check("AC-99 terminum-kanban.md AC-120 the host agent's task stands in the Mac's store, by an agent", ("host parser drops a BOM", 1) in tasks, tasks)
        metas = [json.load(open(m)) | {"_dir": os.path.dirname(m)} for m in glob.glob(os.path.join(t.home, "sessions", "*", "meta.json"))]
        kept = [m for m in metas if "plan the host check" in open(os.path.join(m["_dir"], "sent.jsonl")).read()] if metas else []
        check("AC-99 the sent history keeps the prompt under the host, not local", kept and all(m.get("host") == "devbox" for m in kept), metas)
        tmux_on(REMOTE_SOCKET, "send-keys", "-t", pane, "Enter")   # the agent ends

        # 4. a pane of another tmux whose socket has the same name: not filed
        other = os.path.join(t.root, "o", REMOTE_SOCKET)
        os.makedirs(os.path.dirname(other))
        subprocess.run(["tmux", "-f", "/dev/null", "-S", other, "new-session", "-d", "-s", "x", "-x", "120", "-y", "30"], env={**t.env, "HOME": home})
        open(os.path.join(evdir, "ev-other.json"), "w").write(json.dumps({"session_id": "s-other", "cwd": evdir, "prompt_id": "p2",
                                                                         "hook_event_name": "UserPromptSubmit", "prompt": "foreign tmux prompt"}))
        subprocess.run(["tmux", "-S", other, "send-keys", "-t", "x", "-l", f"cd {evdir}; {hb} hook claude < ev-other.json; echo other-done"])
        subprocess.run(["tmux", "-S", other, "send-keys", "-t", "x", "Enter"])
        time.sleep(3)
        sent = "".join(open(f).read() for f in glob.glob(os.path.join(t.home, "sessions", "*", "sent.jsonl")))
        check("AC-99 a pane of another tmux with the same socket name is not filed", "foreign tmux prompt" not in sent, sent[-300:])

        # 5. refused forward
        t.env.update(FAKE_SSH_REFUSE_R="1")
        t.stop()
        t.start()
        secret = t.signin()
        ws = t.page(secret)
        h = until(lambda: host_rec(t), lambda h: "refused" in str(h.get("agents", "")), 20)
        check("AC-99 a refused forward: `terminum hosts` says so", "forwarding refused by devbox's sshd" in str(h.get("agents", "")), h.get("agents"))
        st2, body = until(lambda: api(t, secret, "/fb/api/ls?" + urlencode({"path": home}), "devbox"),
                          lambda r: r[0] == 200 and r[1].get("status") == "ready", 5)
        check("AC-99 files still work with the forward refused", st2 == 200 and body.get("status") == "ready"
              and any(e["n"] == "proj" for e in body.get("entries", [])), body)
        out = in_pane(REMOTE_SOCKET, pane, f'{hb} hook claude < ev0.json; echo "rc=$?"', "rc=")
        check("AC-99 the hook with no link exits 0 silently", "rc=0" in out, out[-300:])
        t.env.pop("FAKE_SSH_REFUSE_R")

        # 6. agents off
        st, body = unix_request(t.sock, "POST", "/hosts/agents-off", {"name": "devbox"})
        print("    agents off:", json.dumps(body.get("message") if body else body))
        argv = open(os.path.join(home, "claude-argv")).read()
        cfg = open(os.path.join(home, ".codex", "config.toml")).read() if os.path.exists(os.path.join(home, ".codex", "config.toml")) else ""
        chk = open(os.path.join(home, ".codex", "hooks.json")).read() if os.path.exists(os.path.join(home, ".codex", "hooks.json")) else ""
        check("AC-99 agents off: the plugin uninstalled through Claude's CLI, the Codex entries and terminum-host removed",
              st == 200 and argv.rstrip().endswith("plugin uninstall terminum@terminum-local\nplugin marketplace remove terminum-local")
              and "terminum-host" not in cfg + chk and not os.path.exists(hb), (body, argv[-120:], cfg[:200], chk[:200]))
        check("AC-99 agents off: `terminum hosts` says off; the Mac's link socket is removed",
              host_rec(t).get("agents") == "off" and not glob.glob(os.path.join(t.app, "links", "*.sock")), (host_rec(t), glob.glob(os.path.join(t.app, "links", "*"))))
        gone = until(lambda: glob.glob(os.path.join(home, ".terminum", "run", "*.sock")), lambda v: not v, 10)
        check("AC-99 agents off: the link socket is gone", not gone, gone)
        check("AC-99 agents off: the files helper kept (files on)", any(os.path.exists(os.path.join(home, ".terminum", "bin", n)) for n in ("terminumd-agent", "terminumd")),
              os.listdir(os.path.join(home, ".terminum", "bin")) if os.path.isdir(os.path.join(home, ".terminum", "bin")) else None)

        # 7. agents with files off, past the helper's 90 s rule
        t.stop()
        t.start()
        agents_only(t, home, hb, pane)
    finally:
        t.stop()
        if other:
            os.system(f"tmux -S {other} kill-server 2>/dev/null")
        os.system(f"tmux -L {REMOTE_SOCKET} kill-server 2>/dev/null")
        if os.path.exists(pids):
            for line in open(pids):
                try:
                    os.kill(int(line.split()[0]), 15)
                except (ProcessLookupError, ValueError):
                    pass
        if "--keep" not in sys.argv:
            t.cleanup()
    summary()


if __name__ == "__main__":
    main()
