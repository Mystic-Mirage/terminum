#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Coding agents' states in the list (terminum-agents.md AC-39) for Codex installed with npm,
whose program is `node`, against a private terminumd and a private tmux server (never the
user's): `node` programs draw Codex's screens as measured on Codex 0.162, and a dev server's.

    python3 scripts/e2e_agentstate.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import shutil
import sys
import tempfile
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, summary, tmux_on, wait_layout  # noqa: E402
from testrun import free_port, own, short_tmp  # noqa: E402

PORT, SOCKET = free_port(), own("tm-test-agent")
tmux = partial(tmux_on, SOCKET)

# what each stand-in prints at the screen's bottom, as Codex draws, then it waits; "later" clears
# the screen after 3 s and prints that
WORK = ["• Ran pwd", "", "Working (10s • esc to interrupt)", "", "› Ask Codex to do anything"]
IDLE = ["• Done.", "", "› Ask Codex to do anything", "  ? for shortcuts"]
# Claude Code (its title's ✳) after its turn, a shell still in the background: its footer's count
DONE_CC = ["\u273b Worked for 2m 12s \u00b7 1 shell still running", "", "\u2500" * 20, "\u276f ", "\u2500" * 20, "  ~/p Opus 5.5 ctx:47%"]
SHELL_CC = DONE_CC + ["  \u23f5\u23f5 bypass permissions on \u00b7 1 shell"]
SHELL_FOR = 9  # s the stand-in "shell" shows its background shell
SETTLE = 10    # s a pane stays done before its turn ends (page/turns.rs)
FINAL = 17     # s the stand-in "turn" starts its last, done screen
SCREENS = {
    "idle": {"rows": ["  >_ OpenAI Codex (v0.162.1)", "", "› Ask Codex to do anything", "  ? for shortcuts"]},
    "working": {"rows": ["• Ran pwd", "", "Working (10s • esc to interrupt)", "", "› Ask Codex to do anything"]},
    "ask": {"title": "[ ! ] Action Required | Pick a language | p",
            "rows": ["  Question 1/1 (1 unanswered)", "  Which language?", "  › 1. Python", "    2. Bash", "",
                     "  tab to add notes | enter to submit answer | esc to interrupt"]},
    "dev": {"rows": ["  ready - started server on 0.0.0.0:3000",
                     "? Would you like to run the app on another port instead? › (Y/n)"]},
    # works, looks done for 7 s (a step of its turn: longer than a read, shorter than SETTLE),
    # works again, then is done for good
    "turn": {"rows": [], "phases": [[WORK, 3000], [IDLE, 7000], [WORK, 7000], [IDLE, 0]]},
    "shell": {"title": "\u2733 Fix it", "rows": [], "phases": [[SHELL_CC, SHELL_FOR * 1000], [DONE_CC, 0]]},
    "teammate": {"title": "\u2733 general-purpose", "rows": DONE_CC, "args": ["--agent-name", "simplify-altitude"]},
    "switch": {"rows": ["• Created essay.md", "  Worked for 43s • 11:04", "", "› Ask Codex to do anything"],
               "later": ["  ready - started server on 0.0.0.0:3000", "? Would you like to run the app on another port instead? › (Y/n)"]},
}

PROGRAM = r"""
const s = JSON.parse(process.argv[2]);
const draw = (rows) => process.stdout.write(`\x1b[2J\x1b[${process.stdout.rows - rows.length + 1};1H` + rows.join("\r\n"));
if (s.title) process.stdout.write(`\x1b]2;${s.title}\x07`);
draw(s.rows);
if (s.later) setTimeout(() => { process.stdout.write("\x1b]2;dev\x07"); draw(s.later); }, 3000);
// phases: [rows, ms] drawn one after another, the last one kept
let at = 0;
for (const [rows, ms] of s.phases ?? []) { setTimeout(() => draw(rows), at); at += ms; }
setInterval(() => {}, 1000);
"""


def items(groups):
    return [it for g in groups for it in g["items"]]


def row(groups, title):
    return next((i for i in items(groups) if i["index"] == title), None)


def main():
    t = Terminumd(PORT, SOCKET)
    work = tempfile.mkdtemp(prefix="tma-", dir=short_tmp())
    program = os.path.join(work, "standin.js")
    with open(program, "w") as f:
        f.write(PROGRAM)
    try:
        t.start()
        ws = t.page()
        last = [[]]   # the layout is sent only when it changed: the newest one stands

        def newest(pred, timeout=5):
            def keep(gs):
                last[0] = gs
                return pred(gs)
            return wait_layout(ws, keep, timeout)
        newest(items)
        index = {}

        def run(name):
            out = tmux("new-window", "-d", "-P", "-F", "#{window_index}", "-t", "main", "node", program, json.dumps(SCREENS[name]), *SCREENS[name].get("args", []))
            index[name] = out.strip()
        for name in SCREENS:
            if name not in ("switch", "turn", "shell"):
                run(name)
        at = lambda name: (lambda gs: row(gs, index[name]))  # noqa: E731

        def seen(name, pred, timeout=10):
            def holds(gs):
                return (r := at(name)(gs)) is not None and pred(r)
            if holds(last[0]) or newest(holds, timeout):
                return at(name)(last[0])
            print("  last row:", at(name)(last[0]))
            return None

        newest(lambda gs: len(items(gs)) > len(SCREENS) - 2)
        programs = tmux("list-panes", "-s", "-t", "main", "-F", "#{window_index} #{pane_current_command}")
        check("AC-39 the stand-ins run as node, as npm's Codex does", all(f"{i} node" in programs for i in index.values()), programs.split("\n"))
        idle = seen("idle", lambda r: r["agent"] == "codex")
        check("AC-39 a node pane showing Codex's composer is Codex, done shows no mark", idle and idle["state"] == "done", idle)
        working = seen("working", lambda r: r["state"] == "working")
        check("AC-39 Codex working: \"esc to interrupt\", its title unchanged", working and working["agent"] == "codex", working)
        ask = seen("ask", lambda r: r["state"] == "waiting")
        check("AC-39 Codex's question waits over \"esc to interrupt\"", ask and ask["agent"] == "codex", ask)
        check("AC-39 the list title leaves out Codex's \"Action Required\" mark", ask and ask["title"] == "Pick a language | p", ask)
        mate = seen("teammate", lambda r: r["title"] == "simplify-altitude")
        check("AC-01 a subagent's pane shows its --agent-name, its type under it", mate and mate["agent"] == "claude" and "general-purpose" in mate["sub"], mate)
        dev = seen("dev", lambda r: True)
        check("AC-39 another node program asking a question shows no agent and no state",
              dev and dev["agent"] is None and dev["state"] is None, dev)
        # unread (AC-93) read from the screen: a turn ends when its pane stayed done SETTLE, not
        # at a moment's done between two steps; never while it works (no page shows it)
        run("turn")
        import time
        start, rows, unread_at, work_after = time.time(), [], None, False
        while time.time() - start < FINAL + SETTLE + 8:
            newest(lambda gs: False, 1)
            r = at("turn")(last[0])
            if r:
                rows.append((round(time.time() - start, 1), r["state"], bool(r.get("unread"))))
                if r.get("unread") and unread_at is None:
                    unread_at = time.time() - start
                if r["state"] == "working" and any(s == "done" for _, s, _ in rows):
                    work_after = True
        early = [x for x in rows if x[2] and x[0] < FINAL + SETTLE - 1]
        check("AC-93 a screen's turn end: no unread while it works or between its steps, unread once it stayed done SETTLE",
              work_after and not early and not any(x[2] and x[1] == "working" for x in rows) and unread_at is not None, rows)
        # Claude Code's main agent done, a shell of its turn still running: working until it ends
        run("shell")
        start, rows, unread_at = time.time(), [], None
        while time.time() - start < SHELL_FOR + SETTLE + 8:
            newest(lambda gs: False, 1)
            r = at("shell")(last[0])
            if r:
                rows.append((round(time.time() - start, 1), r["state"], bool(r.get("unread"))))
                if r.get("unread") and unread_at is None:
                    unread_at = time.time() - start
        during = [x for x in rows if x[0] < SHELL_FOR - 1]
        check("AC-93 Claude Code done with a shell still running in the background (its footer's \"1 shell\") is working, not unread",
              during and all(x[1] == "working" and not x[2] for x in during), rows)
        check("AC-93 once the shell ended, the turn ends: unread SETTLE later",
              unread_at is not None and unread_at >= SHELL_FOR + SETTLE - 1, (unread_at, rows[-3:]))
        run("switch")    # its screen changes 3 s after it starts
        before = seen("switch", lambda r: r["agent"] == "codex")
        check("AC-39 Codex in a pane before the switch", before and before["state"] == "done", before)
        after = seen("switch", lambda r: r["agent"] is None, 12)
        check("AC-39 a node pane leaving Codex for a dev server stops being Codex (no false reminder)",
              after and after["state"] is None, after)
    finally:
        t.stop()
        tmux("kill-server")
        t.cleanup()
        shutil.rmtree(work, ignore_errors=True)
    summary()


if __name__ == "__main__":
    main()
