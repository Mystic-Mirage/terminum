# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""The guard of AC-102 on real commands: a clean run passes, what a run leaves fails it and goes,
and a folder of the temporary folder outside the run never counts."""
import importlib.util
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

GUARD = str(Path(__file__).resolve().parents[1] / "check_step.py")


def guarded(script, **kwargs):
    """Runs `script` under the guard; (exit status, its stderr, the folder it was given)."""
    with tempfile.NamedTemporaryFile("r", suffix=".where") as where:
        done = subprocess.run([sys.executable, "-I", GUARD, "sh", "-c", f'echo "$TMPDIR" > {where.name}; {script}'],
                              capture_output=True, text=True, timeout=30, **kwargs)
        return done.returncode, done.stderr, where.read().strip()


class OwnSockets(unittest.TestCase):
    """testrun.own: when the run exits its servers end, and their socket files go."""
    def run_child(self, after):
        with tempfile.TemporaryDirectory(prefix="tm-own-", dir="/tmp") as base:
            code = ("import subprocess, sys\n"
                    f"sys.path.insert(0, {str(Path(GUARD).parent)!r})\n"
                    "from testrun import own\n"
                    "name = own('tm-test-own')\n"
                    "subprocess.run(['tmux', '-f', '/dev/null', '-L', name, 'new-session', '-d', '-s', 'm'], check=True)\n"
                    f"{after}\nprint(name)\n")
            env = {**os.environ, "TMUX_TMPDIR": base}
            name = subprocess.run([sys.executable, "-I", "-c", code], capture_output=True, text=True, check=True, env=env, timeout=20).stdout.strip()
            socket_file = Path(base, f"tmux-{os.getuid()}", name)
            there = socket_file.exists()
            subprocess.run(["tmux", "-L", name, "kill-server"], env=env, stderr=subprocess.DEVNULL)
            return there

    def test_a_dead_servers_socket_goes(self):
        self.assertFalse(self.run_child("subprocess.run(['tmux', '-L', name, 'kill-server'], check=True)"))

    def test_a_live_server_ends_with_its_run_and_its_socket_goes(self):
        self.assertFalse(self.run_child("pass"))


class Guard(unittest.TestCase):
    def test_a_clean_run_passes(self):
        status, err, folder = guarded('[ "$TMUX_TMPDIR" = "$TMPDIR" ] && mkdir "$TMPDIR/d" && touch "$TMPDIR/d/f" && rm -r "$TMPDIR/d"')
        self.assertEqual((status, err), (0, ""))
        self.assertTrue(folder.startswith(("/tmp/tmc.", "/private/tmp/tmc.")), folder)
        self.assertLessEqual(len(folder), 40)
        self.assertFalse(os.path.exists(folder))

    def test_tmuxs_own_empty_folder_does_not_count(self):
        status, err, _ = guarded(f'mkdir "$TMPDIR/tmux-{os.getuid()}"')
        self.assertEqual((status, err), (0, ""))

    def test_a_left_file_fails_a_passing_run_and_goes(self):
        status, err, folder = guarded(f'mkdir -p "$TMPDIR/tmux-{os.getuid()}" "$TMPDIR/tm-x"; touch "$TMPDIR/tmux-{os.getuid()}/tm-test-a" "$TMPDIR/tm-x/f"')
        self.assertEqual(status, 3)
        self.assertIn(f"LEFT tmux-{os.getuid()}/tm-test-a", err)
        self.assertIn("LEFT tm-x/f", err)
        self.assertFalse(os.path.exists(folder))

    def test_a_left_empty_folder_fails(self):
        status, err, _ = guarded('mkdir "$TMPDIR/tm-empty"')
        self.assertEqual(status, 3)
        self.assertIn("LEFT tm-empty", err)

    def test_a_failure_keeps_its_own_status(self):
        status, err, folder = guarded('touch "$TMPDIR/x"; exit 7')
        self.assertEqual(status, 7)
        self.assertIn("LEFT x", err)
        self.assertFalse(os.path.exists(folder))

    def test_a_left_process_fails_and_is_ended(self):
        status, err, _ = guarded("sleep 61 >/dev/null 2>&1 &")
        self.assertEqual(status, 3)
        self.assertIn("LEFT process", err)
        self.assertIn("sleep 61", err)
        pid = int(err.split("LEFT process ")[1].split(":")[0])
        for _ in range(50):
            if subprocess.run(["ps", "-o", "stat=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()[:1] in ("", "Z"):
                break
            time.sleep(0.1)
        else:
            self.fail(f"{pid} still runs")

    def test_another_runs_folder_never_counts(self):
        with tempfile.TemporaryDirectory(prefix="tm-other-", dir="/tmp") as other:
            status, err, _ = guarded(f'touch "{other}/theirs"')
            self.assertEqual((status, err), (0, ""))
            self.assertTrue(os.path.exists(f"{other}/theirs"))

    def test_a_signal_still_removes_the_folder(self):
        with tempfile.NamedTemporaryFile("r", suffix=".where") as where:
            run = subprocess.Popen([sys.executable, "-I", GUARD, "sh", "-c", f'touch "$TMPDIR/x"; echo "$TMPDIR" > {where.name}; exec sleep 30'],
                                   stderr=subprocess.PIPE, text=True)
            for _ in range(1200):    # up to 60 s: a signal before the step started tests nothing, and a busy machine starts it late
                folder = where.read().strip()
                where.seek(0)
                if folder:
                    break
                time.sleep(0.05)
            self.assertTrue(folder, "the step never started")
            run.send_signal(signal.SIGTERM)
            _, err = run.communicate(timeout=20)
            self.assertNotEqual(run.returncode, 0)
            self.assertIn("LEFT x", err)
            self.assertFalse(os.path.exists(folder))


def gone(pid, seconds=5):
    """Whether `pid` has ended within `seconds` (not this test's child: launchd reaps it)."""
    for _ in range(int(seconds * 10)):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return True
        time.sleep(0.1)
    return False


class Stubborn(unittest.TestCase):
    """A leftover that ignores SIGTERM and SIGHUP, as macOS's bash spinning on a terminal gone does."""
    def test_the_guard_ends_one_that_ignores_sigterm(self):
        status, err, _ = guarded("(trap '' TERM HUP; exec sleep 62) >/dev/null 2>&1 &")
        self.assertEqual(status, 3)
        pid = int(err.split("LEFT process ")[1].split(":")[0])
        self.assertTrue(gone(pid), f"{pid} still runs")

    def test_a_landings_steps_run_under_the_guard(self):
        spec = importlib.util.spec_from_file_location("check_changed", Path(GUARD).with_name("check_changed.py"))
        check_changed = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(check_changed)
        root = Path(GUARD).parents[1]
        try:
            self.assertEqual(check_changed.run([("guard-probe", 'touch "$TMPDIR/left"')], root=root), ["guard-probe"])
            self.assertIn("LEFT left", (root / ".checks" / "changed.guard-probe.log").read_text())
        finally:
            (root / ".checks" / "changed.guard-probe.log").unlink(missing_ok=True)

    def test_end_tmux_ends_a_pane_program_that_outlives_kill_server(self):
        sys.path.insert(0, str(Path(GUARD).parent))
        from e2e_ws import end_tmux
        from testrun import own
        name = own("tm-test-stubborn")
        try:
            subprocess.run(["tmux", "-f", "/dev/null", "-L", name, "new-session", "-d", "sh -c \"trap '' HUP TERM; exec sleep 63\""], check=True)
            pid = int(subprocess.run(["tmux", "-L", name, "display", "-p", "#{pane_pid}"], capture_output=True, text=True).stdout)
        finally:
            end_tmux(name)
        self.assertTrue(gone(pid), f"{pid} still runs")


if __name__ == "__main__":
    unittest.main()
