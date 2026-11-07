# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""scripts/check_all.sh's start (spec AC-140): the gate before any step, skipped after a release's own,
and --passed. Run in a copy with stand-in scripts, so no check of the real tree runs."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

HERE = Path(__file__).resolve().parents[1]
STUB = {
    "check_changed.py": "print('0123456789abcdef')",
    "load_gate.py": "import os, sys; open('gate.log', 'a').write('gate\\n'); sys.exit(int(os.environ['GATE_EXIT']))",
    "check_step.py": "import sys; open('steps.log', 'a').write(' '.join(sys.argv[1:3]) + '\\n')",
    "check_docker.py": "import sys; sys.exit(1)",
}


class Start(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        (self.root / "scripts").mkdir()
        shutil.copy(HERE / "check_all.sh", self.root / "scripts")
        for name, body in STUB.items():
            (self.root / "scripts" / name).write_text(body + "\n")
        subprocess.run(["git", "init", "-q", tmp.name], check=True)

    def check(self, *args, **env):
        return subprocess.run(["bash", "scripts/check_all.sh", *args], cwd=self.root, capture_output=True, text=True,
                              env={**os.environ, "GATE_EXIT": "0", **env})

    def read(self, name):
        path = self.root / name
        return path.read_text() if path.exists() else ""

    def test_a_busy_machine_ends_it_before_any_step(self):  # AC-140
        self.assertEqual(self.check(GATE_EXIT="75").returncode, 75)
        self.assertEqual((self.read("gate.log"), self.read("steps.log")), ("gate\n", ""))

    def test_a_quiet_machine_runs_the_steps(self):  # AC-140
        self.check()
        self.assertEqual(self.read("gate.log"), "gate\n")
        self.assertTrue(self.read("steps.log").startswith("make test"), self.read("steps.log"))

    def test_after_a_releases_own_gate_it_does_not_wait_again(self):  # AC-140
        self.check(TM_GATE_PASSED="1")
        self.assertEqual(self.read("gate.log"), "")
        self.assertIn("make test", self.read("steps.log"))

    def test_a_pass_waits_for_nothing_and_passed_says_so(self):  # AC-140
        self.assertEqual(self.check("--passed").returncode, 1)
        (self.root / ".checks").mkdir(exist_ok=True)
        (self.root / ".checks/0123456789abcdef.ok").write_text("2026-11-07 12:00\n")
        self.assertEqual(self.check("--passed").returncode, 0)
        self.assertEqual(self.check(GATE_EXIT="75").returncode, 0)
        self.assertEqual(self.read("gate.log"), "", "the pass runs nothing, so it waits for nothing")
        (self.root / ".checks/0123456789abcdef.docker-skip").write_text("Docker does not answer\n")
        self.assertEqual(self.check("--passed").returncode, 1, "a pass without Docker still has a step to run")


if __name__ == "__main__":
    unittest.main()
