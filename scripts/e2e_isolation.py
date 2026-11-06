#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""One server's trouble stays its own (terminum-remote.md AC-132), against a private terminumd,
a private local tmux and a private host through scripts/fake_ssh.py. The host is frozen by
SIGSTOP of this run's own tmux server, and everything local is timed while it is. Exits non-zero
when a check fails.

    python3 scripts/e2e_isolation.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import signal
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, summary, unix_request, wait_layout  # noqa: E402
from testrun import free_port, own  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
LOCAL, REMOTE = own("tm-test-ilocal"), own("tm-test-iremote")
FAST = 1.0                              # seconds: the targets of §9 Performance


def tmux(socket, *args):
    return subprocess.run(["tmux", "-L", socket, *args], capture_output=True, text=True).stdout.strip()


def host_group(groups):
    return next((g for g in groups if g.get("host") == "devbox" and not g.get("attached")), None)


def local_ids(groups):
    return {i["id"] for g in groups if not g.get("host") for i in g["items"]}


def new_local_window(ws, session, known, limit=15):
    """Seconds until the list told to the page has one more local pane, and the panes then."""
    t0 = time.time()
    tmux(LOCAL, "new-window", "-d", "-t", session)
    groups = wait_layout(ws, lambda g: local_ids(g) - known, limit)
    return (time.time() - t0, local_ids(groups)) if groups else (None, known)


def typed_echo(ws, word):
    """Seconds until a word typed in the shown pane is on its screen."""
    t0 = time.time()
    ws.send({"t": "in", "data": f"echo {word}-$((3*3))\r"})
    seen = ws.until(lambda m: m["t"] == "screen" and f"{word}-9" in json.dumps(m["ch"]), 15)
    return time.time() - t0 if seen else None


def late_window(t, ws, frozen, groups, local_pane, session, host_pane):
    """A short hang: keys and a window asked of the host, the user moves on and does something else
    there: all of it arrives in order, and the window takes nobody away."""
    before = tmux(REMOTE, "list-windows", "-a", "-F", "#{window_id}").split()
    os.kill(frozen, signal.SIGSTOP)
    try:
        ws.send({"t": "in", "data": "echo keys-for-host-$((5*5))\r"})
        ws.send({"t": "new", "group": host_group(groups)["wid"]})
        time.sleep(0.2)
        t0 = time.time()
        ws.send({"t": "sub", "id": local_pane, "cols": 90, "rows": 25})
        opened = ws.until(lambda m: m["t"] == "screen" and m.get("sid") == local_pane, 5)
        check("AC-132 a local pane opens with a host's keys and window waiting", opened is not None and time.time() - t0 < FAST, f"{time.time() - t0:.2f} s")
        ws.send({"t": "rename", "id": local_pane, "name": "renamed-meanwhile"})
        named = wait_layout(ws, lambda g: any(i["id"] == local_pane and i["title"] == "renamed-meanwhile" for x in g for i in x["items"]), 5)
        check("AC-132 an action on this Mac finishes with the host's waiting", named is not None and time.time() - t0 < 2 * FAST, f"{time.time() - t0:.2f} s")
        time.sleep(1.1)                 # one new window a second (AC-07)
        t0 = time.time()
        ws.send({"t": "new", "group": next(g for g in groups if not g.get("host"))["wid"]})
        made = ws.until(lambda m: m["t"] == "created" and m["id"].startswith("tmux:local:"), 5)
        check("AC-132 a window asked on this Mac comes at once, and shows", bool(made) and not made.get("stay") and time.time() - t0 < FAST, made)
        ws.send({"t": "sub", "id": local_pane, "cols": 90, "rows": 25})
    finally:
        os.kill(frozen, signal.SIGCONT)
    late = ws.until(lambda m: m["t"] == "created" and m["id"].startswith("tmux:devbox:"), 15)
    check("AC-132 the host's window comes when it answers, and takes nobody away", bool(late) and late.get("stay") is True, late)
    after = tmux(REMOTE, "list-windows", "-a", "-F", "#{window_id}").split()
    check("AC-132 the host's window was made", len(after) == len(before) + 1, after)
    screen = tmux(REMOTE, "capture-pane", "-p", "-t", host_pane.rsplit(":", 1)[1])
    check("AC-132 the keys for the host went in", "keys-for-host-25" in screen, screen[-200:])
    ws.send({"t": "sub", "id": host_pane, "cols": 90, "rows": 25})
    ws.until(lambda m: m["t"] == "screen" and m.get("sid") == host_pane, 10)


