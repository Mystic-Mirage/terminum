#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""A host whose ssh works and whose tmux gives a control client no answer (terminum-remote.md
AC-81), against a private terminumd and a private host through scripts/fake_ssh.py: after three
tries terminumd stops, says why, and Reconnect makes one try. Exits non-zero when a check fails.

    python3 scripts/e2e_hostsilent.py      (needs terminumd built: cd terminumd && cargo build)
"""
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, host_group, summary, tmux_on as tmux, unix_request, wait_layout  # noqa: E402
from testrun import free_port, own  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
LOCAL, REMOTE = own("tm-test-slocal"), own("tm-test-sremote")


def tries(pids):
    """How many control clients were started, by the stand-in's own record."""
    return sum("new-session" in line for line in open(pids))


def main():
    t = Terminumd(free_port(), LOCAL)
    home, pids, flag = os.path.join(t.root, "rh"), os.path.join(t.root, "ssh-pids"), os.path.join(t.root, "silent")
    os.makedirs(home)
    t.env.update(TM_SSH=os.path.join(HERE, "fake_ssh.py"), FAKE_SSH_HOME=home, FAKE_SSH_TMUX_SOCKET=REMOTE,
                 FAKE_SSH_PIDS=pids, FAKE_SSH_TMUX_SILENT=flag)
    try:
        t.start()
        ws = t.page(t.signin())
        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "", "recipe": "ssh alex@devbox.example"})
        assert st == 200, body
        up = wait_layout(ws, lambda g: (h := host_group(g)) is not None and h["items"] and h["state"] == "attached", 15)
        assert up, "the host did not come up: " + t.log()
        ids = [i["id"] for i in host_group(up)["items"]]
        check("AC-81 a host that answers offers no Reconnect", not host_group(up).get("reconnect"))

        server = tmux(REMOTE, "display-message", "-p", "#{version}").strip()
        open(flag, "w").close()                # from here on a control client hears nothing
        for pid in tmux(REMOTE, "list-clients", "-F", "#{client_pid}").split():
            os.kill(int(pid), 15)              # the link drops, as with a real ssh going

        said = f"its client is 99.9, its running server {server}"
        failing = wait_layout(ws, lambda g: (h := host_group(g)) is not None and said in (h.get("why") or ""), 40)
        check("AC-81 with ssh working, the group says tmux does not answer, and the versions", failing is not None, host_group(failing or []))
        stopped = wait_layout(ws, lambda g: (h := host_group(g)) is not None and h.get("reconnect") is True, 90)
        h = host_group(stopped or []) or {}
        check("AC-81 it stops trying and offers Reconnect", bool(stopped) and h.get("state") == "failed" and said in (h.get("why") or ""), h)
        check("AC-81 its rows stay, dimmed", [i["id"] for i in h.get("items", [])] == ids and all(i.get("offline") for i in h.get("items", [])), h.get("items"))
        at_stop = tries(pids)
        time.sleep(12)                         # more than the back-off a try would come after (8 s)
        check("AC-81 no more tries once stopped: nothing piles up on the host", tries(pids) == at_stop, f"{at_stop} then {tries(pids)}")
        check("AC-81 `terminum status` says so", next(x for x in unix_request(t.sock, "GET", "/hosts")[1]["hosts"] if x["name"] == "devbox")["state"] == "failed")

        unix_request(t.sock, "POST", "/hosts/retry", {"name": "devbox"})
        wait = wait_layout(ws, lambda g: (x := host_group(g)) is not None and x.get("reconnect") is True, 30)
        check("AC-81 Reconnect while tmux stays silent: one try, then it waits again", wait is not None and tries(pids) == at_stop + 1, f"{at_stop} then {tries(pids)}")

        os.remove(flag)                        # the server restarted, the version back
        unix_request(t.sock, "POST", "/hosts/retry", {"name": "devbox"})
        back = wait_layout(ws, lambda g: (x := host_group(g)) is not None and x.get("state") == "attached" and x["items"]
                           and not x.get("reconnect") and not any(i.get("offline") for i in x["items"]), 20)
        check("AC-81 Reconnect once tmux answers: attached, its rows live", back is not None and [i["id"] for i in host_group(back)["items"]] == ids)
    finally:
        if os.path.exists(flag):
            os.remove(flag)                    # the silent clients end by themselves
        t.cleanup()
        subprocess.run(["tmux", "-L", REMOTE, "kill-server"], stderr=subprocess.DEVNULL)
    return summary()


if __name__ == "__main__":
    sys.exit(main())
