---
name: docs-review
description: Review terminum's docs and specs against what changed since the newest release tag: new behavior gets BDD scenarios with acceptance cases, duplicates and superseded requirements go, each fact stays in one place. A step of a minor release (CLAUDE.md §10); also run when the maintainer asks for /docs-review, or /docs-review full for every doc.
---

# docs-review

`/docs-review`: what changed since the newest `v*` tag. `/docs-review full`: every doc and spec, which takes several agents and rounds; only when the maintainer asks.

`scripts/check_docs.py` reports on the shape (spec AC-103); this skill is for what a script cannot judge. Neither blocks a release: only failing tests do (CLAUDE.md §10).

## 1. Scope

`git fetch origin`, then `python3 scripts/worktree.py add docs-review-<date> --kind docs --from main` (in a release, with `/simplify`'s fixes already landed on `main`, so this tree sees them). List `git log --oneline <tag>..HEAD`, and `git diff --stat <tag>..HEAD`. `python3 scripts/check_docs.py`: its findings come first.

## 2. Read the changes, then the docs they touch

For each change since the tag, in the spec that owns it (`docs/README.md` says which):

| Question | What to do |
|---|---|
| New or changed behavior with no scenario | Add it to the owning case's `gherkin`, or a new `AC-nn` (the next free number across the specs) with its §4 row and §11 line. Concrete values, exact messages, an error scenario for I/O, input, auth or concurrency |
| A scenario and the code disagree | The spec says what terminum must do, and the code does not outrank it. Change the scenario only when the maintainer decided otherwise or the code shows a deliberate decision: a commit or a test saying so. Otherwise it is a §7 gap, the scenario stays as it was, and the maintainer hears of it. Never rewrite a scenario, message or number to match what the code happens to do |
| The same rule in two places | Keep it where `docs/README.md` puts it; elsewhere, delete it or point to it |
| A requirement that no longer applies | Delete it only with proof of a decision: a maintainer's word, or a commit that removed it on purpose. Name each deletion and its proof in the report. Absent from the code is not proof: a spec may hold what is not built. It stays, with a §7 gap |
| A §7 gap that is closed, or a §11 name that has gone | Remove the gap; name what proves the case now |
| `OQ-nn` | Never deleted; resolved ones carry their decision and date |

Do not turn every commit into a scenario. A breakable rule a user or security relies on earns one; the why of a change belongs in its commit and the release notes. A spec is a snapshot: no change log, no dates of evidence, no "was" or "now", and no account of how a rule came to be.

## 3. Check and report

`python3 scripts/check_docs.py` should read `docs: 0 findings`: fix each `FAIL`, read each `WARN`. The report to the maintainer: what was added, what was deleted with proof, gaps opened, questions. Decisions on deleting a requirement that lacks proof are theirs; ask together, in one question.

Commit in the tree and land it (`make land SLUG=<slug>`, CLAUDE.md §9).

## Agents

A `/docs-review full` gives each spec one writing agent, each in its own worktree, with a done condition and a cap of at most 15 minutes from `date`, its findings streamed to a report file, and in plain words what it must not do: other files, builds, tests, signals, ssh, git state.
