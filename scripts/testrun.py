# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Names of one test run: a port no one listens on and tmux sockets carrying this run's pid, so
checks in several worktrees (or sessions) at once never meet, and a run's cleanup
(`tmux -L <socket> kill-server`) never ends another run's server."""
import os
import socket


def free_port():
    """A loopback port the system just gave out (TM_TEST_PORT names one instead)."""
    if os.environ.get("TM_TEST_PORT"):
        return int(os.environ["TM_TEST_PORT"])
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def own(name):
    """`name` for this run only: `<name>-<pid>` (short: a tmux socket path ends at 104 bytes)."""
    return f"{name}-{os.getpid()}"
