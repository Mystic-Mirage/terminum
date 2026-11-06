# terminum's docs

Each fact lives in one place; elsewhere, point to it. Read from the top down: this page says which document holds a question, a spec's section 4 lists its cases in a line each, and section 5 and section 9 hold the detail. `scripts/check_docs.py` reports on the shape and `/docs-review` looks after the rest (spec AC-103).

| Looking for | It is in |
|---|---|
| What terminum is and how to install it | [README](../README.md) |
| How to use it, every feature and command | [USAGE](USAGE.md) |
| How the parts fit, one picture | [ARCHITECTURE](ARCHITECTURE.md) |
| What it does exactly: cases, scenarios, limits, the test that proves each | [Specs](#specs) |
| The threat model | [SECURITY](../SECURITY.md) |
| The UI's look and its change checklist | [UI-GUIDELINES](UI-GUIDELINES.md) |
| How to contribute, worktrees | [CONTRIBUTING](../CONTRIBUTING.md) |
| The rules for humans and agents | [CLAUDE.md](../CLAUDE.md) |
| How a release is built, signed and published; CI | [RELEASING](RELEASING.md) |
| What changed in each release | [releases/](releases/) |

## Specs

A spec is a snapshot of what terminum must do: never a chronicle or a change log (git holds those), and it may hold what is not built yet. Acceptance cases `AC-nn` are unique across the specs and never renumbered. A requirement is a BDD scenario under its case.

| Spec | Holds | Cases |
|---|---|---|
| [terminum.md](specs/terminum.md) | The core: session list, screen, typing, sizing, managing sessions, sign-in, themes, keys, budgets, projects, the user's own tmux | AC-01…07, 09, 10, 12, 13, 18…20, 88, 89, 92, 100, 101 |
| [terminum-lifecycle.md](specs/terminum-lifecycle.md) | Install, run, restart, reboot, upgrade, uninstall, update notice; a tidy machine and docs for a release | AC-08, 11, 14…17, 96, 102, 103 |
| [terminum-terminal.md](specs/terminum-terminal.md) | Terminal features: widgets, folds, bookmarks, reminders, search, links, paste, selection, traces, sent history | AC-31…38, 40, 90, 91 |
| [terminum-agents.md](specs/terminum-agents.md) | Coding agents: states, unread, plans and progress, the bookmarks and reminders they keep | AC-39, 93, 97, 98 |
| [terminum-files.md](specs/terminum-files.md) | The Files panel | AC-51…73, 94, 95 |
| [terminum-remote.md](specs/terminum-remote.md) | Remote hosts: tmux over ssh, files through an agent, helpers, builds and releases, agents on a host | AC-81…87, 99 |

Each spec has the same sections: 0 Metadata (status only), 1 Overview, 2 Scope and non-goals, 3 Assumptions, dependencies, open questions, 4 Acceptance cases, 5 BDD scenarios, 6 Flow and sequence diagrams, 7 Current behavior and gaps (open gaps only), 8 Recommendation and ownership, 9 Technical details and specifications, 10 Delivery stages (only what is not shipped), 11 Checklist with Definition of Done (one line per case: what proves it).
