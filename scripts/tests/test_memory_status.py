# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""What a session is told of the project's memory (CLAUDE.md "Project memory")."""
from datetime import date
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("memory_status", Path(__file__).resolve().parents[1] / "memory_status.py")
memory_status = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(memory_status)
TODAY = date(2026, 11, 6)
INDEX = "# Index\n\nLast consolidated: 2026-11-01.\n\n| `state.md` | |\n| `handoff.md` | |\n| `infra.md` | |\n"


class Status(unittest.TestCase):
    def setUp(self):
        self.home = tempfile.TemporaryDirectory()
        self.addCleanup(self.home.cleanup)
        self.dir = Path(self.home.name)
        (self.dir / "INDEX.md").write_text(INDEX)
        (self.dir / "state.md").write_text("v0.31 is out\n")
        (self.dir / "handoff.md").write_text("nothing\n")
        (self.dir / "infra.md").write_text("a private machine\n")

    def test_it_prints_the_entry_files_only(self):
        said = memory_status.report(self.dir, TODAY)
        self.assertIn("v0.31 is out", said)
        self.assertNotIn("a private machine", said)
        self.assertNotIn("Consolidation is due", said)

    def test_no_memory_says_so_and_creates_nothing(self):
        self.assertIn("not found", memory_status.report(self.dir / "none", TODAY))
        self.assertFalse((self.dir / "none").exists())

    def test_consolidation_is_due_when_old(self):
        self.assertIn("over 14 days", memory_status.report(self.dir, date(2026, 11, 20)))

    def test_consolidation_is_due_for_a_long_file_a_bad_stamp_or_an_unlisted_file(self):
        (self.dir / "infra.md").write_text("x\n" * 151)
        (self.dir / "new.md").write_text("x\n")
        (self.dir / "INDEX.md").write_text(INDEX.replace("2026-11-01", "soon"))
        said = memory_status.report(self.dir, TODAY)
        self.assertIn("infra.md has 151 lines", said)
        self.assertIn("new.md is not in INDEX.md", said)
        self.assertIn("no valid", said)

    def test_a_link_is_not_followed(self):
        outside = self.dir / "outside.txt"
        outside.write_text("a secret\n")
        (self.dir / "state.md").unlink()
        (self.dir / "state.md").symlink_to(outside)
        said = memory_status.report(self.dir, TODAY)
        self.assertNotIn("a secret", said)
        self.assertIn("state.md is not a regular file", said)


if __name__ == "__main__":
    unittest.main()
