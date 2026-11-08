#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""`make release`: leftovers cleared, the full checks, then scripts/release.py (spec AC-104). While it
runs, landings still push but leave the primary checkout where it is, so a release is never stopped by
a tree changing under it. Failing tests, here or on the Mac test runner, are what stops a release; a
problem clearing leftovers, or a runner that could not run them, is said. The full run starts on a quiet
machine, one at a time (scripts/load_gate.py, AC-146); still busy after 15 minutes, the release ends
before any test. `--rehearse` (`make rehearse`) runs the same chain with no mark and stops before
anything is signed or public."""
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import load_gate  # noqa: E402
import release  # noqa: E402
from worktree import mark_path, primary_of, releasing  # noqa: E402

MAC_CAP_S = 3600   # a runner gone quiet must not hold a release
BUSY_WAIT_S, BUSY_RETRY_S = 1800, 60
# ci_mac.sh's exits that are no failing test, what each says
COULD_NOT_RUN = {0: "", 2: "the runner could not run them", 3: "the runner is busy, or its lock is stale"}


def mac_checks(repo):
    """The checks on the Mac test runner (scripts/ci_mac.sh): (failing tests, why they did not run)."""
    if not os.environ.get("TM_MAC_RUNNER"):
        return False, "no TM_MAC_RUNNER"
    busy_until = time.monotonic() + BUSY_WAIT_S
    while True:
        run = subprocess.Popen(["bash", "scripts/ci_mac.sh"], cwd=repo, start_new_session=True)
        try:
            status = run.wait(MAC_CAP_S)
        except subprocess.TimeoutExpired:
            os.killpg(run.pid, signal.SIGTERM)   # its ssh too
            run.wait()
            return False, f"it took over {MAC_CAP_S // 60} minutes"
        if status != 3 or time.monotonic() >= busy_until:   # another run, as a rehearsal, may just be ending
            return status not in COULD_NOT_RUN, COULD_NOT_RUN.get(status, "")
        time.sleep(BUSY_RETRY_S)


def chain(repo, tag, rehearsing):
    """Leftovers, a quiet machine, the full checks, the Mac test runner's, then scripts/release.py."""
    subprocess.run([sys.executable, "scripts/leftovers.py", "--end"], cwd=repo)
    head = release.run("git", "rev-parse", "HEAD", cwd=repo)
    with load_gate.Turn(repo) as turn:
        if subprocess.run(["bash", "scripts/check_all.sh", "--passed"], cwd=repo).returncode:   # a pass waits for nothing
            busy = load_gate.wait(busy=turn.busy)
            if busy:
                print(f"release: the build server is not available for a release: {busy} {load_gate.MINUTES}: nothing is public", file=sys.stderr)
                return 1
        if subprocess.run(["bash", "scripts/check_all.sh"], cwd=repo, env={**os.environ, "TM_GATE_PASSED": "1"}).returncode:
            print("release: failing tests stop it: nothing is public", file=sys.stderr)
            return 1
    failed, why = mac_checks(repo)
    if failed:
        print("release: failing tests on the Mac test runner stop it: nothing is public", file=sys.stderr)
        return 1
    said = f"the Mac test runner's checks did not run: {why}" if why else ""
    try:
        release.main(tag, head, said, rehearsing)
    finally:
        if said:
            print(f"release: {said}")
    return 0


def main(argv):
    rehearsing = argv[:1] == ["--rehearse"]
    if len(argv) != 1 + rehearsing or not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", argv[-1]):
        sys.exit("usage: release_turn.py [--rehearse] vX.Y.Z")
    tag = argv[-1]
    repo = Path(__file__).resolve().parent.parent
    if rehearsing:
        if primary_of(repo) == repo:   # its build replaces ui/dist and dist/package under other sessions
            sys.exit("make rehearse runs in a worktree: python3 scripts/worktree.py add <slug> --kind chore")
        if releasing(repo):
            sys.exit("a release is running: rehearse after it")
    release.check_settings()   # before the long run: none of these is any test's fault
    release.check_source(tag)
    if rehearsing:
        return chain(repo, tag, True)
    release.release_notes(tag)
    mark = mark_path(repo)
    mark.write_text(f"{os.getpid()}\n")
    try:
        return chain(repo, tag, False)
    finally:
        mark.unlink(missing_ok=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
