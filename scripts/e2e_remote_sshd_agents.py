#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Coding agents' progress on a host over real ssh (terminum-remote.md AC-99, AS-84): a Linux
container with sshd and tmux (Docker), a throwaway key, a private terminumd whose build carries
the Linux helper. On the host a stub `claude` under ~/.nvm (it records its argv) and a ~/.codex
folder; no real agent runs. Checks: enabling agents (per-agent lines, the link up, the host's
files), `terminum-host hook claude` and `terminum-host mcp` in the host's pane reaching the
Mac's rows, a refused forward (a second sshd with `AllowStreamLocalForwarding local`, user alexb), a lost
link (its sshd session ended in the container), and disabling.

    python3 scripts/e2e_remote_sshd_agents.py MAC_TERMINUMD LINUX_HELPER [alpine|debian]

Both binaries carry the same agent id (`scripts/agents.py build macos-aarch64 linux-aarch64`).
debian: debian:12-slim, bash login shell, a ~/.bashrc that echoes a line at its top (AS-84 f).
"""
import json
import os
import shlex
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_remote import api, host_group  # noqa: E402
from e2e_remote_sshd import host_or_skip, stage  # noqa: E402
from e2e_ws import Terminumd, check, results, summary, unix_request, wait_layout  # noqa: E402
from testrun import container_name, free_port, own, short_tmp  # noqa: E402

VARIANT = sys.argv[3] if len(sys.argv) > 3 else "alpine"
PORT, SOCKET = (free_port(), own("tm-rr9-a")) if VARIANT == "alpine" else (free_port(), own("tm-rr9-d"))
NAME, SSH_PORT, BOB_PORT = container_name(f"rr9-{VARIANT}"), free_port(), free_port()

STUB = r"""#!/bin/sh
echo "$*" >> "$HOME/claude-argv.log"
if [ "$1 $2" = "plugin list" ] && grep -q '^plugin install terminum@terminum-local' "$HOME/claude-argv.log"; then
  printf 'Installed plugins:\n\n  > terminum@terminum-local\n    Version: 1\n    Scope: user\n    Status: enabled\n'
