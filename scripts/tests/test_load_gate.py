# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""A full run on a quiet machine (spec AC-146): the load is played, the minutes counted, nothing waits."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
SPEC = importlib.util.spec_from_file_location("load_gate", Path(__file__).resolve().parents[1] / "load_gate.py")
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


class Machine:
    """Loads one per look, a clock that only sleeps move."""
    def __init__(self, *loads):
        self.loads, self.now, self.said = list(loads), 0.0, []

    def load(self):
        return self.loads.pop(0) if len(self.loads) > 1 else self.loads[0]

    def sleep(self, seconds):
        self.now += seconds

    def wait(self, **kw):
        return gate.wait(load=self.load, say=self.said.append, clock=lambda: self.now, sleep=self.sleep, **kw)


@mock.patch.dict(os.environ, {"TM_GATE_LOAD": "8"})
class Gate(unittest.TestCase):
    def test_a_quiet_machine_starts_at_once(self):  # AC-146
        m = Machine(3.2)
        self.assertIsNone(m.wait())
        self.assertEqual((m.now, m.said), (0, ["gate: load 3.2, at most 8: starting"]))

    def test_a_busy_machine_is_looked_at_each_minute_and_started_3_minutes_after_it_quiets(self):  # AC-146
        m = Machine(40.2, 30, 5, 5, 5, 5.1)
        self.assertIsNone(m.wait())
        self.assertEqual(m.now, 5 * 60)   # quiet at the 2nd minute, held to the 5th
        self.assertEqual(m.said[0], "gate: load 40.2 over 8: waiting, a look each minute, 15 minutes at most")
        self.assertEqual(m.said[-1], "gate: load 5.1, at most 8 for 3 minutes: starting")
        self.assertEqual(len(m.said), 6)   # every look is said

    def test_a_look_over_the_limit_starts_the_3_minutes_again(self):  # AC-146
        m = Machine(20, 5, 5, 9, 5, 5, 5, 5)
        self.assertIsNone(m.wait())
        self.assertEqual(m.now, 7 * 60)

    def test_a_machine_busy_for_15_minutes_gives_up(self):  # AC-146
        m = Machine(31)
        self.assertEqual(m.wait(), "load 31 over 8")
        self.assertEqual(m.now, 15 * 60)

    def test_quiet_late_still_gets_its_3_minutes(self):  # AC-146
        m = Machine(*[31] * 15, 5, 5, 5, 5)
        self.assertIsNone(m.wait())
        self.assertEqual(m.now, 18 * 60)

    def test_another_full_run_is_waited_for_however_low_the_load(self):  # AC-146
        turns = ["another full run holds the turn"] * 2 + [None]
        m = Machine(2)
        self.assertIsNone(m.wait(busy=lambda: turns.pop(0) if len(turns) > 1 else turns[0]))
        self.assertIn("another full run holds the turn: waiting", m.said[0])
        self.assertEqual(m.now, 5 * 60)

    def test_the_limit_is_the_cores_count_unless_set(self):  # AC-146
        self.assertEqual(gate.limit(), 8)
        with mock.patch.dict(os.environ, {"TM_GATE_LOAD": ""}):
            self.assertEqual(gate.limit(), os.cpu_count() or 2)

    def test_a_tasks_checks_run_after_15_minutes_saying_so(self):  # AC-146
        m = Machine(31)
        with mock.patch.object(gate, "releasing", return_value=False):
            gate.for_task(load=m.load, say=m.said.append, clock=lambda: m.now, sleep=m.sleep)
        self.assertEqual(m.said[-1], "gate: running under load 31 over 8: a timing failure may be the load's")

    def test_a_tasks_checks_wait_while_a_release_runs(self):  # AC-146
        m = Machine(1)
        with mock.patch.object(gate, "releasing", return_value=True):
            gate.for_task(load=m.load, say=m.said.append, clock=lambda: m.now, sleep=m.sleep)
        self.assertIn("a release is running: waiting", m.said[0])
        self.assertEqual(m.said[-1], "gate: running under a release is running: a timing failure may be the load's")

    def test_make_check_by_hand_ends_75_when_still_busy(self):  # AC-146
        with mock.patch.object(gate, "wait", return_value="load 31 over 8"), \
                mock.patch("sys.stderr") as err:
            self.assertEqual(gate.main([]), 75)
        self.assertIn("check: the machine is busy: load 31 over 8 for 15 minutes: nothing ran", "".join(c.args[0] for c in err.write.call_args_list))


class OneFullRun(unittest.TestCase):
    def test_a_second_full_run_waits_for_the_turn_until_the_first_ends(self):  # AC-146
        with tempfile.TemporaryDirectory() as home:
            subprocess.run(["git", "init", "-q", home], check=True)
            with gate.Turn(home) as first:
                self.assertIsNone(first.busy())
                with gate.Turn(home) as second:
                    self.assertIn("another full run", second.busy())
            with gate.Turn(home) as third:
                self.assertIsNone(third.busy())


if __name__ == "__main__":
    unittest.main()
