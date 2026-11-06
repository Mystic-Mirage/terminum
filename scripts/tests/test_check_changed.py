# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Which checks a change gets (spec AC-104); nothing is run."""
import importlib.util
from pathlib import Path
import unittest

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
        self.assertIn("build", got)
        self.assertIn("clippy", got)
        self.assertIn("hosts::tunnel", got["cargo"])
        for critical in check_changed.CRITICAL_CARGO:
            self.assertIn(critical, got["cargo"])
        self.assertIn("critical", got)

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
        self.assertNotIn("critical", got)
        self.assertNotIn("cargo", got)

    def test_dockers_checks_run_only_when_changed_themselves(self):
        self.assertNotIn("e2e_remote_sshd", steps(["terminumd/src/hosts/link.rs"], ["scripts/e2e_remote_sshd.py"]))
        self.assertIn("e2e_remote_sshd", steps(["scripts/e2e_remote_sshd.py"]))

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


if __name__ == "__main__":
    unittest.main()
