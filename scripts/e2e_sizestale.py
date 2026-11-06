#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""A resize a tmux server never answered fails after the user moved on (terminum.md AC-13): its
refusal names the pane it was for, not the one shown by then, at the page's protocol, against a
private terminumd. Two private servers stand for a local one and one that stalls and dies; only
the server this check started is stopped and killed, by its own pid.

    python3 scripts/e2e_sizestale.py      (needs terminumd built: cd terminumd && cargo build)
"""
import os
import signal
import sys
import tempfile
import time
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, summary, tmux_on, wait_layout  # noqa: E402
from testrun import free_port, own  # noqa: E402

PORT, SOCKET, OWN = free_port(), own("tm-test-szs"), own("tm-test-szo")
tmux = partial(tmux_on, SOCKET)
other = partial(tmux_on, OWN)


def main():
    work = tempfile.mkdtemp(prefix="tmz-", dir="/tmp")
    other("-f", "/dev/null", "new-session", "-d", "-s", "work", "-c", work, "-x", "90", "-y", "30")
    pid = int(other("display", "-p", "#{pid}").strip())
    t = Terminumd(PORT, SOCKET)
    groups = []
    try:
        t.start()
        ws = t.page()
        wait_layout(ws, lambda gs: bool(gs), 5)
        ws.send({"t": "addtmux", "server": f"local:{OWN}", "sessions": ["work"]})
        def seen(gs):
            groups[:] = gs
            return any(g.get("attached") == OWN and g["items"] for g in gs) and any(g["label"] == "main" for g in gs)
        check("the second server's session is listed", wait_layout(ws, seen, 8))
        here = next(g for g in groups if g["label"] == "main")["items"][0]["id"]
        there = next(g for g in groups if g.get("attached") == OWN)["items"][0]["id"]
        ws.send({"t": "sub", "id": there})
        ws.until(lambda m: m["t"] == "screen", 5)
        ws.send({"t": "size", "cols": 80, "rows": 24, "typing": False})
        ws.until(lambda m: m["t"] == "sized" and not m.get("error"), 5)
        os.kill(pid, signal.SIGSTOP)                       # it stalls with the next resize unanswered
        ws.send({"t": "size", "cols": 100, "rows": 32, "typing": True})
        time.sleep(1)                                      # past the resize's settling: it waits on the stalled server
        ws.send({"t": "sub", "id": here})                  # the user moves on meanwhile
        ws.until(lambda m: m["t"] == "screen", 5)
        os.kill(pid, signal.SIGKILL)                       # and it goes
        said = ["screen"]
        def watch(m):
            if m["t"] == "sized" and m.get("error"):
                said.append(m)
            elif m["t"] == "screen":
                said.append("screen")
            return said.count("screen") and any(x != "screen" for x in said)
        ws.until(watch, 20)
        refused = next((x for x in said if x != "screen"), None)
        check("the refusal came", refused is not None, refused)
        check("AC-13 a failed resize names the pane it was for, not the one shown now",
              refused is not None and refused.get("sid") == there and refused.get("sid") != here, (refused, here, there))
    finally:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        t.cleanup()
        os.system(f"rm -rf '{work}'")
    return summary()


if __name__ == "__main__":
    sys.exit(main())
