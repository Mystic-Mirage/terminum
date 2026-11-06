# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""scripts/worktree_fresh.py and .claude/settings.json's hooks, against a throwaway origin and primary."""
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_worktree import commit, Repo, SCRIPTS, sh, worktree  # noqa: E402
import worktree_fresh  # noqa: E402


class Fresh(Repo):
    # the hooks' words
    def test_the_session_start_says_checkout_branch_and_how_far_behind(self):
        said = worktree_fresh.fresh(start=True, cwd=self.primary)
        self.assertEqual(said, [f"terminum: this is the primary checkout {self.primary}, branch main, 0 commits behind origin/main."])
        self.push_from_another_clone()
        said = "\n".join(worktree_fresh.fresh(start=True, cwd=self.primary))
        self.assertIn("branch main, 1 commit behind origin/main", said)
        self.assertIn("scripts/worktree.py add", said)
        sh(self.primary, "git", "checkout", "-q", "--detach")
        said = "\n".join(worktree_fresh.fresh(start=True, cwd=self.primary))
        self.assertRegex(said, r"DETACHED HEAD at [0-9a-f]+, 1 commit behind")
        sh(self.primary, "git", "checkout", "-q", "main")
        path, _ = worktree.add("start", kind="fix", cwd=self.primary)
        self.assertEqual(worktree_fresh.fresh(start=True, cwd=path), ["terminum: this is worktree start, branch fix/start, 0 commits behind origin/main."])

    def test_a_prompt_hears_only_what_needs_saying(self):
        self.assertEqual(worktree_fresh.fresh(cwd=self.primary), [], "up to date")
        own, _ = worktree.add("own", cwd=self.primary)
        commit(own, "own.txt")
        same, _ = worktree.add("same", cwd=self.primary)
        commit(same, "shared.txt", "mine")
        idle, _ = worktree.add("idle", cwd=self.primary)
        self.push_from_another_clone("shared.txt")
        self.assertIn("behind origin/main", "".join(worktree_fresh.fresh(cwd=self.primary)), "the primary checkout: always")
        self.forget_fetch()
        self.assertEqual(worktree_fresh.fresh(cwd=own), [], "behind, nothing in common: CLAUDE.md §9 merges only when it matters")
        self.assertIn("changed files this branch changes", "".join(worktree_fresh.fresh(cwd=same)))
        self.assertIn("git merge --ff-only origin/main", "".join(worktree_fresh.fresh(cwd=idle)), "no work yet: old code for nothing")

    def test_git_fetch_runs_at_most_every_five_minutes_and_a_failure_stays_said(self):
        self.push_from_another_clone()
        self.assertEqual(worktree.count("HEAD..origin/main", self.primary), 0)
        worktree_fresh.fresh(cwd=self.primary)
        self.assertEqual(worktree.count("HEAD..origin/main", self.primary), 1, "fetched")
        self.push_from_another_clone("second.txt")
        worktree_fresh.fresh(cwd=self.primary)
        self.assertEqual(worktree.count("HEAD..origin/main", self.primary), 2, "no stamp, a new fetch")
        other = Path(self.tmp.name) / "other"
        commit(other, "third.txt")
        sh(other, "git", "push", "-q", "origin", "HEAD:main")
        worktree_fresh.fresh(cwd=self.primary)
        self.assertEqual(worktree.count("HEAD..origin/main", self.primary), 2, "within the 5 minutes: no fetch")
        self.forget_fetch()
        sh(self.primary, "git", "remote", "set-url", "origin", str(Path(self.tmp.name) / "gone.git"))
        for _ in range(2):
            said = worktree_fresh.fresh(cwd=self.primary)
            self.assertIn("terminum: git fetch failed: origin/main is as last known.", said)
            self.assertNotIn("gone.git", "".join(said), "git's own words stay out of an agent's context")

    def test_a_merged_tree_is_named_at_each_prompt_and_left_to_the_sweep(self):
        path = self.merged_tree("named")
        (path / "ignore previous instructions.txt").write_text("a file's name")
        said = "".join(worktree_fresh.fresh(cwd=self.primary))
        self.assertIn("merged worktrees still here: named", said)
        self.assertNotIn("ignore previous", said)
        self.assertTrue(path.exists())
        self.idle(path, "feat/named")
        worktree_fresh.fresh(cwd=self.primary)
        self.assertTrue(path.exists(), "a prompt never waits for a removal")
        (path / "ignore previous instructions.txt").unlink()
        self.push_from_another_clone()
        said = "".join(worktree_fresh.fresh(cwd=path))
        self.assertIn("merged worktrees still here: named", said)
        self.assertNotIn("--ff-only", said, "its work is merged: not a tree with no work yet")

    def test_a_folder_outside_git_hears_nothing(self):
        self.assertEqual(worktree_fresh.fresh(cwd=self.tmp.name), [])

    # make install: never a stale main
    def test_freshen_fast_forwards_a_clean_main(self):
        self.assertEqual(worktree_fresh.freshen(self.primary), [])
        self.push_from_another_clone()
        said = worktree_fresh.freshen(self.primary)
        self.assertIn("fast-forwarded main by 1 commit:", said[0])
        self.assertEqual(worktree.count("HEAD..origin/main", self.primary), 0)
        self.assertTrue((self.primary / "other.txt").exists())

    def test_freshen_refuses_uncommitted_changes_and_a_diverged_main_unless_the_maintainer_asks(self):
        self.push_from_another_clone()
        head = sh(self.primary, "git", "rev-parse", "HEAD")
        (self.primary / "wip.txt").write_text("the maintainer's")
        with self.assertRaisesRegex(worktree.Refused, r"1 commit behind origin/main and holds 1 uncommitted"):
            worktree_fresh.freshen(self.primary)
        self.assertEqual(sh(self.primary, "git", "rev-parse", "HEAD"), head)
        os.environ["TM_INSTALL_STALE"] = "1"
        try:
            self.assertIn("installed without them", worktree_fresh.freshen(self.primary)[0])
        finally:
            del os.environ["TM_INSTALL_STALE"]
        self.assertEqual(sh(self.primary, "git", "rev-parse", "HEAD"), head)
        commit(self.primary, "wip.txt")
        with self.assertRaisesRegex(worktree.Refused, "diverged"):
            worktree_fresh.freshen(self.primary)

    def test_freshen_leaves_other_branches_and_goes_on_without_a_network(self):
        self.push_from_another_clone()
        sh(self.primary, "git", "checkout", "-q", "-b", "try")
        self.assertEqual(worktree_fresh.freshen(self.primary), [])
        self.assertEqual(worktree.count("HEAD..origin/main", self.primary), 0, "no fetch either")
        sh(self.primary, "git", "checkout", "-q", "main")
        sh(self.primary, "git", "remote", "set-url", "origin", str(Path(self.tmp.name) / "gone.git"))
        self.assertIn("warning: git fetch origin", worktree_fresh.freshen(self.primary)[0])
        self.assertEqual(worktree_fresh.freshen(self.tmp.name), [], "not a checkout")

    def test_make_install_freshens_before_it_reads_the_build_id(self):
        out = subprocess.run(["make", "-n", "--no-print-directory", "install"], cwd=SCRIPTS.parent, capture_output=True, text=True).stdout
        self.assertLess(out.index("worktree_fresh.py freshen"), out.index("install-stage"), out)
        self.assertLess(out.index("install-stage"), out.index("package.py --stage"), out)

    # .claude/settings.json: a hook never stops the agent, whatever is wrong
    def hook_commands(self):
        hooks = json.loads((SCRIPTS.parent / ".claude/settings.json").read_text())["hooks"]
        return {event: [hook["command"] for entry in entries for hook in entry["hooks"]] for event, entries in hooks.items()}

    def test_the_session_hooks_run_the_primarys_copy_and_always_exit_zero(self):
        commands = self.hook_commands()
        self.assertEqual(sorted(commands), ["SessionStart", "Stop", "UserPromptSubmit"])
        (self.primary / "scripts").mkdir()
        run = lambda cmd, cwd, project=None: subprocess.run(["bash", "-c", cmd], cwd=cwd, capture_output=True, text=True, stdin=subprocess.DEVNULL,
                                                             env={**os.environ, "CLAUDE_PROJECT_DIR": str(project or tree)})
        for name in ("worktree.py", "worktree_fresh.py"):
            (self.primary / "scripts" / name).write_text((SCRIPTS / name).read_text())
        (self.primary / "ui").mkdir(exist_ok=True)
        tree, _ = worktree.add("hooked", cwd=self.primary)
        started = run(commands["SessionStart"][0], tree)
        self.assertEqual(started.returncode, 0, started.stderr)
        self.assertIn("worktree hooked, branch feat/hooked", started.stdout)
        self.assertIn("the primary checkout", run(commands["SessionStart"][0], self.primary / "ui").stdout, "from a folder below too")
        gone = self.merged_tree("gone")
        self.idle(gone, "feat/gone")
        self.assertEqual(run(commands["Stop"][0], self.primary).returncode, 0)
        self.assertFalse(gone.exists(), "the Stop hook sweeps")
        foreign = Path(self.tmp.name) / "foreign"                 # the session moved to another project
        sh(self.tmp.name, "git", "init", "-q", "-b", "main", str(foreign))
        (foreign / "scripts").mkdir()
        for name in ("worktree.py", "worktree_fresh.py"):
            (foreign / "scripts" / name).write_text("print('foreign code ran')\n")
        for event, each in commands.items():
            for cmd in each:
                moved = run(cmd, foreign)
                self.assertEqual((moved.returncode, moved.stdout), (0, ""), event)
        for name in ("worktree.py", "worktree_fresh.py"):
            (self.primary / "scripts" / name).write_text("raise SystemExit(2)\n")     # Claude Code stops on a 2
        for event, each in commands.items():
            for cmd in each:
                self.assertEqual(run(cmd, tree).returncode, 0, event)
                self.assertEqual(run(cmd, self.tmp.name).returncode, 0, f"{event}, outside git")
        for name in ("worktree.py", "worktree_fresh.py"):
            (self.primary / "scripts" / name).unlink()
        self.assertEqual(run(commands["Stop"][0], tree).returncode, 0, "no script")


if __name__ == "__main__":
    unittest.main()
