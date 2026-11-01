#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""The core end to end, at the page's protocol (terminum.md AC-01, AC-02, AC-03, AC-07,
AC-13, AC-14) against a private terminumd and a private tmux server (never the user's):
the list follows tmux, screens and history, typing, keys by name, pastes, sessions, sizing,
a killed tmux server. Browser checks of the page are in web/test.

    python3 scripts/e2e_core.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import subprocess
import sys
import tempfile
import time
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, results, summary, tmux_on, wait_layout  # noqa: E402
from testrun import free_port, own  # noqa: E402

PORT, SOCKET = free_port(), own("tm-test-core")
tmux = partial(tmux_on, SOCKET)


def items(groups):
    return [it for g in groups for it in g["items"]]


def text_of(ch):
    out = []
    for _, line in ch:
        out.append("".join(r[0] if isinstance(r[0], str) else "".join(r[0]) for r in line["r"]))
    return "\n".join(out)


def screen_with(ws, needle, timeout=5):
    return ws.until(lambda m: m["t"] == "screen" and needle in text_of(m["ch"]), timeout)


def main():
    t = Terminumd(PORT, SOCKET)
    work = tempfile.mkdtemp(prefix="tmw-", dir="/tmp")
    odd = os.path.join(work, "it's #(id) $HOME")
    os.makedirs(odd)
    try:
        t.start()
        ws = t.page()
        groups = wait_layout(ws, lambda gs: items(gs))
        check("AC-01 the first start makes a session main", groups and groups[0]["label"] == "main", [g["label"] for g in groups or []])
        check("AC-01 the user's default tmux server is untouched", "tm-test" not in subprocess.run(["tmux", "ls"], capture_output=True, text=True).stdout)
        first = items(groups)[0]
        ws.send({"t": "sub", "id": first["id"]})
        ws.send({"t": "size", "cols": 80, "rows": 24, "typing": False})
        seen = []                       # the window may be sized before the first frame (AC-04): "sized" first
        shown = ws.until(lambda m: seen.append(m) or m["t"] == "screen", 5)
        hist = next((m for m in seen if m["t"] == "hist"), None)
        check("AC-02 opening a pane sends its history then its screen", hist and hist["mode"] == "reset" and shown)
        sized = next((m for m in seen if m["t"] == "sized"), None) or ws.until(lambda m: m["t"] == "sized", 5)
        check("AC-13 the page that opens a pane sizes it", sized and sized["cols"] == 80 and sized["mine"], sized)
        check("AC-13 the window is 80x24 with window-size manual", "80x24 manual" in tmux("list-windows", "-F", "#{window_width}x#{window_height} #{window-size}"))
        bounds = []
        for cols, rows in ((10, 3), (500, 200), (80, 24)):
            ws.send({"t": "size", "cols": cols, "rows": rows, "typing": True})
            got = ws.until(lambda m: m["t"] == "sized" and m["mine"], 5)
            bounds.append(got and (got["cols"], got["rows"]))
        check("AC-13 a grid stays within 45x14 (an iPhone in portrait) and 240x70 (a big desktop window)",
              bounds == [(45, 14), (240, 70), (80, 24)] and "80x24" in tmux("list-windows", "-F", "#{window_width}x#{window_height}"), bounds)

        t0 = time.time()
        ws.send({"t": "in", "data": f"cd '{work}' && echo typed-$((40+2))\r", "k": 1})
        echo = screen_with(ws, "typed-42")
        check("AC-03 typed text and Enter reach the pane", echo is not None, f"{(time.time() - t0) * 1000:.0f} ms")
        ws.send({"t": "in", "data": "clear; printf '⏺ Bash(ls)\\n  ⎿  total 12\\n     a\\n'; seq -f '     line %g' 1 10; printf '\\n⏺ Done.\\n'\r"})
        folded = ws.until(lambda m: m["t"] == "screen" and any(f[2].startswith("⋯") for f in m.get("folds", [])), 8)
        check("AC-32 a tool's long output still on the screen comes with its fold", folded is not None, folded and folded.get("folds"))
        ws.send({"t": "in", "data": "printf '<%s>' ab\x1b[D\x1b[3~X\r"})
        check("AC-03 special keys go by name (Left, DC)", screen_with(ws, "<aX>") is not None)

        ws.send({"t": "in", "data": "for i in $(seq 1 3000); do echo line-$i; done\r"})
        appended, last = 0, 0
        end = time.time() + 15
        while time.time() < end:
            m = ws.recv(5)
            if m["t"] == "hist" and m["mode"] == "append":
                appended += len(m["lines"])
                last = m["first"] + len(m["lines"])
            if m["t"] == "screen" and "line-3000" in text_of(m["ch"]):
                break
        check("AC-02 lines that scroll off are appended with their numbers", appended >= 2900 and last > 2900, f"{appended} rows, top {last}")
        ws.send({"t": "more", "before": last - 500})
        older = ws.until(lambda m: m["t"] == "hist" and m["mode"] == "prepend", 5)
        check("AC-02 older lines load on request", older and len(older["lines"]) > 0 and older["first"] + len(older["lines"]) == last - 500, older and (older["first"], len(older["lines"])))
        ws.send({"t": "in", "data": "clear && tmux clear-history\r"})
        reset = ws.until(lambda m: m["t"] == "hist" and m["mode"] == "reset", 5)
        check("AC-02 a cleared scrollback starts the page's history anew", reset is not None)

        # lines under the terminal's canonical limit (1024 bytes a line), 20 KB in all
        big = "\n".join(f"line-{i:04d}-" + "y" * 40 for i in range(400))
        ws.send({"t": "in", "data": f"cat > {work}/pasted.txt\r"})
        time.sleep(0.5)
        ws.send({"t": "in", "data": f"\x1b[200~{big}\x1b[201~"})
        ws.send({"t": "in", "data": "\r\x04"})
        time.sleep(1.5)
        got = open(f"{work}/pasted.txt").read() if os.path.exists(f"{work}/pasted.txt") else ""
        check("AC-03 a 20 KB paste goes through a tmux buffer", big in got, f"{len(got)} bytes")
        ws.send({"t": "in", "data": "\x1b[200~bad\x07paste\x1b[201~"})
        err = ws.until(lambda m: m["t"] == "error" and "control characters" in m.get("msg", ""), 5)
        check("AC-03 a paste with control characters is refused", err is not None)

        # AC-07: a new window after the shown one, in its folder (an odd name stays one argument)
        quoted = "'" + odd.replace("'", "'\\''") + "'"
        ws.send({"t": "in", "data": f"cd {quoted}\r"})
        time.sleep(0.8)
        ws.send({"t": "new", "group": groups[0]["wid"]})
        created = ws.until(lambda m: m["t"] in ("created", "error"), 5)
        path = tmux("display", "-p", "-t", (created or {}).get("id", "x").rsplit(":", 1)[-1], "#{pane_current_path}").strip()
        check("AC-07 a new window opens in the shown pane's folder", created and created["t"] == "created" and path == os.path.realpath(odd), path)
        check("AC-07 #(…) in a folder name ran nothing", not os.path.exists(os.path.join(work, "id")))
        time.sleep(1.1)
        ws.send({"t": "split", "id": created["id"], "how": "below"})
        split = ws.until(lambda m: m["t"] in ("created", "error"), 5)
        check("AC-07 a split opens next to the pane", split and split["t"] == "created", split)
        time.sleep(1.1)
        ws.send({"t": "newsession", "name": "bad:name"})
        bad = ws.until(lambda m: m["t"] == "error", 5)
        check("AC-07 a session name tmux refuses is refused in words", bad is not None, bad and bad["msg"])
        time.sleep(1.1)
        ws.send({"t": "newsession", "name": "notes"})
        made = ws.until(lambda m: m["t"] in ("created", "error"), 5)
        check("AC-07 a new session is made", made and made["t"] == "created" and "notes" in tmux("ls", "-F", "#S"), made)
        ws.send({"t": "rename", "id": created["id"], "name": "dep#(id)loy"})
        renamed = wait_layout(ws, lambda gs: any(i["title"] == "dep#(id)loy" for i in items(gs)))
        check("AC-07 a rename shows in the list, # kept literally", renamed is not None, tmux("list-windows", "-a", "-F", "#{window_name}"))
        ws.send({"t": "rename", "id": created["id"], "name": "-x deploy"})
        dashed = wait_layout(ws, lambda gs: any(i["title"] == "-x deploy" for i in items(gs)))
        check("AC-07 a name that starts with \"-\" is a name, not a flag", dashed is not None, tmux("list-windows", "-a", "-F", "#{window_name}"))
        ws.send({"t": "rename", "id": created["id"], "name": "dep#(id)loy"})
        wait_layout(ws, lambda gs: any(i["title"] == "dep#(id)loy" for i in items(gs)))
        ws.send({"t": "move", "id": created["id"], "session": "local/notes"})
        moved = wait_layout(ws, lambda gs: any(g["label"] == "notes" and any(i["id"] == created["id"] for i in g["items"]) for g in gs))
        check("AC-07 a window moves to another session", moved is not None)
        ws.send({"t": "kill", "id": split["id"]})
        killed = wait_layout(ws, lambda gs: not any(i["id"] == split["id"] for i in items(gs)))
        check("AC-07 a closed pane leaves the list", killed is not None)
        # AC-07 Clone and AC-92 projects: a window right after the row's in its folder; a project's
        # Clone and [+] join it; a closed pane leaves it, an empty project goes
        win_of = lambda k: tmux("display", "-p", "-t", k.rsplit(":", 1)[-1], "#{session_name}:#{window_index}").strip()
        time.sleep(1.1)
        ws.send({"t": "clone", "id": created["id"]})
        twin = ws.until(lambda m: m["t"] in ("created", "error"), 5)
        before, after = win_of(created["id"]), twin and win_of(twin.get("id", "x:%0"))
        check("AC-07 Clone opens a window right after the row's, in its folder",
              twin and twin["t"] == "created" and after == f"{before.split(':')[0]}:{int(before.split(':')[1]) + 1}"
              and tmux("display", "-p", "-t", twin["id"].rsplit(":", 1)[-1], "#{pane_current_path}").strip() == os.path.realpath(odd), (before, after, twin))
        ws.send({"t": "project", "action": "add", "id": created["id"]})
        made = ws.until(lambda m: m["t"] == "projects" and m["items"], 5)
        pj = (made or {"items": [{}]})["items"][0]
        check("AC-92 Add to Project makes Group 1 in the pane's folder", pj.get("name") == "Group 1" and pj.get("members") == [created["id"]]
              and pj.get("dir") == os.path.realpath(odd), made)
        # a group's reorder moves only the windows it lists: the project's keeps its place in tmux
        prefix = created["id"].rsplit(":", 1)[0] + ":"
        firsts = tmux("list-panes", "-s", "-t", "notes", "-F", "#{window_index} #{pane_id}").split("\n")
        head = prefix + next(l.split()[1] for l in firsts if l.startswith("0 "))
        place = win_of(created["id"])
        ws.send({"t": "reorder", "group": "local/notes", "ids": [twin["id"], head]})
        wait_layout(ws, lambda gs: win_of(twin["id"]).endswith(":0"), 5)
        check("AC-92 a tmux group's reorder leaves a project's window in its place", win_of(twin["id"]).endswith(":0") and win_of(created["id"]) == place,
              (place, win_of(created["id"]), win_of(twin["id"])))
        time.sleep(1.1)
        ws.send({"t": "clone", "id": created["id"]})
        joined = ws.until(lambda m: m["t"] == "projects" and len(m["items"][0]["members"]) == 2, 5)
        third = ws.until(lambda m: m["t"] in ("created", "error"), 5)
        check("AC-92 a project's session cloned: the window joins the project after it", joined and third and joined["items"][0]["members"] == [created["id"], third["id"]], joined)
        tmux("kill-pane", "-t", third["id"].rsplit(":", 1)[-1])
        # a member added a moment ago is kept 5 s (its window may not be listed yet), then the list is read again
        left = ws.until(lambda m: m["t"] == "projects" and m["items"] and m["items"][0]["members"] == [created["id"]], 8)
        check("AC-92 a pane closed leaves its project", left is not None)
        ws.send({"t": "project", "action": "rename", "project": pj.get("id"), "name": "API"})
        renamed = ws.until(lambda m: m["t"] == "projects" and m["items"] and m["items"][0]["name"] == "API", 5)
        ws.send({"t": "project", "action": "remove", "id": created["id"]})
        gone = ws.until(lambda m: m["t"] == "projects" and not m["items"], 5)
        check("AC-92 renamed; its last session taken out, the project goes; kept in projects.json",
              renamed is not None and gone is not None and json.load(open(os.path.join(t.app, "projects.json")))["projects"] == [], renamed)
        tmux("kill-pane", "-t", twin["id"].rsplit(":", 1)[-1])
        # a tmux session's group renamed: its session, # and a leading - kept literally; a name tmux refuses said
        ws.send({"t": "renamegroup", "group": "local/notes", "name": "-n#otes"})
        regrouped = wait_layout(ws, lambda gs: any(g.get("wid") == "local/-n#otes" for g in gs))
        check("AC-07 a group's Rename names its tmux session, # and a leading - literally", regrouped is not None and "-n#otes" in tmux("ls", "-F", "#S").split("\n"),
              tmux("ls", "-F", "#S"))
        ws.send({"t": "renamegroup", "group": "local/-n#otes", "name": "bad:name"})
        refused = ws.until(lambda m: m["t"] == "error", 5)
        ws.send({"t": "renamegroup", "group": "local/-n#otes", "name": "notes"})
        back = wait_layout(ws, lambda gs: any(g.get("wid") == "local/notes" for g in gs))
        check("AC-07 a group name tmux refuses is refused in words; renamed back", refused is not None and back is not None, refused and refused["msg"])
        tmux("new-window", "-d", "-t", "main:", "-n", "byhand")
        check("AC-01 a window made by hand shows within 2 s", wait_layout(ws, lambda gs: any(i["title"] == "byhand" for i in items(gs)), 3) is not None)

        # AC-38: ?trace=1 times a key from its arrival to the frame that shows it, into logs/
        ws.send({"t": "trace", "on": True})
        on = ws.until(lambda m: m["t"] == "trace" and "on" in m, 5)
        ws.send({"t": "in", "data": "z", "k": 77})
        traced = ws.until(lambda m: m["t"] == "screen" and any(k.get("k") == 77 for k in (m.get("tr") or {}).get("ks", [])), 5)
        ws.send({"t": "in", "data": "\x15"})
        ws.send({"t": "trace", "on": False})
        ws.until(lambda m: m["t"] == "trace", 5)
        logs = os.path.join(t.home, "logs")
        files = [f for f in os.listdir(logs) if f.startswith("trace-")] if os.path.isdir(logs) else []
        body = open(os.path.join(logs, files[0])).read() if files else ""
        check("AC-38 a traced key comes back with its stages in the frame", on and on.get("on") and traced is not None,
              traced and traced["tr"]["ks"][0])
        check("AC-38 the trace is written without typed text", files and '"k":77' in body.replace(" ", "") and '"z"' not in body, files)

        # AC-06: a program that asked for the mouse gets SGR reports for clicks and the wheel (a
        # page's frame of wheel steps as one message with their count), in order with the keys
        reader = os.path.join(work, "mouse.py")
        with open(reader, "w") as f:
            f.write("import os, sys, termios, tty\n"
                    "fd = sys.stdin.fileno(); old = termios.tcgetattr(fd); tty.setraw(fd)\n"
                    "sys.stdout.write('\\x1b[?1049h\\x1b[?1000h\\x1b[?1002h\\x1b[?1006h'); sys.stdout.flush()\n"
                    "got = b''\n"
                    "while got.count(b'M') + got.count(b'm') < 6: got += os.read(fd, 64)\n"
                    "termios.tcsetattr(fd, termios.TCSADRAIN, old)\n"
                    "sys.stdout.write('\\x1b[?1006l\\x1b[?1002l\\x1b[?1000l\\x1b[?1049l')\n"
                    "print('MOUSE', got.decode().replace('\\x1b[<', '<'))\n")
        ws.send({"t": "in", "data": f"python3 {reader}\r"})
        alt = ws.until(lambda m: m["t"] == "screen" and m.get("alt") and m.get("mouse"), 8)
        check("AC-06 a frame says the program asked for the mouse", alt is not None)
        ws.send({"t": "mouse", "x": 9, "y": 4, "button": 0, "action": "press", "mods": {}})
        ws.send({"t": "mouse", "x": 9, "y": 4, "button": 0, "action": "release", "mods": {}})
        ws.send({"t": "mouse", "x": 2, "y": 3, "button": 0, "action": "wheeldown", "mods": {}, "count": 3})
        ws.send({"t": "in", "data": "k"})
        ws.send({"t": "mouse", "x": 2, "y": 3, "button": 0, "action": "wheelup", "mods": {}})
        got = screen_with(ws, "MOUSE", 8)
        line = text_of(got["ch"]) if got else ""
        check("AC-06 clicks and the wheel arrive as SGR mouse reports (1-based cells), a count as that many, in order with a key",
              "MOUSE <0;10;5M<0;10;5m<65;3;4M<65;3;4M<65;3;4Mk<64;3;4M" in line, line.strip()[-80:])

        # AC-13: a second page that opens the pane sizes it; the first takes it back once active again
        other = t.page()
        other.until(lambda m: m["t"] == "layout", 5)
        other.send({"t": "sub", "id": first["id"]})
        other.send({"t": "size", "cols": 48, "rows": 30, "typing": False})
        mine = other.until(lambda m: m["t"] == "sized" and m["mine"], 5)
        check("AC-13 the page that opens the pane sizes it", mine and (mine["cols"], mine["rows"]) == (48, 30), mine)
        theirs = ws.until(lambda m: m["t"] == "sized" and not m["mine"], 5)
        check("AC-13 the other page is told it was sized for another screen", theirs is not None)
        other.send({"t": "size", "cols": 50, "rows": 30, "typing": False})
        own = other.until(lambda m: m["t"] == "sized" and m["mine"], 5)
        check("AC-13 the page that sizes it follows its own view's change", own and own["cols"] == 50, own)
        ws.send({"t": "resume"})
        back = ws.until(lambda m: m["t"] == "sized" and m["mine"], 5)
        other.send({"t": "size", "cols": 52, "rows": 30, "typing": False})
        time.sleep(1.5)
        check("AC-13 a page that becomes active again sizes it back; the other's view change then takes nothing",
              back and "80x24" in tmux("display", "-p", "-t", first["id"].rsplit(":", 1)[-1], "#{window_width}x#{window_height}"), back)
        other.send({"t": "in", "data": " "})
        typed = other.until(lambda m: m["t"] == "sized" and m["mine"], 5)
        check("AC-13 the page that types sizes it", typed and (typed["cols"], typed["rows"]) == (52, 30), typed)
        other.close()

        # AC-14: the tmux server goes away
        tmux("kill-server")
        seen = {"closed": False, "main": False}

        def note(m):
            seen["closed"] |= m["t"] == "error" and "closed" in m.get("msg", "")
            # a layout after the kill is the new server's: main, its windows restored (AC-15); pane ids start anew, so one may be the old one's
            seen["main"] |= m["t"] == "layout" and any(g["label"] == "main" and g["items"] for g in m["groups"])
            return all(seen.values())
        ws.until(note, 8)
        check("AC-14 a shown pane says it has closed", seen["closed"])
        check("AC-14 the server is started again with a session main", seen["main"])
        h = t.health()
        check("AC-08 status names the build, port and tmux", h["port"] == PORT and h["tmux"]["ok"] and h["build"], {k: h[k] for k in ("build", "port", "pages")})

        # AC-93: a mark set just before a stop is on disk after it (the stop writes unread.json)
        mark = first["id"].rsplit(":", 1)[0] + ":" + tmux("list-panes", "-a", "-F", "#{pane_id}").split()[-1]
        ws.send({"t": "unread", "id": mark, "on": True})
        ws.until(lambda m: m["t"] == "layout" and any(i["id"] == mark and i.get("unread") for i in items(m["groups"])), 5)
        t.stop()
        kept = json.load(open(os.path.join(t.app, "unread.json"))).get("unread", []) if os.path.exists(os.path.join(t.app, "unread.json")) else []
        check("AC-93 an unread mark set just before a stop is kept", mark and mark in kept, kept)
    finally:
        if not all(results):
            print(t.log())
        t.cleanup()
        subprocess.run(["rm", "-rf", work])
    summary()


if __name__ == "__main__":
    main()
