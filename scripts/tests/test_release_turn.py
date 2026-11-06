# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""scripts/release_turn.py (spec AC-104): the chain of make release and make rehearse, its steps stubbed: what stops
it, what is only said, and that a rehearsal holds no mark. The real steps have their own tests."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import release_turn  # noqa: E402


def done(code=0, stdout="abc\n"):
    return subprocess.CompletedProcess([], code, stdout, "")


class Chain(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.mark = Path(tmp.name) / "mark"
        self.calls = []
        self.check_status = 0
        self.mac = (False, "")
        self.marked_during = None

    def run_chain(self, argv):
        def fake_run(cmd, **kwargs):
            self.calls.append((cmd, kwargs.get("env")))
            self.marked_during = self.mark.exists()
            if cmd[:2] == ["bash", "scripts/check_all.sh"]:
                return done(self.check_status)
            return done()
        with mock.patch.object(release_turn, "mark_path", return_value=self.mark), \
                mock.patch.object(release_turn.subprocess, "run", fake_run), \
                mock.patch.object(release_turn.release, "check_settings"), \
                mock.patch.object(release_turn.release, "release_notes") as notes, \
                mock.patch.object(release_turn, "mac_checks", return_value=self.mac):
            self.notes = notes
            return release_turn.main(argv)

    def released(self):
        return [c for c, _ in self.calls if "scripts/release.py" in c]

    def test_a_release_passes_the_checked_commit_on_and_holds_its_mark(self):
        self.assertEqual(self.run_chain(["v1.0.0"]), 0)
        self.assertEqual(self.released()[0][-1], "v1.0.0")
        self.assertNotIn("--rehearse", self.released()[0])
        self.assertEqual(self.calls[-1][1]["TM_GATE_HEAD"], "abc")
        self.assertTrue(self.marked_during)
        self.assertFalse(self.mark.exists())
        self.notes.assert_called_once()

    def test_failing_tests_here_or_on_the_mac_stop_it(self):
        self.check_status = 1
        self.assertEqual(self.run_chain(["v1.0.0"]), 1)
        self.assertEqual(self.released(), [])
        self.check_status, self.mac = 0, (True, "")
        self.assertEqual(self.run_chain(["v1.0.0"]), 1)
        self.assertEqual(self.released(), [])

    def test_a_runner_that_could_not_run_them_is_said_and_stops_nothing(self):
        self.mac = (False, "the runner could not run them")
        with mock.patch("builtins.print") as said:
            self.assertEqual(self.run_chain(["v1.0.0"]), 0)
        self.assertEqual(len(self.released()), 1)
        self.assertIn("did not run: the runner could not run them", said.call_args_list[-1].args[0])

    def test_a_rehearsal_holds_no_mark_and_needs_no_notes(self):
        self.assertEqual(self.run_chain(["--rehearse", "v1.0.0"]), 0)
        self.assertIn("--rehearse", self.released()[0])
        self.assertFalse(self.marked_during)
        self.notes.assert_not_called()

    def test_a_rehearsal_waits_for_a_release(self):
        with mock.patch.object(release_turn, "releasing", return_value=True), self.assertRaisesRegex(SystemExit, "a release is running"):
            release_turn.main(["--rehearse", "v1.0.0"])


class MacChecks(unittest.TestCase):
    def status(self, code):
        run = mock.Mock()
        run.wait.return_value = code
        with mock.patch.dict(os.environ, {"TM_MAC_RUNNER": "runner"}), mock.patch.object(release_turn.subprocess, "Popen", return_value=run):
            return release_turn.mac_checks(Path("."))

    def test_each_exit_says_what_happened(self):
        self.assertEqual(self.status(0), (False, ""))
        self.assertEqual(self.status(1)[0], True)
        self.assertEqual(self.status(2), (False, "the runner could not run them"))
        self.assertEqual(self.status(3), (False, "the runner is busy, or its lock is stale"))

    def test_no_runner_is_said(self):
        with mock.patch.dict(os.environ):
            os.environ.pop("TM_MAC_RUNNER", None)
            self.assertEqual(release_turn.mac_checks(Path(".")), (False, "no TM_MAC_RUNNER"))

    def test_a_runner_gone_quiet_is_ended_and_said(self):
        run = mock.Mock()
        run.wait.side_effect = [subprocess.TimeoutExpired("x", 1), 0]
        with mock.patch.dict(os.environ, {"TM_MAC_RUNNER": "runner"}), mock.patch.object(release_turn.subprocess, "Popen", return_value=run):
            failed, why = release_turn.mac_checks(Path("."))
        run.terminate.assert_called_once()
        self.assertFalse(failed)
        self.assertIn("minutes", why)


if __name__ == "__main__":
    unittest.main()
