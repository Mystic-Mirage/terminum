# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""scripts/check_docker.py (spec AC-104), the scripts and Docker stubbed: only failing tests or a container left
behind fail the step; what kept it from running is a skip that stays on record."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import check_docker  # noqa: E402


class WhyNot(unittest.TestCase):
    def test_another_machine_skips(self):
        with mock.patch.object(check_docker.agents, "this_platform", return_value="linux-x86_64"):
            self.assertIn("Apple", check_docker.why_not())

    def test_no_toolchain_skips(self):
        with mock.patch.object(check_docker.agents, "this_platform", return_value="macos-aarch64"), \
                mock.patch.object(check_docker.agents, "TOOLCHAIN", Path("/nonexistent")):
            self.assertIn("make toolchain", check_docker.why_not())

    def test_a_docker_that_hangs_or_fails_skips(self):
        with tempfile.TemporaryDirectory() as home, mock.patch.object(check_docker.agents, "TOOLCHAIN", Path(home)), \
                mock.patch.object(check_docker.agents, "this_platform", return_value="macos-aarch64"):
            (Path(home) / "bin").mkdir()
            for effect in (subprocess.TimeoutExpired("docker", 10), FileNotFoundError(), subprocess.CalledProcessError(1, "docker")):
                with mock.patch.object(check_docker.subprocess, "run", side_effect=effect):
                    self.assertIn("Docker", check_docker.why_not())
            with mock.patch.object(check_docker.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)):
                self.assertEqual(check_docker.why_not(), "")


class Runs(unittest.TestCase):
    def outcome(self, statuses, left=()):
        statuses = iter(statuses)
        popen = mock.Mock(side_effect=lambda *a, **k: mock.Mock(pid=7, wait=mock.Mock(return_value=next(statuses))))

        def docker(cmd, **_):
            named = next((c for c in cmd if c.startswith("name=")), "")
            return subprocess.CompletedProcess(cmd, 0, "id\n" if any(n in named for n in left) else "", "")
        with mock.patch.object(check_docker.subprocess, "Popen", popen), mock.patch.object(check_docker.subprocess, "run", docker):
            return check_docker.run_all("mac", "linux"), [c.args[0][2:] for c in popen.call_args_list]

    def test_all_three_run(self):
        result, ran = self.outcome([0, 0, 0])
        self.assertEqual(result, (False, ""))
        self.assertEqual([r[-1] for r in ran], ["linux", "alpine", "debian"])

    def test_a_failing_test_fails_it(self):
        self.assertTrue(self.outcome([0, 1, 0])[0][0])

    def test_no_host_is_a_skip(self):
        failed, why = self.outcome([os.EX_TEMPFAIL, 0, 0])[0]
        self.assertFalse(failed)
        self.assertIn("could not start", why)

    def test_a_container_left_fails_it(self):
        self.assertTrue(self.outcome([0, 0, 0], left=["tm-rr9-debian-7"])[0][0])


class Record(unittest.TestCase):
    def test_a_skip_stays_on_record_and_a_run_clears_it(self):
        with tempfile.TemporaryDirectory() as repo, mock.patch.object(check_docker, "REPO", Path(repo)):
            (Path(repo) / ".checks").mkdir()
            skip = Path(repo) / ".checks" / "abc.docker-skip"
            with mock.patch.object(check_docker, "why_not", return_value="Docker does not answer"):
                self.assertEqual(check_docker.main(["abc"]), 0)
            self.assertEqual(skip.read_text(), "Docker does not answer\n")
            with mock.patch.object(check_docker, "why_not", return_value=""), mock.patch.object(check_docker, "run_all", return_value=(False, "")), \
                    mock.patch.object(check_docker.agents, "build", return_value=dict.fromkeys(check_docker.PLATFORMS, "x")), \
                    mock.patch.object(check_docker.agents, "agent_id", return_value="id"):
                self.assertEqual(check_docker.main(["abc"]), 0)
            self.assertFalse(skip.exists())


if __name__ == "__main__":
    unittest.main()
