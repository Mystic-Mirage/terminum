#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Agents and installs on a fresh origin/main (CLAUDE.md §9).

  fresh [--start] the hooks' words for the agent (.claude/settings.json), per prompt or (--start) per
      session: this checkout, its branch, how far behind origin/main; git fetch at most every 5
      minutes; nothing in another repository
  freshen         make install, before anything is built: fetch; a clean main behind origin/main is
      fast-forwarded; one behind with uncommitted changes, or diverged, is refused
      (TM_INSTALL_STALE=1: the maintainer's own install as it is); any other branch is left alone
"""
import argparse
import os
from pathlib import Path
import re
import sys
import time

from worktree import count, fetched, finished, git, has_ref, ours, primary_of, Refused, said_name

FETCH_EVERY = 300      # seconds between the hooks' fetches, for the whole repository


def commits(n):
    return f"{n} commit{'' if n == 1 else 's'}"


def behind_words(cwd):
    """Why a tree behind origin/main should hear of it, or None: main moves all the time and CLAUDE.md
    §9 merges it only when that matters."""
    mine = set(git("diff", "--name-only", "origin/main...HEAD", cwd=cwd).splitlines())
    mine |= {line[3:] for line in git("status", "--porcelain", cwd=cwd).splitlines()}
    if not mine:
        return "it holds no work yet: git merge --ff-only origin/main"
    if mine & set(git("diff", "--name-only", "HEAD...origin/main", cwd=cwd).splitlines()):
        return "origin/main changed files this branch changes: read them there, merge per CLAUDE.md §9"
    return None


def fetch_failed(primary):
    """A fetch at most every FETCH_EVERY seconds in the whole repository, the last one's result."""
    stamp = Path(git("rev-parse", "--path-format=absolute", "--git-common-dir", cwd=primary)) / "terminum-fetch"
    try:
        if time.time() - stamp.stat().st_mtime < FETCH_EVERY:
            return stamp.read_text() == "failed"
    except OSError:
        pass
    stamp.write_text("")  # first: the sessions of one moment share this fetch
    failed = fetched(primary, 5) is not None
    stamp.write_text("failed" if failed else "")
    return failed


def fresh(start=False, cwd="."):
    """The hooks' words for the agent's context: fixed sentences, counts and checked names, never a
    file's name or git's own words. At a session's start always; at a prompt only what needs saying."""
    top = git("rev-parse", "--show-toplevel", cwd=cwd, check=False)
    if top.returncode != 0:
        return []
    top, primary = Path(top.stdout.strip()), primary_of(cwd)
    failed, done = fetch_failed(primary), finished(primary)
    behind = count("HEAD..origin/main", cwd)
    if behind is None:
        return []
    why = ("read fresh code with git grep and git log against origin/main; new work: scripts/worktree.py add"
           if behind and top == primary else behind_words(cwd) if behind and top not in done else None)
    lines = []
    if start or why:
        branch = git("symbolic-ref", "--short", "-q", "HEAD", cwd=cwd, check=False).stdout.strip()
        head = (f"branch {branch}" if re.fullmatch(r"[\w./-]{1,80}", branch) else "a branch of an unusual name" if branch
                else f"DETACHED HEAD at {git('rev-parse', '--short', 'HEAD', cwd=cwd)}")
        where = f"the primary checkout {top}" if top == primary else f"worktree {said_name(top)}"
        state = f"terminum: this is {where}, {head}, {commits(behind)} behind origin/main"
        lines.append(f"{state}: {why}." if why else f"{state}.")
    if failed:
        lines.append("terminum: git fetch failed: origin/main is as last known.")
    if names := [said_name(path) for path in done]:
        lines.append(f"terminum: merged worktrees still here: {', '.join(names)}. Your own: scripts/worktree.py remove "
                     "<slug> from the primary checkout; clean ones idle for an hour go by themselves.")
    return lines


def freshen(cwd="."):
    """What `make install` runs first, so that the install carries what was merged (CLAUDE.md §9)."""
    if git("rev-parse", "--is-inside-work-tree", cwd=cwd, check=False).returncode != 0 or not has_ref("origin/main", cwd) \
            or git("symbolic-ref", "--short", "-q", "HEAD", cwd=cwd, check=False).stdout.strip() != "main":
        return []
    failed = fetched(cwd, 30)
    lines = [f"warning: git fetch origin: {failed}; installing against origin/main as last known"] if failed else []
    behind = count("HEAD..origin/main", cwd)
    if not behind:
        return lines
    if os.environ.get("TM_INSTALL_STALE"):
        return lines + [f"warning: {commits(behind)} behind origin/main, installed without them (TM_INSTALL_STALE)"]
    override = "the maintainer alone may install it as it is: TM_INSTALL_STALE=1 make install"
    if ahead := count("origin/main..HEAD", cwd):
        raise Refused(f"make install: main and origin/main have diverged ({ahead} and {behind} commits); {override}")
    if dirty := git("status", "--porcelain", cwd=cwd).splitlines():
        raise Refused(f"make install: main is {commits(behind)} behind origin/main and holds {len(dirty)} uncommitted "
                      f"files, so it would install without what was merged; {override}")
    before = git("rev-parse", "--short", "HEAD", cwd=cwd)
    git("merge", "--quiet", "--ff-only", "origin/main", cwd=cwd)
    return lines + [f"fast-forwarded main by {commits(behind)}: {before}..{git('rev-parse', '--short', 'HEAD', cwd=cwd)}"]


def main(argv):
    parser = argparse.ArgumentParser(prog="worktree_fresh.py", description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("fresh").add_argument("--start", action="store_true")
    commands.add_parser("freshen")
    args = parser.parse_args(argv)
    lines = freshen() if args.command == "freshen" else fresh(args.start) if ours(".") else []
    if lines:
        print("\n".join(lines))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Refused as refused:
        print(f"worktree: {refused}", file=sys.stderr)
        sys.exit(1)
