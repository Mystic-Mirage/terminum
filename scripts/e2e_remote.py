#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Remote hosts end to end (terminum-remote.md AC-81–AC-85, AC-87) against a private
terminumd, with this Mac playing the host through scripts/fake_ssh.py (a private tmux
server, a fake home). Checks: add a host (and refusals), its session in the list within
5 s, typing into a host's pane, a dropped link reconnecting, a host whose ssh fails, enabling
its files (the helper copied, listings through the agent), a paste saved on the host, the
helper removed. Exits non-zero when a check fails.

    python3 scripts/e2e_remote.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import shutil
import signal
import subprocess
import sys
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, check, results, summary, unix_request, wait_layout  # noqa: E402
from testrun import free_port, own  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
PORT, LOCAL_SOCKET, REMOTE_SOCKET = free_port(), own("tm-test-rlocal"), own("tm-test-remote")


def host_group(layout, name):
    """The host's own group (its `terminum` session or its state), not a session attached there (AC 89)."""
    return next((g for g in layout if g.get("host") == name and not g.get("attached")), None)


def api(t, secret, path, host=None, method="GET", body=None, headers=None):
    h = {"X-TM-Session": secret, **(headers or {})}
    if host:
        h["X-TM-Host"] = host
    req = urllib.request.Request(f"http://127.0.0.1:{t.port}{path}", data=body, method=method, headers=h)
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return r.status, json.loads(r.read() or b"null")
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read() or b"null")


def raw(t, secret, path, host=None):
    """(status, Content-Type, Content-Security-Policy, bytes) of a raw file's answer."""
    h = {"X-TM-Session": secret} if secret else {}
    if host:
        h["X-TM-Host"] = host
    req = urllib.request.Request(f"http://127.0.0.1:{t.port}{path}", headers=h)
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return r.status, r.headers.get("Content-Type"), r.headers.get("Content-Security-Policy") or "", r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.headers.get("Content-Type"), e.headers.get("Content-Security-Policy") or "", e.read()


def recipe_edits(t, ws, pids):
    """AC-87 edge: settings.json edited by hand under an attached host keeps its link until a new
    recipe answers; a broken or failing one says why and changes nothing."""
    path = os.path.join(t.home, "settings.json")

    def edit(recipe):
        s = json.load(open(path))
        s["hosts"]["devbox"]["ssh"] = recipe
        json.dump(s, open(path + ".tmp", "w"))
        os.replace(path + ".tmp", path)

    def why_has(text, t_s=10):
        return wait_layout(ws, lambda gs: text in ((host_group(gs, "devbox") or {}).get("why") or ""), t_s)

    def typed(word):
        ws.send({"t": "in", "data": f"echo {word}-$((3*3))\r"})
        return ws.until(lambda m: m["t"] == "screen" and f"{word}-9" in json.dumps(m["ch"]), 10) is not None

    good = ["-p", "2222", "alex@devbox.example"]
    edit(["-o", "ProxyCommand=x", "alex@devbox.example"])
    held = why_has("still connected with the previous recipe")
    check("AC-87 a recipe made invalid by hand keeps the attached link and says why",
          held and (host_group(held, "devbox") or {}).get("state") == "attached" and typed("kept"), host_group(held or [], "devbox"))
    edit(["-p", "2222", "alex@refused.example"])
    failed = why_has("the new recipe failed: Permission denied (publickey)")
    check("AC-87 a new recipe that fails over ssh keeps the attached link and says why",
          failed and (host_group(failed, "devbox") or {}).get("state") == "attached" and typed("still"), host_group(failed or [], "devbox"))
    for line in open(pids):                                   # the old link drops meanwhile
        pid, _, rest = line.partition(" ")
        if "new-session" in rest:
            try:
                os.kill(int(pid), signal.SIGTERM)
            except ProcessLookupError:
                pass
    taken = wait_layout(ws, lambda gs: (host_group(gs, "devbox") or {}).get("state") == "failed"
                        and "Permission denied" in ((host_group(gs, "devbox") or {}).get("why") or ""), 20)
    check("AC-87 a held host whose old link drops takes its new record, never the old one again", taken is not None,
          host_group(taken or [], "devbox"))
    edit(["-p", "2223", "alex@devbox.example"])
    moved = wait_layout(ws, lambda gs: "-p 2223" in ((host_group(gs, "devbox") or {}).get("attach") or "")
                        and (host_group(gs, "devbox") or {}).get("state") == "attached" and not (host_group(gs, "devbox") or {}).get("why"), 20)
    check("AC-87 a new recipe that answers takes over the link", moved is not None, host_group(moved or [], "devbox"))
    edit(good)
    back = wait_layout(ws, lambda gs: "-p 2222" in ((host_group(gs, "devbox") or {}).get("attach") or "")
                       and (host_group(gs, "devbox") or {}).get("state") == "attached" and (host_group(gs, "devbox") or {}).get("items"), 20)
    check("AC-87 and back to the first recipe", back is not None, host_group(back or [], "devbox"))
    item = (host_group(back or [], "devbox") or {"items": [{}]})["items"][0]
    ws.send({"t": "sub", "id": item.get("id")})
    ws.send({"t": "size", "cols": 90, "rows": 25, "typing": False})


