# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""scripts/worktree.py against a throwaway origin and primary checkout in a temporary folder."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import worktree  # noqa: E402
ENV = {"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com", "GIT_COMMITTER_NAME": "t",
       "GIT_COMMITTER_EMAIL": "t@example.com", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}


def sh(cwd, *args):
    return subprocess.run(args, cwd=cwd, check=True, capture_output=True, text=True,
                          env={**os.environ, **ENV}).stdout.strip()


def commit(cwd, name, text="x"):
    (Path(cwd) / name).parent.mkdir(parents=True, exist_ok=True)
    (Path(cwd) / name).write_text(text)
    sh(cwd, "git", "add", name)
    sh(cwd, "git", "commit", "-q", "-m", f"add {name}")


class Repo(unittest.TestCase):
    def setUp(self):
        self.saved = {k: os.environ.get(k) for k in [*ENV, "TM_WORKTREES"]}
        os.environ.update(ENV)
        os.environ.pop("TM_WORKTREES", None)
        self.tmp = tempfile.TemporaryDirectory(prefix="tm-wt-")
        base = Path(self.tmp.name).resolve()
        self.origin, self.primary = base / "origin.git", base / "primary"
        sh(base, "git", "init", "-q", "--bare", "-b", "main", str(self.origin))
        sh(base, "git", "clone", "-q", str(self.origin), str(self.primary))
        sh(self.primary, "git", "checkout", "-q", "-b", "main")
        (self.primary / ".gitignore").write_text("ui/node_modules/\nterminumd/target/\n.toolchain/\nlocal.mk\n")
        sh(self.primary, "git", "add", ".gitignore")
        commit(self.primary, "ui/package-lock.json", "{}")
        sh(self.primary, "git", "push", "-q", "origin", "main")
        # the primary's local files and caches
        (self.primary / "ui/node_modules/pkg").mkdir(parents=True)
        (self.primary / "ui/node_modules/pkg/index.js").write_text("module")
        (self.primary / "terminumd/target/release/deps").mkdir(parents=True)
        (self.primary / "terminumd/target/release/deps/libdep.rlib").write_text("dep")
        (self.primary / "terminumd/target/release/terminumd").write_text("primary's binary")
        (self.primary / "terminumd/target/debug/deps").mkdir(parents=True)
        (self.primary / "terminumd/target/debug/deps/libdep.rlib").write_text("dep")
        (self.primary / "terminumd/target/debug/deps/terminumd-0123456789abcdef").write_text("the crate's own test binary")
        (self.primary / ".toolchain/bin").mkdir(parents=True)
        (self.primary / ".toolchain/bin/zig").write_text("zig")
        (self.primary / "local.mk").write_text("TM_CODESIGN_IDENTITY=x\n")
        # a folder with a file: an empty one copies with --reflink=always on any disk (nothing to clone)
        probe = Path(self.tmp.name) / "probe"
        probe.mkdir()
        (probe / "f").write_text("x")
        self.clones = worktree.clone(probe, Path(self.tmp.name) / "probe-clone")

    def tearDown(self):
        for key, value in self.saved.items():
            os.environ.pop(key, None) if value is None else os.environ.__setitem__(key, value)
        self.tmp.cleanup()

    def merged_tree(self, slug):
        path, _ = worktree.add(slug, cwd=self.primary)
        commit(path, f"{slug}.txt")
        sh(self.primary, "git", "merge", "-q", "--no-edit", f"feat/{slug}")
        sh(self.primary, "git", "push", "-q", "origin", "main")
        return path

    def idle(self, path, branch):
        names = sh(path, "git", "rev-parse", "--path-format=absolute", "--git-path", "index", "--git-path", f"logs/refs/heads/{branch}")
        for name in names.splitlines():
            os.utime(name, (1, 1))

    def push_from_another_clone(self, name="other.txt"):
        other = Path(self.tmp.name) / "other"
        if not other.exists():
            sh(self.tmp.name, "git", "clone", "-q", str(self.origin), str(other))
        sh(other, "git", "pull", "-q", "origin", "main")
        commit(other, name)
        sh(other, "git", "push", "-q", "origin", "HEAD:main")
        self.forget_fetch()

    def forget_fetch(self):
        (self.primary / ".git" / "terminum-fetch").unlink(missing_ok=True)


