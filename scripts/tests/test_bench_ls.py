# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""The real benchmark command with private, deterministic timing/process stand-ins."""
import os
import signal
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "bench_ls.sh"


class BenchmarkExit(unittest.TestCase):
    def run_benchmark(self, cold="0.001", warm="0.001", rss="1024", last_cold="", wait=30):
        with tempfile.TemporaryDirectory(prefix="tm-bench-test-") as root:
            root = Path(root)
            bin_dir = root / "bin"
            bin_dir.mkdir()
            stubs = {
                "daemon": '#!/bin/sh\nprintf "%s\\n" "$$" > "$BENCH_TEST_DAEMON_PID"\nexec sleep 60\n',
                "ps": '#!/bin/sh\nprintf "%s\\n" "$BENCH_TEST_RSS"\n',
                "tmux": "#!/bin/sh\nexit 0\n",
                "curl": '''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
if '-w' in args:
    counter = pathlib.Path(os.environ['BENCH_TEST_COUNTER'])
    n = int(counter.read_text()) if counter.exists() else 0
    counter.write_text(str(n + 1))
    sample = os.environ['BENCH_TEST_WARM'] if n % 2 else os.environ['BENCH_TEST_COLD']
    if n == 18 and os.environ['BENCH_TEST_LAST_COLD']:
        sample = os.environ['BENCH_TEST_LAST_COLD']
    print(sample)
elif '/auth/login' in args[-1]:
    print(json.dumps({'secret': 'synthetic-benchmark-session'}))
elif 'limit=1' in args[-1]:
    print(json.dumps({'total': 1000}))
''',
            }
            for name, body in stubs.items():
                file = bin_dir / name
                file.write_text(body)
                file.chmod(0o700)
            env = {**os.environ, "PATH": str(bin_dir) + os.pathsep + os.environ["PATH"],
                   "TERMINUMD_BIN": str(bin_dir / "daemon"), "BENCH_TEST_COUNTER": str(root / "counter"),
                   "BENCH_TEST_COLD": cold, "BENCH_TEST_WARM": warm, "BENCH_TEST_RSS": rss,
                   "BENCH_TEST_LAST_COLD": last_cold, "BENCH_TEST_DAEMON_PID": str(root / "daemon.pid")}
            command = ["bash", str(SCRIPT), str(root), "10"]
            process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       text=True, start_new_session=True)
            try:
                # a short wait counts from the daemon's start: a loaded machine starts it late
                started = time.monotonic() + 10
                while wait < 1 and not (root / "daemon.pid").exists() and time.monotonic() < started:
                    try:
                        stdout, stderr = process.communicate(timeout=0.05)
                        return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
                    except subprocess.TimeoutExpired:
                        pass
                stdout, stderr = process.communicate(timeout=wait)
                return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
            except BaseException:
                pidfile = root / "daemon.pid"
                self.timed_out_daemon = int(pidfile.read_text()) if pidfile.exists() else None
                # The benchmark owns a background daemon: end the entire group we made.
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    process.communicate(timeout=2)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.communicate()
                raise

    def test_within_all_budgets_succeeds(self):
        out = self.run_benchmark()
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn(" PASS", out.stdout)

    def test_any_missed_budget_fails_the_command(self):
        for kwargs in ({"cold": "2.6"}, {"warm": "0.060"}, {"rss": "160000"}):
            with self.subTest(kwargs=kwargs):
                out = self.run_benchmark(**kwargs)
                self.assertIn(" FAIL", out.stdout)
                self.assertNotEqual(out.returncode, 0, out.stdout)

    def test_ten_samples_p95_includes_the_slowest_sample(self):
        out = self.run_benchmark(last_cold="3.0")
        self.assertIn("cold_ms p50=1 p95=3000 ", out.stdout)
        self.assertNotEqual(out.returncode, 0, out.stdout)

    def test_timeout_ends_the_owned_benchmark_group(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            self.run_benchmark(wait=0.3)
        self.assertIsNotNone(self.timed_out_daemon, "the test must start a daemon before timing out")
        state = subprocess.run(["/bin/ps", "-o", "stat=", "-p", str(self.timed_out_daemon)],
                               capture_output=True, text=True, timeout=5).stdout.strip()
        self.assertTrue(not state or state.startswith("Z"), f"test daemon still running: {state}")
