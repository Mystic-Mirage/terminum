---
name: docs-review
description: Review terminum's docs and specs against what changed since the newest release tag: new behavior gets BDD scenarios with acceptance cases, duplicates and superseded requirements go, each fact stays in one place. A step of a minor release (CLAUDE.md §10); also run when the maintainer asks for /docs-review, or /docs-review full for every doc.
---

# docs-review

`/docs-review`: what changed since the newest `v*` tag. `/docs-review full`: every doc and spec, which takes several agents and rounds; only when the maintainer asks.

`scripts/check_docs.py`, in `make test`, already holds the shape (spec AC-103). This skill is for what a script cannot judge.

## 1. Scope

`git fetch origin`, then `python3 scripts/worktree.py add docs-review-<date> --kind docs --from main` (in a release, with `/simplify`'s fixes already committed on `main`, so this tree sees them). List `git log --oneline <tag>..HEAD`, and `git diff --stat <tag>..HEAD`. `python3 scripts/check_docs.py`: its findings come first.

## 2. Read the changes, then the docs they touch

For each change since the tag, in the spec that owns it (`docs/README.md` says which):

| Question | What to do |
|---|---|
| New or changed behavior with no scenario | Add it to the owning case's `gherkin`, or a new `AC-nn` (the next free number across the specs) with its §4 row and §11 line. Concrete values, exact messages, an error scenario for I/O, input, auth or concurrency |
| A scenario the change made false | Correct it to what the code does on purpose. If the code looks wrong instead, it is a defect: tell the maintainer |
| The same rule in two places | Keep it where `docs/README.md` puts it; elsewhere, delete it or point to it |
| A requirement that no longer applies | Delete it only with proof: the code does otherwise on purpose, or a later decision says so. Name each deletion and its proof in the report. Absent from the code but never decided against: a §7 gap and a question for the maintainer, never a deletion |
| A §7 gap that is closed, or a §11 name that has gone | Remove the gap; name what proves the case now |
| `OQ-nn` | Never deleted; resolved ones carry their decision and date |

Do not turn every commit into a scenario. A breakable rule a user or security relies on earns one; the why of a change belongs in its commit and the release notes. A spec's change log keeps at most ten one-line rows, of changes to its specified behavior; older rows go to `docs/specs/history/`.

## 3. Check and report

`python3 scripts/check_docs.py` reads `docs: 0 findings`. Check each `FAIL` fixed and read each `WARN`. The report to the maintainer: what was added, what was deleted with proof, gaps opened, questions. Decisions on deleting a requirement that lacks proof are theirs; ask together, in one question.

A release's request covers its commit (CLAUDE.md §10); otherwise commit only when asked. The tree's commits merge to `main`, then `worktree.py remove <slug>`.

## Agents

A `/docs-review full` gives each spec one writing agent, each in its own worktree, with a done condition and a cap of at most 15 minutes from `date`, its findings streamed to a report file, and in plain words what it must not do: other files, builds, tests, signals, ssh, git state.