class Worktrees(Repo):
    def test_add_branches_from_fresh_origin_main_and_links_the_primarys_files(self):
        # a commit another clone pushed after the primary's last fetch
        other = Path(self.tmp.name) / "other"
        sh(self.tmp.name, "git", "clone", "-q", str(self.origin), str(other))
        commit(other, "fresh.txt")
        sh(other, "git", "push", "-q", "origin", "main")
        path, lines = worktree.add("thing", "fix", cwd=self.primary)
        self.assertEqual(path, self.primary / ".worktrees/thing")
        self.assertTrue((path / "fresh.txt").exists(), lines)
        self.assertEqual(sh(path, "git", "branch", "--show-current"), "fix/thing")
        self.assertEqual(subprocess.run(["git", "rev-parse", "@{upstream}"], cwd=path, capture_output=True).returncode,
                         128, "no upstream: a bare push never lands on main")
        for name in (".toolchain", "local.mk"):
            self.assertTrue((path / name).is_symlink() and (path / name).resolve() == self.primary / name, name)
        modules = path / "ui/node_modules"
        if self.clones:
            self.assertFalse(modules.is_symlink())
            self.assertEqual((modules / "pkg/index.js").read_text(), "module")
            self.assertGreaterEqual(modules.stat().st_mtime, (path / "ui/package-lock.json").stat().st_mtime)
            self.assertTrue((path / "terminumd/target/release/deps/libdep.rlib").exists())
            self.assertFalse((path / "terminumd/target/release/terminumd").exists(), "the primary's binary")
            self.assertTrue((path / "terminumd/target/debug/deps/libdep.rlib").exists(), "a warm debug build (AC-104)")
            self.assertFalse((path / "terminumd/target/debug/deps/terminumd-0123456789abcdef").exists(), "the crate's own: rebuilt")
        else:  # a disk without reflinks (ext4): make and cargo build them, nothing half made is left
            self.assertFalse(modules.exists() or (path / "terminumd/target").exists())
            self.assertIn("no clone on this disk", "\n".join(lines))
        self.assertEqual(sh(path, "git", "status", "--porcelain"), "", "links and clones are not changes")
        self.assertEqual(sh(self.primary, "git", "status", "--porcelain"), "", ".worktrees is excluded")
        self.assertIn("thing", "\n".join(worktree.listing(self.primary)))

    def test_add_refuses_when_the_fetch_fails_and_makes_nothing(self):
        sh(self.primary, "git", "remote", "set-url", "origin", str(Path(self.tmp.name) / "gone.git"))
        with self.assertRaises(worktree.Refused) as said:
            worktree.add("no-fetch", cwd=self.primary)
        self.assertIn("fetch", str(said.exception))
        self.assertFalse((self.primary / ".worktrees" / "no-fetch").exists())
        self.assertFalse(worktree.has_ref("refs/heads/feat/no-fetch", self.primary))

    def test_add_refuses_bad_names_and_existing_trees_or_branches(self):
        for slug, kind in (("Bad Slug", "feat"), ("../x", "feat"), ("ok", "wip")):
            with self.assertRaises(worktree.Refused):
                worktree.add(slug, kind, cwd=self.primary)
        worktree.add("one", cwd=self.primary)
        with self.assertRaises(worktree.Refused):
            worktree.add("one", cwd=self.primary)
        sh(self.primary, "git", "branch", "feat/two")
        with self.assertRaisesRegex(worktree.Refused, "branch feat/two exists"):
            worktree.add("two", cwd=self.primary)

    def test_a_differing_lockfile_leaves_node_modules_to_make(self):
        sh(self.primary, "git", "push", "-q", "origin", "main:side")
        other = Path(self.tmp.name) / "other"
        sh(self.tmp.name, "git", "clone", "-q", "-b", "side", str(self.origin), str(other))
        commit(other, "ui/package-lock.json", '{"changed": 1}')
        sh(other, "git", "push", "-q", "origin", "side")
        path, lines = worktree.add("locks", base="origin/side", cwd=self.primary)
        self.assertFalse((path / "ui/node_modules").exists())
        self.assertIn("make installs it", "\n".join(lines))

    def test_remove_keeps_work_and_never_touches_the_primarys_files(self):
        path, _ = worktree.add("gone", cwd=self.primary)
        (path / "new.txt").write_text("unsaved")
        with self.assertRaisesRegex(worktree.Refused, "uncommitted"):
            worktree.remove("gone", cwd=self.primary)
        commit(path, "new.txt", "work")
        lines = worktree.remove("gone", cwd=self.primary)
        self.assertFalse(path.exists())
        self.assertIn("kept branch feat/gone: 1 commits", "\n".join(lines))
        self.assertEqual((self.primary / ".toolchain/bin/zig").read_text(), "zig")
        self.assertEqual((self.primary / "local.mk").read_text(), "TM_CODESIGN_IDENTITY=x\n")
        self.assertEqual((self.primary / "ui/node_modules/pkg/index.js").read_text(), "module")

    def test_remove_deletes_a_merged_branch_and_discard_drops_changes(self):
        path, _ = worktree.add("done", cwd=self.primary)
        (path / "scratch.txt").write_text("throwaway")
        lines = worktree.remove(str(path), discard=True, cwd=self.primary)
        self.assertFalse(path.exists())
        self.assertIn("deleted branch feat/done", "\n".join(lines))
        self.assertNotIn("feat/done", sh(self.primary, "git", "branch"))

    def test_add_warns_when_main_has_unpushed_commits(self):
        commit(self.primary, "local.txt")
        path, lines = worktree.add("behind", cwd=self.primary)
        self.assertIn("main has 1 commits not in origin/main", "\n".join(lines))
        self.assertFalse((path / "local.txt").exists())
        _, lines = worktree.add("ahead", base="main", cwd=self.primary)
        self.assertNotIn("warning", "\n".join(lines))

    def test_remove_refuses_ignored_files_that_are_not_build_output(self):
        path, _ = worktree.add("notes", cwd=self.primary)
        (path / "ui/node_modules").mkdir(parents=True, exist_ok=True)
        (path / "local.mk").unlink()
        (path / "local.mk").write_text("the tree's own settings")
        with self.assertRaisesRegex(worktree.Refused, "ignored files that are not build output:\nlocal.mk"):
            worktree.remove("notes", cwd=self.primary)
        (path / "local.mk").unlink()
        worktree.remove("notes", cwd=self.primary)
        self.assertFalse(path.exists())

    def test_remove_refuses_commits_on_a_detached_head(self):
        path, _ = worktree.add("loose", cwd=self.primary)
        sh(path, "git", "switch", "-q", "--detach")
        commit(path, "loose.txt")
        with self.assertRaisesRegex(worktree.Refused, "detached HEAD"):
            worktree.remove("loose", cwd=self.primary)
        self.assertIn("detached", "\n".join(worktree.listing(self.primary)))
        worktree.remove("loose", discard=True, cwd=self.primary)
        self.assertFalse(path.exists())

    def test_a_slug_wins_over_a_folder_of_that_name(self):
        path, _ = worktree.add("ui", cwd=self.primary)
        worktree.remove("ui", cwd=self.primary)
        self.assertFalse(path.exists())
        self.assertTrue((self.primary / "ui/package-lock.json").exists())

    def test_make_hooks_names_the_primarys_hooks_by_an_absolute_path_from_any_tree(self):
        path, _ = worktree.add("hooked2", cwd=self.primary)
        for cwd in (self.primary, path):
            subprocess.run(["make", "-s", "-f", str(SCRIPTS.parent / "Makefile"), "hooks"], cwd=cwd, check=True, capture_output=True,
                           env={**os.environ, "PATH": os.environ["PATH"]})
            self.assertEqual(sh(cwd, "git", "config", "core.hooksPath"), str(self.primary / "scripts/hooks"), "a branch's own hooks never run")

    def test_make_install_runs_from_the_primary_checkout_only(self):
        path, _ = worktree.add("inst", cwd=self.primary)
        make = lambda cwd, **env: subprocess.run(["make", "-s", "-f", str(SCRIPTS.parent / "Makefile"), "primary-checkout"],
                                                 cwd=cwd, capture_output=True, text=True, env={**os.environ, **env})
        self.assertEqual(make(self.primary).returncode, 0)
        refused = make(path)
        self.assertEqual(refused.returncode, 2, refused.stderr)
        self.assertIn("refused in a worktree", refused.stdout)
        self.assertEqual(make(path, TM_INSTALL_FROM_WORKTREE="1").returncode, 0)

    def test_remove_refuses_while_a_process_works_in_the_tree(self):
        path, _ = worktree.add("busy", cwd=self.primary)
        sleeper = subprocess.Popen(["sleep", "30"], cwd=path)
        try:
            with self.assertRaisesRegex(worktree.Refused, f"{sleeper.pid} sleep"):
                worktree.remove("busy", cwd=self.primary)
            self.assertTrue(path.exists())
        finally:
            sleeper.kill()
            sleeper.wait()
        worktree.remove("busy", cwd=self.primary)
        self.assertFalse(path.exists())

    # the sweep: a tree finished and forgotten goes, anything that may still be someone's work stays
    def test_sweep_takes_a_merged_idle_tree_and_its_branch(self):
        path = self.merged_tree("done")
        self.assertEqual(worktree.sweep(self.primary), [], "just merged: its agent may be between commands")
        self.assertTrue(path.exists())
        self.idle(path, "feat/done")
        self.assertIn(f"removed {path}", worktree.sweep(self.primary))
        self.assertFalse(path.exists())
        self.assertFalse(worktree.has_ref("refs/heads/feat/done", self.primary))

    def test_sweep_leaves_what_never_held_work_or_still_holds_some(self):
        new, _ = worktree.add("new", cwd=self.primary)            # an ancestor of main with nothing of its own
        forward, _ = worktree.add("forward", cwd=self.primary)
        reset, _ = worktree.add("reset", cwd=self.primary)
        commit(reset, "reset.txt")
        sh(reset, "git", "reset", "-q", "--hard", "HEAD~1")       # dropped, not merged
        ahead, _ = worktree.add("ahead", cwd=self.primary)
        commit(ahead, "ahead.txt")                                # not merged
        partly = self.merged_tree("partly")
        commit(partly, "more.txt")                                # a commit after the merge
        dirty = self.merged_tree("dirty")
        (dirty / "dirty.txt").write_text("uncommitted")
        ignored = self.merged_tree("ignored")
        (ignored / "local.mk").unlink(missing_ok=True)
        (ignored / "local.mk").write_text("own settings")
        commit(self.primary, "newer.txt")
        sh(self.primary, "git", "push", "-q", "origin", "main")
        sh(forward, "git", "merge", "-q", "--ff-only", "origin/main")   # the behind line's advice, nothing of its own
        trees = {new: "new", forward: "forward", reset: "reset", ahead: "ahead", partly: "partly", dirty: "dirty", ignored: "ignored"}
        for tree, slug in trees.items():
            self.idle(tree, f"feat/{slug}")
        said = worktree.sweep(self.primary)
        for tree in trees:
            self.assertTrue(tree.exists(), tree)
        self.assertEqual(sorted(said), ["kept dirty: in use or holding changes (worktree.py list)",
                                        "kept ignored: in use or holding changes (worktree.py list)"])

    def test_sweep_leaves_a_tree_with_a_process_and_trees_elsewhere(self):
        busy = self.merged_tree("busy")
        self.idle(busy, "feat/busy")
        elsewhere = Path(self.tmp.name) / "elsewhere"
        sh(self.primary, "git", "worktree", "add", "-q", "--no-track", "-b", "feat/elsewhere", str(elsewhere), "origin/main")
        commit(elsewhere, "e.txt")
        sh(self.primary, "git", "merge", "-q", "--no-edit", "feat/elsewhere")
        self.idle(elsewhere, "feat/elsewhere")
        sleeper = subprocess.Popen(["sleep", "30"], cwd=busy)
        try:
            self.assertEqual(worktree.sweep(self.primary), ["kept busy: in use or holding changes (worktree.py list)"])
            self.assertTrue(busy.exists())
        finally:
            sleeper.kill()
            sleeper.wait()
        self.assertIn(f"removed {busy}", worktree.sweep(self.primary))
        self.assertTrue(elsewhere.exists(), "only the script's own folder is swept")

    def test_sweep_looks_through_fast_forwards_and_rebases(self):
        pulled = self.merged_tree("pulled")                       # merged, then the old advice's fast-forward
        self.push_from_another_clone()
        sh(pulled, "git", "merge", "-q", "--ff-only", "origin/main")
        rebased, _ = worktree.add("rebased", cwd=self.primary)    # no work of its own, a rebase moved it
        self.push_from_another_clone("second.txt")
        sh(rebased, "git", "fetch", "-q", "origin")
        sh(rebased, "git", "rebase", "-q", "origin/main")
        self.idle(pulled, "feat/pulled")
        self.idle(rebased, "feat/rebased")
        said = worktree.sweep(self.primary)
        self.assertIn(f"removed {pulled}", said)
        self.assertTrue(rebased.exists(), said)

    def test_a_sweep_that_loses_a_race_says_nothing_and_add_still_makes_its_tree(self):
        old = self.merged_tree("raced")
        self.idle(old, "feat/raced")
        with mock.patch.object(worktree, "idle_for", side_effect=FileNotFoundError("gone")):
            self.assertEqual(worktree.sweep(self.primary), ["kept raced: in use or holding changes (worktree.py list)"])
        with mock.patch.object(worktree, "sweep", side_effect=OSError("gone")):
            path, lines = worktree.add("after", cwd=self.primary)
        self.assertTrue(path.exists())
        self.assertIn("warning: gone", lines)

    def test_list_says_why_a_merged_tree_is_still_here(self):
        path = self.merged_tree("listed")
        (path / "local.mk").unlink(missing_ok=True)
        (path / "local.mk").write_text("own settings")
        said = "\n".join(worktree.listing(self.primary))
        self.assertIn("sweep takes it once clean and idle for an hour", said)
        self.assertIn("1 ignored to keep", said)

    def test_add_sweeps_for_those_without_claude_codes_hooks(self):
        old = self.merged_tree("old")
        self.idle(old, "feat/old")
        _, lines = worktree.add("next", cwd=self.primary)
        self.assertIn(f"removed {old}", lines)

    def test_a_folder_made_again_after_removal_never_reaches_the_primary_checkout(self):
        path = self.primary / ".worktrees/again"                  # made by hand: no guard from link
        sh(self.primary, "git", "worktree", "add", "-q", "--no-track", "-b", "feat/again", str(path), "origin/main")
        self.assertFalse((self.primary / ".worktrees/.git").exists())
        commit(path, "again.txt")
        sh(self.primary, "git", "merge", "-q", "--no-edit", "feat/again")
        self.idle(path, "feat/again")
        worktree.sweep(self.primary)
        (self.primary / "tracked.txt").write_text("the maintainer's")
        sh(self.primary, "git", "add", "tracked.txt")
        before = sh(self.primary, "git", "rev-parse", "main")
        path.mkdir()                                              # a write by absolute path, its agent unaware
        (path / "f.txt").write_text("x")
        run = subprocess.run(["git", "commit", "-qam", "stray"], cwd=path, capture_output=True, text=True)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("invalid gitfile", run.stderr)
        self.assertEqual(sh(self.primary, "git", "rev-parse", "main"), before)
        self.assertIn("A  tracked.txt", sh(self.primary, "git", "status", "--porcelain"))

    def test_remove_refuses_the_primary_and_strangers(self):
        with self.assertRaisesRegex(worktree.Refused, "primary"):
            worktree.remove(str(self.primary), cwd=self.primary)
        with self.assertRaisesRegex(worktree.Refused, "no worktree"):
            worktree.remove("nothing", cwd=self.primary)

    def test_the_command_line_prints_usage_and_refuses_unknown_commands(self):
        run = lambda *a: subprocess.run([sys.executable, str(SCRIPTS / "worktree.py"), *a], cwd=self.primary,
                                        capture_output=True, text=True)
        self.assertEqual(run("--help").returncode, 0)
        for wrong in (["bogus"], ["remove", "x", "--dicard"], ["add", "x", "--kind"], ["add", "x", "--kind", "wip"]):
            self.assertEqual(run(*wrong).returncode, 2, wrong)
        added = run("add", "cli", "--kind", "docs")
        self.assertEqual(added.returncode, 0, added.stderr)
        self.assertIn("docs/cli at", added.stdout)
        self.assertEqual(run("add", "cli").returncode, 1)


if __name__ == "__main__":
    unittest.main()
