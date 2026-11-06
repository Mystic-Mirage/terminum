# Contributing

Thanks for helping. Read [CLAUDE.md](CLAUDE.md) first: it holds the project rules (English
only, the 500-line cap, tests and the Definition of Done) for humans and AI agents alike.

## Contributor license agreement

The project is dual-licensed (AGPL-3.0 and a commercial license, see
[LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md)). To keep that possible, every contribution
needs this agreement. By opening a pull request you confirm that:

1. You wrote the contribution, or have the right to submit it; if your employer has rights
   in it, your employer has allowed you to contribute it under these terms.
2. You grant Gregory Zemskov and any successors and assigns ("the Maintainer") a
   perpetual, worldwide, non-exclusive, royalty-free, irrevocable license to use, modify,
   sublicense and relicense the contribution, including under the commercial license, and
   to transfer these rights.
3. You grant the Maintainer a patent license for any of your patents that the contribution
   necessarily infringes, on the same terms.
4. You keep the copyright of your contribution and may use it under any terms.

The agreement is recorded by a CLA check on the pull request (until it is set up:
`Signed-off-by: Your Name <you@example.com>` on each commit, `git commit -s`).

## Worktrees

Each writing agent, and any change made while another session's or agent's changes are in the
checkout, gets its own branch and worktree, made and removed only by `scripts/worktree.py`;
read-only work (a question, a review) and `make release` (the primary checkout's `main`) need none.

| Step | How |
| --- | --- |
| Make | `python3 scripts/worktree.py add <slug> --kind feat\|fix\|test\|docs\|chore`: branch `<kind>/<slug>` named from the task (`fix/paste-newlines`, never asked for), from a freshly fetched `origin/main` (`--from <ref>`; it warns when `main` has unpushed commits), without upstream (a bare `git push` never lands on `main`), checked out in `<primary>/.worktrees/<slug>` (git-excluded); a tree made otherwise (`git worktree add`, Claude Code's own) gets the links below by `worktree.py link` in it |
| Linked | `.toolchain`, `local.mk`, `CLAUDE.local.md`, `MEMORY.md`, `.claude/settings.local.json`: symlinks to the primary checkout's, one copy |
| Cloned | copy-on-write on APFS (a reflink on Linux, else not made): `ui/node_modules` when `ui/package-lock.json` matches the primary's (else `make` runs `npm ci`), `terminumd/target/release` without its binaries (the dependencies' builds: only terminumd compiles) |
| Own | `terminumd/target/debug` (a cold `cargo build`, about 30 s; the primary's is too big to clone), `ui/dist`, `dist/`, `.checks/`; shared by git itself: the hooks (`core.hooksPath`) and the stash (never a bare `git stash`) |
| Parallel | `make test` runs in several trees at once (cargo tests' tmux sockets carry their pid); `make check` and the checks that start terminumd, tmux or Docker (`web/test/e2e_live.mjs`, `ui/test/e2e_panel.mjs`, `scripts/e2e_*.py`) use fixed ports (47900–47999), tmux sockets (`tm-test-live`…) whose cleanup ends that socket's server whichever tree started it, and Docker's `tm-test-sshd` on `127.0.0.1:2222` (`e2e_remote_sshd*.py`, the only Docker users: no stack to bring up, Docker Desktop running): run them in one tree at a time |
| Install | the machine has one running terminumd, so `make install` runs from the primary checkout only (in a worktree it is refused; `TM_INSTALL_FROM_WORKTREE=1` is the maintainer's own try of a branch): a tree makes `make build` and its checks, and after the merge the primary checkout's `make install` and `make installed` carry it: `make install` fetches first (`scripts/worktree_fresh.py`) and fast-forwards a clean `main` behind `origin/main`; behind with uncommitted changes, or diverged, it refuses (`TM_INSTALL_STALE=1`: the maintainer's install as it is; a failed fetch only warns), any other branch it leaves alone |
| Finish | `worktree.py list`: each tree's branch, uncommitted files, commits not in `origin/main`, processes in it; after the merge, from the primary checkout, `worktree.py remove <slug>`: it refuses while a process works in the tree (exit that shell or session; it ends nothing) or, without `--discard`, while the tree holds uncommitted changes, ignored files that are not build output, or commits on a detached HEAD; it deletes the branch only when it is in `origin/main` or `main` |
| Forgotten | `worktree.py sweep`, from Claude Code's Stop hook (`.claude/settings.json`) and every `worktree.py add`, so with any agent or by hand: a tree in `.worktrees` whose branch's last move was its own work (a commit or a real merge, looking through fast-forwards and rebases; not its creation or a reset), all in `origin/main` or `main`, with no commit and no index write for an hour, is removed as `remove` would, with its refusals and never `--discard`; a squash merge is not seen, nor are Claude Code's own `.claude/worktrees`. Its remote branch stays. `.worktrees/.git` is an invalid git file on purpose: a removed tree's folder made again finds it, so git there fails instead of working on the primary checkout's `main` |
| Hooks | `.claude/settings.json` runs the primary checkout's `scripts/worktree_fresh.py` and `worktree.py`, always exiting 0 and doing nothing in another repository: at a session's start `fresh --start` says the checkout, the branch (or a detached HEAD) and how far behind `origin/main`; at each prompt `fresh` says it for the primary checkout when behind, for a tree when it holds no work yet or `origin/main` changed files it changes, and names merged trees still here; `git fetch` at most every 5 minutes for the repository. Only sessions in this repository: not `terminum hooks install`, not the package |

## Before a pull request

```bash
make test                         # cargo test, typecheck, UI unit tests
cd ui && node test/e2e_panel.mjs  # browser checks (once: npx playwright-core install chromium webkit)
```

Changes to behavior update the specs in `docs/specs/` (scenarios first) in the same
pull request.
