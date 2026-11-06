#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Remote hosts over real ssh (terminum-remote.md AC-81–AC-84): a Linux container with sshd
and tmux on 127.0.0.1:2222 (Docker), a throwaway key, and a private terminumd whose build
carries the Linux helper. Checks: the host's session over ssh, typing, a dropped link,
enabling files (the static Linux helper copied and listing), and AC-84: a helper of another
build is replaced by this build's at terminumd's next start, and the page is told.

    python3 scripts/e2e_remote_sshd.py MAC_TERMINUMD LINUX_HELPER

Both binaries must carry the same agent id (`scripts/agents.py build macos-aarch64
linux-aarch64`). Needs Docker; pulls alpine:3.20 and its openssh and tmux packages. The
user's ssh config, agent and known_hosts are not used.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_remote import api, host_group  # noqa: E402
from e2e_ws import Terminumd, check, results, summary, unix_request, wait_layout  # noqa: E402
from testrun import free_port, own, short_tmp  # noqa: E402

PORT, SOCKET, NAME, SSH_PORT = free_port(), own("tm-test-rsshd"), "tm-test-sshd", 2222
HOME = "/home/alex"
OLD = "0123456789ab"


def docker(*args, check_ok=True):
    r = subprocess.run(["docker", *args], capture_output=True, text=True)
    if check_ok and r.returncode != 0:
        raise RuntimeError(f"docker {args[0]}: {r.stderr.strip()}")
    return r.stdout


def on_host(cmd):
    return docker("exec", "-u", "alex", NAME, "sh", "-c", cmd, check_ok=False)


def start_host(work):
    subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "tm-test", "-f", f"{work}/key"], check=True)
    pub = open(f"{work}/key.pub").read().strip()
    # TM_SSHD_BUSYBOX=1: the host's ps is busybox's (no -U, no -p): the orphans are found through /proc
    procps = "" if os.environ.get("TM_SSHD_BUSYBOX") == "1" else " procps"
    setup = (f"apk add --no-cache openssh tmux{procps} >/dev/null && ssh-keygen -A && adduser -D -s /bin/sh alex && "
             "echo 'alex:*' | chpasswd -e && mkdir -p /home/alex/.ssh && "
             f"echo '{pub}' > /home/alex/.ssh/authorized_keys && chown -R alex /home/alex/.ssh && "
             "chmod 700 /home/alex/.ssh && chmod 600 /home/alex/.ssh/authorized_keys && "
             "echo '# on the host' > /home/alex/notes.md && chown alex /home/alex/notes.md && "
             # Alpine ships `AllowTcpForwarding no`, which alone denies a Unix -R, and no
             # AllowStreamLocalForwarding line for a sed to change (AS-84): set the one, add the other
             "sed -i 's/^#*AllowTcpForwarding.*/AllowTcpForwarding yes/; /^#*AllowStreamLocalForwarding/d' /etc/ssh/sshd_config && "
             "echo 'AllowStreamLocalForwarding yes' >> /etc/ssh/sshd_config && "
             "exec /usr/sbin/sshd -D -e")
    docker("rm", "-f", NAME, check_ok=False)
    docker("run", "-d", "--rm", "--name", NAME, "-p", f"127.0.0.1:{SSH_PORT}:22", "alpine:3.20", "sh", "-c", setup)
    # ssh without the user's config, agent or known_hosts
    wrapper = f"{work}/ssh"
    with open(wrapper, "w") as f:
        f.write("#!/bin/sh\nexec /usr/bin/ssh -F /dev/null -o UserKnownHostsFile=" + work + "/kh "
                "-o StrictHostKeyChecking=accept-new -o IdentitiesOnly=yes -o IdentityAgent=none \"$@\"\n")
    os.chmod(wrapper, 0o755)
    for _ in range(60):
        r = subprocess.run([wrapper, "-o", "BatchMode=yes", "-p", str(SSH_PORT), "-i", f"{work}/key", "alex@127.0.0.1", "tmux -V"],
                           capture_output=True, text=True)
        if r.returncode == 0:
            return wrapper
        time.sleep(2)
    raise RuntimeError("the container's sshd did not answer: " + r.stderr)


