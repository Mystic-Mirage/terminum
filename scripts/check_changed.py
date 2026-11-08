#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""A worktree's targeted checks (spec AC-104): the tests of what changed, the tests that cite the
acceptance cases it touches, and the critical functions; a pass is kept per content
(.checks/changed-<id>.ok), so the same content runs once. Before its first timed step it waits for a
quiet machine (scripts/load_gate.py, AC-146); still busy after 15 minutes, the timed steps are skipped and
said in red last, here and by make land (.checks/skipped). --land: a landing's smoke set on its merged result, the
basics on one platform, no timed check. The full run is the release's gate, not this (`make check`).

  check_changed.py [--print] [--force] [--base <ref>]   default origin/main; --print only says the plan
  check_changed.py --land | --passed | --tree-id        the smoke set; 0 if this content passed; its id
Each step's output goes to .checks/changed.<step>.log, one line here, as scripts/check_all.sh does."""
import argparse
import hashlib
from pathlib import Path
import re
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_docs import AC  # noqa: E402
from load_gate import for_task, MINUTES  # noqa: E402
from worktree import releasing  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
HEAD_LINES = 6  # a source file names its cases at its top
# what breaks everything when it breaks: signing in, the tmux client, a pane's page
CRITICAL_CARGO = ["auth::", "tmux::", "page::client"]
DOCKER = re.compile(r"e2e_remote_sshd")  # needs Docker and built binaries as arguments: by hand, never here
SCAN = "env -u TM_PORT -u TM_APP_DIR bash scripts/security_check.sh"
NICE = "nice -n 10"   # several trees build at once: their builds leave room for the rest
DEBUG_BUILD = f"cd terminumd && {NICE} cargo build --locked"   # what the e2e checks run; no release build


def cargo_test(modules):
    return f"cd terminumd && {NICE} cargo test --locked -- " + " ".join(sorted(modules))


def git(*args, cwd=REPO):
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, check=True).stdout


def changed_files(base, cwd=REPO):
    merge_base = git("merge-base", base, "HEAD", cwd=cwd).strip()
    named = set(git("diff", "--name-only", merge_base, cwd=cwd).splitlines())
    named |= set(git("ls-files", "-o", "--exclude-standard", cwd=cwd).splitlines())
    return sorted(named), merge_base


