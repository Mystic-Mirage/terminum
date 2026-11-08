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


class Turn:
    """The one-full-run turn, always free here (scripts/tests/test_load_gate.py has the real one)."""
    def __init__(self, _):
        pass

    def busy(self):
        return None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        pass


class Chain(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.mark = Path(tmp.name) / "mark"
        self.calls = []
        self.check_status = 0
        self.mac = (False, "")
        self.marked_during = None
        self.released = []
        self.passed_before = False
        self.busy = None

    def fake_release(self, tag, gate_head, said="", rehearsing=False):
        self.released.append((tag, gate_head, said, rehearsing))
        self.marked_during = self.mark.exists()

    def run_chain(self, argv):
        def fake_run(cmd, **kwargs):
            self.calls.append(cmd)
            if cmd == ["bash", "scripts/check_all.sh", "--passed"]:
                return done(0 if self.passed_before else 1)
            if cmd[:2] == ["bash", "scripts/check_all.sh"]:
                self.gate_skipped = kwargs.get("env", {}).get("TM_GATE_PASSED") == "1"
                return done(self.check_status)
            return done()
        with mock.patch.object(release_turn, "mark_path", return_value=self.mark), \
                mock.patch.object(release_turn.subprocess, "run", fake_run), \
                mock.patch.object(release_turn.release, "check_settings"), \
                mock.patch.object(release_turn.release, "check_source"), \
                mock.patch.object(release_turn, "primary_of", return_value=Path("/elsewhere")), \
                mock.patch.object(release_turn.release, "release_notes") as notes, \
                mock.patch.object(release_turn, "mac_checks", return_value=self.mac), \
                mock.patch.object(release_turn.load_gate, "Turn", Turn), \
                mock.patch.object(release_turn.load_gate, "wait", lambda **_: self.busy) as self.waited, \
                mock.patch.object(release_turn.release, "main", self.fake_release):
            self.notes = notes
            return release_turn.main(argv)

    def test_a_release_passes_the_checked_commit_on_and_holds_its_mark(self):
        self.assertEqual(self.run_chain(["v1.0.0"]), 0)
        self.assertEqual(self.released, [("v1.0.0", "abc", "", False)])
        self.assertTrue(self.marked_during)
        self.assertFalse(self.mark.exists())
        self.notes.assert_called_once()

    def test_a_busy_build_server_ends_the_release_before_any_test(self):  # AC-146
        self.busy = "load 31 over 8"
        with mock.patch("sys.stderr") as err:
            self.assertEqual(self.run_chain(["v1.0.0"]), 1)
        self.assertIn("release: the build server is not available for a release: load 31 over 8 for 15 minutes: nothing is public",
                      "".join(c.args[0] for c in err.write.call_args_list))
        self.assertNotIn(["bash", "scripts/check_all.sh"], self.calls)
        self.assertEqual(self.released, [])
        self.assertFalse(self.mark.exists())

    def test_the_full_run_after_the_gate_does_not_wait_again(self):  # AC-146
        self.assertEqual(self.run_chain(["v1.0.0"]), 0)
        self.assertTrue(self.gate_skipped)

    def test_a_tree_that_passed_already_waits_for_nothing(self):  # AC-146
        self.passed_before, self.busy = True, "load 31 over 8"
        self.assertEqual(self.run_chain(["v1.0.0"]), 0)
        self.assertEqual(len(self.released), 1)

    def test_failing_tests_here_or_on_the_mac_stop_it(self):
        self.check_status = 1
        self.assertEqual(self.run_chain(["v1.0.0"]), 1)
        self.assertEqual(self.released, [])
        self.check_status, self.mac = 0, (True, "")
        self.assertEqual(self.run_chain(["v1.0.0"]), 1)
        self.assertEqual(self.released, [])

    def test_a_runner_that_could_not_run_them_is_said_and_stops_nothing(self):
        self.mac = (False, "the runner could not run them")
        with mock.patch("builtins.print") as said:
            self.assertEqual(self.run_chain(["v1.0.0"]), 0)
        self.assertEqual(len(self.released), 1)
        self.assertIn("did not run: the runner could not run them", said.call_args_list[-1].args[0])
        self.assertIn("did not run: the runner could not run them", self.released[0][2], "for the release's notes")

    def test_a_rehearsal_holds_no_mark_and_needs_no_notes(self):
        self.assertEqual(self.run_chain(["--rehearse", "v1.0.0"]), 0)
        self.assertTrue(self.released[0][3])
        self.assertFalse(self.marked_during)
        self.notes.assert_not_called()

    def test_a_rehearsal_waits_for_a_release(self):
        with mock.patch.object(release_turn, "primary_of", return_value=Path("/elsewhere")), \
                mock.patch.object(release_turn, "releasing", return_value=True), self.assertRaisesRegex(SystemExit, "a release is running"):
            release_turn.main(["--rehearse", "v1.0.0"])

    def test_a_rehearsal_stays_out_of_the_primary_checkout(self):
        with mock.patch.object(release_turn, "primary_of", return_value=Path(release_turn.__file__).resolve().parent.parent), \
                self.assertRaisesRegex(SystemExit, "runs in a worktree"):
            release_turn.main(["--rehearse", "v1.0.0"])

    def test_a_refused_release_still_says_what_did_not_run_and_clears_its_mark(self):
        self.mac = (False, "the runner could not run them")
        self.fake_release = mock.Mock(side_effect=SystemExit("HEAD moved"))
        with mock.patch("builtins.print") as said, self.assertRaisesRegex(SystemExit, "HEAD moved"):
            self.run_chain(["v1.0.0"])
        self.assertIn("did not run", said.call_args_list[-1].args[0])
        self.assertFalse(self.mark.exists())

    def test_settings_are_looked_at_before_anything_runs(self):
        with mock.patch.object(release_turn.release, "check_settings", side_effect=SystemExit("no identity")), \
                mock.patch.object(release_turn.release, "check_source") as source, mock.patch.object(release_turn.subprocess, "run") as run, \
                self.assertRaisesRegex(SystemExit, "no identity"):
            release_turn.main(["v1.0.0"])
        source.assert_not_called()
        run.assert_not_called()

    def test_a_bad_version_is_refused_at_once(self):
        with self.assertRaisesRegex(SystemExit, "usage"):
            release_turn.main(["1.0"])

    def test_a_tree_that_cannot_release_is_refused_before_the_long_run(self):
        with mock.patch.object(release_turn.release, "check_settings"), mock.patch.object(release_turn.release, "check_source", side_effect=SystemExit("the tree has changes")), \
                mock.patch.object(release_turn.subprocess, "run") as run, self.assertRaisesRegex(SystemExit, "the tree has changes"):
            release_turn.main(["v1.0.0"])
        run.assert_not_called()


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
        with mock.patch.object(release_turn, "BUSY_WAIT_S", 0):
            self.assertEqual(self.status(3), (False, "the runner is busy, or its lock is stale"))

    def test_a_busy_runner_is_tried_again(self):
        runs = [mock.Mock(wait=mock.Mock(return_value=3)), mock.Mock(wait=mock.Mock(return_value=0))]
        with mock.patch.dict(os.environ, {"TM_MAC_RUNNER": "runner"}), mock.patch.object(release_turn.subprocess, "Popen", side_effect=runs), \
                mock.patch.object(release_turn.time, "sleep") as slept:
            self.assertEqual(release_turn.mac_checks(Path(".")), (False, ""))
        slept.assert_called_once()

    def test_no_runner_is_said(self):
        with mock.patch.dict(os.environ):
            os.environ.pop("TM_MAC_RUNNER", None)
            self.assertEqual(release_turn.mac_checks(Path(".")), (False, "no TM_MAC_RUNNER"))

    def test_a_runner_gone_quiet_is_ended_and_said(self):
        run = mock.Mock(pid=4242)
        run.wait.side_effect = [subprocess.TimeoutExpired("x", 1), 0]
        with mock.patch.dict(os.environ, {"TM_MAC_RUNNER": "runner"}), mock.patch.object(release_turn.subprocess, "Popen", return_value=run), \
                mock.patch.object(release_turn.os, "killpg") as ended:
            failed, why = release_turn.mac_checks(Path("."))
        ended.assert_called_once_with(4242, release_turn.signal.SIGTERM)
        self.assertFalse(failed)
        self.assertIn("minutes", why)


if __name__ == "__main__":
    unittest.main()