def stage(work, mac, linux):
    """A build folder as the package lays it out: terminumd and agents/<platform>/terminumd."""
    d = f"{work}/build"
    os.makedirs(f"{d}/agents/linux-aarch64")
    shutil.copy2(mac, f"{d}/terminumd")
    shutil.copy2(linux, f"{d}/agents/linux-aarch64/terminumd")
    return f"{d}/terminumd"


def files_up(t, secret, timeout=40):
    end = time.time() + timeout
    st, body = 0, None
    while time.time() < end:
        st, body = api(t, secret, f"/fb/api/ls?path={HOME}", host="devbox")
        if st == 200:
            return [e["n"] for e in body.get("entries", [])]
        time.sleep(0.5)
    return body


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__.split("\n\n")[1])
    work = tempfile.mkdtemp(prefix="tmr-", dir=short_tmp())
    t = None
    try:
        wrapper = start_host(work)
        import e2e_ws
        e2e_ws.BIN = stage(work, sys.argv[1], sys.argv[2])
        t = Terminumd(PORT, SOCKET, env={"TM_SSH": wrapper})
        t.start()
        own = subprocess.run([e2e_ws.BIN, "--agent-id"], capture_output=True, text=True).stdout.strip()
        secret = t.signin()
        ws = t.page(secret)
        t0 = time.time()
        st, body = unix_request(t.sock, "POST", "/hosts/add", {"name": "devbox", "recipe": f"alex@127.0.0.1 -p {SSH_PORT} -i {work}/key"})
        groups = wait_layout(ws, lambda gs: (g := host_group(gs, "devbox")) is not None and g["items"], 15)
        check("AC-81 over real ssh the host's session is listed within 5 s", st == 200 and groups and time.time() - t0 < 5, f"{time.time() - t0:.1f} s")
        item = host_group(groups or [], "devbox")["items"][0]
        ws.send({"t": "sub", "id": item["id"]})
        ws.send({"t": "size", "cols": 90, "rows": 25, "typing": False})
        ws.until(lambda m: m["t"] == "screen", 10)
        ws.send({"t": "in", "data": "echo on-$(uname -s)-$((6*7))\r"})
        check("AC-81 typing reaches the Linux host's pane",
              ws.until(lambda m: m["t"] == "screen" and "on-Linux-42" in json.dumps(m["ch"]), 10) is not None)

        # AC-03 edge: the host's tmux has extended-keys off, so Shift+Enter goes as its bytes
        ws.send({"t": "in", "data": "printf '\\033[>4;2m'; stty raw -echo; head -c 7 > /tmp/got; stty sane\r"})
        time.sleep(1.0)
        ws.send({"t": "in", "data": "\x1b[13;2u"})
        time.sleep(1.0)
        got = on_host("od -An -c /tmp/got | tr -s ' '").strip()
        check("AC-03 Shift+Enter reaches a program on a host whose tmux has extended-keys off", got == "033 [ 1 3 ; 2 u", got)

        # a client of terminum's shape whose parent went, as a link that dropped while the host stayed up leaves one
        on_host("(sleep 60 | tmux -u -C attach-session -E -f ignore-size -t terminum >/dev/null 2>&1) &")
        time.sleep(0.5)
        planted = on_host("pgrep -f '^tmux -u -C attach-session'").split()
        server = on_host("tmux display -p '#{pid}'").strip()
        docker("exec", NAME, "pkill", "-f", "sshd-session: alex|sshd: alex", check_ok=False)
        lost = wait_layout(ws, lambda gs: (g := host_group(gs, "devbox")) is not None and g.get("state") in ("reconnecting", "failed"), 10)
        back = wait_layout(ws, lambda gs: (g := host_group(gs, "devbox")) is not None and g.get("state") == "attached" and g["items"], 20)
        check("AC-81 a dropped ssh link reconnects by itself", lost is not None and back is not None)
        # the link dropped: terminum's control clients it orphaned on the host are ended (tmux 3.4)
        alive = lambda: planted and on_host(f"[ -d /proc/{planted[0]} ] && echo {planted[0]} || true").strip()
        procs = "ps -e -o pid,ppid,comm,args 2>/dev/null || ps -o pid,ppid,comm,args"
        for _ in range(40):
            if not alive():
                break
            time.sleep(0.25)
        for _ in range(40):
            if logged := [line for line in open(t.log_path).read().splitlines() if "clients.orphans" in line]:
                break
            time.sleep(0.25)
        check("AC-81 the control client the dropped link orphaned is ended, and logged", len(planted) == 1 and not alive()
              and any(f"ended=[{planted[0]}]" in line for line in logged), [planted, logged, on_host(procs)])
        check("AC-81 the host's tmux server, started by terminum's own command, is never taken for one",
              server and on_host("tmux display -p '#{pid}'").strip() == server, [server, on_host(procs)])

        st, body = unix_request(t.sock, "POST", "/hosts/enable", {"name": "devbox"})
        copied = on_host(f"test -x $HOME/.terminum/bin/terminumd-agent-{own} && echo yes").strip()
        check("AC-83 enabling copies the static Linux helper", st == 200 and copied == "yes", body)
        names = files_up(t, secret)
        check("AC-82 the host's home is listed through the Linux helper", isinstance(names, list) and "notes.md" in names, names)
        st, body = api(t, secret, f"/fb/api/file?path={HOME}/notes.md", host="devbox")
        check("AC-82 a host's file is read through the helper", st == 200 and body.get("text") == "# on the host\n", body)

        # AC-84: the host's helper is of another build (a stand-in that says an old id)
        t.stop()
        on_host(f"cd $HOME/.terminum/bin && rm -f terminumd-agent-{own} && "
                f"printf '#!/bin/sh\\necho terminumd-agent ready {OLD} h u linux-aarch64\\nexec cat >/dev/null\\n' > terminumd-agent-{OLD} && "
                f"chmod 755 terminumd-agent-{OLD} && ln -sfn terminumd-agent-{OLD} terminumd-agent")
        t0 = time.time()
        t.start()
        names = files_up(t, secret, 60)
        took = time.time() - t0
        current = on_host(f"readlink $HOME/.terminum/bin/terminumd-agent; test -x $HOME/.terminum/bin/terminumd-agent-{own} && echo ok").split()
        check("AC-84 at the next start the helper is replaced by this build's within 30 s",
              isinstance(names, list) and "notes.md" in names and current == [f"terminumd-agent-{own}", "ok"] and took < 30, (f"{took:.1f} s", current))
        st, state = api(t, secret, "/fb/api/state?host=devbox")
        check("AC-84 the panel is told the helper was updated", st == 200 and state["remote"].get("updated") == state["build"], state.get("remote"))
        rec = next(h for h in unix_request(t.sock, "GET", "/hosts")[1]["hosts"] if h["name"] == "devbox")
        check("AC-84 the host's record names the helper's build", rec.get("helper") == own, rec.get("helper"))

        st, body = unix_request(t.sock, "POST", "/hosts/files-remove", {"name": "devbox"})
        left = on_host("ls $HOME/.terminum/bin 2>/dev/null | grep terminumd-agent || echo none").strip()
        check("AC-83 the helper is removed from the host", st == 200 and left == "none", left)
    finally:
        if t:
            if not all(results):
                print(t.log())
            t.cleanup()
        docker("rm", "-f", NAME, check_ok=False)
        shutil.rmtree(work, ignore_errors=True)
    summary()


if __name__ == "__main__":
    main()
