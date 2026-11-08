# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Which checks a change gets (spec AC-104, AC-146); nothing is run."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("check_changed", Path(__file__).resolve().parents[1] / "check_changed.py")
check_changed = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_changed)
REPO = Path(__file__).resolve().parents[2]


def steps(files, cited=()):
    return dict(check_changed.plan(files, set(), root=REPO, tests_of_cases=list(cited)))


class Plan(unittest.TestCase):
    def test_docs_alone_build_nothing_and_their_check_blocks_nothing(self):
        got = steps(["docs/USAGE.md", "docs/specs/terminum.md"])
        self.assertEqual(list(got), ["security", "docs"])
        self.assertTrue(got["docs"].startswith("-"))
        self.assertFalse(got["security"].startswith("-"), "a public repository: the scan blocks")

    def test_a_rust_change_builds_lints_tests_its_module_and_the_critical_ones(self):
        got = steps(["terminumd/src/hosts/tunnel.rs"])
        self.assertNotIn("build", got, "no release build in a tree: the primary checkout's install builds it")
        self.assertIn("debug-build", got)
        self.assertIn("clippy", got)
        self.assertIn("hosts::tunnel", got["cargo"])
        for critical in check_changed.CRITICAL_CARGO:
            self.assertIn(critical, got["cargo"])
        self.assertIn("critical", got)

    def test_a_rust_source_naming_cases_at_its_top_brings_the_e2e_scripts_citing_them(self):  # AC-104
        cases = check_changed.touched_cases(["terminumd/src/tmux/server_local.rs"], "HEAD", cwd=REPO)
        self.assertLessEqual({"01", "89"}, cases)
        self.assertIn("scripts/e2e_attach.py", check_changed.citing(cases, REPO))

    def test_a_rust_source_naming_no_case_is_said(self):  # AC-104
        with tempfile.TemporaryDirectory() as root:
            src = Path(root, "terminumd/src")
            src.mkdir(parents=True)
            (src / "plain.rs").write_text("//! A helper.\n" + "\n" * 6 + "// AC-07 too far down to count\n")
            (src / "named.rs").write_text("//! Sizing (AC-13).\n")
            (src / "plain_tests.rs").write_text("")
            files = ["terminumd/src/plain.rs", "terminumd/src/named.rs", "terminumd/src/plain_tests.rs", "terminumd/src/cli/tests/mod.rs", "terminumd/src/gone.rs", "web/static/a.js"]
            self.assertEqual(check_changed.caseless(files, Path(root)), ["terminumd/src/plain.rs"])

    def test_the_module_of_a_file(self):
        self.assertEqual(check_changed.module_of("terminumd/src/cli/install_tests.rs"), "cli::install")
        self.assertEqual(check_changed.module_of("terminumd/src/cli/tests/mod.rs"), "cli::tests")
        self.assertEqual(check_changed.module_of("terminumd/src/agentmarks.rs"), "agentmarks")
        self.assertIsNone(check_changed.module_of("terminumd/src/main.rs"))

    def test_the_tests_citing_a_touched_case_run(self):
        got = steps(["terminumd/src/agentmarks.rs"], ["web/test/e2e_marks.mjs", "scripts/e2e_hooks.py", "terminumd/src/hosts/link_tests.rs"])
        self.assertIn("webMarks", got["page"])
        self.assertEqual(got["e2e_hooks"], "python3 scripts/e2e_hooks.py")
        self.assertIn("hosts::link", got["cargo"])

    def test_a_page_change_runs_what_shows_it(self):
        got = steps(["web/static/banners.js"], ["web/test/e2e_screen.mjs"])
        self.assertIn("page-unit", got)
        self.assertIn("webScreen", got["page"])
        self.assertIn("critical", got)
        self.assertNotIn("clippy", got)

    def test_a_panel_change_runs_the_panel(self):
        got = steps(["ui/src/tree.ts"])
        self.assertIn("ui-unit", got)
        self.assertIn("panel", got)

    def test_a_script_runs_the_scripts_tests_only(self):
        got = steps(["scripts/worktree.py"])
        self.assertIn("scripts", got)
        self.assertNotIn("ui", got)
        self.assertNotIn("critical", got)
        self.assertNotIn("cargo", got)

    def test_dockers_checks_take_arguments_and_are_run_by_hand(self):
        self.assertNotIn("e2e_remote_sshd", steps(["terminumd/src/hosts/link.rs"], ["scripts/e2e_remote_sshd.py"]))
        self.assertNotIn("e2e_remote_sshd", steps(["scripts/e2e_remote_sshd.py"]))

    def test_a_live_part_runs_with_the_critical_step(self):
        got = steps(["scripts/worktree.py"], ["web/test/e2e_livenotes.mjs"])
        self.assertEqual(got["critical"], "node web/test/e2e_live.mjs chromium")
        self.assertIn("debug-build", got)

    def test_an_e2e_script_has_the_debug_build_it_runs(self):
        got = steps(["terminumd/src/x.rs"], ["scripts/e2e_hooks.py"])
        self.assertLess(list(got).index("debug-build"), list(got).index("e2e_hooks"))

    def test_a_helper_of_a_page_check_runs_with_it(self):
        self.assertIn("webApp", steps(["web/static/x.js"], ["web/test/e2e_appscreen.mjs"])["page"])

    def test_a_new_case_is_found_and_its_tests_looked_for(self):
        new = "7" + "77"  # spelled so this file does not cite it
        diff = f"+### AC-{new} — A thing [SHOULD / P1]\n### AC-01 — Old\n-### AC-02 — Gone\n"
        self.assertEqual(check_changed.new_cases(diff), {new})
        self.assertEqual(check_changed.uncited({new}, REPO), [new])
        self.assertEqual(check_changed.uncited({"104"}, REPO), [])

    def test_every_plan_is_the_full_run_less(self):
        got = steps(["terminumd/src/hosts/tunnel.rs"])
        self.assertNotIn("make check", " ".join(got.values()))


