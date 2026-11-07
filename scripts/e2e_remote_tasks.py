#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Dispatcher on a host (terminum-dispatcher.md AC-130) against a private terminumd, with this Mac
playing the host through scripts/fake_ssh.py and scripts/fake_agent.py as its agent: RUN opens the
window there, the gate types there, the walk's `ps` and the ending's signals go over the host's link.

    python3 scripts/e2e_remote_tasks.py   (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, summary, tmux_on, unix_request, wait_for  # noqa: E402
from e2e_remote import api  # noqa: E402
from testrun import free_port, own  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
PORT, LOCAL_SOCKET, REMOTE_SOCKET = free_port(), own("tm-test-tlocal"), own("tm-test-tremote")
STUB = "#!/bin/sh\nexit 0\n"


def until(fn, timeout=25):
    return wait_for(fn, bool, timeout, 0.2, last=True)


def holds_terminal(pid):
    """The pane's shell is its terminal's foreground: at a prompt, not waiting for the terminal (spinning, as macOS's bash does)."""
    group, foreground = subprocess.run(["ps", "-o", "pgid=,tpgid=", "-p", pid], capture_output=True, text=True).stdout.split()
    return group == foreground


def main():
    t = Terminumd(PORT, LOCAL_SOCKET, env={"TM_TEST_CLOCK_SCALE": "300"})
    home = os.path.join(t.root, "rh")
    for d in (".local/bin", ".codex", "proj"):
        os.makedirs(os.path.join(home, d))
    stub = os.path.join(home, ".local", "bin", "claude")
    with open(stub, "w", encoding="utf8") as f:
        f.write(STUB)
    os.chmod(stub, 0o700)
    t.through_fake_ssh(home, REMOTE_SOCKET, os.path.join(t.root, "ssh-pids"))
    subprocess.run(["tmux", "-L", REMOTE_SOCKET, "kill-server"], stderr=subprocess.DEVNULL, check=False)
    try:
        t.start()
        secret = t.signin()
        call = lambda path, method="GET", body=None: api(t, secret, path, method=method, body=json.dumps(body).encode() if body is not None else None,  # noqa: E731
                                                         headers={"Content-Type": "application/json", "Origin": f"http://127.0.0.1:{PORT}"})
        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "devbox", "recipe": "alex@devbox.example", "files": True})
        check("setup: the host is added", st == 200, body)
        folder = os.path.join(home, "proj")
        project = call("/api/task-projects", "POST", {"name": "Far", "host": "devbox", "dir": folder})[1]["id"]
        hb = os.path.join(home, ".terminum", "bin", "terminum-host")
        command = f"python3 -I {os.path.join(HERE, 'fake_agent.py')} {hb} --kind claude"
        st, added = call(f"/api/task-projects/{project}/agents", "POST", {"name": "Far agent", "kind": "claude", "host": "devbox", "command": command})
        check("setup: an agent on the host", st == 200, added)
        agent = added["id"]
        status = lambda i: call(f"/api/tasks/{i}")[1]["status"]  # noqa: E731
        refused = call("/api/tasks", "POST", {"title": "no human move", "project": project})[1]["id"]
        st, said = call(f"/api/tasks/{refused}", "PATCH", {"status": "DONE"})
        check("AC-107 a human moves a task only by running it", st == 400 and "running it" in said.get("error", "") and status(refused) == "TODO", said)
        history = lambda i: " | ".join(e.get("text") or "" for e in call(f"/api/tasks/{i}")[1]["history"])  # noqa: E731
        task = lambda title: call("/api/tasks", "POST", {"title": title, "project": project})[1]["id"]  # noqa: E731
        run = lambda i: call(f"/api/tasks/{i}/run", "POST", {"agent": agent})  # noqa: E731

        host = lambda: next((h for h in unix_request(t.sock, "GET", "/hosts")[1].get("hosts", []) if h["name"] == "devbox"), {})  # noqa: E731
        shown = lambda pane, fmt: tmux_on(REMOTE_SOCKET, "display", "-p", "-t", pane, fmt).strip()  # noqa: E731
        pane_of = lambda i: call(f"/api/tasks/{i}")[1]["session"]["key"].rsplit(":", 1)[1]  # noqa: E731

        def window_holds_shell(i):
            pid = shown(pane_of(i), "#{pane_pid}")   # the wrapper execs its shell: the same pid
            return until(lambda: holds_terminal(pid))

        attached = until(lambda: host().get("state") == "attached")
        check("setup: the host is attached", attached)

        # hooks missing there: said, nothing starts
        off = task("no hooks yet")
        st, said = run(off)
        check("AC-130 hooks missing there: said, nothing starts", st == 400 and "hooks install on devbox" in said.get("error", "") and status(off) == "TODO", said)

        st, body = unix_request(t.sock, "POST", "/hosts/agents-on", {"name": "devbox"})
        check("setup: agents on", st == 200, body)
        up = until(lambda: str(host().get("agents", "")).startswith("up"))
        check("setup: the link is up", up)

        # RUN there: window, gate, hook, finish_task
        done = task("finishes on the host FINISHES")
        st, said = run(done)
        check("AC-130 RUN opens the window on the host", st == 200, said)
        check("AC-130 the agent there takes it and ends it", until(lambda: status(done) == "DONE"), history(done))
        check("AC-125 its start says where", "on devbox." in history(done), history(done))
        key = call(f"/api/tasks/{done}")[1]["session"]["key"]
        check("AC-130 its pane is the host's", key.startswith("tmux:devbox:"), key)
        mark = shown(pane_of(done), "#{@terminum-run}")
        check("AC-125 marked on the host's tmux", bool(mark), mark)

        # the walk reads the agent's end by the host's ps and exit
        dies = task("dies there DIES")
        run(dies)
        check("AC-130 an agent dead on the host: FAILED with its exit", until(lambda: status(dies) == "FAILED" and "exit 3" in history(dies)), history(dies))
        check("AC-123 its window holds a shell at its prompt, with the terminal", window_holds_shell(dies))

        # the limit: SIGTERM over the link, by pid and start time
        timed = task("runs long there WORKS")
        st, said = call(f"/api/tasks/{timed}", "PATCH", {"budget_minutes": 5})
        check("setup: a budget", st == 200, said)
        run(timed)
        check("AC-130 at the limit the task is FAILED", until(lambda: status(timed) == "FAILED"), history(timed))
        tkey = pane_of(timed)
        ended = until(lambda: shown(tkey, "#{@terminum-exit}"), 10)
        check("AC-130 its agent is ended over the host's link", bool(ended), ended)
        check("AC-123 so does the window of an agent ended", window_holds_shell(timed))
    finally:
        t.cleanup()
        subprocess.run(["tmux", "-L", REMOTE_SOCKET, "kill-server"], stderr=subprocess.DEVNULL, check=False)
    return summary()


if __name__ == "__main__":
    sys.exit(main())
