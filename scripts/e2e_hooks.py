#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Coding agents' hooks (terminum-agents.md AC-39) through `terminum hook`, as an agent runs it
in its pane, against a private terminumd and a private tmux server (never the user's): states,
tools, questions and failures in the list over the screen's reading; no output; its time.

    python3 scripts/e2e_hooks.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import statistics
import subprocess
import sys
import time
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import BIN, Terminumd, check, summary, tmux_on, wait_layout  # noqa: E402
from e2e_agentstate import DONE_CC, PROGRAM, SHELL_CC  # noqa: E402
from testrun import free_port, own  # noqa: E402

PORT, SOCKET = free_port(), own("tm-test-hooks")
tmux = partial(tmux_on, SOCKET)


def main():
    t = Terminumd(PORT, SOCKET)
    try:
        t.start()
        ws = t.page()
        last = [[]]

        def row(pane):
            return next((i for g in last[0] for i in g["items"] if i["id"].endswith(":" + pane)), None)

        def seen(pane, pred, timeout=8):
            def keep(gs):
                last[0] = gs
                return (r := row(pane)) is not None and pred(r)
            if (r := row(pane)) is not None and pred(r):
                return r
            return row(pane) if wait_layout(ws, keep, timeout) else (print("  last row:", row(pane)) or None)

        wait_layout(ws, lambda gs: bool(gs), 5)
        pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-t", "main", "sleep", "600").strip()
        sock = tmux("display", "-p", "-t", pane, "#{socket_path}").strip()
        env = {**t.env, "TMUX": f"{sock},1,0", "TMUX_PANE": pane}
        took = []

        def hook(agent, ev, pane_env=None):
            start = time.monotonic()
            p = subprocess.run([BIN, "hook", agent], input=json.dumps(ev), capture_output=True, text=True, env={**env, **(pane_env or {})})
            took.append(time.monotonic() - start)
            return p

        first = seen(pane, lambda r: True)
        check("AC-39 a pane with no hooks and no agent on its screen shows no state", first and first["state"] is None, first)
        p = hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt_id": "p1", "prompt": "run the tests"})
        check("AC-39 the hook prints nothing and exits 0", p.returncode == 0 and p.stdout == "" and p.stderr == "", p)
        hook("claude", {"hook_event_name": "PreToolUse", "session_id": "s1", "prompt_id": "p1", "tool_name": "Bash",
                        "tool_input": {"command": "npm test --token=abc"}, "transcript_path": "/x"})
        r = seen(pane, lambda r: r["state"] == "working" and r.get("tool"))
        check("AC-39 hooks say working and the tool, on a screen that shows no agent", r and r["agent"] == "claude" and r["tool"] == "Bash: npm test", r)
        hook("claude", {"hook_event_name": "SubagentStart", "session_id": "s1", "prompt_id": "p1", "agent_id": "a1"})
        r = seen(pane, lambda r: r.get("subagents") == 1)
        check("AC-39 a subagent counted", r is not None, r)
        hook("claude", {"hook_event_name": "PermissionRequest", "session_id": "s1", "tool_name": "Bash", "tool_input": {"command": "rm -rf build"}})
        r = seen(pane, lambda r: r["state"] == "waiting")
        check("AC-39 an approval waits and says for what", r and r.get("why") == "Approve Bash: rm -rf", r)
        hook("claude", {"hook_event_name": "StopFailure", "session_id": "s1", "prompt_id": "p1", "error": "rate_limit"})
        r = seen(pane, lambda r: r["state"] == "failed")
        check("AC-39 a failed turn says why", r and r.get("why") == "rate_limit" and "subagents" not in r and "tool" not in r, r)
        hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt_id": "p2"})
        hook("claude", {"hook_event_name": "Stop", "session_id": "s1", "prompt_id": "p2"})
        r = seen(pane, lambda r: r["state"] == "done")
        check("AC-39 a stop is done", r is not None, r)
        # a stop while a subagent runs in the background: working until it stops too
        hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt_id": "p3"})
        hook("claude", {"hook_event_name": "SubagentStart", "session_id": "s1", "prompt_id": "p3", "agent_id": "bg1"})
        seen(pane, lambda r: r["state"] == "working" and r.get("subagents") == 1)
        hook("claude", {"hook_event_name": "Stop", "session_id": "s1", "prompt_id": "p3"})
        r = seen(pane, lambda r: r["state"] == "done", 2)
        held = seen(pane, lambda r: r["state"] == "working" and r.get("subagents") == 1, 2)
        hook("claude", {"hook_event_name": "SubagentStop", "session_id": "s1", "prompt_id": "p3", "agent_id": "bg1"})
        done = seen(pane, lambda r: r["state"] == "done")
        check("AC-39 a stop with a subagent in the background stays working until it stops", r is None and held is not None and done is not None, (r, held, done))
        # the folder the agent itself works in (AC-51): its own events move it, a subagent's do not; its
        # row says it (the header) and "files" does (Files' root, printed paths)
        wt = os.path.join(tmux("display", "-p", "-t", pane, "#{pane_current_path}").strip(), "worktrees", "fix")
        hook("claude", {"hook_event_name": "PostToolUse", "session_id": "s1", "prompt_id": "p3", "cwd": wt, "tool_name": "Bash"})
        moved = seen(pane, lambda r: r.get("agentDir") == wt)
        hook("claude", {"hook_event_name": "PreToolUse", "session_id": "s1", "prompt_id": "p3", "agent_id": "sub1", "cwd": "/tmp", "tool_name": "Bash"})
        ws.send({"t": "sub", "id": f"tmux:local:{SOCKET}:{pane}"})
        ws.send({"t": "files"})
        files = ws.until(lambda m: m["t"] == "files" and "agent" in m, 5)
        check("AC-51 the agent's own event moves its folder: its row and \"files\" say it; a subagent's does not",
              moved is not None and moved.get("agentFolder") and files and files["agent"] == wt and files["cwd"] != wt, (moved, files))
        # Mark as Unread from a row's menu (AC-93): the row is unread on every page; Mark as Read takes it away
        ws.send({"t": "unread", "id": f"tmux:local:{SOCKET}:{pane}", "on": True})
        marked = seen(pane, lambda r: r.get("unread") is True)
        ws.send({"t": "unread", "id": f"tmux:local:{SOCKET}:{pane}", "on": False})
        cleared = seen(pane, lambda r: "unread" not in r)
        check("AC-93 Mark as Unread marks the row in the list sent to every page; Mark as Read takes it away", marked is not None and cleared is not None, (marked, cleared))
        p = hook("claude", {"hook_event_name": "Stop"}, {"TMUX_PANE": "%999"})
        p2 = hook("claude", {"hook_event_name": "Stop"}, {"TMUX": "/private/tmp/tmux-1/other,1,0"})
        check("AC-39 a pane of no known server is ignored, quietly", p.returncode == 0 == p2.returncode and "panicked" not in t.log(), t.log()[-300:])
        # sent history (AC-90): the hook's prompt, then a page's shell command, per folder
        cwd = tmux("display", "-p", "-t", pane, "#{pane_current_path}").strip()
        hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt_id": "p3", "cwd": cwd,
                        "prompt": "explain **this**\nexport API_TOKEN=abc123"})
        ws.until(lambda m: m["t"] == "sentnew", 5)        # kept after the hook was answered (it never waits for the disk)
        ws.send({"t": "sub", "id": f"tmux:local:{SOCKET}:{pane}"})
        ws.send({"t": "sentget", "n": 0})
        one = ws.until(lambda m: m["t"] == "sentone", 5)
        check("AC-90 a hook's prompt is kept, its secret's value left out", one and one["item"] and one["item"]["text"] == "explain **this**\nexport API_TOKEN=[redacted]"
              and one["item"]["agent"] == "claude" and one["path"] == cwd, one)
        ws.send({"t": "sent", "text": "make test", "agent": "claude"})
        ws.send({"t": "sent", "text": " secret", "agent": ""})
        ws.send({"t": "sent", "text": "ls -la", "agent": ""})
        new = ws.until(lambda m: m["t"] == "sentnew", 5)
        ws.send({"t": "sentget", "n": 0})
        one = ws.until(lambda m: m["t"] == "sentone", 5)
        check("AC-90 a shell's command from the page; an agent's with hooks and one after a space are not", new and one and one["total"] == 2
              and one["item"]["text"] == "ls -la" and "agent" not in one["item"], one)
        ws.send({"t": "sentget", "n": 1})
        older = ws.until(lambda m: m["t"] == "sentone", 5)
        check("AC-90 the one before", older and older["n"] == 1 and older["item"]["agent"] == "claude", older)
        files = [os.path.join(dp, f) for dp, _, fs in os.walk(os.path.join(t.home, "sessions")) for f in fs]
        check("AC-90 kept in TM_HOME/sessions/<project>/, 0600", len(files) == 2 and all(os.stat(f).st_mode & 0o077 == 0 for f in files), files)
        # where a message was sent (AC-90): a hooked prompt takes the line the page told for its text;
        # terminumd finds it by its text in the pane's history for the strip and for Jump to message
        printed = "for i in $(seq 1 300); do echo out $i; done; echo; echo '> deploy the staging build'; for i in $(seq 1 40); do echo after $i; done; sleep 600"
        p2 = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-t", "main", "sh", "-c", printed).strip()
        k2 = f"tmux:local:{SOCKET}:{p2}"
        time.sleep(1)
        ws.send({"t": "sub", "id": k2})
        ws.until(lambda m: m["t"] == "hist" and m.get("mode") == "reset" and m.get("sid") == k2, 5)
        cwd2 = tmux("display", "-p", "-t", p2, "#{pane_current_path}").strip()
        env2 = {"TMUX_PANE": p2}
        hook("claude", {"hook_event_name": "SessionStart", "session_id": "s2", "cwd": cwd2}, env2)
        seen(p2, lambda r: r.get("agent") == "claude")
        ws.send({"t": "locate", "id": "sent:probe", "text": "deploy the staging build"})
        probe = ws.until(lambda m: m["t"] == "located" and m["id"] == "sent:probe", 8)
        at = probe and probe.get("n")
        check("AC-90 a message's text is found in the pane's history without a line, nearest the end", isinstance(at, int) and at > 300, probe)
        ws.send({"t": "sent", "text": "deploy the staging\n  build", "agent": "claude", "line": 7})   # hooked: only its line is kept for the hook
        hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s2", "prompt_id": "q1", "cwd": cwd2, "prompt": "Background task finished"}, env2)
        hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s2", "prompt_id": "q2", "cwd": cwd2, "prompt": "deploy the staging build"}, env2)
        for _ in range(2):
            ws.until(lambda m: m["t"] == "sentnew", 5)
        ws.send({"t": "sentget", "n": 0})
        kept = ws.until(lambda m: m["t"] == "sentone", 5)
        ws.send({"t": "sentget", "n": 1})
        own = ws.until(lambda m: m["t"] == "sentone", 5)
        item, other = (kept or {}).get("item") or {}, (own or {}).get("item") or {}
        check("AC-90 a hooked prompt takes the page's line for the same text, an agent's own turn none; each keeps its pane",
              item.get("line") == 7 and item.get("key") == k2 and other.get("text") == "Background task finished" and "line" not in other, (item, other))
        ws.send({"t": "sentmarks"})
        marks = ws.until(lambda m: m["t"] == "sentmarks", 8)
        check("AC-90 the strip's places: found by text near the line (7 is far off: the whole history), only the pane's own with a line",
              marks and marks["sid"] == k2 and [i["n"] for i in marks["items"]] == [at] and marks["items"][0]["text"] == "deploy the staging build", marks)
        ws.send({"t": "locate", "id": "sent:near", "text": "deploy the staging build", "n": at - 5})
        near = ws.until(lambda m: m["t"] == "located" and m["id"] == "sent:near", 8)
        ws.send({"t": "locate", "id": "sent:gone", "text": "never printed in this pane", "n": at})
        gone = ws.until(lambda m: m["t"] == "located" and m["id"] == "sent:gone", 8)
        check("AC-90 Jump to message: found near its line; a text not there says so", near and near["n"] == at and gone and gone["n"] is None
              and "no longer" in gone.get("why", ""), (near, gone))
        # Ask's numbers (AC-37): per pane, counting up, also from what the pane sent
        ws.send({"t": "askno"})
        a1 = ws.until(lambda m: m["t"] == "askno", 5)
        ws.send({"t": "askno"})
        a2 = ws.until(lambda m: m["t"] == "askno", 5)
        hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s2", "prompt_id": "q3", "cwd": cwd2,
                        "prompt": "\nQuote #0041:\n - Quote Start ----\n«x»\n- Quote End ----\nwhy?\n----"}, env2)
        ws.until(lambda m: m["t"] == "sentnew" and m.get("key") == k2, 5)
        ws.send({"t": "askno"})
        a3 = ws.until(lambda m: m["t"] == "askno", 5)
        check("AC-37 Ask's quote numbers count up per pane, from the highest the pane sent; sentnew names its pane",
              a1 and a2 and a3 and (a1["n"], a2["n"], a3["n"]) == (1, 2, 42) and a3["sid"] == k2, (a1, a2, a3))
        # where an answer ended (AC-90): at its Stop terminumd reads the screen, the last row above the input box
        rule = "─" * 40
        drawn = f"for i in $(seq 1 50); do echo out $i; done; echo '⏺ The staging deploy is done and verified.'; echo; echo '✻ Worked for 9s'; echo; echo '{rule}'; echo '❯ '; echo '{rule}'; sleep 600"
        p3 = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-t", "main", "sh", "-c", drawn).strip()
        k3, env3 = f"tmux:local:{SOCKET}:{p3}", {"TMUX_PANE": p3}
        time.sleep(1)
        hook("claude", {"hook_event_name": "SessionStart", "session_id": "s3", "cwd": cwd2}, env3)
        hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s3", "prompt_id": "e1", "cwd": cwd2, "prompt": "deploy staging"}, env3)
        hook("claude", {"hook_event_name": "Stop", "session_id": "s3", "prompt_id": "e1"}, env3)
        ended = ws.until(lambda m: m["t"] == "endnew" and m.get("key") == k3, 8)
        ws.send({"t": "sub", "id": k3})
        ws.until(lambda m: m["t"] == "hist" and m.get("mode") == "reset" and m.get("sid") == k3, 5)
        ws.send({"t": "sentmarks"})
        placed = ws.until(lambda m: m["t"] == "sentmarks" and m.get("sid") == k3, 8)
        ends = (placed or {}).get("ends") or []
        check("AC-90 at a Stop terminumd keeps where the answer ended: the last row above the input box, not the turn line; found by its text",
              ended and len(ends) == 1 and ends[0]["text"] == "⏺ The staging deploy is done and verified." and isinstance(ends[0]["n"], int), (ended, placed))
        tmux("kill-pane", "-t", p3)
        tmux("kill-pane", "-t", p2)
        ws.send({"t": "sub", "id": f"tmux:local:{SOCKET}:{pane}"})
        # a reminder (AC-91): up when the pane's turn ends, put down by Close, up again at the next
        ws.send({"t": "mark", "ref": "r1", "sid": f"tmux:local:{SOCKET}:{pane}", "n": 0, "text": "check the retry", "note": "", "remind": True})
        made = ws.until(lambda m: m["t"] == "marked" and m.get("ref") == "r1", 5)
        rid = made and made.get("id")

        def turn(n):
            hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt_id": f"t{n}"})
            seen(pane, lambda r: r["state"] == "working")
            hook("claude", {"hook_event_name": "Stop", "session_id": "s1", "prompt_id": f"t{n}"})
            return ws.until(lambda m: m["t"] == "mark" and m["item"]["id"] == rid and m["item"].get("due"), 8)
        up = turn(1)
        check("AC-91 a reminder comes up when its pane's turn ends", rid and up is not None, made)
        ws.send({"t": "remind", "id": rid, "action": "close"})
        down = ws.until(lambda m: m["t"] == "mark" and m["item"]["id"] == rid, 5)
        again = turn(2)
        check("AC-91 closed, it comes up again at the next turn's end", down and not down["item"].get("due") and again is not None, down)
        ws.send({"t": "remind", "id": rid, "action": "resolve"})
        ws.until(lambda m: m["t"] == "mark" and m["item"]["id"] == rid, 5)
        check("AC-91 resolved, it comes up no more", turn(3) is None)
        # unread (AC-93): a turn that ended while no page in sight showed it; shown again, it is read
        key = f"tmux:local:{SOCKET}:{pane}"
        ws.send({"t": "pause"})                           # this page is hidden (a locked phone)
        hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt_id": "t4"})
        seen(pane, lambda r: r["state"] == "working")
        hook("claude", {"hook_event_name": "Stop", "session_id": "s1", "prompt_id": "t4"})
        r = seen(pane, lambda r: r.get("unread"))
        check("AC-93 a pane whose turn ended unseen is unread in the list", r is not None, r)
        ws.send({"t": "resume"})
        ws.send({"t": "sub", "id": key})
        r = seen(pane, lambda r: not r.get("unread"))
        check("AC-93 shown, it is read", r is not None, r)
        hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt_id": "t5"})
        seen(pane, lambda r: r["state"] == "working")
        hook("claude", {"hook_event_name": "Stop", "session_id": "s1", "prompt_id": "t5"})
        check("AC-93 a turn that ends while it is shown leaves it read", seen(pane, lambda r: r.get("unread"), 2) is None)
        # a Stop while a shell it started runs on (Claude Code's footer: "1 shell"): the turn ends
        # once the shell has (AC-93); the stand-in draws Claude Code's screen, then drops the count
        prog = os.path.join(t.app, "standin.js")
        with open(prog, "w") as f:
            f.write(PROGRAM)
        screens = {"title": "\u2733 Fix it", "rows": [], "phases": [[SHELL_CC, 8000], [DONE_CC, 0]]}
        cc = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-t", "main", "node", prog, json.dumps(screens)).strip()
        seen(cc, lambda r: r["agent"] == "claude")
        hook("claude", {"hook_event_name": "UserPromptSubmit", "session_id": "s2", "prompt_id": "b1"}, {"TMUX_PANE": cc})
        hook("claude", {"hook_event_name": "Stop", "session_id": "s2", "prompt_id": "b1"}, {"TMUX_PANE": cc})
        start = time.monotonic()
        early = seen(cc, lambda r: r.get("unread"), 5)
        r = seen(cc, lambda r: r["state"] == "working", 1)
        check("AC-93 hooks' Stop with a shell still running (its footer counts it): working, not unread",
              early is None and r is not None, (early, r))
        r = seen(cc, lambda r: r.get("unread"), 10)
        check("AC-93 the shell ended: the turn ends, unread", r is not None and r["state"] == "done" and time.monotonic() - start > 2, r)
        # a turn's end is never lost, and one that leaves it to the user waits (AC-39)
        ws.send({"t": "pause"})
        def asked(agent, session, turn):
            hook(agent, {"hook_event_name": "UserPromptSubmit", "session_id": session, "turn_id": turn})
        def clean():
            ws.send({"t": "unread", "id": key, "on": False})
            return seen(pane, lambda r: not r.get("unread"), 3)
        clean()
        asked("codex", "ghost", "g1")
        hook("codex", {"hook_event_name": "SubagentStart", "session_id": "ghost", "turn_id": "g1", "agent_id": "scout"})
        hook("codex", {"hook_event_name": "Stop", "session_id": "ghost", "turn_id": "g1"})
        r = seen(pane, lambda r: r.get("unread"), 8)
        check("AC-39 a Codex Stop ends its turn though an interrupted subagent is still counted: unread", r is not None and "subagents" not in r, r)
        clean()
        asked("codex", "ask", "a1")
        secret = "Разрешаете замаскировать токены?"
        hook("codex", {"hook_event_name": "Stop", "session_id": "ask", "turn_id": "a1", "last_assistant_message": secret})
        r = seen(pane, lambda r: r["state"] == "waiting" and r.get("turnOver") and r.get("unread"), 8)
        check("AC-39 a last message ending in a question waits, in any language; its turn ended: unread", r is not None, r)
        check("AC-39 the message itself never reaches terminumd", r is not None and secret not in json.dumps(r))
        time.sleep(3)
        r = seen(pane, lambda r: r["state"] == "waiting", 2)
        check("AC-39 it stands with no event", r is not None, r)
        asked("codex", "ask", "a2")
        r = seen(pane, lambda r: r["state"] == "working" and not r.get("turnOver"))
        check("AC-39 the next prompt answers it", r is not None, r)
        hook("codex", {"hook_event_name": "PreToolUse", "session_id": "ask", "turn_id": "a2", "tool_name": "request_user_input_async",
                       "tool_input": {"questions": [{"title": "Mask them", "options": ["Yes", "No"]}]}})
        hook("codex", {"hook_event_name": "PostToolUse", "session_id": "ask", "turn_id": "a2", "tool_name": "request_user_input_async", "tool_input": {}})
        check("AC-39 Codex's async question lets its turn go on", seen(pane, lambda r: r["state"] == "working", 3) is not None)
        hook("codex", {"hook_event_name": "Stop", "session_id": "ask", "turn_id": "a2", "last_assistant_message": "Done."})
        r = seen(pane, lambda r: r["state"] == "waiting" and r.get("why") == "Mask them")
        check("AC-39 it stands at its turn's end and says what it asks", r is not None, r)
        hook("codex", {"hook_event_name": "SessionEnd", "session_id": "ask"})
        clean()
        asked("claude", "ghost2", "c1")
        hook("claude", {"hook_event_name": "SubagentStart", "session_id": "ghost2", "prompt_id": "c1", "agent_id": "killed"})
        hook("claude", {"hook_event_name": "Stop", "session_id": "ghost2", "prompt_id": "c1"})
        r = seen(pane, lambda r: r.get("subagents") == 1, 3)
        held = r is not None
        r = seen(pane, lambda r: "subagents" not in r and r.get("unread"), 15)
        check("AC-39 Claude Code's Stop with a killed subagent: counted at first, gone once the screen shows none; unread", held and r is not None, r)
        hook("claude", {"hook_event_name": "SessionEnd", "session_id": "ghost2"})
        clean()
        asked("claude", "nostop", "n1")
        seen(pane, lambda r: r["state"] == "working")
        r = seen(pane, lambda r: r.get("unread"), 50)     # its lease, then 10 s done on the screen
        check("AC-39 a turn with no Stop (Esc, an error) ends by its screen: unread", r is not None, r)
        hook("claude", {"hook_event_name": "SessionEnd", "session_id": "nostop"})
        ws.send({"t": "resume"})
        h = t.health()
        check("AC-39 /health says when Claude Code's hooks last spoke", isinstance(h.get("hooks", {}).get("claude"), int), h.get("hooks"))
        stopped = Terminumd.__new__(Terminumd)
        stopped.env = {**env, "TM_APP_DIR": t.app + "-none"}
        os.makedirs(stopped.env["TM_APP_DIR"], mode=0o700, exist_ok=True)
        start = time.monotonic()
        p = subprocess.run([BIN, "hook", "claude"], input='{"hook_event_name":"Stop"}', capture_output=True, text=True, env=stopped.env)
        check("AC-39 without terminumd the hook returns at once", p.returncode == 0 and time.monotonic() - start < 0.5, time.monotonic() - start)
        med = statistics.median(took)
        check("AC-39 a hook takes under 50 ms (median of its runs)", med < 0.05, f"{med * 1000:.1f} ms of {len(took)}")
    finally:
        t.stop()
        tmux("kill-server")
        t.cleanup()
    summary()


if __name__ == "__main__":
    main()
