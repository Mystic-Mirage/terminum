#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""The layout survives a reboot and a killed tmux (terminum.md AC-15), end to end against a
private terminumd and tmux server: a checkpoint is written, a "reboot" (another TM_BOOT_ID,
the tmux server gone) brings the sessions, windows, splits and folders back with a report; a
restart of terminumd recreates nothing; the server killed while terminumd runs comes back; a
session of "the user's own" tmux (another private server, attached by name) comes back when
its whole server is killed; layout.json holds no output or environment.

    python3 scripts/e2e_restore.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import subprocess
import sys
import tempfile
import time
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, results, summary, tmux_on  # noqa: E402
from testrun import free_port, own  # noqa: E402

PORT, SOCKET, OWN = free_port(), own("tm-test-restore"), own("tm-test-rown")
tmux = partial(tmux_on, SOCKET)
own = partial(tmux_on, OWN)


def main():
    work = os.path.realpath(tempfile.mkdtemp(prefix="tmr-", dir="/tmp"))
    # a HOME of its own: the user's own tmux made again by terminumd reads no tmux.conf of the user's
    t = Terminumd(PORT, SOCKET, env={"TM_BOOT_ID": "boot-A", "HOME": work})
    for d in ("api", "docs", "notes"):
        os.makedirs(os.path.join(work, d))
    try:
        os.makedirs(t.home, exist_ok=True)
        with open(os.path.join(t.home, "settings.json"), "w") as f:
            json.dump({"attach": {OWN: ["work"]}}, f)
        subprocess.run(["tmux", "-L", OWN, "kill-server"], stderr=subprocess.DEVNULL)
        own("-f", "/dev/null", "new-session", "-d", "-s", "work", "-n", "logs", "-c", f"{work}/notes")
        own("new-window", "-t", "work:", "-n", "build", "-c", f"{work}/docs")
        t.start()
        time.sleep(1)
        tmux("rename-window", "-t", "main:", "api")
        tmux("send-keys", "-t", "main:api", f"cd {work}/api", "Enter")
        tmux("split-window", "-t", "main:api", "-c", f"{work}/api")
        tmux("new-window", "-t", "main:", "-n", "docs", "-c", f"{work}/docs")
        tmux("send-keys", "-t", "main:docs", "export SECRET_TOKEN=abc123; echo private-output", "Enter")
        tmux("new-session", "-d", "-s", "notes", "-c", f"{work}/notes")
        quick = tmux("new-window", "-d", "-t", "main:", "-n", "quick", "-P", "-F", "#{window_id}").strip()
        tmux("set-option", "-w", "-t", quick, "@tm-quick", "%0")   # a quick shell (AC-88): not restored
        layout = os.path.join(t.app, "layout.json")
        saved = None
        for _ in range(30):
            time.sleep(0.5)
            if os.path.exists(layout):
                saved = json.load(open(layout))
                if len(saved["sessions"]) == 2 and sum(len(w["panes"]) for s in saved["sessions"] for w in s["windows"]) == 4:
                    break
        check("AC-15 a checkpoint is written within 5 s of a change", saved and len(saved["sessions"]) == 2, saved and [s["name"] for s in saved["sessions"]])
        text = open(layout).read()
        check("AC-15 layout.json holds no output or environment", "private-output" not in text and "abc123" not in text)
        check("AC-88 a quick shell is not in the checkpoint", '"quick"' not in text and quick.startswith("@"), quick)
        t.stop()
        tmux("kill-server")
        t.env["TM_BOOT_ID"] = "boot-B"
        t.start()
        ws = t.page()
        report = ws.until(lambda m: m["t"] == "restore", 8)
        check("AC-15 after a reboot the first page gets the report", report is not None and "Restored 2 sessions, 3 windows" in report["report"]["text"], report and report["report"]["text"])
        sessions = tmux("ls", "-F", "#S").split()
        check("AC-15 the sessions are back", sorted(sessions) == ["main", "notes"], sessions)
        wins = tmux("list-windows", "-t", "main", "-F", "#{window_name} #{window_panes}").splitlines()
        check("AC-15 windows and splits are back", wins == ["api 2", "docs 1"], wins)
        renames = tmux("list-windows", "-a", "-F", "#{session_name}:#{window_index} #{automatic-rename}").splitlines()
        check("AC-15 a window the user named keeps its name; one tmux named after its program is tmux's to name again",
              renames == ["main:0 0", "main:1 0", "notes:0 1"], renames)
        paths = set(tmux("list-panes", "-a", "-F", "#{pane_current_path}").split())
        check("AC-15 the panes are in their folders", {f"{work}/api", f"{work}/docs", f"{work}/notes"} <= paths, paths)
        check("AC-15 the report is kept in the state folder", os.path.exists(os.path.join(t.app, "restore-report.json")))
        t.stop()
        t.start()
        time.sleep(1.5)
        check("AC-15 a restart in the same boot recreates nothing", sorted(tmux("ls", "-F", "#S").split()) == ["main", "notes"])

        # terminum's tmux killed while terminumd runs (the same boot): it comes back
        tmux("kill-server")
        back = []
        for _ in range(40):
            time.sleep(0.5)
            try:
                back = sorted(tmux("ls", "-F", "#S").split())
            except Exception:
                back = []
            if back == ["main", "notes"]:
                break
        check("AC-15 terminum's tmux killed while terminumd runs comes back, its windows and folders too",
              back == ["main", "notes"] and tmux("list-windows", "-t", "main", "-F", "#{window_name} #{window_panes}").splitlines() == ["api 2", "docs 1"], back)

        # the user's own tmux, attached by name: kept in the checkpoint, made again when its whole server is gone
        saved = json.load(open(layout))
        kept = saved.get("servers", {}).get(f"local:{OWN}", {})
        check("AC-15 the checkpoint keeps an attached session of the user's own tmux, its windows and folders",
              [s["name"] for s in kept.get("sessions", [])] == ["work"] and [w["name"] for w in kept["sessions"][0]["windows"]] == ["logs", "build"], kept)
        own("kill-server")
        made = []
        for _ in range(60):
            time.sleep(0.5)
            try:
                made = own("list-windows", "-t", "work", "-F", "#{window_name} #{pane_current_path}").splitlines()
            except Exception:
                made = []
            if len(made) == 2:
                break
        check("AC-15 the user's own tmux killed: its attached session is made again with its windows and folders",
              made == [f"logs {work}/notes", f"build {work}/docs"], made)

        # the last pane closed by the user ends that tmux with it: nothing is made again (here and in the user's own)
        def kept_windows(server):
            try:
                v = json.load(open(layout))
            except (OSError, ValueError):
                return None
            ss = v.get("servers", {}).get(server, {}).get("sessions", []) if server else v.get("sessions", [])
            return [w["name"] for s in ss for w in s["windows"]]

        def down_to_one(run, server, target):
            for name in run("list-sessions", "-F", "#S").split():
                if name != target.split(":")[0]:
                    run("kill-session", "-t", name)
            for line in run("list-windows", "-a", "-F", "#{session_name}:#{window_name} #{window_id}").splitlines():
                if not line.startswith(target + " "):
                    run("kill-window", "-t", line.split()[-1])
            for _ in range(40):
                time.sleep(0.5)
                if kept_windows(server) == [target.split(":")[1]]:
                    break
            time.sleep(1.5)
            run("kill-window", "-t", target)

        down_to_one(own, f"local:{OWN}", "work:logs")
        time.sleep(4)
        left = subprocess.run(["tmux", "-L", OWN, "list-sessions"], capture_output=True, text=True)
        check("AC-15 the user's own tmux ended by its last pane closing is not made again", left.returncode != 0, left.stdout)
        down_to_one(tmux, None, "main:docs")
        fresh = []
        for _ in range(30):
            time.sleep(0.5)
            try:
                fresh = tmux("list-windows", "-a", "-F", "#{session_name}:#{window_name}").splitlines()
            except Exception:
                fresh = []
            if fresh:
                break
        time.sleep(2)
        fresh = tmux("list-windows", "-a", "-F", "#{session_name}:#{window_name}").splitlines()
        check("AC-15 terminum's tmux ended by its last pane closing starts a fresh main, nothing restored",
              len(fresh) == 1 and fresh[0].startswith("main:") and fresh[0] != "main:docs", fresh)
    finally:
        if not all(results):
            print(t.log())
        t.cleanup()
        subprocess.run(["tmux", "-L", OWN, "kill-server"], stderr=subprocess.DEVNULL)
        subprocess.run(["rm", "-rf", work])
    summary()


if __name__ == "__main__":
    main()