class Land(unittest.TestCase):
    def test_a_landings_smoke_set_is_the_basics_on_one_platform(self):  # AC-104
        got = dict(check_changed.land_plan(["terminumd/src/hosts/tunnel.rs", "docs/USAGE.md"]))
        self.assertEqual(list(got), ["security", "ui", "debug-build", "cargo", "e2e_core", "e2e_attach", "smoke"])
        self.assertEqual(got["smoke"], "node web/test/e2e_live.mjs chromium smoke")
        self.assertNotIn("hosts::tunnel", got["cargo"], "the tree's own tests passed before it landed")
        self.assertEqual(dict(check_changed.land_plan(["docs/USAGE.md"])), {"security": check_changed.SCAN})

    def test_builds_leave_room_for_other_trees(self):  # AC-104
        for got in (steps(["terminumd/src/hosts/tunnel.rs"]), dict(check_changed.land_plan(["terminumd/src/x.rs"]))):
            for name in ("debug-build", "cargo"):
                self.assertIn("nice -n 10 cargo", got[name])

    def test_only_timed_steps_wait_for_a_quiet_machine(self):  # AC-146
        self.assertEqual([n for n in ("security", "cargo", "panel", "page", "critical", "e2e_hooks", "smoke") if check_changed.timed(n)],
                         ["panel", "page", "critical", "e2e_hooks"])


