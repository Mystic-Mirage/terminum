#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Sessions of the user's own tmux attached by name (terminum.md AC-89), at the page's
protocol, against a private terminumd. "The user's own tmux" is a second private server
(`-L tm-test-own`), never the user's: attach, a session that does not run yet, sized on
opening as any pane, two servers with the same window ids, a new window there, detach.

    python3 scripts/e2e_attach.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import subprocess
import sys
import tempfile
import time
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, summary, tmux_on, unix_request, wait_layout  # noqa: E402
from testrun import free_port, own  # noqa: E402

PORT, SOCKET, OWN = free_port(), own("tm-test-att"), own("tm-test-own")
tmux = partial(tmux_on, SOCKET)
own = partial(tmux_on, OWN)


def group(groups, label):
    return next((g for g in groups or [] if g.get("attached") == OWN and g["label"] == label), None)


def size_of(run, pane):
    return run("display", "-p", "-t", pane, "#{window_width}x#{window_height} #{window-size}")


def main():
    work = tempfile.mkdtemp(prefix="tma-", dir="/tmp")
    own("-f", "/dev/null", "new-session", "-d", "-s", "work", "-c", work, "-x", "90", "-y", "30")
    t = Terminumd(PORT, SOCKET, env={"DISPLAY": ":tm-test"})
    try:
        t.start()
        ws = t.page()
        wait_layout(ws, lambda gs: any(g["label"] == "main" for g in gs))
        # a session tmux destroys with its last client is told apart in the choices; it lives
        # only while a client holds it (a control client here, standing for the user's terminal)
        own("new-session", "-d", "-s", "scratch", "-c", work)
        holder = subprocess.Popen(["tmux", "-L", OWN, "-C", "attach", "-t", "=scratch"], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL)
        time.sleep(0.5)
        own("set-option", "-t", "scratch", "destroy-unattached", "on")
        ws.send({"t": "tmuxsessions", "server": f"local:{OWN}"})
        offer = ws.until(lambda m: m["t"] == "tmuxsessions", 5)
        holder.stdin.close()
        holder.wait(5)
        check("AC-89 the choices name the running sessions and those tmux destroys with their last client",
              offer and sorted(offer["names"]) == ["scratch", "work"] and offer["ends"] == ["scratch"], offer)

        ws.send({"t": "addtmux", "server": f"local:{OWN}", "sessions": ["work"]})
        added = ws.until(lambda m: m["t"] in ("tmuxadded", "error"), 5)
        groups = wait_layout(ws, lambda gs: (g := group(gs, "work")) and g["items"])
        g = group(groups, "work")
        check("AC-89 a session of the user's tmux is attached by name and listed with its panes",
              added and added["t"] == "tmuxadded" and g and g["where"] == f"tmux -L {OWN}" and g["wid"] == f"local:{OWN}/work"
              and g["items"][0]["id"].startswith(f"tmux:local:{OWN}:%"), added)
        saved = json.load(open(os.path.join(t.home, "settings.json")))
        check("AC-89 the attached session is kept in settings.json", saved.get("attach") == {OWN: ["work"]}, saved)
        check("AC-89 the attach command is the user's own", g and g["attach"] == f"tmux -L {OWN} attach -t '=work'", g and g["attach"])
        check("AC-89 terminum's own sessions are still listed apart", any(x["label"] == "main" and not x.get("attached") for x in groups))
        env = own("show-environment", "-t", "work", "DISPLAY").strip()
        check("AC-89 attaching leaves the session's environment as it was (no update-environment)", ":tm-test" not in env, env)

        ws.send({"t": "addtmux", "server": f"local:{OWN}", "sessions": ["work"]})
        dup = ws.until(lambda m: m["t"] == "error", 5)
        ws.send({"t": "addtmux", "server": f"local:{SOCKET}", "sessions": ["main"]})
        mine = ws.until(lambda m: m["t"] == "error", 5)
        check("AC-89 a session attached twice, or one of terminum's own server, is refused",
              dup and "attached already" in dup["msg"] and mine and "terminum's own tmux" in mine["msg"], [dup, mine])

        ws.send({"t": "addtmux", "server": f"local:{OWN}", "sessions": ["later"]})
        groups = wait_layout(ws, lambda gs: group(gs, "later"))
        later = group(groups, "later")
        check("AC-89 a named session that does not run shows as a group that says so",
              later and later["state"] == "absent" and "not running" in later["why"] and later["items"] == [], later)
        own("new-session", "-d", "-s", "later", "-c", work)
        groups = wait_layout(ws, lambda gs: (x := group(gs, "later")) and x["items"], 8)
        check("AC-89 it attaches once it starts", group(groups, "later") is not None and group(groups, "later").get("state") != "absent")

        # sizing: opening sizes the user's window as any pane, and only that window
        main_pane = next(x for x in groups if x["label"] == "main")["items"][0]["id"]
        ws.send({"t": "sub", "id": main_pane})
        ws.send({"t": "size", "cols": 80, "rows": 24, "typing": False})
        ws.until(lambda m: m["t"] == "sized", 5)
        work_pane = g["items"][0]["id"]
        before = size_of(own, "work:")
        ws.send({"t": "sub", "id": work_pane})
        ws.until(lambda m: m["t"] == "screen", 5)
        opened = ws.until(lambda m: m["t"] == "sized", 5)
        check("AC-89 opening a session of the user's tmux sizes it as any pane (window-size manual)",
              opened and not before.startswith("80x24 manual") and size_of(own, "work:").startswith("80x24 manual"), [before, size_of(own, "work:")])
        ws.send({"t": "size", "cols": 100, "rows": 32, "typing": False})      # after its first frame: a view that changed
        ws.until(lambda m: m["t"] == "sized", 5)
        check("AC-89 its view's new size follows", "100x32 manual" in size_of(own, "work:"), size_of(own, "work:"))
        check("AC-89 the private server's window with the same id keeps its own size", size_of(tmux, "main:").startswith("80x24 "), size_of(tmux, "main:"))

        ws.send({"t": "new", "group": f"local:{OWN}/work"})
        made = ws.until(lambda m: m["t"] in ("created", "error"), 5)
        check("AC-89 New window in an attached group makes it in the user's session",
              made and made["t"] == "created" and made["id"].startswith(f"tmux:local:{OWN}:%") and len(own("list-windows", "-t", "work").splitlines()) == 2, made)

        listed = unix_request(t.sock, "GET", "/tmux")[1]
        check("AC-89 terminum tmux lists both sessions with their state", [x["session"] for x in listed["attached"]] == ["work", "later"]
              and listed["attached"][0]["state"].startswith("attached"), listed)

        ws.send({"t": "mark", "ref": "r1", "sid": work_pane, "n": 0, "text": "of the attached session", "note": "", "remind": False})
        ws.until(lambda m: m["t"] == "marked", 5)
        ws.send({"t": "detachtmux", "server": f"local:{OWN}", "session": "work"})
        groups = wait_layout(ws, lambda gs: not group(gs, "work"))
        time.sleep(1.5)  # a list read after the detach
        marks = json.load(open(os.path.join(t.app, "web-bookmarks.json")))["items"]
        check("AC-33 a session detached keeps its bookmark, not orphaned (its pane still runs)", len(marks) == 1 and not marks[0].get("orphaned"), marks)
        check("AC-89 detaching forgets the session and leaves it running", not group(groups, "work") and "work" in own("ls"), own("ls"))
        check("AC-89 its windows get tmux's own size back (no window-size manual left)", "manual" not in size_of(own, "work:"), size_of(own, "work:"))
    finally:
        t.cleanup()
        own("kill-server")
        os.system(f"rm -rf '{work}'")
    return summary()


if __name__ == "__main__":
    sys.exit(main())
