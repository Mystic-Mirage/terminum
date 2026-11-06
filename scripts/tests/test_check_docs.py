# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""The docs' shape (spec AC-103) on a small synthetic repository."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("check_docs", Path(__file__).resolve().parents[1] / "check_docs.py")
check_docs = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_docs)

WRONG = "terminum." + "md AC-39"  # the same: a citation that names the wrong spec
GONE = "AC-" + "26"  # no spec has it; spelled so this file does not cite it
LOG = "| 0.1.0 | 2026-01-01 | First. |\n"


def spec(cases="| AC-01 | [MUST / P0] | The user types. |\n", scenarios="### AC-01 — Typing [MUST / P0]\n\n```gherkin\nScenario: AC-01 happy path\n```\n",
         proof="- [x] AC-01 — web/test/e2e_x.mjs\n", log=LOG):
    bodies = {0: f"| Version | 0.1.0 |\n\n{log}", 4: cases, 5: scenarios, 11: proof}
    return "# Spec\n\n" + "\n".join(f"## {i}. {title}\n\n{bodies.get(i, 'Text.')}\n" for i, title in enumerate(check_docs.SECTIONS))


class Shape(unittest.TestCase):
    def setUp(self):
        self.home = tempfile.TemporaryDirectory()
        self.addCleanup(self.home.cleanup)
        self.root = Path(self.home.name)
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)
        self.write("docs/specs/terminum.md", spec())

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def problems(self):
        return "\n".join(check_docs.check_all(self.root))

    def test_a_good_tree_has_no_findings(self):
        self.assertEqual(check_docs.check_all(self.root), [])

    def test_a_section_out_of_order(self):
        self.write("docs/specs/terminum.md", spec().replace("## 7. Current behavior and gaps", "## 7. Today"))
        self.assertIn("sections must be", self.problems())

    def test_a_tag_that_is_not_allowed(self):
        self.write("docs/specs/terminum.md", spec(cases="| AC-01 | [MAY / P2] | Typing. |\n", scenarios="### AC-01 — Typing [MAY / P2]\n\nScenario: x\n"))
        self.assertIn("AC-01 has the tag '[MAY / P2]'", self.problems())

    def test_tags_that_differ(self):
        self.write("docs/specs/terminum.md", spec(scenarios="### AC-01 — Typing [SHOULD / P1]\n\nScenario: x\n"))
        self.assertIn("AC-01 is [MUST / P0] in section 4, [SHOULD / P1] in section 5", self.problems())

    def test_a_case_with_no_scenario_or_no_proof(self):
        self.write("docs/specs/terminum.md", spec(scenarios="", proof=""))
        self.assertIn("AC-01 has no scenarios in section 5", self.problems())
        self.assertIn("AC-01 has no line of proof in section 11", self.problems())

    def test_a_heading_with_no_scenario_inside(self):
        self.write("docs/specs/terminum.md", spec(scenarios="### AC-01 — Typing [MUST / P0]\n\nNothing yet.\n"))
        self.assertIn("AC-01 has no scenario", self.problems())

    def test_scenarios_with_no_row(self):
        self.write("docs/specs/terminum.md", spec(cases=""))
        self.assertIn("AC-01 has scenarios but no row in section 4", self.problems())

    def test_a_case_in_two_specs(self):
        self.write("docs/specs/terminum-agents.md", spec())
        self.assertIn("AC-01 in both", self.problems())

    def test_a_dropped_case_needs_only_its_row(self):
        self.write("docs/specs/terminum.md", spec(cases="| AC-01 | [MUST / P0] | The user types. [DROPPED — no need, 2026-01-01] |\n", scenarios="", proof=""))
        self.assertEqual(check_docs.check_all(self.root), [])

    def test_a_citation_of_a_case_no_spec_has(self):
        self.write("web/test/x.mjs", f"// {GONE} from the old product\n")
        self.assertIn(f"web/test/x.mjs: cites {GONE}, which no spec has", self.problems())

    def test_a_citation_naming_the_wrong_spec(self):
        self.write("docs/specs/terminum-agents.md", spec(
            cases="| AC-39 | [MUST / P0] | Agents. |\n", scenarios="### AC-39 — Agents [MUST / P0]\n\nScenario: x\n", proof="- [x] AC-39 — x\n"))
        self.write("x.rs", f"// {WRONG}\n")
        self.assertIn(f"x.rs: says {WRONG}; it is in terminum-agents.md", self.problems())
        self.write("x.rs", "// terminum-agents.md AC-39\n")
        self.assertEqual(check_docs.check_all(self.root), [])

    def test_history_and_release_notes_may_cite_what_is_gone(self):
        self.write("docs/specs/history/terminum.md", f"{GONE} and [a link](gone.md)\n")
        self.write("docs/releases/v0.1.0.md", f"{GONE}\n")
        self.assertEqual(check_docs.check_all(self.root), [])

    def test_a_link_that_leads_nowhere(self):
        self.write("docs/USAGE.md", "[Specs](specs/gone.md), [ok](specs/terminum.md), [web](https://example.com/x), [part](#use), ![img](x.png)\n")
        problems = self.problems()
        self.assertIn("docs/USAGE.md: the link specs/gone.md leads nowhere", problems)
        self.assertEqual(problems.count("leads nowhere"), 1)

    def test_too_many_change_log_rows(self):
        self.write("docs/specs/terminum.md", spec(log="".join(f"| 0.1.{i} | 2026-01-01 | Row. |\n" for i in range(11))))
        self.assertIn("11 change-log rows, at most 10", self.problems())

    def test_a_big_spec_and_an_uncited_case_only_warn(self):
        self.write("docs/specs/terminum.md", spec(cases="| AC-01 | [MUST / P0] | " + "x" * 160_000 + " |\n"))
        self.assertEqual(check_docs.check_all(self.root), [])
        found = "\n".join(check_docs.warnings(self.root))
        self.assertIn("past 150 KB", found)
        self.assertIn("no test cites AC-01", found)

    def test_a_copy_with_no_git_is_still_read(self):
        subprocess.run(["rm", "-rf", str(self.root / ".git")], check=True)
        self.write("web/test/x.mjs", f"// {GONE}\n")
        self.write("terminumd/target/debug/build.rs", f"// {GONE}\n")
        problems = self.problems()
        self.assertIn(f"web/test/x.mjs: cites {GONE}", problems)
        self.assertNotIn("target", problems)

    def test_no_specs_is_a_finding_not_a_pass(self):
        (self.root / "docs" / "specs" / "terminum.md").unlink()
        self.assertIn("no specs", self.problems())


if __name__ == "__main__":
    unittest.main()
