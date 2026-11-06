# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Names of one test run: a port no one listens on and tmux sockets carrying this run's pid, so
checks in several worktrees (or sessions) at once never meet, and a run's cleanup
(`tmux -L <socket> kill-server`) never ends another run's server. Where its folders go, and the
socket files `kill-server` leaves behind, removed when the run exits (spec AC-102)."""
import atexit
import os
import socket
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


def remove_dead_sockets():
    """The socket file of each `own` name whose tmux server has gone (a killed one may take a moment
    to); a live one stays and is seen."""
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


def own(name):
    """`name` for this run only: `<name>-<pid>` (short: a tmux socket path ends at 104 bytes)."""
    if not _sockets:
        atexit.register(remove_dead_sockets)
    _sockets.add(f"{name}-{os.getpid()}")
    return f"{name}-{os.getpid()}"
