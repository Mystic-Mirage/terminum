#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""The input log (terminum.md AC-147) against a private terminumd and tmux: what a page types
gathers in a row until Enter, a paste is a row of its own, each with the page's browser, address
and the pane's machine, session and folder; a line begun with a space and what is typed at sudo's
password prompt keep only why they are not kept (the prompt is cancelled with Ctrl-C, nothing is
ever entered there); Dispatcher's launch and prompt are its rows, with its run, tasks and agent
(scripts/fake_agent.py as the agent).

    python3 scripts/e2e_inputs.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import sqlite3
import sys
import tempfile
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import BIN, Terminumd, check, summary, tmux_on, wait_for, wait_layout  # noqa: E402
from e2e_core import screen_with  # noqa: E402
from e2e_remote import api  # noqa: E402
from testrun import free_port, own, short_tmp  # noqa: E402

FAKE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fake_agent.py")

PORT, SOCKET = free_port(), own("tm-test-inp")
tmux = partial(tmux_on, SOCKET)
SAFARI = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1"


def rows(t, until_n, timeout=10):
    """The log's rows, oldest first, once there are `until_n` (or after `timeout` s)."""
    def read():
        try:
            with sqlite3.connect(f"file:{os.path.join(t.app, 'kanban.sqlite')}?mode=ro", uri=True) as db:
                db.row_factory = sqlite3.Row
                return [dict(r) for r in db.execute("SELECT * FROM inputs ORDER BY id")]
        except sqlite3.Error:
            return []
    return wait_for(read, lambda got: len(got) >= until_n, timeout, 0.2, last=True)


def main():
    t = Terminumd(PORT, SOCKET, env={"TM_TEST_HOOKS": "1"})
    work = tempfile.mkdtemp(prefix="tmi-", dir=short_tmp())
    try:
        t.start()
        ws = t.page(headers=[f"User-Agent: {SAFARI}"])
        groups = wait_layout(ws, lambda gs: any(g["items"] for g in gs))
        key = next(it["id"] for g in groups for it in g["items"])
        ws.send({"t": "sub", "id": key})
        ws.until(lambda m: m["t"] == "screen", 5)
        for ch in "echo hi":                       # a key a message, as a page sends them
            ws.send({"t": "in", "data": ch})
        ws.send({"t": "in", "data": "\r"})
        screen_with(ws, "hi\n")
        ws.send({"t": "paste", "data": "pasted words", "bracket": True})
        ws.send({"t": "in", "data": "\x15"})       # Ctrl-U: the pasted line goes
        ws.send({"t": "in", "data": " echo private\r"})
        ws.send({"t": "in", "data": "sudo -k true\r"})
        asked = screen_with(ws, "assword", 10)
        ws.send({"t": "in", "data": "hunter2"})    # never entered: the row ends by itself, then Ctrl-C
        got = rows(t, 4, 10)
        ws.send({"t": "in", "data": "\x03"})
        got = rows(t, 5, 10)
        by = {r["text"]: r for r in got}
        typed = by.get("echo hi<Enter>")
        check("AC-147 a page's keys gather in one row until Enter, the keys as sent", typed is not None and typed["kind"] == "typed" and typed["bytes"] == 8, [r["text"] for r in got])
        check("AC-147 the row says who and from where: the user, the page, its browser, its address",
              typed and typed["sender"] == "human" and typed["component"] == "page" and typed["browser"] == "Safari 18 · iPhone" and typed["ip"] == "127.0.0.1" and typed["device"] == "mac" and typed["page"],
              typed)
        check("AC-147 and into what: the pane, its machine, session, window, program and folder, the time it began and ended",
              typed and typed["pane"] == key and typed["machine"] and typed["session"] == "main" and typed["window"] and typed["program"] and typed["cwd"] and typed["at"] <= typed["ended"], typed)
        paste = next((r for r in got if r["kind"] == "paste"), None)
        check("AC-147 a paste is a row of its own, its text whole", paste and paste["text"] == "pasted words", paste)
        check("AC-147 a line begun with a space is not kept", "[not kept: leading space]" in by and not any("private" in r["text"] for r in got), [r["text"] for r in got])
        check("AC-147 what is typed at sudo's password prompt is not kept", asked is not None and any(r["text"].startswith("[not kept:") and r["program"] == "sudo" for r in got)
              and not any("hunter2" in r["text"] for r in got), [(r["text"], r["program"]) for r in got])
        check("AC-147 the command that asked is kept: the prompt it opened is not its row's", "sudo -k true<Enter>" in by, [r["text"] for r in got])

        # Dispatcher runs a task: its launch and its prompt are its rows
        secret = t.signin()
        call = lambda path, method="GET", body=None: api(t, secret, path, method=method, body=json.dumps(body).encode() if body is not None else None,  # noqa: E731
                                                         headers={"Content-Type": "application/json", "Origin": f"http://127.0.0.1:{PORT}"})
        project = call("/api/task-projects", "POST", {"name": "Logged", "dir": work})[1]["id"]
        agent = call("/api/task-agents", "POST", {"name": "Fake", "kind": "claude", "command": f"python3 -I {FAKE} {BIN} --kind claude"})[1]["id"]
        task = call("/api/tasks", "POST", {"title": "logged FINISHES", "project": project})[1]["id"]
        st, said = call(f"/api/tasks/{task}/run", "POST", {"agent": agent})
        check("setup: the task runs", st == 200, said)
        done = wait_for(lambda: call(f"/api/tasks/{task}")[1]["status"], lambda s: s == "DONE", 25, 0.3, last=True)
        mine = [r for r in rows(t, len(got) + 2, 10) if r["sender"] == "dispatcher"]
        launch = next((r for r in mine if r["kind"] == "launch"), None)
        prompt = next((r for r in mine if r["kind"] == "prompt"), None)
        check("AC-147 Dispatcher's launch is its row: the command, its run, task and agent", done == "DONE" and launch and FAKE in launch["text"] and launch["run"]
              and launch["tasks"] == task and launch["agent"] == agent and launch["project_name"] == "Logged" and launch["component"] == "dispatcher.start", launch)
        check("AC-147 Dispatcher's prompt is its row, sent with Enter, its project and no browser",
              prompt and prompt["text"].endswith("<Enter>") and "logged FINISHES" in prompt["text"] and prompt["run"] == launch["run"] and prompt["agent"] == agent
              and prompt["project_name"] == "Logged" and prompt["component"] == "dispatcher.gate" and prompt["browser"] is None and prompt["error"] is None, prompt)
    finally:
        t.cleanup()
        os.system(f"rm -rf '{work}'")
    return summary()


if __name__ == "__main__":
    sys.exit(main())