def tree_id(root=REPO):
    """The content of every tracked and new file, not the commit: committing a checked tree keeps its pass."""
    files = subprocess.run(["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=root, capture_output=True, check=True).stdout
    digest = hashlib.sha256()
    for name in sorted(filter(None, files.split(b"\0"))):
        try:
            digest.update(name + b"\0" + (root / name.decode()).read_bytes() + b"\0")
        except (FileNotFoundError, IsADirectoryError):   # deleted, not yet staged; a submodule
            digest.update(name + b"\0-\0")
    return digest.hexdigest()[:16]


def is_doc(path):
    return path.endswith(".md") or path.startswith("docs/")


def touched_cases(files, merge_base, cwd=REPO):
    """Cases named on the changed lines, or at the top of a changed source file."""
    cases = set(AC.findall("\n".join(line for line in git("diff", "-U0", merge_base, cwd=cwd).splitlines()
                                     if line[:1] in "+-" and line[:3] not in ("+++", "---"))))
    for name in files:
        if not is_doc(name):
            cases |= head_cases(cwd / name)
    return cases


def head_cases(path):
    if not path.is_file():
        return set()
    with path.open(errors="replace") as f:
        return set(AC.findall("".join(f.readline() for _ in range(HEAD_LINES))))


def caseless(files, cwd=REPO):
    """Changed Rust sources that name no case at their top: no test is known to cite their area's cases,
    so the e2e scripts of that area do not run (their module's cargo tests do)."""
    return [f for f in files if f.startswith("terminumd/src/") and f.endswith(".rs") and not is_rust_test(f)
            and (cwd / f).is_file() and not head_cases(cwd / f)]


def module_of(path):
    """The cargo test filter of a Rust file: terminumd/src/cli/install_tests.rs -> cli::install."""
    rel = path.removeprefix("terminumd/src/").removesuffix(".rs")
    rel = re.sub(r"(_tests?|/mod)$", "", rel)
    return None if rel in ("main", "lib") else rel.replace("/", "::")


def page_checks(root):
    """{web/test file: the check of e2e_all.mjs that runs it}, a helper its check imports included."""
    path = root / "web" / "test" / "e2e_all.mjs"
    text = path.read_text() if path.is_file() else ""
    found = {f"web/test/{file}": name for name, file in re.findall(r'import \{ (web\w+) \} from "\./([\w.]+)"', text)}
    for file, name in list(found.items()):
        if (root / file).is_file():
            for helper in re.findall(r'from "\./(e2e_[\w.]+)"', (root / file).read_text(errors="replace")):
                found.setdefault(f"web/test/{helper}", name)
    return found


def is_rust_test(path):
    return "_tests" in Path(path).name or "/tests/" in path


def test_files(root):
    """Files that hold tests: a Rust file only if it is a test module."""
    found = []
    for pattern in ("web/test/*.mjs", "ui/test/*.mjs", "scripts/e2e_*.py", "scripts/tests/*.py", "terminumd/src/**/*.rs"):
        found += [p for p in root.glob(pattern) if p.is_file() and (p.suffix != ".rs" or is_rust_test(str(p)))]
    return found


def citing(cases, root):
    """Test files that cite one of the cases, relative to `root`."""
    if not cases:
        return []
    wanted = re.compile(r"\bAC-(?:" + "|".join(sorted(cases)) + r")\b")
    return sorted(str(p.relative_to(root)) for p in test_files(root) if wanted.search(p.read_text(errors="replace")))


def new_cases(diff):
    """Cases this change adds to a spec's scenarios: `+### AC-nn — …`."""
    return set(re.findall(r"^\+### AC-(\d{2,3}) — ", diff, re.M))


def uncited(cases, root):
    """Those of `cases` no test cites: new behavior comes with tests from its acceptance cases."""
    tests = "\n".join(p.read_text(errors="replace") for p in test_files(root))
    return sorted(c for c in cases if not re.search(rf"\bAC-{c}\b", tests))


def plan(files, cases, root=REPO, tests_of_cases=None):
    """[(step's name, shell command)] for these changed files and touched cases."""
    code = [f for f in files if not is_doc(f)]
    # the repository is public, and a landing pushes by itself: always scanned first
    steps = [("security", SCAN)]
    if any(is_doc(f) for f in files):
        steps.append(("docs", "-python3 scripts/check_docs.py"))
    if not code:
        return steps
    cited = citing(cases, root) if tests_of_cases is None else tests_of_cases
    tests = set(code) | set(cited)
    rust = [f for f in code if f.startswith("terminumd/")]
    modules = {module_of(f) for f in tests if f.startswith("terminumd/src/") and f.endswith(".rs")} - {None}
    web = [f for f in tests if f.startswith("web/")]
    ui = [f for f in tests if f.startswith("ui/")]
    pages = page_checks(root)
    named_pages = sorted({pages[f] for f in web if f in pages})
    # every e2e_live*.mjs is a part of e2e_live.mjs, so it runs with the critical step
    shows = bool(rust) or any(f.startswith(("web/static/", "ui/src/")) for f in code) or any(f.startswith("web/test/e2e_live") for f in tests)
    e2e = [f for f in sorted(tests) if re.fullmatch(r"scripts/e2e_\w+\.py", f) and not DOCKER.search(f)]
    if rust or modules or ui or web or e2e or shows:  # the Panel's files: the browsers load them, terminumd embeds them
        steps.append(("ui", "make ui"))
    if e2e or shows:
        steps.append(("debug-build", DEBUG_BUILD))
    if rust:
        steps.append(("clippy", f"cd terminumd && {NICE} cargo clippy --locked --all-targets -- -D warnings"))
    if rust or modules:
        steps.append(("cargo", cargo_test(modules | set(CRITICAL_CARGO))))
    if any(f.startswith("scripts/") or f == "Makefile" or f.startswith(".claude/") for f in tests):
        steps.append(("scripts", "python3 -m unittest discover -s scripts/tests"))
    if ui:
        steps.append(("ui-unit", "cd ui && npx tsc -p . && npm test"))
        steps.append(("panel", "cd ui && node test/e2e_panel.mjs"))
    if web:
        steps.append(("page-unit", "node --test web/test/*.test.mjs"))
    if named_pages:
        steps.append(("page", "node web/test/e2e_all.mjs " + " ".join(named_pages)))
    for f in e2e:
        steps.append((Path(f).stem, f"python3 {f}"))
    if shows:  # a real terminumd, tmux and browser: open a pane, type, see it
        steps.append(("critical", "node web/test/e2e_live.mjs chromium"))
    return steps


def land_plan(files):
    """A landing's smoke set on its merged result (AC-104): the basics on one platform, no timed check."""
    steps = [("security", SCAN)]
    if any(not is_doc(f) for f in files):
        steps += [("ui", "make ui"), ("debug-build", DEBUG_BUILD),
                  ("cargo", cargo_test(CRITICAL_CARGO)),
                  ("e2e_core", "python3 scripts/e2e_core.py"), ("e2e_attach", "python3 scripts/e2e_attach.py"),
                  ("smoke", "node web/test/e2e_live.mjs chromium smoke")]
    return steps


def timed(name):
    """A step whose checks measure time, which a busy machine fails (AC-146)."""
    return name in ("panel", "page", "critical") or name.startswith("e2e_")


def red(text):
    return f"\033[31m{text}\033[0m" if sys.stdout.isatty() else text


def skipped_note(root):
    """The line a run that skipped timed steps left in root, said last by make land, or None."""
    try:
        return (root / ".checks" / "skipped").read_text().strip() or None
    except FileNotFoundError:
        return None


def say_skipped(root):
    """Said last, red: the report names these checks under Not verified (AC-146)."""
    note = skipped_note(root)
    if note:
        print(red(note), flush=True)


def run(steps, root=REPO, gate=True, nice=False):
    """Returns the steps failed. gate: a quiet machine is waited for before the first timed step, and the timed
    steps are skipped when it stays busy (AC-146); nice: every step yields (a release runs)."""
    logs = root / ".checks"
    logs.mkdir(exist_ok=True)
    failed, skipped, busy, asked = [], [], None, False
    if gate:   # what needs no quiet machine runs before the wait
        steps = sorted(steps, key=lambda step: timed(step[0]))
    for name, command in steps:
        if gate and timed(name):
            busy, asked, gate = for_task(root), True, False
        if busy and timed(name):
            print(red(f"SKIP {name}: {busy} {MINUTES}"), flush=True)
            skipped.append(name)
            continue
        optional = command.startswith("-")
        started = time.monotonic()
        log = logs / f"changed.{name}.log"
        with log.open("w") as out:
            # under make check's guard (AC-102): a step that leaves a process or a file fails here too
            guarded = (NICE.split() if nice else []) + [sys.executable, "-I", str(root / "scripts" / "check_step.py"), "sh", "-c", command.lstrip("-")]
            code = subprocess.run(guarded, cwd=root, stdout=out, stderr=subprocess.STDOUT).returncode
        seconds = round(time.monotonic() - started)
        if code == 0 or optional:
            print(f"{'ok  ' if code == 0 else 'note'} {name} ({seconds} s)" + ("" if code == 0 else f": {log}"), flush=True)
        else:
            print(f"FAIL {name} ({seconds} s): {log}", flush=True)
            print("".join(f"     {line}\n" for line in log.read_text(errors="replace").splitlines()[-15:]), end="", flush=True)
            failed.append(name)
    note = logs / "skipped"
    if skipped:
        note.write_text(f"targeted: SKIPPED timed checks, the machine busy {MINUTES}: {' '.join(skipped)}\n")
    elif asked:
        note.unlink(missing_ok=True)   # this run's timed checks ran on a quiet machine
    return failed


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--print", action="store_true", dest="only_print")
    parser.add_argument("--force", action="store_true")
    parser.add_argument("--land", action="store_true")
    parser.add_argument("--passed", action="store_true")
    parser.add_argument("--tree-id", action="store_true")
    parser.add_argument("--base", default="origin/main")
    args = parser.parse_args(argv)
    if args.tree_id or args.passed:
        ident = tree_id()
        if args.tree_id:
            print(ident)
            return 0
        return 0 if (REPO / ".checks" / f"changed-{ident}.ok").exists() else 1
    files, merge_base = changed_files(args.base)
    if args.land:
        failed = run(land_plan(files), gate=False, nice=releasing(REPO))
        print(f"smoke: FAIL {' '.join(failed)}" if failed else "smoke: PASS")
        return 1 if failed else 0
    steps = plan(files, touched_cases(files, merge_base))
    print(f"changed: {len(files)} files against {merge_base[:9]}")
    for name in caseless(files):
        print(f"note: {name} names no case at its top: the e2e scripts of its area are not known")
    # a new case in a change with no code may be what is not built yet; with code it has tests
    untested = uncited(new_cases(git("diff", merge_base)), REPO) if any(not is_doc(f) for f in files) else []
    if untested and not args.only_print:
        print("targeted: FAIL new cases no test cites: " + " ".join(f"AC-{c}" for c in untested))
        return 1
    if args.only_print:
        for name, command in steps:
            print(f"  {name}: {command}")
        return 0
    ident = tree_id()
    passed = REPO / ".checks" / f"changed-{ident}.ok"
    if passed.exists() and not args.force:
        print(f"targeted: passed already on {ident}")
        if skipped_note(REPO):
            say_skipped(REPO)
            print("note: --force runs them")
        return 0
    failed = run(steps)
    print(f"targeted: FAIL {' '.join(failed)}" if failed else "targeted: PASS")
    say_skipped(REPO)
    if not failed and tree_id() == ident:   # changed during the run: no pass recorded
        passed.write_text(time.strftime("%Y-%m-%d %H:%M\n"))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
