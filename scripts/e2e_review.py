#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Full-tree review regressions against private daemons, tmux and a local fake ssh.

    python3 scripts/e2e_review.py [checkpoint|host|security ...]

No real host, installed daemon, credentials or personal state is accessed.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

from e2e_remote import api
from e2e_ws import Terminumd, check, results, summary, unix_request, wait_for
from testrun import free_port, own  # noqa: E402

HERE = Path(__file__).resolve().parent



def checkpoint():
    t = Terminumd(free_port(), own("tm-review-checkpoint"))
    t.env["HOME"] = t.root
    stage = "initial checkpoint"
    try:
        t.start()
        target = Path(t.app) / "layout.json"

        def read():
            try:
                return json.loads(target.read_text())
            except (OSError, ValueError):
                return {}

        def windows(value):
            return sum(len(s.get("windows", [])) for s in value.get("sessions", []))

        first = wait_for(read, lambda v: windows(v) == 1)
        stage = "publication failure"
        target.unlink()
        target.mkdir()  # deterministic publication failure, independent of uid/permissions
        subprocess.run(["tmux", "-L", t.tmux_socket, "new-window", "-d", "-t", "main:",
                        "-n", "checkpoint-review", "-c", t.root, "/bin/sh"], check=True)
        wait_for(t.log, lambda text: "restore.checkpoint_failed" in text)
        stage = "recovery without another layout change"
        target.rmdir()
        target.write_text(json.dumps(first))
        recovered = wait_for(read, lambda v: windows(v) == 2, seconds=12)
        check("AC-15 failed checkpoint is retried after storage recovers without another layout change",
              windows(recovered) == 2)
    except AssertionError as e:
        check("AC-15 checkpoint publication recovery", False, f"{stage}: {e}")
    finally:
        t.cleanup()


def host():
    t = Terminumd(free_port(), own("tm-review-host"))
    t.env["HOME"] = t.root
    remote_socket = own("tm-review-rhost")
    remote = Path(t.root) / "remote"
    remote.mkdir()
    note = remote / "note.txt"
    note.write_text("synthetic retired host")
    t.env.update(TM_SSH=str(HERE / "fake_ssh.py"), FAKE_SSH_HOME=str(remote),
                 FAKE_SSH_TMUX_SOCKET=remote_socket, FAKE_SSH_FAIL_DEST="refused",
                 FAKE_SSH_FAIL="Test destination refused")
    stage = "initial host"
    try:
        t.start()
        secret = t.signin()
        st, _ = unix_request(t.sock, "POST", "/hosts/add",
                             {"name": "reviewbox", "recipe": "alex@devbox.example"})
        assert st == 200
        wait_for(lambda: unix_request(t.sock, "GET", "/hosts")[1]["hosts"],
                 lambda hs: any(h["name"] == "reviewbox" and h["state"] == "attached" for h in hs))
        st, body = unix_request(t.sock, "POST", "/hosts/enable",
                                {"name": "reviewbox", "action": "enable"})
        assert st == 200, body
        endpoint = "/fb/api/file?path=" + str(note)
        wait_for(lambda: api(t, secret, endpoint, "reviewbox"), lambda r: r[0] == 200)
        st, _ = unix_request(t.sock, "POST", "/hosts/off",
                             {"name": "reviewbox", "action": "off"})
        assert st == 200
        off = api(t, secret, endpoint, "reviewbox")[0]
        check("AC-87 turning a host off removes its old Files registration", off == 404, off)
        unix_request(t.sock, "POST", "/hosts/on", {"name": "reviewbox"})
        wait_for(lambda: api(t, secret, endpoint, "reviewbox"), lambda r: r[0] == 200)
        st, _ = unix_request(t.sock, "POST", "/hosts/add",
                             {"name": "reviewbox", "recipe": "alex@refused.example"})
        assert st == 200
        # AC-87: the old link and its helper stay until a new recipe answers, and the group says so
        host = lambda: next(h for h in unix_request(t.sock, "GET", "/hosts")[1]["hosts"] if h["name"] == "reviewbox")
        wait_for(host, lambda h: "still connected with the previous one" in h.get("why", ""))
        check("AC-87 a recipe that does not answer keeps the old link and says so",
              "still connected with the previous one" in host().get("why", ""), host().get("why"))
        check("AC-87 ... and its Files stay", api(t, secret, endpoint, "reviewbox")[0] == 200)
        # A malformed settings record must also retire its existing server and helper.
        stage = "return to the original host"
        unix_request(t.sock, "POST", "/hosts/add", {"name": "reviewbox", "recipe": "alex@devbox.example"})
        wait_for(lambda: api(t, secret, endpoint, "reviewbox"), lambda r: r[0] == 200)
        stage = "invalid replacement record"
        settings = Path(t.home) / "settings.json"
        value = json.loads(settings.read_text())
        value["hosts"]["reviewbox"]["ssh"] = ["-o", "ProxyCommand=forbidden", "devbox.example"]
        settings.write_text(json.dumps(value))
        wait_for(lambda: unix_request(t.sock, "GET", "/hosts")[1]["hosts"],
                 lambda hs: any(h["name"] == "reviewbox" and "option not allowed" in h.get("why", "") for h in hs))
        check("AC-87 an invalid record keeps the old link and says so",
              "still connected with the previous recipe" in host().get("why", ""), host().get("why"))
        check("AC-87 ... and its Files stay", api(t, secret, endpoint, "reviewbox")[0] == 200)
    except AssertionError as e:
        check("AC-87 private host lifecycle", False, f"{stage}: {e}")
        print("Private host state:", unix_request(t.sock, "GET", "/hosts")[1])
        print("Private daemon log:", t.log())
    finally:
        t.cleanup()
        subprocess.run(["tmux", "-L", remote_socket, "kill-server"], stderr=subprocess.DEVNULL)


def security():
    t = Terminumd(free_port(), own("tm-review-security"))
    try:
        t.start()
        result = subprocess.run(["bash", str(HERE / "security_check.sh")], env=t.env)
        check("AC-09 tree and live private-daemon refusals", result.returncode == 0)
    finally:
        t.cleanup()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checks", nargs="*", choices=["checkpoint", "host", "security"])
    selected = parser.parse_args().checks or ["checkpoint", "host", "security"]
    for name in selected:
        globals()[name]()
    summary()
