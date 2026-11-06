# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Names of one test run: a port no one listens on and tmux sockets carrying this run's pid, so
checks in several worktrees (or sessions) at once never meet, and a run's cleanup
(`tmux -L <socket> kill-server`) never ends another run's server. Where its folders go, and the
socket files `kill-server` leaves behind, removed when the run exits (spec AC-102)."""
import atexit
import os
import re
import signal
import socket
import subprocess
import tempfile
import time

_sockets = set()


def free_port():
    """A loopback port the system just gave out (TM_TEST_PORT names one instead)."""
    if os.environ.get("TM_TEST_PORT"):
        return int(os.environ["TM_TEST_PORT"])
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def short_tmp():
    """Where a test's folders go: the temporary folder if paths of sockets in it fit in 104 bytes
    (check_step.py's short one, Linux's /tmp), else /tmp: a Mac's own is 57 bytes once resolved."""
    folder = os.path.realpath(tempfile.gettempdir())
    return folder if len(folder) <= 40 else "/tmp"


def own_server_pids(ps_text, names):
    """The pids in `pid command` lines of tmux servers on these sockets: named with this run's pid."""
    pids = []
    for line in ps_text.splitlines():
        pid, _, command = line.strip().partition(" ")
        if pid.isdigit() and any(re.search(rf"\btmux\b.* (-L |-S \S*/){re.escape(name)}( |$)", command) for name in names):
            pids.append(int(pid))
    return pids


def end_own_servers():
    """Ends each tmux server of an `own` name by its pid. `kill-server` misses one whose socket file has
    gone: a dying server unlinks the file its successor made, when a client reconnects at that moment."""
    listed = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    for pid in own_server_pids(listed, _sockets):
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass


def remove_dead_sockets():
    """Ends what still runs, then the socket file of each `own` name whose tmux server has gone (a killed
    one may take a moment to)."""
    end_own_servers()
    base = os.path.join(os.environ.get("TMUX_TMPDIR") or "/tmp", f"tmux-{os.getuid()}")
    for name in _sockets:
        path = os.path.join(base, name)
        for _ in range(20):
            with socket.socket(socket.AF_UNIX) as probe:
                try:
                    probe.connect(path)
                except ConnectionRefusedError:
                    os.unlink(path)
                    break
                except OSError:
                    break
            time.sleep(0.05)


def container_name(kind, pid=None):
    """This run's Docker container (`pid`: another run's, to look for what it left)."""
    return f"tm-{kind}-{pid or os.getpid()}"


def own(name):
    """`name` for this run only: `<name>-<pid>` (short: a tmux socket path ends at 104 bytes)."""
    if not _sockets:
        atexit.register(remove_dead_sockets)
    _sockets.add(f"{name}-{os.getpid()}")
    return f"{name}-{os.getpid()}"
