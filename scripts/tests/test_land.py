# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Landing a worktree (spec AC-104), on real repositories in a temporary folder; no remote is contacted."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("land", Path(__file__).resolve().parents[1] / "land.py")
land = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(land)


def git(*args, cwd):
    return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


class Landing(unittest.TestCase):
    def setUp(self):
        self.home = tempfile.TemporaryDirectory()
        self.addCleanup(self.home.cleanup)
        root = Path(self.home.name)
        self.origin = root / "origin.git"
        git("init", "-q", "--bare", "-b", "main", str(self.origin), cwd=root)
        self.primary = root / "primary"
        git("clone", "-q", str(self.origin), str(self.primary), cwd=root)
        git("config", "user.name", "t", cwd=self.primary)
        git("config", "user.email", "t@example.com", cwd=self.primary)
        git("checkout", "-q", "-b", "main", cwd=self.primary)
        self.commit(self.primary, "a.txt", "one\n")
        self.commit(self.primary, "b.txt", "one\n")
        git("push", "-q", "origin", "main", cwd=self.primary)
        self.checks = []
        builds = land.builds
        land.builds = lambda _: True  # no Makefile here
        self.addCleanup(setattr, land, "builds", builds)

    def commit(self, cwd, name, text):
        (cwd / name).write_text(text)
        git("add", name, cwd=cwd)
        git("commit", "-q", "-m", name, cwd=cwd)
        return git("rev-parse", "HEAD", cwd=cwd)

    def tree(self, slug):
        path = self.primary / ".worktrees" / slug
        git("worktree", "add", "-q", "-b", f"feat/{slug}", str(path), "origin/main", cwd=self.primary)
        return path

    def green(self, tree):
        self.checks.append(tree.name)
        return True

    def on_origin(self):
        return git("rev-parse", "main", cwd=self.origin)

    def test_green_lands_and_the_primary_follows(self):
        tree = self.tree("one")
        made = self.commit(tree, "new.txt", "x\n")
        self.assertEqual(land.land(tree, check=self.green, say=lambda _: None), made)
        self.assertEqual(self.on_origin(), made)
        self.assertEqual(self.checks, ["one"])
        self.assertIsNone(land.update_primary(self.primary))
        self.assertEqual(git("rev-parse", "main", cwd=self.primary), made)

    def test_red_stays_with_nothing_pushed(self):
        tree = self.tree("one")
        self.commit(tree, "new.txt", "x\n")
        before = self.on_origin()
        with self.assertRaises(land.Stop) as stop:
            land.land(tree, check=lambda _: False, say=lambda _: None)
        self.assertEqual(stop.exception.code, 1)
        self.assertEqual(self.on_origin(), before)
        self.assertTrue(tree.is_dir())

    def test_uncommitted_work_is_refused_before_any_check(self):
        tree = self.tree("one")
        (tree / "new.txt").write_text("x\n")
        with self.assertRaises(land.Stop) as stop:
            land.land(tree, check=self.green, say=lambda _: None)
        self.assertIn("uncommitted", str(stop.exception))
        self.assertEqual(self.checks, [])

    def test_a_tree_with_nothing_of_its_own_has_nothing_to_land(self):
        tree = self.tree("one")
        before = self.on_origin()
        self.assertEqual(land.land(tree, check=self.green, say=lambda _: None), before)
        self.assertEqual(self.checks, [])

    def test_two_trees_touching_other_files_each_test_once(self):
        one, two = self.tree("one"), self.tree("two")
        self.commit(one, "one.txt", "x\n")
        self.commit(two, "two.txt", "x\n")
        land.land(one, check=self.green, say=lambda _: None)
        land.land(two, check=self.green, say=lambda _: None)
        self.assertEqual(self.checks, ["one", "two"])
        files = git("ls-tree", "--name-only", "main", cwd=self.origin).split()
        self.assertIn("one.txt", files)
        self.assertIn("two.txt", files)

    def test_another_landing_during_the_checks_is_merged_and_not_retested_unless_it_overlaps(self):
        one, other = self.tree("one"), self.tree("other")
        self.commit(one, "one.txt", "x\n")
        self.commit(other, "other.txt", "x\n")
        def lands_other_first(tree):
            self.checks.append(tree.name)
            if len(self.checks) == 1:
                land.land(other, check=lambda _: True, say=lambda _: None)
            return True
        land.land(one, check=lands_other_first, say=lambda _: None)
        self.assertEqual(self.checks, ["one"])
        self.assertIn("other.txt", git("ls-tree", "--name-only", "main", cwd=self.origin).split())
        self.assertIn("one.txt", git("ls-tree", "--name-only", "main", cwd=self.origin).split())

    def test_a_landing_on_the_same_file_during_the_checks_makes_them_run_again(self):
        one, other = self.tree("one"), self.tree("other")
        self.commit(one, "a.txt", "one\ntwo\n")
        self.commit(other, "a.txt", "zero\none\n")
        def lands_other_first(tree):
            self.checks.append(tree.name)
            if len(self.checks) == 1:
                land.land(other, check=lambda _: True, say=lambda _: None)
            return True
        land.land(one, check=lands_other_first, say=lambda _: None)
        self.assertEqual(self.checks, ["one", "one"])
        self.assertEqual(git("show", "main:a.txt", cwd=self.origin), "zero\none\ntwo")

    def test_a_clean_merge_with_others_code_builds_before_it_pushes(self):
        one, other = self.tree("one"), self.tree("other")
        self.commit(one, "one.rs", "x\n")
        self.commit(other, "other.rs", "x\n")
        def lands_other_first(tree):
            land.land(other, check=lambda _: True, build=lambda _: True, say=lambda _: None)
            return True
        before_red = lambda _: False
        with self.assertRaises(land.Stop) as stop:
            land.land(one, check=lands_other_first, build=before_red, say=lambda _: None)
        self.assertIn("build fails", str(stop.exception))
        self.assertNotIn("one.rs", git("ls-tree", "--name-only", "main", cwd=self.origin).split())

    def test_a_refused_push_with_an_unmoved_base_says_why_at_once(self):
        tree = self.tree("one")
        self.commit(tree, "new.txt", "x\n")
        hook = self.origin / "hooks" / "pre-receive"
        hook.write_text("#!/bin/sh\necho nobody may push >&2\nexit 1\n")
        hook.chmod(0o755)
        with self.assertRaises(land.Stop) as stop:
            land.land(tree, check=self.green, say=lambda _: None)
        self.assertIn("nobody may push", str(stop.exception))
        self.assertEqual(self.checks, ["one"])

    def test_after_runs_once_with_the_trees_own_files(self):
        tree = self.tree("one")
        self.commit(tree, "new.txt", "x\n")
        seen = []
        land.land(tree, check=self.green, say=lambda _: None, after=seen.append)
        self.assertEqual(seen, [{"new.txt"}])

    def test_a_sweep_taking_the_merged_tree_during_after_still_lands(self):
        tree = self.tree("one")
        made = self.commit(tree, "new.txt", "x\n")
        swept = lambda _: git("worktree", "remove", str(tree), cwd=self.primary)
        self.assertEqual(land.land(tree, check=self.green, say=lambda _: None, after=swept), made)
        self.assertFalse(tree.exists())
        self.assertEqual(self.on_origin(), made)

    def test_only_a_change_with_code_counts_as_code(self):
        self.assertFalse(land.has_code({"docs/USAGE.md", "README.md"}))
        self.assertTrue(land.has_code({"docs/USAGE.md", "terminumd/src/a.rs"}))

    def test_a_conflict_stays_in_the_tree_and_lands_once_resolved(self):
        one, other = self.tree("one"), self.tree("other")
        self.commit(one, "a.txt", "ours\n")
        self.commit(other, "a.txt", "theirs\n")
        land.land(other, check=lambda _: True, say=lambda _: None)
        before = self.on_origin()
        with self.assertRaises(land.Stop) as stop:
            land.land(one, check=self.green, say=lambda _: None)
        self.assertEqual(stop.exception.code, 2)
        self.assertIn("a.txt", str(stop.exception))
        self.assertEqual(self.on_origin(), before)
        self.assertEqual(self.checks, [])
        (one / "a.txt").write_text("both\n")
        git("add", "a.txt", cwd=one)
        git("-c", "core.editor=true", "commit", "-q", "--no-edit", cwd=one)
        land.land(one, check=self.green, say=lambda _: None)
        self.assertEqual(git("show", "main:a.txt", cwd=self.origin), "both")

    def test_a_primary_with_changes_in_the_way_stays_and_says_so(self):
        tree = self.tree("one")
        self.commit(tree, "a.txt", "changed\n")
        land.land(tree, check=self.green, say=lambda _: None)
        (self.primary / "a.txt").write_text("the maintainer's work\n")
        git("fetch", "-q", "origin", cwd=self.primary)
        self.assertIsNotNone(land.update_primary(self.primary))
        self.assertEqual((self.primary / "a.txt").read_text(), "the maintainer's work\n")


if __name__ == "__main__":
    unittest.main()
