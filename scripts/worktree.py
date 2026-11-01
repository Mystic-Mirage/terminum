#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""One task, one branch, one worktree in <primary>/.worktrees/<slug> (CONTRIBUTING.md "Worktrees").

  add <slug> [--kind feat|fix|test|docs|chore] [--from <ref>]
      fetch origin, then branch <kind>/<slug> (no upstream) from <ref> (default origin/main),
      check it out in .worktrees/<slug> and link it; warns when main has unpushed commits
  link [<path>]   share the primary checkout's local files with the tree at <path> (default:
      this one): symlinks to its .toolchain, local.mk, CLAUDE.local.md, MEMORY.md and
      .claude/settings.local.json; clones (copy-on-write where the disk can) of ui/node_modules
      when the lockfiles match and of terminumd/target/release (its dependencies' builds)
  list            every worktree: branch, uncommitted files, commits not in origin/main,
      processes working in it
  remove <slug|path> [--discard]
      refuse while a process works in the tree, or (without --discard) while it holds
      uncommitted changes, ignored files that are not build output, or commits on a
      detached HEAD; then remove the tree, and its branch only when that branch is in
      origin/main or main (an unmerged branch is kept and named)

Nothing here starts, stops or kills a process or container.
"""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

KINDS = ("feat", "fix", "test", "docs", "chore")
SLUG = re.compile(r"^[a-z0-9][a-z0-9-]{0,40}$")
# the primary checkout's local files every tree uses as they are: linked, never copied
SHARED = (".toolchain", "local.mk", "CLAUDE.local.md", "MEMORY.md", ".claude/settings.local.json")
# repository-wide and append-only; no trailing slash: a symlink to a folder is no folder to git
EXCLUDES = ("/.worktrees", *(f"/{name}" for name in SHARED))
# ignored but rebuilt by make or cargo: removing a tree may drop them
REBUILT = ("ui/node_modules/", "ui/dist/", "ui/.build-id", "terminumd/target/", "dist/", ".checks/",
           ".playwright-cli/")
REBUILT_NAMES = ("__pycache__", ".DS_Store")


class Refused(Exception):
    """A refusal the user acts on: printed without a traceback, exit 1."""


def git(*args, cwd, check=True):
    run = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if check and run.returncode != 0:
        raise Refused(f"git {' '.join(args)}: {run.stderr.strip() or f'exit {run.returncode}'}")
    return run.stdout.strip() if check else run


def has_ref(ref, cwd):
    return git("rev-parse", "--verify", "--quiet", ref, cwd=cwd, check=False).returncode == 0


def count(range_, cwd):
    """Commits in `range_`, or None when a ref of it is absent."""
    run = git("rev-list", "--count", range_, cwd=cwd, check=False)
    return int(run.stdout) if run.returncode == 0 else None


def trees(cwd):
    """{path: branch or None} of every worktree, the primary checkout first."""
    found = {}
    for block in git("worktree", "list", "--porcelain", cwd=cwd).split("\n\n"):
        fields = dict(line.split(" ", 1) for line in block.splitlines() if " " in line)
        if "worktree" in fields:
            found[Path(fields["worktree"])] = fields.get("branch", "").removeprefix("refs/heads/") or None
    return found


def primary_of(cwd):
    return next(iter(trees(cwd)))


def exclude(primary):
    file = Path(git("rev-parse", "--path-format=absolute", "--git-path", "info/exclude", cwd=primary))
    file.parent.mkdir(parents=True, exist_ok=True)
    present = file.read_text() if file.exists() else ""
    missing = [line for line in EXCLUDES if line not in present.splitlines()]
    if missing:
        glue = "" if not present or present.endswith("\n") else "\n"
        file.write_text(present + glue + "\n".join(missing) + "\n")
    return f"excluded {' '.join(missing)}" if missing else "excludes present"


def clone(source, target):
    """`cp -c` (APFS clonefile; it copies where the disk cannot clone), a reflink elsewhere;
    False, with nothing left at `target`, where neither is made."""
    how = ["-cR"] if sys.platform == "darwin" else ["-R", "--reflink=always"]
    run = subprocess.run(["cp", *how, str(source), str(target)], capture_output=True, text=True)
    if run.returncode != 0:
        shutil.rmtree(target, ignore_errors=True)
    return run.returncode == 0


def link(tree, primary):
    tree, lines = Path(tree).resolve(), []
    if tree == primary:
        raise Refused("link: this is the primary checkout; give a worktree's path")
    for name in SHARED:
        source, target = primary / name, tree / name
        if not source.exists():
            continue
        if target.is_symlink():
            target.unlink()
        elif target.exists():
            lines.append(f"kept own {name}")
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.symlink_to(source)
        lines.append(f"linked {name}")
    lines.append(link_modules(tree, primary))
    lines.append(link_target(tree, primary))
    lines.append(exclude(primary))
    return lines


def link_modules(tree, primary):
    source, target = primary / "ui/node_modules", tree / "ui/node_modules"
    if target.exists():
        return "kept own ui/node_modules"
    lock_a, lock_b = primary / "ui/package-lock.json", tree / "ui/package-lock.json"
    if not (source.is_dir() and lock_a.exists() and lock_b.exists() and lock_a.read_bytes() == lock_b.read_bytes()):
        return "ui/node_modules: make installs it (npm ci: the lockfile differs or the primary has none)"
    if not clone(source, target):
        return "ui/node_modules: no clone on this disk; make installs it (npm ci)"
    os.utime(target)  # newer than the checkout's package files: make's rule takes it as installed
    return "cloned ui/node_modules"


def link_target(tree, primary):
    source, target = primary / "terminumd/target/release", tree / "terminumd/target"
    if target.exists():
        return "kept own terminumd/target"
    if not source.is_dir():
        return "terminumd/target: cargo builds it (the primary has no release build)"
    target.mkdir(parents=True)
    if not clone(source, target / "release"):
        target.rmdir()
        return "terminumd/target: no clone on this disk; cargo builds it"
    for entry in (target / "release").iterdir():  # the primary's own binaries: never this tree's
        if entry.is_file() or entry.is_symlink():
            entry.unlink()
    return "cloned terminumd/target/release (its dependencies' builds; terminumd itself rebuilds)"


def add(slug, kind="feat", base="origin/main", cwd="."):
    if not SLUG.match(slug):
        raise Refused(f"invalid slug {slug!r}: lowercase letters, digits and '-', at most 41")
    if kind not in KINDS:
        raise Refused(f"invalid kind {kind!r}: one of {', '.join(KINDS)}")
    primary = primary_of(cwd)
    path, branch = primary / ".worktrees" / slug, f"{kind}/{slug}"
    if path.exists():
        raise Refused(f"{path} exists")
    if has_ref(f"refs/heads/{branch}", primary):
        raise Refused(f"branch {branch} exists: another slug, or remove its tree first")
    if base.startswith("origin/"):
        git("fetch", "--quiet", "origin", cwd=primary)
    path.parent.mkdir(parents=True, exist_ok=True)
    git("worktree", "add", "--quiet", "--no-track", "-b", branch, str(path), base, cwd=primary)
    lines = [f"{branch} at {path} from {base} ({git('log', '--oneline', '-1', cwd=path)})"]
    unpushed = count(f"{base}..main", primary) if base != "main" and has_ref("main", primary) else 0
    if unpushed:
        lines.append(f"warning: main has {unpushed} commits not in {base}, so this tree lacks them "
                     f"(their rules and tools too); --from main starts from them")
    return path, lines + link(path, primary)


def working_in(tree):
    """{pid: command} of the processes whose current folder is in `tree`, this one excepted."""
    tree, found = str(Path(tree).resolve()), {}
    inside = lambda folder: folder == tree or folder.startswith(tree + os.sep)
    if Path("/proc/self/cwd").exists():
        for entry in Path("/proc").iterdir():
            try:
                if entry.name.isdigit() and inside(os.readlink(entry / "cwd")):
                    found[int(entry.name)] = (entry / "comm").read_text().strip()
            except OSError:
                continue
    elif shutil.which("lsof"):
        out = subprocess.run(["lsof", "-a", "-d", "cwd", "-F", "pcn"], capture_output=True, text=True).stdout
        pid = command = None
        for line in out.splitlines():
            tag, value = line[:1], line[1:]
            if tag == "p":
                pid = int(value)
            elif tag == "c":
                command = value
            elif tag == "n" and inside(value):
                found[pid] = command
    else:
        raise Refused("no /proc and no lsof: cannot tell which processes work in the tree")
    found.pop(os.getpid(), None)
    return found


def merged(rev, cwd):
    """Whether `rev` is in origin/main or main (absent refs count as not)."""
    return any(git("merge-base", "--is-ancestor", rev, ref, cwd=cwd, check=False).returncode == 0
               for ref in ("origin/main", "main") if has_ref(ref, cwd))


def kept_files(path):
    """The tree's ignored files that make and cargo do not rebuild (the shared links aside)."""
    out = git("status", "--porcelain", "--ignored=matching", cwd=path)
    names = [line[3:] for line in out.splitlines() if line.startswith("!! ")]
    return [name for name in names
            if not name.startswith(REBUILT) and not (path / name.rstrip("/")).is_symlink()
            and not any(part in REBUILT_NAMES for part in Path(name).parts)]


def remove(target, discard=False, cwd="."):
    primary = primary_of(cwd)
    known = trees(primary)
    slug_path = primary / ".worktrees" / target
    path = slug_path if slug_path in known else Path(target).resolve()
    if path == primary:
        raise Refused("remove: that is the primary checkout")
    if path not in known:
        raise Refused(f"{target} is no worktree of this repository (list shows them)")
    busy = working_in(path)
    if busy:
        names = ", ".join(f"{pid} {command}" for pid, command in sorted(busy.items()))
        raise Refused(f"processes work in {path}: {names}; end them yourself (exit that shell, session or "
                      "server; run this from the primary checkout), then remove again")
    branch = known[path]
    if not discard:
        problems = []
        if dirty := git("status", "--porcelain", cwd=path):
            problems.append(f"uncommitted changes:\n{dirty}")
        if kept := kept_files(path):
            problems.append("ignored files that are not build output:\n" + "\n".join(kept))
        if branch is None and not merged("HEAD", path):
            problems.append(f"a detached HEAD ({git('log', '--oneline', '-1', cwd=path)}) not in origin/main "
                            "or main: give it a branch (git switch -c <name>)")
        if problems:
            raise Refused(f"{path} holds " + "; ".join(problems) + "\n(commit or move them, or --discard)")
    for name in SHARED:  # the links only, never what they point to; link restores them
        if (path / name).is_symlink():
            (path / name).unlink()
    git("worktree", "remove", "--force", str(path), cwd=primary)
    git("worktree", "prune", cwd=primary)
    lines = [f"removed {path}"]
    if branch and branch != "main":
        if merged(branch, primary):
            git("branch", "-D", branch, cwd=primary)
            lines.append(f"deleted branch {branch} (in origin/main or main)")
        else:
            ahead = count(f"origin/main..{branch}", primary)
            lines.append(f"kept branch {branch}: {'?' if ahead is None else ahead} commits not in origin/main or main")
    return lines


def listing(cwd="."):
    lines = []
    for path, branch in list(trees(cwd).items())[1:]:
        if not path.exists():
            lines.append(f"{path}\tmissing (git worktree prune clears it)")
            continue
        dirty = len(git("status", "--porcelain", cwd=path).splitlines())
        ahead, own = count("origin/main..HEAD", path), count("main..HEAD", path)
        state = ("no commits of its own" if not ahead and not own
                 else "merged into origin/main or main" if merged("HEAD", path)
                 else f"{'?' if ahead is None else ahead} commits not in origin/main")
        lines.append(f"{path}\t{branch or '(detached)'}\t{dirty} uncommitted\t{state}\t"
                     f"{len(working_in(path))} processes in it")
    return lines or ["no worktrees besides the primary checkout"]


def main(argv):
    parser = argparse.ArgumentParser(prog="worktree.py", description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    made = commands.add_parser("add")
    made.add_argument("slug")
    made.add_argument("--kind", choices=KINDS, default="feat")
    made.add_argument("--from", dest="base", default="origin/main")
    commands.add_parser("link").add_argument("path", nargs="?", default=".")
    commands.add_parser("list")
    gone = commands.add_parser("remove")
    gone.add_argument("target")
    gone.add_argument("--discard", action="store_true")
    args = parser.parse_args(argv)
    if args.command == "add":
        path, lines = add(args.slug, args.kind, args.base)
        lines.append(f"next: cd {path}")
    elif args.command == "link":
        lines = link(args.path, primary_of(args.path))
    elif args.command == "list":
        lines = listing()
    else:
        lines = remove(args.target, args.discard)
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Refused as refused:
        print(f"worktree: {refused}", file=sys.stderr)
        sys.exit(1)
