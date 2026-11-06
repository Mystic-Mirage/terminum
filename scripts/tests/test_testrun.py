# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""A run's tmux servers end with it (spec AC-102), a server whose socket file went included."""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest

SPEC = importlib.util.spec_from_file_location("testrun", Path(__file__).resolve().parents[1] / "testrun.py")
testrun = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(testrun)


class Servers(unittest.TestCase):
    def test_only_this_runs_sockets_match(self):
        ps = """7 /opt/homebrew/bin/tmux -L tm-test-remote-41 -f /dev/null -u -C new-session -A
8 tmux -S /tmp/x/tmux-501/tm-test-remote-41 new-session -d
9 tmux -L tm-test-remote-411 new-session -d
10 tmux -L terminum new-session -d
11 /usr/bin/ssh devbox.example tmux -L other
12 vim tm-test-remote-41
"""
        self.assertEqual(testrun.own_server_pids(ps, {"tm-test-remote-41"}), [7, 8])

    @unittest.skipUnless(shutil.which("tmux"), "no tmux")
    def test_a_server_whose_socket_went_is_ended(self):
        with tempfile.TemporaryDirectory(prefix="tt", dir="/tmp") as home:
            name = f"tm-test-testrun-{os.getpid()}"
            env = dict(os.environ, TMUX_TMPDIR=home)
            subprocess.run(["tmux", "-L", name, "-f", "/dev/null", "new-session", "-d", "sleep 60"], env=env, check=True)
            socket_file = Path(home) / f"tmux-{os.getuid()}" / name
            self.assertTrue(socket_file.exists())
            socket_file.unlink()  # what a dying predecessor does
            self.assertEqual(subprocess.run(["tmux", "-L", name, "kill-server"], env=env, capture_output=True).returncode, 1,
                             "kill-server cannot reach it")
            listed = lambda: testrun.own_server_pids(subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout, {name})
            self.assertEqual(len(listed()), 1)
            before = set(testrun._sockets)
            testrun._sockets.add(name)
            try:
                testrun.end_own_servers()
                for _ in range(100):
                    if not listed():
                        break
                    time.sleep(0.05)
                self.assertEqual(listed(), [])
            finally:
                testrun._sockets.clear()
                testrun._sockets.update(before)


if __name__ == "__main__":
    unittest.main()