class Run(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root, self.events = Path(tmp.name), []

    def run_steps(self, steps, busy=None, **kw):
        def fake(cmd, **_):
            self.events.append(cmd)
            return subprocess.CompletedProcess(cmd, 0)

        def gate(_):
            self.events.append("gate")
            return busy
        with mock.patch.object(check_changed.subprocess, "run", fake), \
                mock.patch.object(check_changed, "for_task", gate), mock.patch("sys.stdout"):
            return check_changed.run(steps, root=self.root, **kw)

    def test_a_machine_still_busy_skips_the_timed_steps_and_says_so_last(self):  # AC-146
        self.assertEqual(self.run_steps([("cargo", "true"), ("page", "true"), ("e2e_hooks", "true")], busy="load 31 over 16"), [])
        self.assertEqual([e if e == "gate" else e[-1] for e in self.events], ["true", "gate"])
        self.assertEqual(check_changed.skipped_note(self.root),
                         "targeted: SKIPPED timed checks, the machine busy for 15 minutes: page e2e_hooks")

    def test_timed_steps_that_ran_clear_an_earlier_skip(self):  # AC-146
        self.run_steps([("page", "true")], busy="load 31 over 16")
        self.run_steps([("cargo", "true")])
        self.assertIsNotNone(check_changed.skipped_note(self.root))   # nothing timed ran: the skip stands
        self.run_steps([("e2e_core", "true")], gate=False)   # a landing's smoke set asks no gate
        self.assertIsNotNone(check_changed.skipped_note(self.root))
        self.run_steps([("page", "true")])
        self.assertIsNone(check_changed.skipped_note(self.root))

    def test_what_needs_no_quiet_machine_runs_before_the_wait(self):  # AC-146
        self.run_steps([("panel", "a"), ("page-unit", "b"), ("e2e_hooks", "c")])
        self.assertEqual([e if e == "gate" else e[-1] for e in self.events], ["b", "gate", "a", "c"])

    def test_the_first_timed_step_waits_once_for_a_quiet_machine(self):  # AC-146
        self.run_steps([("cargo", "true"), ("page", "true"), ("e2e_hooks", "true")])
        self.assertEqual([e if e == "gate" else e[-1] for e in self.events], ["true", "gate", "true", "true"])

    def test_a_landings_steps_never_wait_and_yield_during_a_release(self):  # AC-104, AC-146
        self.run_steps([("e2e_core", "true")], gate=False, nice=True)
        self.assertEqual(self.events[0][:3], ["nice", "-n", "10"])
        self.assertNotIn("gate", self.events)


class Passed(unittest.TestCase):
    def main_with(self, tmp, run, ids, argv=()):
        with mock.patch.object(check_changed, "REPO", Path(tmp)), \
                mock.patch.object(check_changed, "tree_id", side_effect=ids), \
                mock.patch.object(check_changed, "changed_files", return_value=(["docs/x.md"], "HEAD")), \
                mock.patch.object(check_changed, "touched_cases", return_value=set()), \
                mock.patch.object(check_changed, "run", run), mock.patch("sys.stdout") as out:
            (Path(tmp) / ".checks").mkdir(exist_ok=True)
            return check_changed.main(list(argv)), "".join(c.args[0] for c in out.write.call_args_list)

    def test_the_same_content_runs_once(self):  # AC-104
        with tempfile.TemporaryDirectory() as tmp:
            ran = []
            run = lambda steps, **_: ran.append(steps) or []   # noqa: E731
            same = ["3f9a21c40b7d5e11"] * 9
            self.assertEqual(self.main_with(tmp, run, same, ["--passed"])[0], 1)
            self.assertEqual(self.main_with(tmp, run, same)[0], 0)
            code, said = self.main_with(tmp, run, same)
            self.assertEqual((code, len(ran)), (0, 1))
            self.assertIn("targeted: passed already on 3f9a21c40b7d5e11", said)
            self.assertEqual(self.main_with(tmp, run, same, ["--passed"])[0], 0)

    def test_a_red_run_records_no_pass(self):  # AC-104
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(self.main_with(tmp, lambda steps, **_: ["page"], ["aaaa", "aaaa"])[0], 1)
            self.assertEqual(list((Path(tmp) / ".checks").glob("changed-*.ok")), [])

    def test_content_changed_during_the_run_records_no_pass(self):  # AC-104
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(self.main_with(tmp, lambda steps, **_: [], ["aaaa", "bbbb"])[0], 0)
            self.assertEqual(list((Path(tmp) / ".checks").glob("changed-*.ok")), [])

    def test_the_id_is_the_content_not_the_commit(self):  # AC-104
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(["git", "init", "-q", tmp], check=True)
            (root / ".gitignore").write_text(".checks/\n")
            (root / "a.txt").write_text("one\n")
            first = check_changed.tree_id(root)
            self.assertRegex(first, r"^[0-9a-f]{16}$")
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "-C", tmp, "add", "-A"], check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "-C", tmp, "commit", "-qm", "x"], check=True)
            self.assertEqual(check_changed.tree_id(root), first, "a commit keeps the content")
            (root / ".checks").mkdir()
            (root / ".checks" / "changed-x.ok").write_text("")
            self.assertEqual(check_changed.tree_id(root), first, "what is ignored is no content")
            (root / "a.txt").write_text("two\n")
            self.assertNotEqual(check_changed.tree_id(root), first, "an edit")
            (root / "a.txt").write_text("one\n")
            (root / "new.txt").write_text("")
            self.assertNotEqual(check_changed.tree_id(root), first, "a new file")


if __name__ == "__main__":
    unittest.main()
