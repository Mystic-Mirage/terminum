#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Lands a finished worktree on origin/main (spec AC-104): merged with origin/main in the tree, its
smoke set green on that result, pushed, the primary checkout fast-forwarded and the tree deleted. Its
targeted checks passed on its own content count (scripts/check_changed.py records it); they run again
on the merged result when none is recorded or origin/main changed the same files. Several trees land at
once: each tests alone, and only the last look and the push take turns. A tree is merged with what
others landed meanwhile, its smoke set run again on each merge. During a release the primary checkout
stays where it is (scripts/release_turn.py), and the smoke set yields to it (nice).

  land.py <slug>    from the primary checkout (`make land SLUG=<slug>`), the work committed;
                    TM_LAND_INSTALL=1 (the maintainer's local.mk) also installs a change with code
Exit: 0 landed; 1 checks failed or refused (the tree stays); 2 a merge conflict, left in the tree
to resolve and commit before running this again. No force, ever."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_changed import is_doc, red, skipped_note  # noqa: E402
from worktree import primary_of, Refused, releasing, Turn as TreeTurn  # noqa: E402

BASE = "origin/main"
TRIES = 8


class Stop(Exception):
    def __init__(self, code, words):
        super().__init__(words)
        self.code = code


def git(*args, cwd, check=True):
    done = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if check and done.returncode:
        raise Stop(1, f"git {' '.join(args)}: {done.stderr.strip() or done.stdout.strip()}")
    return done


def has_base(tree):
    return git("merge-base", "--is-ancestor", BASE, "HEAD", cwd=tree, check=False).returncode == 0


def files_between(a, b, cwd):
    return set(git("diff", "--name-only", f"{a}...{b}", cwd=cwd).stdout.splitlines())


class Turn(TreeTurn):
    """One tree at a time merges and pushes, however long its install takes."""
    def __init__(self, tree):
        try:
            super().__init__(tree, "terminum-land.turn")
        except Refused as refused:
            raise Stop(1, str(refused)) from refused

    def __enter__(self):
        super().__enter__()
        if not self.take():
            print("land: another tree's landing has its turn: waiting", flush=True)
            self.take(wait=True)
        return self


def merge_base_in(tree):
    """origin/main merged into the tree; the conflict, if any, stays there to resolve."""
    done = git("merge", "--no-edit", BASE, cwd=tree, check=False)
    if done.returncode:
        conflicts = git("diff", "--name-only", "--diff-filter=U", cwd=tree).stdout.split()
        if not conflicts:
            raise Stop(1, f"merge of {BASE}: {done.stderr.strip() or done.stdout.strip()}")
        raise Stop(2, f"conflict with {BASE} in: {' '.join(conflicts)}\nresolve them in {tree}, commit, and land again")


def update_primary(primary):
    """The primary checkout on the fresh origin/main; one that cannot move is said, never forced."""
    branch = git("branch", "--show-current", cwd=primary).stdout.strip()
    if branch == "main":
        done = git("merge", "--ff-only", BASE, cwd=primary, check=False)
    else:
        done = git("fetch", ".", f"{BASE}:main", cwd=primary, check=False)
    if done.returncode == 0:
        return None
    lines = [line.strip() for line in (done.stderr or done.stdout).splitlines() if line.strip()]
    return "; ".join(line for line in lines if line != "Aborting")[:300] or "git said nothing"


def ci_status(tree, run=subprocess.run):
    """One line on GitHub CI's last run on main, as gh says: what a landing adds to; it never stops one."""
    ask = ["gh", "run", "list", "--branch", "main", "--workflow", "ci", "--limit", "1", "--json", "status,conclusion,headSha,url"]
    try:
        done = run(ask, cwd=tree, capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired) as e:
        return f"CI on main: unknown ({e})"
    if done.returncode:
        return f"CI on main: unknown ({(done.stderr.strip().splitlines() or ['gh failed'])[-1]})"
    try:
        runs = json.loads(done.stdout or "[]")
        if not runs:
            return "CI on main: no run"
        last = runs[0]
        sha, state, url = last["headSha"], last["conclusion"] or last["status"], last["url"]
    except (ValueError, KeyError, TypeError, IndexError) as e:
        return f"CI on main: unknown (gh's answer: {e!r})"
    head = git("rev-parse", BASE, cwd=tree).stdout.strip()
    of = "" if sha == head else f", not {BASE}'s {head[:9]}"
    return f"CI on main: {state} on {sha[:9]}{of}: {url}"


def checks_of(tree, *flags):
    return subprocess.run([sys.executable, "scripts/check_changed.py", *flags], cwd=tree).returncode == 0


def has_code(files):
    return any(not is_doc(f) for f in files)


def land(tree, check=checks_of, smoke=lambda tree: checks_of(tree, "--land"), passed=lambda tree: checks_of(tree, "--passed"),
         say=print, after=lambda files: None):
    """Returns the commit landed. Stop if it did not land. `after(its files)` runs still in turn."""
    if git("status", "--porcelain", cwd=tree).stdout.strip():
        raise Stop(1, f"{tree} has uncommitted changes: commit them first")
    git("fetch", "--quiet", "origin", cwd=tree)
    if git("merge-base", "--is-ancestor", "HEAD", BASE, cwd=tree, check=False).returncode == 0:
        return git("rev-parse", "HEAD", cwd=tree).stdout.strip()  # nothing of its own to land
    checked = passed(tree)   # the tree's own content, before the merge
    own_files = files_between(BASE, "HEAD", tree)
    for _ in range(TRIES):
        git("fetch", "--quiet", "origin", cwd=tree)
        base_now = git("rev-parse", BASE, cwd=tree).stdout.strip()
        ours = files_between(BASE, "HEAD", tree)
        others = files_between("HEAD", BASE, tree)
        if not has_base(tree):
            merge_base_in(tree)
            say(f"merged {BASE} ({base_now[:9]})")
        if not checked or ours & others:
            if not check(tree):
                raise Stop(1, f"targeted checks failed: {tree} stays with its merge of {BASE}, nothing pushed")
            checked = True
        # each pass follows a merge: a renamed function, a new caller, what others landed meets this tree's code
        if not smoke(tree):
            raise Stop(1, f"smoke checks failed: {tree} stays with its merge of {BASE}, nothing pushed")
        with Turn(tree):
            git("fetch", "--quiet", "origin", cwd=tree)
            if not has_base(tree):
                say(f"{BASE} moved while this tree was checked")
                continue
            pushed = git("push", "--quiet", "origin", "HEAD:main", cwd=tree, check=False)
            if pushed.returncode == 0:
                git("fetch", "--quiet", "origin", cwd=tree)
                commit = git("rev-parse", "HEAD", cwd=tree).stdout.strip()
                after(own_files)  # minutes, when it installs: a sweep may take the tree, merged now
                return commit
            git("fetch", "--quiet", "origin", cwd=tree)
            if has_base(tree):
                raise Stop(1, f"push refused, and {BASE} has not moved: {pushed.stderr.strip()}")
            say(f"{BASE} moved during the push")
    raise Stop(1, f"{BASE} kept moving: {TRIES} tries; land again")


def after_push(primary):
    """The primary checkout on the fresh origin/main, and installed if the maintainer asked for that."""
    def run(files):
        if releasing(primary):
            print("primary checkout stays where it is: a release is running")
            return
        stuck = update_primary(primary)
        print("main: on the fresh origin/main" if stuck is None else f"primary checkout stays where it is: {stuck}")
        if stuck is None and os.environ.get("TM_LAND_INSTALL") == "1" and has_code(files):
            for target in ("build", "install", "installed"):
                done = subprocess.run(["make", target], cwd=primary, capture_output=True, text=True)
                last = (done.stdout.strip().splitlines() or [""])[-1]
                print(f"make {target}: exit {done.returncode}" + (f": {last}" if target == "installed" else ""))
                if done.returncode:
                    print(done.stderr.strip()[-600:], file=sys.stderr)
                    break
    return run


def main(argv):
    if len(argv) != 1 or not re.fullmatch(r"[A-Za-z0-9][\w.-]*", argv[0]):
        sys.exit("usage: land.py <slug>")
    try:
        primary = primary_of(Path.cwd())
    except Refused as refused:
        sys.exit(str(refused))
    tree = primary / ".worktrees" / argv[0]
    if not tree.is_dir():
        sys.exit(f"no worktree {tree}")
    git("fetch", "--quiet", "origin", cwd=tree, check=False)   # main's head as GitHub has it, for ci_status
    ci = ci_status(tree)
    print(ci)   # said again last when not green: minutes of checks scroll the first out of sight
    green = ci.startswith("CI on main: success")
    install, skipped = after_push(primary), []

    def after(files):   # read before the install's minutes, in which a sweep may take the tree
        skipped.append(skipped_note(tree))
        install(files)
    try:
        commit = land(tree, after=after)
    except Stop as stop:
        print(f"land: {stop}", file=sys.stderr)
        if not green:
            print(ci)
        return stop.code
    print(f"landed {commit[:9]} on {BASE}")
    if not green:
        print(ci)
    if skipped and skipped[0]:
        print(red(skipped[0]))   # its timed checks never ran: the report says so
    os.chdir(primary)
    if not tree.is_dir():
        return 0
    gone = subprocess.run([str(primary / "scripts" / "delete_worktree.sh"), argv[0]])
    if gone.returncode:
        print(f"{tree} stays: see above; the Stop hook's sweep takes it when it is free", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
