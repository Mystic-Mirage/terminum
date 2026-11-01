#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""A session's last lines survive its tmux restarting (terminum.md AC-96), end to end against a
private terminumd and tmux server: the last 300 lines of each pane (under a full-screen program,
the normal screen's, not the program's) are kept within 5 s in private files; the server killed, AC-15 makes
the panes again and each new pane's "before the restart" lines are the old pane's; a page asks
for them; a pane closed loses its files once it stays absent 30 s; lastLines 0 deletes them all.

    python3 scripts/e2e_lastlines.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, results, summary, tmux_on, wait_layout  # noqa: E402
from testrun import free_port, own  # noqa: E402

PORT, SOCKET = free_port(), own("tm-test-last")
tmux = partial(tmux_on, SOCKET)


def wait(pred, timeout):
    end = time.time() + timeout
    while time.time() < end:
        got = pred()
        if got:
            return got
        time.sleep(0.5)
    return pred()


def ask(ws, key):
    ws.send({"t": "lastlines", "id": key})
    return ws.until(lambda m: m["t"] == "lastlines" and m["id"] == key, 8)


def main():
    work = os.path.realpath(tempfile.mkdtemp(prefix="tml-", dir="/tmp"))
    t = Terminumd(PORT, SOCKET, env={"HOME": work})
    os.makedirs(os.path.join(work, "api"))
    key = lambda pane: f"tmux:local:{SOCKET}:{pane}"   # noqa: E731
    try:
        t.start()
        wait(lambda: "main" in tmux("ls", "-F", "#S").split(), 5)
        api = tmux("new-window", "-t", "main:", "-n", "api", "-c", f"{work}/api", "-P", "-F", "#{pane_id}").strip()
        tmux("send-keys", "-t", api, "seq -f 'ended-%g' 1 400", "Enter")
        prog = tmux("new-window", "-t", "main:", "-n", "prog", "-P", "-F", "#{pane_id}").strip()
        tmux("send-keys", "-t", prog, "seq -f 'under-%g' 1 5; printf '\\033[?1049h\\033[H%s-%s' PROGRAM SCREEN; sleep 600", "Enter")
        # a coding agent's pane: its marked title names its row (AC-15)
        talk = tmux("new-window", "-t", "main:", "-P", "-F", "#{pane_id}").strip()
        tmux("send-keys", "-t", talk, "printf '\\033]2;\\342\\234\\263 Fix the login form\\033\\\\'; sleep 600", "Enter")
        identity = wait(lambda: tmux("show", "-sqv", "@terminum-id").strip(), 10)
        folder = os.path.join(t.app, "lastlines", f"local@{SOCKET}")
        api_file = os.path.join(folder, f"{identity}_{api[1:]}.json")

        def kept(path, needle):
            try:
                v = json.load(open(path))
            except (OSError, ValueError):
                return None
            return v if needle in v["text"] else None

        # the last 300: the end of seq's 400 lines and the prompt after them
        first = wait(lambda: kept(api_file, "ended-400\n"), 12)
        text = first["text"].split("\n") if first else []
        check("AC-96 a pane's last 300 lines are kept within 5 s of its output", len(text) == 300 and text[-2] == "ended-400" and text[0] == "ended-102",
              text[:2] + text[-2:])
        check("AC-96 the file names the pane, its session, window and folder",
              first and first["key"] == key(api) and first["session"] == "main" and first["window"] == "api" and first["dir"] == f"{work}/api", first and {k: v for k, v in first.items() if k != "text"})
        modes = (stat.S_IMODE(os.stat(api_file).st_mode), stat.S_IMODE(os.stat(folder).st_mode), stat.S_IMODE(os.stat(os.path.dirname(folder)).st_mode))
        check("AC-96 files 0600 in 0700 folders", modes == (0o600, 0o700, 0o700), [oct(m) for m in modes])
        if sys.platform == "darwin":
            ex = subprocess.run(["tmutil", "isexcluded", os.path.dirname(folder)], capture_output=True, text=True).stdout
            check("AC-96 the folder is kept out of Time Machine", "[Excluded]" in ex, ex.strip())
        prog_file = os.path.join(folder, f"{identity}_{prog[1:]}.json")
        under = wait(lambda: kept(prog_file, "under-5"), 12)
        check("AC-96 under a full-screen program the normal screen's lines are kept, not the program's screen",
              under and under["text"].endswith("under-5") and "PROGRAM-SCREEN" not in under["text"], under and under["text"][-200:])

        ws = t.page()
        now = ask(ws, key(api))
        check("AC-96 a page gets a pane's latest lines, none from before a restart",
              now and now["before"] is None and "ended-400\n" in now["latest"]["text"] and now["lines"] == 300, now and {k: v for k, v in now.items() if k not in ("latest",)})

        ws.send({"t": "unread", "id": key(talk), "on": True})
        layout = os.path.join(t.app, "layout.json")
        check("AC-96 the checkpoint names the server's identity (no output)",
              wait(lambda: os.path.exists(layout) and json.load(open(layout)).get("identity") == identity and "ended-" not in open(layout).read(), 10))
        check("AC-15 the checkpoint keeps the agent's title for its pane",
              wait(lambda: "Fix the login form" in open(layout).read(), 10), open(layout).read()[:300])
        check("AC-93 the mark is kept", wait(lambda: key(talk) in open(os.path.join(t.app, "unread.json")).read(), 5))
        old_talk = talk

        old = {"api": api, "prog": prog}
        talk_index = tmux("display", "-p", "-t", talk, "#{window_index}").strip()
        tmux("kill-server")

        def remade():
            r = subprocess.run(["tmux", "-L", SOCKET, "list-windows", "-t", "main", "-F", "#{window_name} #{pane_id}"], capture_output=True, text=True)
            names = dict(line.split(" ", 1) for line in r.stdout.split("\n") if " " in line)
            return names if "api" in names and "prog" in names else None

        made = wait(remade, 20) or {}
        check("AC-15 the killed server's windows are made again", set(old) <= set(made), made)
        report = ws.until(lambda m: m["t"] == "restore" and m.get("report"), 10)
        check("AC-96 the restore report points to the last lines", report and "Show Last 300 Lines" in report["report"]["text"], report and report["report"]["text"])
        if made:
            got = wait(lambda: (lambda m: m if m and m.get("before") else None)(ask(ws, key(made["api"]))), 10)
            before = (got or {}).get("before") or {}
            bt = before.get("text", "").split("\n")
            check("AC-96 the pane made again shows the old pane's last lines from before the restart",
                  len(bt) == 300 and bt[-2] == "ended-400" and before.get("window") == "api", [bt[:1], bt[-2:], before.get("key")])
            got = ask(ws, key(made["prog"]))
            check("AC-96 a full-screen program's pane made again shows the lines under it",
                  got and got["before"] and got["before"]["text"].endswith("under-5"), got and got.get("before"))
            new_identity = tmux("show", "-sqv", "@terminum-id").strip()
            gone_old = not [f for f in os.listdir(folder) if f.startswith(identity + "_" + api[1:] + ".")]
            check("AC-96 the old pane's file became the new pane's (a new identity)", new_identity != identity and gone_old and
                  os.path.exists(os.path.join(folder, f"{new_identity}_{made['api'][1:]}.before.json")), sorted(os.listdir(folder)))

            history = tmux("capture-pane", "-p", "-J", "-S", "-1000", "-t", made["api"])
            check("AC-96 the pane made again has the old lines in its terminal, then a note and its shell",
                  "ended-102\n" in history and history.index("ended-400") < history.index("from before tmux restarted")
                  and tmux("display", "-p", "-t", made["api"], "#{pane_current_command}").strip() in ("bash", "zsh", "sh"),
                  history[-300:])
            check("AC-96 the file it printed is gone", not [f for f in os.listdir(folder) if f.startswith("print-")], sorted(os.listdir(folder)))
            new_talk = tmux("list-panes", "-t", f"main:{talk_index}", "-F", "#{pane_id}").strip()
            check("AC-15 the agent's pane made again keeps its title while a shell runs there",
                  tmux("show", "-pqv", "-t", new_talk, "@tm-title").strip() == "Fix the login form", new_talk)
            fresh = t.page()                            # the list as a page opening now gets it
            gs = wait_layout(fresh, lambda gs: any(i["id"] == key(new_talk) for g in gs for i in g.get("items", [])), 10) or []
            row = next((i for g in gs for i in g.get("items", []) if i["id"] == key(new_talk)), {})
            check("AC-15 its row shows that title", row.get("title") == "Fix the login form", row)
            unread = json.load(open(os.path.join(t.app, "unread.json")))
            check("AC-93 the mark went to the pane made in its place", unread["unread"] == [key(new_talk)], [unread, key(old_talk)])
            check("AC-93 its row is unread", row.get("unread") is True, row)

            # a pane closed by the user: its files go once it stays absent 30 s
            files = lambda: [f for f in os.listdir(folder) if f.startswith(f"{new_identity}_{made['api'][1:]}.")]   # noqa: E731
            wait(lambda: len(files()) == 2, 10)
            tmux("kill-window", "-t", "main:api")
            time.sleep(12)
            check("AC-96 a pane just closed keeps its lines (a reboot closes panes before tmux ends)", len(files()) == 2, files())
            check("AC-96 a pane closed for 30 s loses its lines, those from before the restart too", wait(lambda: not files(), 40), files())

        with open(os.path.join(t.home, "settings.json"), "w") as f:
            json.dump({"lastLines": 0}, f)
        off = ws.until(lambda m: m["t"] == "settings" and m.get("page", {}).get("lastLines") == 0, 10)
        check("AC-96 lastLines 0: the page is told (no menu item)", off is not None)
        check("AC-96 lastLines 0 deletes every kept line", wait(lambda: not os.path.exists(os.path.dirname(folder)), 12))
    finally:
        if not all(results):
            for args in (["show", "-sv", "@terminum-id"], ["show", "-gv", "@terminum-id"], ["list-panes", "-a", "-F", "#{pane_id} #{@terminum-id} #{session_name}"]):
                r = subprocess.run(["tmux", "-L", SOCKET, *args], capture_output=True, text=True)
                print("tmux", *args, "->", r.stdout.strip(), r.stderr.strip())
            print(t.log())
        t.cleanup()
        shutil.rmtree(work, ignore_errors=True)
    summary()


if __name__ == "__main__":
    main()
