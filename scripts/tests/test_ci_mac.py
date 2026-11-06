# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""The Mac checks' runner script (scripts/ci_mac.sh) against a stub ssh and rsync: the runner's
lock is taken before its tree is replaced, so a second run never changes the files of one under
way; it goes once the run ended, and stays (said) when the link dropped."""
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "ci_mac.sh"
# ssh: its commands run in the stub runner's home but the run itself, only logged (it ends with
# RUN_RC, 255 a dropped link); rsync: logged
SSH = """#!/bin/bash
for a; do last=$a; done
echo "ssh ${last//$'\n'/ }" >> "$LOG"
case $last in *"bash -lc"*) exit "${RUN_RC:-0}" ;; *"mkdir ci/terminum.lock"*) [ -z "${LOCK_RC:-}" ] || exit "$LOCK_RC" ;; esac
cd "$RUNNER" && bash -c "$last"
"""
RSYNC = """#!/bin/bash
echo "rsync" >> "$LOG"
[ -z "${RSYNC_RC:-}" ] || exit "$RSYNC_RC"
src=${@: -2:1}
python3 - "$src" "$STAGED_MANIFEST" <<'PY'
from pathlib import Path
import sys
root = Path(sys.argv[1])
Path(sys.argv[2]).write_text(str(root) + '\\n' + '\\n'.join(str(p.relative_to(root)) for p in root.rglob('*')))
PY
"""
GIT = """#!/bin/sh
case "$*" in
  *--cached*) printf 'AGENTS.md\\0CLAUDE.md\\0Makefile\\0scripts/ci_source.py\\0credentials.json\\0' ;;
  *--others*) printf 'scripts/tests/test_ci_source.py\\0' ;;
  *) exit 1 ;;
esac
"""


class CiMacTest(unittest.TestCase):
    def run_script(self, root, rc=0, **stubs):
        env = {**os.environ, "RUN_RC": str(rc), "PATH": f"{root / 'bin'}:{os.environ['PATH']}", "LOG": str(root / "log"),
               "RUNNER": str(root / "runner"), "TM_MAC_RUNNER": "mac-runner", "STAGED_MANIFEST": str(root / "manifest"), **stubs}
        r = subprocess.run(["bash", str(SCRIPT)], env=env, capture_output=True, text=True, timeout=30)
        log = (root / "log").read_text().splitlines() if (root / "log").exists() else []
        (root / "log").unlink(missing_ok=True)
        return r, log

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / "bin").mkdir()
        (self.root / "runner").mkdir()
        # The synced Mac-runner tree has no .git: this unit fixture supplies its inventory.
        for name, text in (("ssh", SSH), ("rsync", RSYNC), ("git", GIT)):
            (self.root / "bin" / name).write_text(text)
            (self.root / "bin" / name).chmod(0o755)

    def tearDown(self):
        self.tmp.cleanup()

    def test_a_run_under_way_keeps_its_tree(self):
        (self.root / "runner/ci/terminum.lock").mkdir(parents=True)
        r, log = self.run_script(self.root)
        self.assertEqual(r.returncode, 3, r.stderr)
        self.assertNotIn("rsync", log, "the tree of the run under way is left as it is")
        self.assertTrue((self.root / "runner/ci/terminum.lock").is_dir(), "and its lock too")

    def test_the_lock_comes_before_the_tree_and_goes_when_the_run_ended(self):
        for rc in (0, 1):
            with self.subTest(rc=rc):
                r, log = self.run_script(self.root, rc)
                self.assertEqual(r.returncode, rc, r.stderr)
                self.assertEqual([x.split()[0] for x in log], ["ssh", "rsync", "ssh", "ssh"], log)
                self.assertIn("command -v tmux", log[2], "a runner without tmux is refused, not a run of skipped checks")
                self.assertFalse((self.root / "runner/ci/terminum.lock").exists(), "the run ended, passed or failed: its lock went")

    def test_a_dropped_link_leaves_the_lock_and_says_so(self):
        r, log = self.run_script(self.root, 255)
        self.assertEqual(r.returncode, 2, "could not run: it stops no release")
        self.assertIn("remove ~/ci/terminum.lock", r.stderr)
        self.assertTrue((self.root / "runner/ci/terminum.lock").is_dir(), "the run may go on there")

    def test_its_exit_says_what_happened(self):  # AC-104: make release stops on 1 and goes on past 2 and 3
        for run, code in ((2, 1), (127, 1), (4, 2)):
            self.assertEqual(self.run_script(self.root, run)[0].returncode, code, f"the run's exit {run}")
        self.assertEqual(self.run_script(self.root, LOCK_RC="255")[0].returncode, 2, "no answer")
        self.assertEqual(self.run_script(self.root, LOCK_RC="1")[0].returncode, 2, "its folder could not be made")
        self.assertEqual(self.run_script(self.root, RSYNC_RC="12")[0].returncode, 2, "the tree did not get there")

    def test_rsync_receives_a_staged_source_tree_without_local_files(self):
        r, _ = self.run_script(self.root)
        self.assertEqual(r.returncode, 0, r.stderr)
        lines = (self.root / "manifest").read_text().splitlines()
        self.assertTrue(lines[0].startswith("/tmp/tm-ci-src."), lines[0])
        self.assertIn("scripts/ci_source.py", lines)
        self.assertIn("AGENTS.md", lines)
        for name in ["local.mk", "CLAUDE.local.md", "MEMORY.md", "project-memory", "ui/node_modules", ".codex", ".claude"]:
            self.assertNotIn(name, lines)
        self.assertFalse(Path(lines[0]).exists(), "the owned staging tree is removed on exit")

    def test_staging_creation_failure_releases_the_remote_lock(self):
        file = self.root / "bin/mktemp"
        file.write_text("#!/bin/sh\nexit 1\n"); file.chmod(0o700)
        r, log = self.run_script(self.root)
        self.assertNotEqual(r.returncode, 0)
        self.assertNotIn("rsync", log)
        self.assertFalse((self.root / "runner/ci/terminum.lock").exists())


if __name__ == "__main__":
    unittest.main()