def main():
    t = Terminumd(PORT, LOCAL_SOCKET)
    remote_home = os.path.join(t.root, "remote-home")
    os.makedirs(remote_home)
    open(os.path.join(remote_home, "notes.md"), "w").write("# on the host\n")
    # a host's pictures: a PNG, and an SVG that holds script (AC-09: it must never run in terminum's origin)
    png = b"\x89PNG\r\n\x1a\n" + bytes(range(64))          # its bytes come back as they are
    open(os.path.join(remote_home, "shot.png"), "wb").write(png)
    open(os.path.join(remote_home, "mark.svg"), "w").write('<svg xmlns="http://www.w3.org/2000/svg"><script>steal()</script></svg>')
    pids = os.path.join(t.root, "ssh-pids")
    t.env.update(TM_SSH=os.path.join(HERE, "fake_ssh.py"), FAKE_SSH_HOME=remote_home, FAKE_SSH_TMUX_SOCKET=REMOTE_SOCKET,
                 FAKE_SSH_PIDS=pids, FAKE_SSH_FAIL="Permission denied (publickey).", FAKE_SSH_FAIL_DEST="refused")
    os.system(f"tmux -L {REMOTE_SOCKET} kill-server 2>/dev/null")
    try:
        t.start()
        secret = t.signin()
        ws = t.page(secret)
        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "bad", "recipe": "alex@h -o ProxyCommand=x"})
        check("AC-87 an option not allowed is refused", st == 409 and "option not allowed: -o" in body["message"], body)
        t0 = time.time()
        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "", "recipe": "ssh alex@devbox.example -p 2222"})
        check("AC-81 a host is added by its ssh string; no name: its destination's host", st == 200 and body.get("name") == "devbox", body)
        groups = wait_layout(ws, lambda gs: (g := host_group(gs, "devbox")) is not None and g["items"], 10)
        check("AC-81 its terminum session is in the list within 5 s", groups and time.time() - t0 < 5, f"{time.time() - t0:.1f} s")
        g = host_group(groups or [], "devbox") or {"items": [{}]}
        item = g["items"][0]
        check("AC-81 the group and its rows name the host", g.get("label") == "devbox" and item.get("host") == "devbox"
              and item.get("id", "").startswith("tmux:devbox:default:%"), item.get("id"))
        os.system(f"tmux -L {REMOTE_SOCKET} new-session -d -s other -n theirs")   # the host's own session
        last, end = groups, time.time() + 2.5   # the newest layout within that time (one alike is not sent again)
        while time.time() < end:
            last = wait_layout(ws, lambda gs: True, max(0.1, end - time.time())) or last
        groups = last or []
        devbox = [g for g in groups if g.get("host") == "devbox"]
        check("AC-81 only the session terminum of the host is listed, with how to attach to it",
              len(devbox) == 1 and devbox[0].get("session") == "terminum" and not any(i.get("title") == "theirs" for i in devbox[0]["items"])
              and devbox[0].get("attach") == "ssh -t -p 2222 -- alex@devbox.example \"tmux attach -t '=terminum'\"", devbox)
        recs = unix_request(t.sock, "GET", "/hosts")[1]["hosts"]
        check("AC-87 the record keeps the checked recipe", recs and recs[0]["ssh"] == ["-p", "2222", "alex@devbox.example"], recs)

        ws.send({"t": "sub", "id": item["id"]})
        ws.send({"t": "size", "cols": 90, "rows": 25, "typing": False})
        check("AC-81 a host's pane shows its screen", ws.until(lambda m: m["t"] == "screen", 10) is not None)
        ws.send({"t": "in", "data": "echo remote-hello-$((6*7))\r"})
        echo = ws.until(lambda m: m["t"] == "screen" and "remote-hello-42" in json.dumps(m["ch"]), 10)
        check("AC-81 typing reaches the host's pane", echo is not None)

        # the link drops: the control client's ssh ends
        for line in open(pids):
            pid, _, rest = line.partition(" ")
            if "new-session" in rest:
                try:
                    os.kill(int(pid), signal.SIGTERM)
                except ProcessLookupError:
                    pass
        lost = wait_layout(ws, lambda gs: (g := host_group(gs, "devbox")) is not None and g.get("state") in ("reconnecting", "failed"), 10)
        check("AC-81 a dropped link says reconnecting", lost is not None, (host_group(lost or [], "devbox") or {}).get("why"))
        back = wait_layout(ws, lambda gs: (g := host_group(gs, "devbox")) is not None and g.get("state") == "attached" and g["items"], 15)
        check("AC-81 the host comes back by itself", back is not None)
        ws.send({"t": "in", "data": "echo again-$((2+3))\r"})
        check("AC-81 typing works again after the reconnect",
              ws.until(lambda m: m["t"] == "screen" and "again-5" in json.dumps(m["ch"]), 10) is not None)
        ws.send({"t": "paste", "data": "-x pasted-on-host", "bracket": False})
        pasted = ws.until(lambda m: m["t"] == "screen" and "-x pasted-on-host" in json.dumps(m["ch"]), 10)
        err = ws.until(lambda m: m["t"] == "error" and "paste" in m.get("msg", ""), 0.5)
        check("AC-03 a paste on a host that starts with \"-\" is text (set-buffer --), not a flag", pasted is not None and err is None, err)
        ws.send({"t": "in", "data": "\x03"})
        recipe_edits(t, ws, pids)

        # AC-15: the host's tmux started anew (a reboot, its server killed): its session's windows come back
        os.makedirs(os.path.join(remote_home, "build"), exist_ok=True)
        os.system(f"tmux -L {REMOTE_SOCKET} new-window -d -t terminum: -n build -c {os.path.join(remote_home, 'build')}")
        layout_json, kept = os.path.join(t.app, "layout.json"), {}
        for _ in range(40):
            time.sleep(0.5)
            try:
                kept = json.load(open(layout_json)).get("servers", {}).get("devbox:default", {})
            except (OSError, ValueError):
                kept = {}
            if any(w["name"] == "build" for s in kept.get("sessions", []) for w in s["windows"]):
                break
        check("AC-15 a host's session is kept in the checkpoint, its windows and folders", any(w["name"] == "build" for s in kept.get("sessions", []) for w in s["windows"]), kept)
        time.sleep(1.2)                                   # its session made after the checkpoint: a second later at least
        os.system(f"tmux -L {REMOTE_SOCKET} kill-server")
        made = ""
        for _ in range(60):
            time.sleep(0.5)
            made = subprocess.run(["tmux", "-L", REMOTE_SOCKET, "list-windows", "-t", "terminum", "-F", "#{window_name} #{pane_current_path}"], capture_output=True, text=True).stdout
            if "build" in made:
                break
        check("AC-15 the host's tmux killed: once it is attached again its windows are back in their folders",
              any(line.startswith("build ") and line.endswith("/remote-home/build") for line in made.splitlines()), made)
        # its session closed while its tmux runs (the user closed the last window; another session keeps the
        # server up, as the user's own do): made anew, empty, nothing restored
        os.system(f"tmux -L {REMOTE_SOCKET} new-session -d -s keep")
        time.sleep(1.5)
        os.system(f"tmux -L {REMOTE_SOCKET} kill-session -t terminum")
        again = ""
        for _ in range(30):
            time.sleep(0.5)
            again = subprocess.run(["tmux", "-L", REMOTE_SOCKET, "list-windows", "-t", "terminum", "-F", "#{window_name}"], capture_output=True, text=True).stdout
            if again.strip():
                break
        time.sleep(3)
        again = subprocess.run(["tmux", "-L", REMOTE_SOCKET, "list-windows", "-t", "terminum", "-F", "#{window_name}"], capture_output=True, text=True).stdout
        check("AC-15 the host's session closed while its tmux runs is made anew without the closed windows (no restore)",
              again.strip() and "build" not in again, again)
        os.system(f"tmux -L {REMOTE_SOCKET} kill-session -t keep")

        # its last pane closed with no other session: its tmux ends with it, and the fresh session is
        # not filled with what the user closed (the window "Restored host … 1 window" brought back)
        def windows():
            return subprocess.run(["tmux", "-L", REMOTE_SOCKET, "list-windows", "-t", "terminum", "-F", "#{window_id} #{window_name}"],
                                  capture_output=True, text=True).stdout.split("\n")
        os.system(f"tmux -L {REMOTE_SOCKET} new-window -d -t terminum: -n solo -c {os.path.join(remote_home, 'build')}")
        for line in windows():
            if line and not line.endswith(" solo"):
                os.system(f"tmux -L {REMOTE_SOCKET} kill-window -t {line.split()[0]}")
        for _ in range(40):
            time.sleep(0.5)
            try:
                kept = json.load(open(layout_json)).get("servers", {}).get("devbox:default", {})
            except (OSError, ValueError):
                kept = {}
            if [w["name"] for s in kept.get("sessions", []) for w in s["windows"]] == ["solo"]:
                break
        time.sleep(1.2)
        ws.until(lambda m: False, 1.5)                    # what came before is not this restore
        os.system(f"tmux -L {REMOTE_SOCKET} kill-window -t terminum:solo")
        after = []
        for _ in range(30):
            time.sleep(0.5)
            after = [w for w in windows() if w]
            if after:
                break
        time.sleep(3)
        after = [w for w in windows() if w]
        report = ws.until(lambda m: m["t"] == "restore" and m.get("report") and "devbox" in m["report"]["text"], 1)
        check("AC-15 the last pane closed ends the host's tmux: made anew with one shell, nothing restored",
              len(after) == 1 and not after[0].endswith(" solo") and report is None, (after, report))

        st, _ = unix_request(t.sock, "POST", "/hosts/add", {"name": "refused", "recipe": "alex@refused.example"})
        failed = wait_layout(ws, lambda gs: (g := host_group(gs, "refused")) is not None and g.get("state") == "failed", 10)
        why = (host_group(failed or [], "refused") or {}).get("why", "")
        check("AC-81 an ssh that fails says why, and offers no new window (no tmux to make it in)",
              "Permission denied (publickey)" in why and (host_group(failed or [], "refused") or {}).get("new") is None, host_group(failed or [], "refused"))
        unix_request(t.sock, "POST", "/hosts/remove", {"name": "refused"})

        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "", "recipe": "alex@devbox.example -p 2200"})
        check("AC-81 a name made that another host has is refused", st == 409 and "devbox is already a host" in body["message"], body)

        # AC-89: a second host that is the same machine lists none of its panes twice
        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "twin", "recipe": "alex@devbox.example -p 2222"})
        twin = wait_layout(ws, lambda gs: "shown there" in ((host_group(gs, "twin") or {}).get("why") or ""), 20)
        tg, dg = host_group(twin or [], "twin") or {}, host_group(twin or [], "devbox") or {}
        check("AC-89 a host that is the same tmux as another lists its panes once, and says where",
              st == 200 and tg.get("items") == [] and "the same tmux as devbox" in tg.get("why", "") and dg.get("items"), (st, tg, dg.get("items")))
        unix_request(t.sock, "POST", "/hosts/remove", {"name": "twin"})

        # AC-83: files ticked in Add host are set up once the host is attached; a failure asks again
        def offer_until(pred):
            remote = {}
            for _ in range(80):
                remote = (api(t, secret, "/fb/api/state?host=devbox")[1] or {}).get("remote") or {}
                if pred(remote):
                    break
                time.sleep(0.25)
            return remote
        unix_request(t.sock, "POST", "/hosts/remove", {"name": "devbox"})
        blocker = os.path.join(remote_home, ".terminum")
        shutil.rmtree(blocker, ignore_errors=True)
        open(blocker, "w").close()   # a file where the helper's folder goes: the copy fails
        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "", "recipe": "alex@devbox.example -p 2222", "files": True})
        remote = offer_until(lambda r: r.get("why"))
        rec = next((h for h in unix_request(t.sock, "GET", "/hosts")[1]["hosts"] if h["name"] == "devbox"), {})
        check("AC-83 a ticked setup that fails clears the tick; Files asks again with the reason",
              st == 200 and remote.get("state") == "ask" and "Could not enable devbox" in remote.get("why", "") and rec.get("files") is None, (remote, rec))
        os.remove(blocker)
        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "", "recipe": "alex@devbox.example -p 2222", "files": True})
        remote = offer_until(lambda r: r.get("state") == "up")
        rec = next((h for h in unix_request(t.sock, "GET", "/hosts")[1]["hosts"] if h["name"] == "devbox"), {})
        check("AC-83 files ticked in Add host: the helper is copied and files are enabled",
              st == 200 and remote.get("state") == "up" and rec.get("files") == "enabled"
              and os.path.exists(os.path.join(remote_home, ".terminum/bin/terminumd-agent")), (remote, rec))
        listing = None
        for _ in range(60):
            st, listing = api(t, secret, f"/fb/api/ls?path={remote_home}", host="devbox")
            if st == 200:
                break
            time.sleep(0.25)
        names = [e["n"] for e in (listing or {}).get("entries", [])] if st == 200 else listing
        check("AC-82 a host's folder is listed through its agent", st == 200 and "notes.md" in names, names)
        st, body = api(t, secret, f"/fb/api/file?path={remote_home}/notes.md", host="devbox")
        check("AC-82 a host's file is read through its agent", st == 200 and body.get("text") == "# on the host\n", body)
        for name, want in (("shot.png", "image/png"), ("mark.svg", "image/svg+xml")):
            got = raw(t, secret, f"/fb/api/raw?path={remote_home}/{name}", host="devbox")
            check(f"AC-82 a host's picture {name} is read through its agent with the sign-in in the header, typed by terminumd from its name under the sandbox policy",
                  got[0] == 200 and got[1] == want and "sandbox" in got[2] and got[3] == open(os.path.join(remote_home, name), "rb").read(), got[:3])
        got = raw(t, None, f"/fb/api/raw?path={remote_home}/shot.png&host=devbox&t={secret}")
        check("AC-09 a host's picture is not read with the sign-in in its address", got[0] == 401, got[:3])
        st, body = api(t, secret, "/fb/api/os/reveal", host="devbox", method="POST", body=json.dumps({"path": remote_home}).encode(),
                       headers={"Content-Type": "application/json"})
        check("AC-82 Finder actions are refused for remote files", st == 400 and "remote" in json.dumps(body).lower(), body)
        png = b"\x89PNG\r\n\x1a\n" + b"\0" * 32
        st, body = api(t, secret, "/paste?sid=" + urllib.request.quote(item["id"]), method="POST", body=png,
                       headers={"Content-Type": "image/png", "Origin": f"http://127.0.0.1:{PORT}"})
        path = (body or {}).get("path", "")
        check("AC-85 a pasted image is saved on the host", st == 200 and path.startswith(remote_home + "/.terminum/tmp/") and os.path.exists(path), body)
        # a file dropped on a host's tree (AC-95): its bytes saved there through the agent, never over a file
        blob, drop = bytes(range(256)) * 64, "/fb/api/fs/upload?dir=" + urllib.request.quote(remote_home) + "&name=dropped.bin"
        hdr = {"Content-Type": "application/octet-stream", "Origin": f"http://127.0.0.1:{PORT}"}
        first = api(t, secret, drop, host="devbox", method="POST", body=blob, headers=hdr)
        again = api(t, secret, drop, host="devbox", method="POST", body=b"other", headers=hdr)
        both = api(t, secret, drop + "&both=true", host="devbox", method="POST", body=b"other", headers=hdr)
        on_host = lambda n: open(os.path.join(remote_home, n), "rb").read() if os.path.exists(os.path.join(remote_home, n)) else None
        check("AC-95 a file dropped on a host's tree is saved there through its agent; its name taken is refused, Keep Both saves \"dropped 2.bin\"",
              first[0] == 201 and (first[1] or {}).get("path") == f"{remote_home}/dropped.bin" and on_host("dropped.bin") == blob
              and again[0] == 409 and (again[1] or {}).get("error") == "exists"
              and both[0] == 201 and (both[1] or {}).get("path") == f"{remote_home}/dropped 2.bin" and on_host("dropped 2.bin") == b"other", (first, again, both))

        # a host set up before the server was named terminumd has only terminusd-agent: it is
        # started, answers with its old build, and is replaced by this build's helper (AC-84)
        t.stop()
        bin_dir = os.path.join(remote_home, ".terminum/bin")
        for f in os.listdir(bin_dir):
            os.remove(os.path.join(bin_dir, f))
        with open(os.path.join(bin_dir, "terminusd-agent-0123456789ab"), "w") as f:
            f.write("#!/bin/sh\necho terminusd-agent ready 0123456789ab h u darwin-aarch64\nexec cat >/dev/null\n")
        os.chmod(os.path.join(bin_dir, "terminusd-agent-0123456789ab"), 0o755)
        os.symlink("terminusd-agent-0123456789ab", os.path.join(bin_dir, "terminusd-agent"))
        t.start()
        ws = t.page(secret)
        remote = offer_until(lambda r: r.get("state") == "up")
        left = sorted(os.listdir(bin_dir))
        check("AC-84 a helper of the old name is replaced by terminumd-agent, its files removed",
              remote.get("state") == "up" and "terminumd-agent" in left and not any(n.startswith("terminusd-agent") for n in left), (remote, left))
        st, body = unix_request(t.sock, "POST", "/hosts/files-remove", {"name": "devbox"})
        check("AC-83 the helper is removed from the host", st == 200 and not os.path.exists(os.path.join(remote_home, ".terminum/bin/terminumd-agent")), body)
        st, body = unix_request(t.sock, "POST", "/hosts/remove", {"name": "devbox"})
        gone = wait_layout(ws, lambda gs: host_group(gs, "devbox") is None, 10)
        check("AC-87 a removed host leaves the list", st == 200 and gone is not None)
    finally:
        if not all(results):
            print(t.log())
        t.cleanup()
        os.system(f"tmux -L {REMOTE_SOCKET} kill-server 2>/dev/null")
    summary()


if __name__ == "__main__":
    main()
