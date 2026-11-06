# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Mac-runner source selection with synthetic paths and files; no remote is contacted."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("ci_source", Path(__file__).resolve().parents[1] / "ci_source.py")
ci_source = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci_source)


class SourceSelection(unittest.TestCase):
    def test_sensitive_and_local_names_are_rejected_even_if_tracked(self):
        for name in ["credentials.json", "cert.pem", ".codex/config.toml", "ui/.env.local", "docs/secrets.txt",
                     "local.mk", "CLAUDE.local.md", "MEMORY.md", "project-memory/INDEX.md", "docs/project-memory/x.md", "ui/node_modules/x.js", "terminumd/target/x.rs",
                     "scripts/private.key", "scripts/.docker/config.json", "scripts/.kube/config", "../outside.rs", "/tmp/outside.rs"]:
            for tracked in [True, False]:
                with self.subTest(name=name, tracked=tracked):
                    self.assertFalse(ci_source.approved(name, tracked))

    def test_new_source_and_json_fixtures_are_kept_but_arbitrary_local_files_are_not(self):
        for name in ["scripts/new_test.py", "web/test/new.test.mjs", "ui/test/fixture.json", "terminumd/src/new.rs"]:
            self.assertTrue(ci_source.approved(name, False), name)
        self.assertTrue(ci_source.approved("web/static/tokens.css", True))
        self.assertFalse(ci_source.approved("personal-notes.md", False))
        self.assertFalse(ci_source.approved("scripts/output.log", False))

    def test_staging_never_opens_excluded_files_or_follows_symlinks(self):
        with tempfile.TemporaryDirectory() as folder:
            root, destination = Path(folder) / "repo", Path(folder) / "stage"
            (root / "scripts").mkdir(parents=True)
            (root / "scripts/new.py").write_text("print(1)\n")
            (root / "AGENTS.md").symlink_to("missing-target")
            (root / "scripts/outside.py").symlink_to("/synthetic-private/location")
            (root / "scripts/escape.py").symlink_to("../../outside")
            (root / "scripts/private.py").symlink_to("../credentials.json")
            # Excluded names exist only in the synthetic Git inventory: opening one would fail.
            tracked = b"AGENTS.md\0credentials.json\0cert.pem\0.codex/config.toml\0scripts/outside.py\0scripts/escape.py\0scripts/private.py\0"
            added = b"scripts/new.py\0personal-notes.md\0"
            with patch.object(ci_source.subprocess, "check_output", side_effect=[tracked, added]):
                ci_source.stage(root, destination)
            self.assertEqual((destination / "scripts/new.py").read_text(), "print(1)\n")
            self.assertEqual((destination / "AGENTS.md").readlink(), Path("missing-target"))
            self.assertEqual(sorted(p.name for p in destination.iterdir()), ["AGENTS.md", "scripts"])

    def test_a_symlinked_ancestor_cannot_move_a_tracked_source_outside_the_repo(self):
        with tempfile.TemporaryDirectory() as folder:
            root, outside, destination = Path(folder) / "repo", Path(folder) / "outside", Path(folder) / "stage"
            (root / "ui").mkdir(parents=True); outside.mkdir(); destination.mkdir()
            (outside / "module.rs").write_text("synthetic outside text")
            (root / "ui/src").symlink_to(outside)
            with patch.object(ci_source.subprocess, "check_output", side_effect=[b"ui/src/module.rs\0", b""]):
                ci_source.stage(root, destination)
            self.assertFalse((destination / "ui/src/module.rs").exists())
