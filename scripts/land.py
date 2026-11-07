#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Lands a finished worktree on origin/main (spec AC-104): merged with origin/main in the tree, its
targeted checks green on that result, pushed, the primary checkout fast-forwarded and the tree deleted.
Several trees land at once: each tests alone, and only the last look and the push take turns. A tree is
merged with what others landed meanwhile; its checks run again if they changed the same files, and it
builds again if they changed any code. During a release the primary checkout stays where it is (scripts/release_turn.py).

  land.py <slug>    from the primary checkout (`make land SLUG=<slug>`), the work committed;
                    TM_LAND_INSTALL=1 (the maintainer's local.mk) also installs a change with code
Exit: 0 landed; 1 checks failed or refused (the tree stays); 2 a merge conflict, left in the tree
to resolve and commit before running this again. No force, ever."""
import fcntl
import os
from pathlib import Path
import re
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_changed import is_doc  # noqa: E402
from worktree import common_dir, primary_of, Refused, releasing  # noqa: E402

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


class Turn:
    """One tree at a time merges and pushes, however long its install takes; the kernel frees the turn when its holder dies."""
    def __init__(self, tree):
        try:
            self.path = common_dir(tree) / "terminum-land.turn"
        except Refused as refused:
            raise Stop(1, str(refused)) from refused

    def __enter__(self):
        self.file = self.path.open("w")   # not inherited: a build it starts holds nothing
        fcntl.flock(self.file, fcntl.LOCK_EX)
        return self

    def __exit__(self, *_):
        self.file.close()


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


def checks_of(tree):
    return subprocess.run([sys.executable, "scripts/check_changed.py"], cwd=tree).returncode == 0


def builds(tree):
    return subprocess.run(["make", "build"], cwd=tree, stdout=subprocess.DEVNULL).returncode == 0


def has_code(files):
    return any(not is_doc(f) for f in files)


def land(tree, check=checks_of, build=lambda tree: builds(tree), say=print, after=lambda files: None):
    """Returns the commit landed. Stop if it did not land. `after(its files)` runs still in turn."""
    if git("status", "--porcelain", cwd=tree).stdout.strip():
        raise Stop(1, f"{tree} has uncommitted changes: commit them first")
    git("fetch", "--quiet", "origin", cwd=tree)
    if git("merge-base", "--is-ancestor", "HEAD", BASE, cwd=tree, check=False).returncode == 0:
        return git("rev-parse", "HEAD", cwd=tree).stdout.strip()  # nothing of its own to land
    tested_at = None
    own_files = files_between(BASE, "HEAD", tree)
    for _ in range(TRIES):
        git("fetch", "--quiet", "origin", cwd=tree)
        base_now = git("rev-parse", BASE, cwd=tree).stdout.strip()
        ours = files_between(BASE, "HEAD", tree)
        others = files_between("HEAD", BASE, tree)
        if not has_base(tree):
            merge_base_in(tree)
            say(f"merged {BASE} ({base_now[:9]})")
        if tested_at is None or ours & others:
            if not check(tree):
                raise Stop(1, f"targeted checks failed: {tree} stays with its merge of {BASE}, nothing pushed")
            tested_at = git("rev-parse", "HEAD", cwd=tree).stdout.strip()
        elif has_code(others) and has_code(ours) and not build(tree):  # a renamed function, a new caller
            raise Stop(1, f"the build fails merged with {BASE}: {tree} stays, nothing pushed")
        with Turn(tree):
            git("fetch", "--quiet", "origin", cwd=tree)
            if not has_base(tree):
                say(f"{BASE} moved while this tree was checked")
                continue
            pushed = git("push", "--quiet", "origin", "HEAD:main", cwd=tree, check=False)
            if pushed.returncode == 0:
                git("fetch", "--quiet", "origin", cwd=tree)
                after(own_files)
                return git("rev-parse", "HEAD", cwd=tree).stdout.strip()
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
    try:
        commit = land(tree, after=after_push(primary))
    except Stop as stop:
        print(f"land: {stop}", file=sys.stderr)
        return stop.code
    print(f"landed {commit[:9]} on {BASE}")
    os.chdir(primary)
    gone = subprocess.run([str(primary / "scripts" / "delete_worktree.sh"), argv[0]])
    if gone.returncode:
        print(f"{tree} stays: see above; the Stop hook's sweep takes it when it is free", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
