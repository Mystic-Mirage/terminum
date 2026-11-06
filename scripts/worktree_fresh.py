#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Agents and installs on a fresh origin/main (CLAUDE.md §9).

  fresh [--start] the hooks' words for the agent (.claude/settings.json), per prompt or (--start) per
      session: this checkout, its branch, how far behind origin/main; git fetch at most every 5
      minutes; nothing in another repository
  freshen         make install, before anything is built: fetch; a clean main behind origin/main is
      fast-forwarded; one behind with uncommitted changes, or diverged, is refused
      (TM_INSTALL_STALE=1: the maintainer's own install as it is); any other branch is left alone
  edit-check      the PreToolUse hook: reads Claude Code's JSON on stdin and refuses an edit of the
      primary checkout's files (TM_EDIT_PRIMARY=1: the maintainer's own session may)
  bash-check      the same for a shell command (shell_writes.py): its common writes and git's verbs that
      change a checkout; a heuristic, with scripts/hooks/pre-commit as its backstop
"""
import argparse
import json
import os
from pathlib import Path
import re
import sys
import time

from shell_writes import writes
from worktree import count, fetched, finished, git, has_ref, ours, primary_of, Refused, said_name, trees

FETCH_EVERY = 300      # seconds between the hooks' fetches, for the whole repository
EDIT_REFUSED = ("terminum: the primary checkout is the maintainer's; make this change in a worktree: "
                "python3 scripts/worktree.py add <slug> --kind <kind>, then edit the file there (CLAUDE.md §9). "
                "Never route around this with a shell command; only the maintainer allows it, by starting "
                "the session with TM_EDIT_PRIMARY=1.")


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


def holds_work(path):
    """Uncommitted files in a tree that a parallel sweep may take or a lost .git may have broken: never a
    reason to lose the hook's other words; no index write, which would put off the sweep's idle hour."""
    try:
        return bool(git("--no-optional-locks", "status", "--porcelain", cwd=path, check=False).stdout.strip())
    except OSError:
        return False


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
    if names := [said_name(path) + (" (holds uncommitted work: not finished)" if holds_work(path) else "") for path in done]:
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
    from release_turn import releasing
    if releasing(cwd):   # it publishes the commit its checks saw
        return lines + [f"warning: a release is running: main stays {commits(behind)} behind origin/main"]
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


def edit_refused(target, own_place=False):
    """Whether an agent's edit of `target` (absolute: links resolved, so a tree's link to a shared local
    file counts where it lives; with `own_place`, as rm and mv act, a link counts where it stands) is one
    of the primary checkout's files. Judged by the file's repository, not the session's folder: another
    project's files and the trees' own are none of this."""
    path = Path(target).expanduser()
    path = path.parent.resolve() / path.name if own_place else path.resolve()
    folder = next((parent for parent in path.parents if parent.is_dir()), None)
    if folder is None or not ours(folder):
        return False
    primary, *others = trees(folder)
    if not path.is_relative_to(primary) or any(path.is_relative_to(tree) for tree in others):
        return False
    return git("check-ignore", "-q", str(path), cwd=primary, check=False).returncode == 1  # 0 ignored, else git failed


def refusal(why):
    return json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
                                              "permissionDecisionReason": why}})


def edit_check(stdin):
    """The hook's answer as JSON, or nothing: what cannot be read or judged is let through, as a hook
    never stops the agent over its own failure."""
    if os.environ.get("TM_EDIT_PRIMARY"):
        return []
    try:
        given = json.load(stdin)["tool_input"]
        target = given.get("file_path") or given.get("notebook_path")
        if not target or not edit_refused(target):
            return []
    except (ValueError, KeyError, TypeError, AttributeError, OSError, Refused):
        return []
    return [refusal(EDIT_REFUSED)]


def in_primary(folder):
    """Whether `folder` is in the primary checkout of this repository, not in one of its trees."""
    path = Path(folder).expanduser().resolve()
    if not path.is_dir() or not ours(path):
        return False
    primary, *others = trees(path)
    return path.is_relative_to(primary) and not any(path.is_relative_to(tree) for tree in others)


def bash_refused(command, cwd):
    """Why this shell command is refused, or nothing (AC-104)."""
    inline, paths, git_folders = writes(command, cwd)
    if inline and ours(cwd):
        return "terminum: TM_EDIT_PRIMARY is the maintainer's, set when they start the session (CLAUDE.md §9)."
    if any(edit_refused(path, own) for path, own in paths) or any(in_primary(folder) for folder in git_folders):
        return EDIT_REFUSED
    return ""


def bash_check(stdin):
    """The hook's answer as JSON, or nothing: as for edit_check, what cannot be judged is let through."""
    if os.environ.get("TM_EDIT_PRIMARY"):
        return []
    try:
        given = json.load(stdin)
        why = bash_refused(given["tool_input"]["command"], given.get("cwd") or os.getcwd())
    except (ValueError, KeyError, TypeError, AttributeError, OSError, Refused):
        return []
    return [refusal(why)] if why else []


def main(argv):
    parser = argparse.ArgumentParser(prog="worktree_fresh.py", description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("fresh").add_argument("--start", action="store_true")
    commands.add_parser("freshen")
    commands.add_parser("edit-check")
    commands.add_parser("bash-check")
    args = parser.parse_args(argv)
    if args.command in ("edit-check", "bash-check"):
        lines = (edit_check if args.command == "edit-check" else bash_check)(sys.stdin)
    else:
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