fi
exit 0
"""


def docker(*args, check_ok=True):
    r = subprocess.run(["docker", *args], capture_output=True, text=True)
    if check_ok and r.returncode != 0:
        raise RuntimeError(f"docker {args[0]}: {r.stderr.strip()}")
    return r.stdout


def on_host(cmd, user="alex"):
    return docker("exec", "-u", user, NAME, "sh", "-c", cmd, check_ok=False)


def setup_script(pub):
    users = ("adduser -D -s /bin/sh alex && adduser -D -s /bin/sh alexb && echo 'alex:*' | chpasswd -e && echo 'alexb:*' | chpasswd -e"
             if VARIANT == "alpine" else "useradd -m -s /bin/bash alex && useradd -m -s /bin/bash alexb && usermod -p '*' alex && usermod -p '*' alexb")
    pkgs = ("apk add --no-cache openssh tmux procps >/dev/null && ssh-keygen -A" if VARIANT == "alpine" else
            "apt-get update -qq >/dev/null && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq openssh-server tmux procps >/dev/null && mkdir -p /run/sshd")
    keys = " && ".join(f"mkdir -p /home/{u}/.ssh && echo '{pub}' > /home/{u}/.ssh/authorized_keys && chmod 700 /home/{u}/.ssh && "
                       f"chmod 600 /home/{u}/.ssh/authorized_keys && echo '# on the host' > /home/{u}/notes.md && chown -R {u} /home/{u}"
                       for u in ("alex", "alexb"))
    rc = ("" if VARIANT == "alpine" else
          " && sed -i '1i echo bashrc-says-hi' /home/alex/.bashrc /home/alexb/.bashrc && echo 'echo profile-says-hi' >> /home/alex/.profile")
    sshd = "/usr/sbin/sshd"
    return (f"{pkgs} && {users} && {keys}{rc} && "
            "sed -i 's/^#*AllowTcpForwarding.*/AllowTcpForwarding yes/; /^#*AllowStreamLocalForwarding/d' /etc/ssh/sshd_config && "
            "echo 'AllowStreamLocalForwarding yes' >> /etc/ssh/sshd_config && "
            # the refusing sshd: alexb only, -R refused, -L kept (AS-84 a); files ride -L, so
            # `DisableForwarding yes` would refuse files too
            "grep -v '^Match\\|^ *ForceCommand\\|^AllowStreamLocalForwarding' /etc/ssh/sshd_config > /etc/ssh/sshd_refuse && "
            "echo 'AllowStreamLocalForwarding local' >> /etc/ssh/sshd_refuse && echo 'AllowUsers alexb' >> /etc/ssh/sshd_refuse && "
            f"echo 'AllowUsers alex' >> /etc/ssh/sshd_config && "
            f"{sshd} -f /etc/ssh/sshd_refuse -p 2200 -E /tmp/refuse.log && exec {sshd} -D -e")


def start_host(work):
    subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "tm-test", "-f", f"{work}/key"], check=True)
    pub = open(f"{work}/key.pub").read().strip()
    image = "alpine:3.20" if VARIANT == "alpine" else "debian:12-slim"
    docker("rm", "-f", NAME, check_ok=False)
    docker("run", "-d", "--rm", "--name", NAME, "-p", f"127.0.0.1:{SSH_PORT}:22", "-p", f"127.0.0.1:{BOB_PORT}:2200", image, "sh", "-c", setup_script(pub))
    wrapper = f"{work}/ssh"
    with open(wrapper, "w") as f:
        f.write("#!/bin/sh\nexec /usr/bin/ssh -F /dev/null -o UserKnownHostsFile=" + work + "/kh "
                "-o StrictHostKeyChecking=accept-new -o IdentitiesOnly=yes -o IdentityAgent=none \"$@\"\n")
    os.chmod(wrapper, 0o755)
    r = None
    for _ in range(120):
        r = subprocess.run([wrapper, "-o", "BatchMode=yes", "-p", str(BOB_PORT), "-i", f"{work}/key", "alexb@127.0.0.1", "tmux -V"],
                           capture_output=True, text=True)
        if r.returncode == 0:
            break
        time.sleep(2)
    else:
        raise RuntimeError("the container's sshd did not answer: " + r.stderr)
    stub_dir = "$HOME/.nvm/versions/node/v22.0.0/bin"
    for u in ("alex", "alexb"):
        subprocess.run(["docker", "exec", "-i", "-u", u, NAME, "sh", "-c", f"mkdir -p {stub_dir} $HOME/.codex && cat > {stub_dir}/claude && chmod 755 {stub_dir}/claude"],
                       input=STUB, text=True, check=True)
    return wrapper


def item_of(ws, host):
    gs = wait_layout(ws, lambda gs: (g := host_group(gs, host)) is not None and g.get("state") == "attached" and g["items"], 20)
    return host_group(gs or [], host)["items"][0] if gs else None


def row_until(ws, host, pred, timeout=10):
    gs = wait_layout(ws, lambda gs: (g := host_group(gs, host)) is not None and g["items"] and pred(g["items"][0]), timeout)
    return host_group(gs, host)["items"][0] if gs else None


def record(t, host):
    return next((h for h in unix_request(t.sock, "GET", "/hosts")[1]["hosts"] if h["name"] == host), {})


def state_until(t, host, pred, timeout=40):
    end, rec = time.time() + timeout, {}
    while time.time() < end:
        rec = record(t, host)
        if pred(rec.get("agents", "")):
            return rec
        time.sleep(0.5)
    return rec


def in_pane(cmd, user="alex", timeout=20):
    """`cmd` typed into the host's pane (session terminum) by the container's tmux; its output."""
    on_host("rm -f $HOME/pane.out $HOME/pane.done", user)
    on_host(f"tmux send-keys -t terminum: {shlex.quote(f'({cmd}) > $HOME/pane.out 2>&1; echo $? > $HOME/pane.done')} Enter", user)
    end = time.time() + timeout
    while time.time() < end:
        if on_host("cat $HOME/pane.done 2>/dev/null", user).strip():
            return on_host("cat $HOME/pane.done", user).strip(), on_host("cat $HOME/pane.out", user)
        time.sleep(0.3)
    return None, on_host("cat $HOME/pane.out 2>/dev/null", user)


def events(cwd):
    base = {"session_id": "s-rr9", "cwd": cwd, "prompt_id": "p-rr9-1", "permission_mode": "default"}
    post = lambda name, inp, resp, n: {**base, "hook_event_name": "PostToolUse", "tool_name": name, "tool_input": inp,
                                       "tool_response": resp, "tool_use_id": f"toolu_rr9{n}", "duration_ms": 12}
    create = lambda i, s: post("TaskCreate", {"subject": s, "description": f"Task {s}"}, {"task": {"id": i, "subject": s}}, i)
    update = lambda i, a, b, n: post("TaskUpdate", {"taskId": i, "status": b}, {"success": True, "taskId": i, "updatedFields": ["status"],
                                                                                   "statusChange": {"from": a, "to": b}}, n)
    return [{**base, "hook_event_name": "UserPromptSubmit", "prompt": "plan the host check"}, create("1", "alpha"), create("2", "beta"),
            create("3", "gamma"), update("1", "pending", "in_progress", 4), update("1", "in_progress", "completed", 5),
            update("2", "pending", "in_progress", 6)]


def put_events(evs, user="alex", prefix="ev"):
    for i, e in enumerate(evs):
        subprocess.run(["docker", "exec", "-i", "-u", user, NAME, "sh", "-c", f"cat > $HOME/{prefix}{i}.json"], input=json.dumps(e), text=True, check=True)
    return f"for f in $HOME/{prefix}*.json; do ~/.terminum/bin/terminum-host hook claude < $f || echo hook-failed-$?; done"


RPC = [{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "rr9", "version": "1"}}},
       {"jsonrpc": "2.0", "method": "notifications/initialized"},
       {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "update_session_plan", "arguments": {
           "items": [{"text": "one", "status": "completed"}], "doing": "Checking the host", "eta_minutes": 30}}},
       {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "create_task", "arguments": {"title": "host task", "text": "A host task."}}},
       {"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "get_session_plan", "arguments": {}}}]


def run_links():
    return on_host("cd $HOME/.terminum/run 2>/dev/null && stat -c '%a' . && for f in *; do printf '%s ' \"$f\"; "
                   "[ -L \"$f\" ] && readlink \"$f\" || { [ -S \"$f\" ] && echo socket || echo other; }; done").strip()


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__.split("\n\n")[1])
    work = tempfile.mkdtemp(prefix="tmr9-", dir=short_tmp())
    t = None
    try:
        wrapper = host_or_skip(start_host, work)
        import e2e_ws
        e2e_ws.BIN = stage(work, sys.argv[1], sys.argv[2])
        t = Terminumd(PORT, SOCKET, env={"TM_SSH": wrapper})
        t.start()
        secret = t.signin()
        ws = t.page(secret)
        v = VARIANT
        st, _ = unix_request(t.sock, "POST", "/hosts/add", {"name": "devbox", "recipe": f"alex@127.0.0.1 -p {SSH_PORT} -i {work}/key"})
        check(f"[{v}] the host attaches over real ssh", st == 200 and item_of(ws, "devbox") is not None)

        st, body = unix_request(t.sock, "POST", "/hosts/agents-on", {"name": "devbox"})
        lines = (body.get("message") or "").splitlines() if isinstance(body, dict) else []
        check(f"[{v}] AC-99 enabling answers one line per agent: claude and codex installed", st == 200 and len(lines) == 2
              and any(line.startswith("claude: installed") for line in lines) and any(line.startswith("codex: installed") for line in lines), body)
        rec = state_until(t, "devbox", lambda s: s == "up")
        check(f"[{v}] AC-99 the host's link comes up", rec.get("agents") == "up", rec)
        own = subprocess.run([e2e_ws.BIN, "--agent-id"], capture_output=True, text=True).stdout.strip()
        th = on_host("readlink $HOME/.terminum/bin/terminum-host").strip()
        check(f"[{v}] AC-99 ~/.terminum/bin/terminum-host is the helper", th.endswith(f"terminumd-agent-{own}") or th == "terminumd-agent", th)
        links = run_links()
        rows = dict(line.split(" ", 1) for line in links.splitlines()[1:] if " " in line)
        mac = [k for k in rows if k.startswith("mac-")]
        check(f"[{v}] AC-99 ~/.terminum/run is 0700 with mac-<id>.sock → its m-<id>-*.sock", links.splitlines()[:1] == ["700"] and len(mac) == 1
              and rows[mac[0]].split("/")[-1].startswith("m-" + mac[0][4:-5] + "-") and rows.get(rows[mac[0]].split("/")[-1]) == "socket", links)
        hooks = on_host("cat $HOME/.terminum/hooks/claude/plugins/terminum/hooks/hooks.json 2>/dev/null")
        argv = on_host("cat $HOME/claude-argv.log")
        try:
            batch = json.loads(hooks)["hooks"]["PostToolBatch"][0]["hooks"][0]
        except (ValueError, KeyError, IndexError, TypeError):
            batch = {}
        check(f"[{v}] AC-99 the plugin's files: a synchronous PostToolBatch entry running terminum-host; installed through the stub",
              "terminum-host" in batch.get("command", "") and not batch.get("async") and "plugin install terminum@terminum-local" in argv, [hooks[:300], argv])
        codex = on_host("cat $HOME/.codex/hooks.json $HOME/.codex/config.toml 2>/dev/null")
        check(f"[{v}] AC-99 Codex's block names terminum-host", "terminum-host" in codex and "[mcp_servers.terminum]" in codex, codex[:400])

        # the hook and MCP in the host's pane
        code, out = in_pane(put_events(events("/home/alex")) + "; echo hooks-done")
        check(f"[{v}] AC-99 `terminum-host hook claude` in the host's pane takes each event silently, exit 0", code == "0" and out.strip() == "hooks-done", [code, out])
        row = row_until(ws, "devbox", lambda it: (it.get("plan") or {}).get("total") == 3 and it["plan"].get("done") == 1)
        check(f"[{v}] AC-99 the host row's plan says 1 of 3", row is not None, row)
        subprocess.run(["docker", "exec", "-i", "-u", "alex", NAME, "sh", "-c", "cat > $HOME/rpc.jsonl"], input="\n".join(json.dumps(r) for r in RPC) + "\n", text=True, check=True)
        code, out = in_pane("~/.terminum/bin/terminum-host mcp < $HOME/rpc.jsonl")
        answers = {}
        for line in out.splitlines():
            try:
                a = json.loads(line)
                answers[a.get("id")] = a
            except ValueError:
                pass
        text = lambda i: "".join(c.get("text", "") for c in ((answers.get(i) or {}).get("result") or {}).get("content", []))
        check(f"[{v}] AC-99 MCP in the host's pane: initialize is terminum's, the plan's doing/ETA kept, the task made",
              (answers.get(1) or {}).get("result", {}).get("serverInfo", {}).get("name") == "terminum" and "ETA" in text(2)
              and "Checking the host" in text(4) and 3 in answers and not answers[3].get("error") and not answers[3]["result"].get("isError"), out[:1500])
        row = row_until(ws, "devbox", lambda it: it.get("plan", {}).get("eta"))
        db = sqlite3.connect(f"file:{os.path.join(t.app, 'kanban.sqlite')}?mode=ro", uri=True)
        try:
            tasks = db.execute("SELECT title, by_agent FROM tasks").fetchall()
        finally:
            db.close()
        check(f"[{v}] AC-99 the host row has the ETA, and the agent's task stands in the Mac's store, by an agent (terminum-kanban.md AC-120)", row is not None and ("host task", 1) in tasks, [row, tasks])

        # agents with files off, past the helper's 90 s rule (the longest wait here)
        log = lambda: on_host("cat $HOME/.terminum/logs/agent.log")   # noqa: E731
        starts, began = log().count('event="start"'), time.time()
        while time.time() - began < 95 and "no Mac connected" not in log() and record(t, "devbox").get("agents") == "up":
            time.sleep(1)
        check(f"[{v}] AC-99 with files off the helper and the link stay past 90 s over real ssh ({time.time() - began:.0f} s)",
              time.time() - began >= 95 and "no Mac connected" not in log() and log().count('event="start"') == starts, log()[-300:])

        # refused forward: alexb's sshd allows only local stream forwards
        st, _ = unix_request(t.sock, "POST", "/hosts/add", {"name": "refuser", "recipe": f"alexb@127.0.0.1 -p {BOB_PORT} -i {work}/key", "files": True})
        item_of(ws, "refuser")
        st, body = unix_request(t.sock, "POST", "/hosts/agents-on", {"name": "refuser"})
        rec = state_until(t, "refuser", lambda s: s.startswith("refused"))
        check(f"[{v}] AC-99 a refused forward: the host's agents state says refused…", st == 200 and rec.get("agents", "").startswith("refused"), [body, rec])
        for _ in range(60):
            st, lst = api(t, secret, "/fb/api/ls?path=/home/alexb", host="refuser")
            if st == 200:
                break
            time.sleep(0.5)
        check(f"[{v}] AC-99 a refused forward keeps files working", st == 200 and "notes.md" in [e["n"] for e in lst.get("entries", [])], lst)
        code, out = in_pane(put_events(events("/home/alexb")[:2], "alexb") + "; echo hooks-done", "alexb")
        check(f"[{v}] AC-99 with no link the hook exits 0 silently", code == "0" and out.strip() == "hooks-done", [code, out])

        # link loss: alex's sshd sessions ended inside this container
        before = run_links()
        docker("exec", NAME, "pkill", "-f", "sshd-session: alex|sshd: alex", check_ok=False)
        lost = state_until(t, "devbox", lambda s: s != "up", 10)
        back = state_until(t, "devbox", lambda s: s == "up", 60)
        check(f"[{v}] AC-99 the lost link comes back up", lost.get("agents") != "up" and back.get("agents") == "up", [lost, back])
        idle = row_until(ws, "devbox", lambda it: it.get("state") != "working" or it.get("agent") != "claude", 10)
        check(f"[{v}] AC-99 the lost link dropped the host's hook state (the row is no longer working)", idle is not None, idle)
        after = run_links()
        old_target = next((l.split(" ", 1)[1] for l in before.splitlines() if l.startswith("mac-")), "")
        new_target = next((l.split(" ", 1)[1] for l in after.splitlines() if l.startswith("mac-")), "")
        socks = [l.split(" ")[0] for l in after.splitlines() if l.endswith(" socket")]
        check(f"[{v}] AC-99 the old socket is swept, the link points at the new live one",
              old_target and new_target and old_target != new_target and old_target.split("/")[-1] not in socks and new_target.split("/")[-1] in socks, [before, after])
        upd = events("/home/alex")[0:1] + [dict(events("/home/alex")[6], tool_use_id="toolu_rr9x", tool_input={"taskId": "2", "status": "completed"},
                                                 tool_response={"success": True, "taskId": "2", "updatedFields": ["status"], "statusChange": {"from": "in_progress", "to": "completed"}})]
        code, out = in_pane(put_events(upd, "alex", "ev2-") + "; echo hooks-done")
        row = row_until(ws, "devbox", lambda it: (it.get("plan") or {}).get("done") == 2, 10)
        check(f"[{v}] AC-99 events flow again after the link came back (2 of 3)", code == "0" and row is not None, [code, out, row])

        # disable
        st, body = unix_request(t.sock, "POST", "/hosts/agents-off", {"name": "devbox"})
        left = on_host("ls $HOME/.terminum/bin/terminum-host 2>&1; grep -l terminum-host $HOME/.codex/hooks.json $HOME/.codex/config.toml 2>/dev/null; tail -2 $HOME/claude-argv.log")
        check(f"[{v}] AC-99 disabling removes terminum-host and the agents' entries", st == 200 and "No such file" in left and ".codex" not in left
              and "plugin uninstall terminum@terminum-local" in left, [body, left])
    finally:
        if t:
            if not all(results):
                print(t.log()[-6000:])
            t.cleanup()
        docker("rm", "-f", NAME, check_ok=False)
        shutil.rmtree(work, ignore_errors=True)
    summary()


if __name__ == "__main__":
    main()
