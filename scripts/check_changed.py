#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""A worktree's targeted checks (spec AC-104): the build, the tests of what changed, the tests
that cite the acceptance cases it touches, and the critical functions. The full run is the
release's gate, not this (`make check`, CLAUDE.md §10).

  check_changed.py [--print] [--base <ref>]     default origin/main; --print only says the plan
Each step's output goes to .checks/changed.<step>.log, one line here, as scripts/check_all.sh does."""
import argparse
from pathlib import Path
import re
import subprocess
import sys
import time

REPO = Path(__file__).resolve().parent.parent
AC = re.compile(r"\bAC-(\d{2,3})\b")
HEAD_LINES = 6  # a source file names its cases at its top
# what breaks everything when it breaks: signing in, the tmux client, a pane's page
CRITICAL_CARGO = ["auth::", "tmux::", "page::client"]
DOCKER = re.compile(r"e2e_remote_sshd")  # slow, and it needs Docker: only when changed itself


def git(*args, cwd=REPO):
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, check=True).stdout


def changed_files(base, cwd=REPO):
    merge_base = git("merge-base", base, "HEAD", cwd=cwd).strip()
    named = set(git("diff", "--name-only", merge_base, cwd=cwd).splitlines())
    named |= set(git("ls-files", "-o", "--exclude-standard", cwd=cwd).splitlines())
    return sorted(named), merge_base


def is_doc(path):
    return path.endswith(".md") or path.startswith("docs/")


def touched_cases(files, merge_base, cwd=REPO):
    """Cases named on the changed lines, or at the top of a changed source file."""
    cases = set(AC.findall("\n".join(line for line in git("diff", "-U0", merge_base, cwd=cwd).splitlines()
                                     if line[:1] in "+-" and line[:3] not in ("+++", "---"))))
    for name in files:
        path = cwd / name
        if not is_doc(name) and path.is_file():
            with path.open(errors="replace") as f:
                cases |= set(AC.findall("".join(f.readline() for _ in range(HEAD_LINES))))
    return cases


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


def test_files(root):
    found = []
    for pattern in ("web/test/*.mjs", "ui/test/*.mjs", "scripts/e2e_*.py", "scripts/tests/*.py", "terminumd/src/**/*.rs"):
        found += [p for p in root.glob(pattern) if p.is_file()]
    return found


def citing(cases, root):
    """Test files that cite one of the cases, relative to `root`."""
    if not cases:
        return []
    wanted = re.compile(r"\bAC-(?:" + "|".join(sorted(cases)) + r")\b")
    return sorted(str(p.relative_to(root)) for p in test_files(root)
                  if (not p.suffix == ".rs" or "_tests" in p.name or "/tests/" in str(p)) and wanted.search(p.read_text(errors="replace")))


def new_cases(diff):
    """Cases this change adds to a spec's scenarios: `+### AC-nn — …`."""
    return set(re.findall(r"^\+### AC-(\d{2,3}) — ", diff, re.M))


def uncited(cases, root):
    """Those of `cases` no test cites: new behavior comes with tests from its acceptance cases."""
    tests = "\n".join(p.read_text(errors="replace") for p in test_files(root) if p.suffix != ".rs" or "_tests" in p.name or "/tests/" in str(p))
    return sorted(c for c in cases if not re.search(rf"\bAC-{c}\b", tests))


def plan(files, cases, root=REPO, tests_of_cases=None):
    """[(step's name, shell command)] for these changed files and touched cases."""
    code = [f for f in files if not is_doc(f)]
    # the repository is public, and a landing pushes by itself: always scanned first
    steps = [("security", "env -u TM_PORT -u TM_APP_DIR bash scripts/security_check.sh")]
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
    e2e = [f for f in sorted(tests) if re.fullmatch(r"scripts/e2e_\w+\.py", f) and (f in code or not DOCKER.search(f))]
    steps.append(("build", "make build"))
    if e2e or shows:  # what the e2e checks run
        steps.append(("debug-build", "cd terminumd && cargo build --locked"))
    if rust:
        steps.append(("clippy", "cd terminumd && cargo clippy --locked --all-targets -- -D warnings"))
    if rust or modules:
        steps.append(("cargo", "cd terminumd && cargo test --locked -- " + " ".join(sorted(modules | set(CRITICAL_CARGO)))))
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


def run(steps, root=REPO):
    logs = root / ".checks"
    logs.mkdir(exist_ok=True)
    failed = []
    for name, command in steps:
        optional = command.startswith("-")
        started = time.monotonic()
        log = logs / f"changed.{name}.log"
        with log.open("w") as out:
            code = subprocess.run(command.lstrip("-"), shell=True, cwd=root, stdout=out, stderr=subprocess.STDOUT).returncode
        seconds = round(time.monotonic() - started)
        if code == 0 or optional:
            print(f"{'ok  ' if code == 0 else 'note'} {name} ({seconds} s)" + ("" if code == 0 else f": {log}"), flush=True)
        else:
            print(f"FAIL {name} ({seconds} s): {log}", flush=True)
            print("".join(f"     {line}\n" for line in log.read_text(errors="replace").splitlines()[-15:]), end="", flush=True)
            failed.append(name)
    return failed


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--print", action="store_true", dest="only_print")
    parser.add_argument("--base", default="origin/main")
    args = parser.parse_args(argv)
    files, merge_base = changed_files(args.base)
    steps = plan(files, touched_cases(files, merge_base))
    print(f"changed: {len(files)} files against {merge_base[:9]}")
    # a new case in a change with no code may be what is not built yet; with code it has tests
    untested = uncited(new_cases(git("diff", merge_base)), REPO) if any(not is_doc(f) for f in files) else []
    if untested and not args.only_print:
        print("targeted: FAIL new cases no test cites: " + " ".join(f"AC-{c}" for c in untested))
        return 1
    if args.only_print or not steps:
        for name, command in steps:
            print(f"  {name}: {command}")
        return 0
    failed = run(steps)
    print(f"targeted: FAIL {' '.join(failed)}" if failed else "targeted: PASS")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
