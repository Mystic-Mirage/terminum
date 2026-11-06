#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""A stand-in for ssh in the remote e2e checks (scripts/e2e_remote.py): this Mac plays the
host. It takes ssh's options, runs the remote command with `sh -c` in a fake home
(FAKE_SSH_HOME) and with a `tmux` that only reaches a private server (FAKE_SSH_TMUX_SOCKET),
forwards `-L <local socket>:<remote socket>` between Unix sockets, and can fail as a refused
key would (FAKE_SSH_FAIL, for destinations containing FAKE_SSH_FAIL_DEST). `-R <remote socket>:<local
socket>` is bound before the command runs and, as real sshd does, left on disk after the exit;
FAKE_SSH_REFUSE_R=1 (for destinations containing FAKE_SSH_REFUSE_R_DEST) refuses it as sshd's
AllowStreamLocalForwarding no would, and a stale file or a missing folder is refused too: ssh's
`Warning:` line and the command runs, or with `-o ExitOnForwardFailure=yes` its `Error:` line
and exit 255 (terminum-remote.md AS-84). Other `-o` options (ControlPath=none…) are ignored. SIGTERM drops
the link (the remote side's processes end with it). Each run writes its pid into FAKE_SSH_PIDS, so a check can drop
"the link" by killing it. Never used outside tests: terminumd takes it through TM_SSH."""
import os
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import time

WITH_VALUE = set("BbcDEeFIiJLlmOopQRSWw")


def parse(argv):
    """(-L forwards, -R forwards, -o options, destination, remote command) from ssh's arguments;
    an option's first value wins, as in ssh."""
    forwards, reverse, opts, i = [], [], {}, 0
    while i < len(argv):
        a = argv[i]
        if a == "--":
            i += 1
            break
        if not a.startswith("-") or a == "-":
            break
        j = 1
        while j < len(a):
            c = a[j]
            if c in WITH_VALUE:
                value = a[j + 1:] or (argv[i + 1] if i + 1 < len(argv) else "")
                if not a[j + 1:]:
                    i += 1
                if c == "L":
                    forwards.append(value)
                elif c == "R":
                    reverse.append(value)
                elif c == "o":
                    key, _, v = value.replace("=", " ", 1).partition(" ")
                    opts.setdefault(key.lower(), v.strip())
                break
            j += 1
        i += 1
    if i >= len(argv):
        sys.exit("fake ssh: no destination")
    return forwards, reverse, opts, argv[i], " ".join(argv[i + 1:])


def pipe(a, b):
    try:
        while True:
            data = a.recv(65536)
            if not data:
                break
            b.sendall(data)
    except OSError:
        pass
    finally:
        for s in (a, b):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass


def forward(local, remote):
    """Listen on `local`; each connection is joined to `remote` once it exists."""
    try:
        os.unlink(local)
    except FileNotFoundError:
        pass
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(local)
    os.chmod(local, 0o600)
    srv.listen(16)

    def serve():
        while True:
            try:
                c, _ = srv.accept()
            except OSError:
                return
            r = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            for _ in range(100):
                try:
                    r.connect(remote)
                    break
                except OSError:
                    time.sleep(0.05)
            else:
                c.close()
                continue
            threading.Thread(target=pipe, args=(c, r), daemon=True).start()
            threading.Thread(target=pipe, args=(r, c), daemon=True).start()
    threading.Thread(target=serve, daemon=True).start()
    return srv


def reverse(spec, opts, dest):
    """`-R <remote>:<local>`: listen on the remote path, join each connection to the local
    socket. A refusal is said as ssh says it; None then (or exit 255 under ExitOnForwardFailure)."""
    remote, _, local = spec.partition(":")
    if not remote or "/" not in local:
        sys.stderr.write(f"Bad remote forwarding specification '{spec}'\n")
        sys.exit(255)
    refuse = os.environ.get("FAKE_SSH_REFUSE_R") == "1" and os.environ.get("FAKE_SSH_REFUSE_R_DEST", "") in dest
    srv = None
    if not refuse:
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        old = os.umask(0o177)  # sshd's StreamLocalBindMask: the socket is 0600
        try:
            srv.bind(remote)  # a stale file or a missing folder fails here, as in sshd
            srv.listen(16)
        except OSError:
            srv.close()
            srv = None
        finally:
            os.umask(old)
    if srv is None:
        level = opts.get("loglevel", "info").lower()
        if opts.get("exitonforwardfailure", "no").lower() == "yes":
            if level not in ("quiet", "fatal"):
                sys.stderr.write(f"Error: remote port forwarding failed for listen path {remote}\n")
            sys.exit(255)
        if level not in ("quiet", "fatal", "error"):
            sys.stderr.write(f"Warning: remote port forwarding failed for listen path {remote}\n")
        return None

    def serve():
        while True:
            try:
                c, _ = srv.accept()
            except OSError:
                return
            r = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                r.connect(local)
            except OSError:  # ssh: "connect_to <local>: failed", the channel is closed
                r.close()
                c.close()
                continue
            threading.Thread(target=pipe, args=(c, r), daemon=True).start()
            threading.Thread(target=pipe, args=(r, c), daemon=True).start()
    threading.Thread(target=serve, daemon=True).start()
    return srv


def tmux_shim(socket_name):
    """A folder with a `tmux` that reaches only the private server `socket_name`."""
    real = next((os.path.join(d, "tmux") for d in os.environ.get("PATH", "").split(":") + ["/opt/homebrew/bin", "/usr/local/bin"]
                 if os.path.isfile(os.path.join(d, "tmux"))), "tmux")
    # in the test's home, which its run removes: terminumd ends this stand-in by SIGKILL, which runs no cleanup
    d = tempfile.mkdtemp(prefix=".fssh-", dir=os.environ.get("FAKE_SSH_HOME"))
    shim = os.path.join(d, "tmux")
    with open(shim, "w") as f:
        f.write(f'#!/bin/sh\nexec "{real}" -L "{socket_name}" -f /dev/null "$@"\n')
    os.chmod(shim, stat.S_IRWXU)
    return d


def main():
    if "-G" in sys.argv[1:]:
        # `ssh -G`: the configuration it would use, nothing connected (FAKE_SSH_CONNECT_TIMEOUT: a
        # user's ssh config that sets one)
        print(f"connecttimeout {os.environ.get('FAKE_SSH_CONNECT_TIMEOUT', 'none')}")
        return
    forwards, reverses, opts, dest, command = parse(sys.argv[1:])
    if os.environ.get("FAKE_SSH_FAIL") and os.environ.get("FAKE_SSH_FAIL_DEST", "") in dest:
        sys.stderr.write(os.environ["FAKE_SSH_FAIL"] + "\n")
        sys.exit(255)
    if os.environ.get("FAKE_SSH_PIDS"):
        with open(os.environ["FAKE_SSH_PIDS"], "a") as f:
            f.write(f"{os.getpid()} {dest} {command[:100]}\n")
    home = os.environ.get("FAKE_SSH_HOME") or sys.exit("fake ssh: FAKE_SSH_HOME is not set")
    sock = os.environ.get("FAKE_SSH_TMUX_SOCKET") or sys.exit("fake ssh: FAKE_SSH_TMUX_SOCKET is not set")
    listeners = []
    for spec in forwards:
        local, _, remote = spec.partition(":")
        listeners.append((forward(local, remote), local))
    # bound before the command runs, as sshd answers the forward before the session starts
    remotes = [srv for spec in reverses if (srv := reverse(spec, opts, dest))]
    shim = tmux_shim(sock)
    # TMUX_TMPDIR stays: a run's private folder is where its tests look for this "host's" server
    env = {k: v for k, v in os.environ.items() if k not in ("TMUX", "TMUX_PANE") and not k.startswith("TM_")}
    env.update(HOME=home, PATH=shim + ":" + os.environ.get("PATH", "/usr/bin:/bin"), SHELL="/bin/sh")
    p = subprocess.Popen(["/bin/sh", "-c", command or "exec /bin/sh"], env=env, cwd=home, start_new_session=True)

    def drop(_sig, _frame):
        # the link drops: the remote side's processes go with it, as sshd would end them
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        shutil.rmtree(shim, ignore_errors=True)
        os._exit(255)
    signal.signal(signal.SIGTERM, drop)
    signal.signal(signal.SIGHUP, drop)
    try:
        code = p.wait()
    except KeyboardInterrupt:
        p.terminate()
        code = 130
    shutil.rmtree(shim, ignore_errors=True)
    for srv in remotes:
        srv.close()  # its file stays, as sshd leaves it (AS-84)
    for srv, local in listeners:
        srv.close()
        try:
            os.unlink(local)
        except FileNotFoundError:
            pass
    sys.exit(code)


if __name__ == "__main__":
    main()
