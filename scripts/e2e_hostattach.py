#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Sessions of a host's own tmux attached by name (terminum.md AC-89 on a host), at the page's
protocol, against a private terminumd, with this Mac playing the host through
scripts/fake_ssh.py (a private tmux server stands for the host's). Checks: the host's other
sessions counted and offered, attach by id over one more ssh connection, keys and screens,
sized on opening and given back when the page is hidden, a client that keeps ending started
again with back-off, the host's link dropping and coming back, its orphaned control clients
ended then, windows given their size back (reclaim, detach, host removed).

    python3 scripts/e2e_hostattach.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import signal
import subprocess
import sys
import time
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, summary, tmux_on, unix_request, wait_layout  # noqa: E402
from testrun import free_port, own  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
PORT, LOCAL, REMOTE = free_port(), own("tm-test-hal"), own("tm-test-hrem")
remote = partial(tmux_on, REMOTE)


def own(groups):
    return next((g for g in groups or [] if g.get("host") == "devbox" and not g.get("attached")), None)


def att(groups, label):
    return next((g for g in groups or [] if g.get("host") == "devbox" and g.get("attached") and g["label"] == label), None)


def size_of(target):
    return remote("display", "-p", "-t", target, "#{window_width}x#{window_height} #{window-size} #{@tm-sized}").strip()


def ssh_runs(pids, what):
    try:
        return [line.split(" ", 2) for line in open(pids) if what in line]
    except FileNotFoundError:
        return []


def drop(pids, what):
    for pid, _, _ in ssh_runs(pids, what):
        try:
            os.kill(int(pid), signal.SIGTERM)
        except (ProcessLookupError, ValueError):
            pass


def clients():
    return {int(p): c for p, c in (line.split(" ", 1) for line in remote("list-clients", "-F", "#{client_pid} #{client_name}").splitlines())}


fixtures = []


def cleanup_fixtures():
    for process in fixtures:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
    fixtures.clear()


def orphan(args, shell=False, live=False):
    """A control client `tmux -L <host's> <args>` of the host's tmux as a dropped ssh link
    leaves one: init's child, or (`shell`) a shell's whose parent is init; `live`: under a
    parent that lives (this check's). Its pid."""
    cmd = f"tmux -L {REMOTE} {args}"
    line = f"sleep 60 | {cmd} >/dev/null 2>&1"
    command = line if live else f"({line}) &" if shell else f"{line} &"
    process = subprocess.Popen(["sh", "-c", command], start_new_session=True)
    fixtures.append(process)
    if not live:
        process.wait(timeout=5)  # the control client is orphaned, its owned group remains tracked
    for _ in range(50):
        found = subprocess.run(["pgrep", "-fx", cmd], capture_output=True, text=True).stdout.split()
        if found and int(found[0]) in clients():
            return int(found[0])
        time.sleep(0.1)
    return None


def settle(ws, seconds):
    """The newest layout within `seconds` (one alike is not sent again)."""
    last, end = None, time.time() + seconds
    while time.time() < end:
        last = wait_layout(ws, lambda gs: True, max(0.1, end - time.time())) or last
    return last


def main():
    t = Terminumd(PORT, LOCAL, env={"DISPLAY": ":tm-test"})
    home = os.path.join(t.root, "remote-home")
    os.makedirs(home)
    pids = os.path.join(t.root, "ssh-pids")
    t.env.update(TM_SSH=os.path.join(HERE, "fake_ssh.py"), FAKE_SSH_HOME=home, FAKE_SSH_TMUX_SOCKET=REMOTE, FAKE_SSH_PIDS=pids)
    remote("kill-server")
    for s in ("work", "api"):
        remote("-f", "/dev/null", "new-session", "-d", "-s", s, "-c", home, "-x", "90", "-y", "30")
    try:
        t.start()
        ws = t.page()
        unix_request(t.sock, "POST", "/hosts/add", {"name": "", "recipe": "alex@devbox.example"})
        groups = wait_layout(ws, lambda gs: (g := own(gs)) and g["items"] and g.get("others") == 2, 10)
        check("AC-89 a host's group counts the other sessions of its tmux, never lists them",
              groups and not any(g.get("label") in ("work", "api") for g in groups), own(groups or []))
        ws.send({"t": "tmuxsessions", "server": "devbox:default"})
        offer = ws.until(lambda m: m["t"] == "tmuxsessions", 5)
        check("AC-89 the host's running sessions are offered by name, terminum's own not",
              offer and offer["server"] == "devbox:default" and sorted(offer["names"]) == ["api", "work"], offer)

        ws.send({"t": "addtmux", "server": "devbox:default", "sessions": ["terminum"]})
        refused = ws.until(lambda m: m["t"] == "error", 5)
        ws.send({"t": "addtmux", "server": "nohost:default", "sessions": ["work"]})
        nohost = ws.until(lambda m: m["t"] == "error", 5)
        check("AC-89 the host's terminum session and an unknown host are refused",
              refused and "terminum's own session on devbox" in refused["msg"] and nohost and "no host named nohost" in nohost["msg"], [refused, nohost])

        ws.send({"t": "addtmux", "server": "devbox:default", "sessions": ["work"]})
        added = ws.until(lambda m: m["t"] in ("tmuxadded", "error"), 5)
        groups = wait_layout(ws, lambda gs: (g := att(gs, "work")) and g["items"], 10)
        g = att(groups, "work") or {"items": [{}]}
        check("AC-89 a host's session is attached by name and listed with its panes, apart from the host's own",
              added and added["t"] == "tmuxadded" and g["wid"] == "devbox/work" and g["items"][0].get("id", "").startswith("tmux:devbox:default:%")
              and g["items"][0].get("host") == "devbox" and own(groups) and own(groups).get("others") == 1, [added, g])
        rec = json.load(open(os.path.join(t.home, "settings.json")))["hosts"]["devbox"]
        check("AC-89 it is kept in the host's record", rec.get("attach") == ["work"], rec)
        for _ in range(50):      # listed from the host's list at once; its own client follows
            if runs := ssh_runs(pids, "attach-session"):
                break
            time.sleep(0.1)
        check("AC-89 it is one more ssh connection, attached by id with -E (no name in the host's shell)",
              len(runs) == 1 and "attach-session -E -f ignore-size -t '$" in runs[0][2] and "work" not in runs[0][2], runs)
        env = remote("show-environment", "-t", "work", "DISPLAY").strip()
        check("AC-89 attaching leaves the host session's environment as it was", ":tm-test" not in env, env)

        pane = g["items"][0]["id"]
        before = size_of("work:")
        ws.send({"t": "sub", "id": pane})
        ws.until(lambda m: m["t"] == "screen", 5)
        ws.send({"t": "size", "cols": 100, "rows": 32, "typing": False})
        opened = ws.until(lambda m: m["t"] == "sized", 5)
        check("AC-89 opening a host's session sizes it to the page, without a key (marked @tm-sized)",
              opened and before != "100x32 manual 1" and size_of("work:") == "100x32 manual 1", [before, size_of("work:")])
        ws.send({"t": "pause"})
        for _ in range(30):
            if "manual" not in size_of("work:"):
                break
            time.sleep(0.1)
        check("AC-89 the page hidden with no other page on it: the window gets tmux's own size back", "manual" not in size_of("work:"), size_of("work:"))
        ws.send({"t": "resume"})
        resumed = ws.until(lambda m: m["t"] == "sized", 5)
        check("AC-89 the page back in sight sizes it again", resumed and size_of("work:") == "100x32 manual 1", size_of("work:"))
        ws.send({"t": "in", "data": "echo host-own-$((6*7))\r", "k": 1})
        echo = ws.until(lambda m: m["t"] == "screen" and "host-own-42" in json.dumps(m.get("ch")), 10)
        check("AC-89 typing reaches it", echo is not None)
        phone = t.page()
        phone.send({"t": "sub", "id": pane})
        phone.until(lambda m: m["t"] == "screen", 5)
        phone.send({"t": "pause"})
        ws.send({"t": "sub", "id": own(groups)["items"][0]["id"]})
        ws.until(lambda m: m["t"] == "screen", 5)
        for _ in range(30):
            if "manual" not in size_of("work:"):
                break
            time.sleep(0.1)
        check("AC-89 another pane opened while only a hidden page still shows it: its own size back", "manual" not in size_of("work:"), size_of("work:"))
        phone.close()
        ws.send({"t": "sub", "id": pane})
        ws.until(lambda m: m["t"] == "sized", 5)

        # a session client that keeps ending is started again after 1, 2, 4 … s, never in a loop
        start, end = len(ssh_runs(pids, "attach-session")), time.time() + 6
        while time.time() < end:
            drop(pids, "attach-session")
            time.sleep(0.2)
        spawned = len(ssh_runs(pids, "attach-session")) - start
        check("AC-89 a host's session client that keeps ending is started again with back-off", 1 <= spawned <= 4, spawned)
        groups = wait_layout(ws, lambda gs: (x := att(gs, "work")) and x["items"], 15)
        ws.send({"t": "sub", "id": pane})
        ws.until(lambda m: m["t"] == "screen", 5)
        ws.send({"t": "in", "data": "echo back-$((2+3))\r", "k": 1})
        check("AC-89 then it comes back: typing reaches it again",
              ws.until(lambda m: m["t"] == "screen" and "back-5" in json.dumps(m.get("ch")), 15) is not None)

        # the host's link drops: its sessions wait with its state, and come back with it
        remote("set-option", "-w", "-t", "api:", "window-size", "manual")
        remote("set-option", "-w", "-t", "api:", "@tm-sized", "1")
        unix_request(t.sock, "POST", "/tmux/attach", {"session": "api", "host": "devbox"})
        wait_layout(ws, lambda gs: (x := att(gs, "api")) and x["items"], 15)
        ours = orphan("-u -C new-session -A -f ignore-size -s terminum")
        under_shell = orphan("-u -C attach-session -E -f ignore-size -t work", shell=True)
        flagged = orphan("-C attach -f ignore-size -t api")       # not terminum's command line: by tmux's list alone
        other = orphan("-u -C attach-session -E -t work")
        alive = orphan("-u -C attach-session -E -f ignore-size -t api", live=True)
        drop(pids, "new-session")
        lost = wait_layout(ws, lambda gs: (x := att(gs, "work")) and x.get("state") == "absent", 10)
        why = (att(lost or [], "work") or {}).get("why", "")
        check("AC-89 while the host is not attached its sessions show with its state", "devbox" in why, why)
        back = wait_layout(ws, lambda gs: (x := att(gs, "work")) and x["items"] and (y := att(gs, "api")) and y["items"], 20)
        check("AC-89 they come back with the host", back is not None)
        for _ in range(30):
            if "manual" not in size_of("api:"):
                break
            time.sleep(0.2)
        check("AC-89 a window marked @tm-sized that no page sizes gets its size back on reconnect (reclaim)",
              "manual" not in size_of("api:"), size_of("api:"))
        for _ in range(50):
            if not {ours, under_shell, flagged} & set(clients()):
                break
            time.sleep(0.1)
        now = clients()
        check("AC-81 orphaned terminum control clients of the host's tmux (init's, a shell's of init's) are ended on reconnect",
              ours and under_shell and ours not in now and under_shell not in now, [ours, under_shell, now])
        check("AC-81 one known by tmux's flags alone (a command line tmux rewrote) is ended too", flagged and flagged not in now, [flagged, now])
        for _ in range(40):      # the host's answer is read within 10 s
            if logged := [line for line in open(t.log_path).read().splitlines() if "clients.orphans" in line]:
                break
            time.sleep(0.25)
        check("AC-81 the log names exactly the pids the host ended", len(logged) == 1 and all(str(p) in logged[0] for p in (ours, under_shell, flagged))
              and str(other) not in logged[0] and str(alive) not in logged[0], logged)
        check("AC-81 the script's answer is taken off the host's tmux", "@tm-orphans" not in remote("show-options", "-g"), remote("show-options", "-g"))
        check("AC-81 an orphaned client of another shape is left alone", other and other in now, [other, now])
        check("AC-81 a live client of terminum's shape (another Mac's) is left alone", alive and alive in now, [alive, now])
        ws.send({"t": "sub", "id": pane})
        ws.send({"t": "in", "data": "echo swept-$((3+4))\r", "k": 1})
        check("AC-81 terminumd's own new clients are untouched: typing reaches the host's session",
              ws.until(lambda m: m["t"] == "screen" and "swept-7" in json.dumps(m.get("ch")), 10) is not None)

        listed = unix_request(t.sock, "GET", "/tmux")[1]["attached"]
        check("AC-89 terminum tmux lists the host's sessions with the host",
              [(x["session"], x["host"]) for x in listed] == [("work", "devbox"), ("api", "devbox")], listed)

        ws.send({"t": "detachtmux", "server": "devbox:default", "session": "api"})
        groups = wait_layout(ws, lambda gs: not att(gs, "api"), 5)
        check("AC-89 detaching forgets a host's session and leaves it running", groups is not None and "api" in remote("ls"), remote("ls"))

        # removing the host gives the user's windows their size back, sessions go on
        ws.send({"t": "sub", "id": pane})
        ws.until(lambda m: m["t"] == "screen", 5)
        ws.send({"t": "size", "cols": 96, "rows": 28, "typing": True})
        ws.send({"t": "in", "data": "\r", "k": 1})
        ws.until(lambda m: m["t"] == "sized" and m["cols"] == 96, 5)      # after the opening's own size
        held = size_of("work:")
        st, _ = unix_request(t.sock, "POST", "/hosts/remove", {"name": "devbox"})
        for _ in range(30):
            if "manual" not in size_of("work:"):
                break
            time.sleep(0.2)
        check("AC-89 a removed host's attached windows get their size back and run on",
              held == "96x28 manual 1" and st == 200 and "manual" not in size_of("work:") and "work" in remote("ls"), [held, st, size_of("work:")])
    finally:
        cleanup_fixtures()
        t.cleanup()
        remote("kill-server")
    return summary()


if __name__ == "__main__":
    sys.exit(main())
