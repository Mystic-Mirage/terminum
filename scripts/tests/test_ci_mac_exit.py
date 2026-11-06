# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""scripts/ci_mac.sh's exit (spec AC-104), ssh and rsync stubbed: failing tests must never read as a runner that
could not run them, since `make release` stops on the one and goes on past the other."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "ci_mac.sh"
STUB = """#!/bin/bash
case "$*" in
  *"bash -lc"*) exit "${STUB_RUN:-0}" ;;
  *"mkdir ci/terminum.lock"*) exit "${STUB_LOCK:-0}" ;;
esac
exit 0
"""


class Exit(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        for name, body in (("ssh", STUB), ("rsync", "#!/bin/bash\nexit \"${STUB_RSYNC:-0}\"\n")):
            path = Path(tmp.name) / name
            path.write_text(body)
            path.chmod(0o755)
        self.path = tmp.name

    def exit_of(self, **stubs):
        env = {**os.environ, "PATH": f"{self.path}:{os.environ['PATH']}", "TM_MAC_RUNNER": "mac-runner", "TMPDIR": self.path, **stubs}
        return subprocess.run(["bash", str(SCRIPT)], env=env, capture_output=True, text=True).returncode

    def test_the_run_says_what_happened(self):
        self.assertEqual(self.exit_of(), 0)
        for run, code in (("1", 1), ("2", 1), ("127", 1), ("4", 2), ("255", 2)):
            self.assertEqual(self.exit_of(STUB_RUN=run), code, f"the run's exit {run}")

    def test_before_the_run(self):
        self.assertEqual(self.exit_of(STUB_LOCK="255"), 2, "no answer")
        self.assertEqual(self.exit_of(STUB_LOCK="3"), 3, "busy")
        self.assertEqual(self.exit_of(STUB_LOCK="1"), 2, "its folder could not be made")
        self.assertEqual(self.exit_of(STUB_RSYNC="12"), 2, "the tree did not get there")


if __name__ == "__main__":
    unittest.main()