def main():
    t = Terminumd(free_port(), LOCAL)
    frozen = None
    home = os.path.join(t.root, "remote-home")
    os.makedirs(home)
    t.env.update(TM_SSH=os.path.join(HERE, "fake_ssh.py"), FAKE_SSH_HOME=home, FAKE_SSH_TMUX_SOCKET=REMOTE,
                 FAKE_SSH_PIDS=os.path.join(t.root, "ssh-pids"))
    try:
        t.start()
        ws = t.page(t.signin())
        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "", "recipe": "ssh alex@devbox.example"})
        assert st == 200, body
        groups = wait_layout(ws, lambda g: (h := host_group(g)) is not None and h["items"], 15)
        assert groups, "the host did not come up: " + t.log()
        session = tmux(LOCAL, "list-sessions", "-F", "#S").splitlines()[0]
        known, host_pane = local_ids(groups), host_group(groups)["items"][0]["id"]
        local_pane = sorted(known)[0]

        took, known = new_local_window(ws, session, known)
        check("AC-132 a local window reaches the list, the host answering", took is not None and took < FAST, f"{took} s")

        ws.send({"t": "sub", "id": host_pane, "cols": 90, "rows": 25})
        check("AC-81 the host's pane shows", ws.until(lambda m: m["t"] == "screen", 10) is not None)

        frozen = int(tmux(REMOTE, "display-message", "-p", "#{pid}"))
        late_window(t, ws, frozen, groups, local_pane, session, host_pane)
        os.kill(frozen, signal.SIGSTOP)
        ws.send({"t": "in", "data": "keys for a host that hangs\r"})
        ws.send({"t": "new", "group": host_group(groups)["wid"]})   # an action waiting on it
        time.sleep(1)

        t0 = time.time()
        ws.send({"t": "sub", "id": local_pane, "cols": 90, "rows": 25})
        opened = ws.until(lambda m: m["t"] == "screen" and m.get("sid") == local_pane, 15)
        check("AC-132 a local pane opens while the host hangs", opened is not None and time.time() - t0 < FAST, f"{time.time() - t0:.2f} s")
        echo = typed_echo(ws, "local")
        check("AC-132 a key on a local pane echoes while the host hangs", echo is not None and echo < FAST, f"{echo} s")
        for at in range(3):                 # at once, and on through the watchdog's 10 s
            took, known = new_local_window(ws, session, known)
            check(f"AC-132 a local window reaches the list while the host hangs ({at})", took is not None and took < FAST, f"{took} s")
            time.sleep(6)

        dim = wait_layout(ws, lambda g: (h := host_group(g)) is not None and h["items"] and all(i.get("offline") for i in h["items"]), 40)
        h = host_group(dim or []) or {}
        check("AC-132 rows not read for 30 s dim, with no agent state", bool(dim) and all(i.get("state") is None for i in h["items"]), h.get("items"))
        panes = next((s["panes"] for s in unix_request(t.sock, "GET", "/health")[1]["servers"] if s["name"] == "devbox"), None)
        check("AC-132 `terminum status` does not count rows it cannot read", panes == 0, panes)
        took, known = new_local_window(ws, session, known)
        check("AC-132 a local window reaches the list after all that", took is not None and took < FAST, f"{took} s")

        os.kill(frozen, signal.SIGCONT)
        frozen = None
        back = wait_layout(ws, lambda g: (h := host_group(g)) is not None and h["items"] and not any(i.get("offline") for i in h["items"]), 40)
        check("AC-132 the host answers again: its rows are live", back is not None)

        # the link cut: the group says so, the rows stay where they were, a shown pane with them (AC-81)
        ws.send({"t": "sub", "id": host_pane, "cols": 90, "rows": 25})
        ws.until(lambda m: m["t"] == "screen", 15)
        ids = [i["id"] for i in host_group(back or [{}])["items"]] if back else []
        # the control clients end, as with a real ssh going (the watchdog's kill left fake_ssh's child up)
        for pid in tmux(REMOTE, "list-clients", "-F", "#{client_pid}").split():
            os.kill(int(pid), signal.SIGTERM)
        lost = wait_layout(ws, lambda g: (h := host_group(g)) is not None and h.get("state") in ("reconnecting", "failed"), 20)
        h = host_group(lost or []) or {}
        check("AC-81 the link cut: its group says so", bool(lost), h.get("why"))
        check("AC-81 the link cut: its rows stay, offline, with no agent state", bool(h.get("items")) and [i["id"] for i in h["items"]] == ids
              and all(i.get("offline") is True and i.get("state") is None for i in h["items"]), h.get("items"))
        took, known = new_local_window(ws, session, known)
        check("AC-132 a local window reaches the list with the link cut", took is not None and took < FAST, f"{took} s")
        again = wait_layout(ws, lambda g: (h := host_group(g)) is not None and h.get("state") == "attached" and h["items"]
                            and not any(i.get("offline") for i in h["items"]), 40)
        check("AC-81 the link back: its rows are live, the same panes", again is not None and [i["id"] for i in host_group(again)["items"]] == ids)
        ws.send({"t": "sub", "id": local_pane, "cols": 90, "rows": 25})
        ws.until(lambda m: m["t"] == "screen", 15)
        echo = typed_echo(ws, "after")
        check("AC-132 keys echo on a local pane afterwards", echo is not None and echo < FAST, f"{echo} s")
        check("AC-14 a silent client is ended by the watchdog", "tmux.timeout" in open(t.log_path).read(), t.log()[-200:])
    finally:
        if frozen:
            os.kill(frozen, signal.SIGCONT)
        t.cleanup()
        subprocess.run(["tmux", "-L", REMOTE, "kill-server"], stderr=subprocess.DEVNULL)
    return summary()


if __name__ == "__main__":
    sys.exit(main())
