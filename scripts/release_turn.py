#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""`make release`: leftovers cleared, the full checks, then scripts/release.py (spec AC-104). While it
runs, landings still push but leave the primary checkout where it is, so a release is never stopped by
a tree changing under it. Failing tests, here or on the Mac test runner, are what stops a release; a
problem clearing leftovers, or a runner that could not run them, is said. `--rehearse` (`make rehearse`)
runs the same chain with no mark and stops before anything is signed or public."""
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release  # noqa: E402
from worktree import mark_path, releasing  # noqa: E402

MAC_CAP_S = 3600   # a runner gone quiet must not hold a release
# ci_mac.sh's exits that are no failing test, what each says
COULD_NOT_RUN = {0: "", 2: "the runner could not run them", 3: "the runner is busy, or its lock is stale"}


def mac_checks(repo):
    """The checks on the Mac test runner (scripts/ci_mac.sh): (failing tests, why they did not run)."""
    if not os.environ.get("TM_MAC_RUNNER"):
        return False, "no TM_MAC_RUNNER"
    run = subprocess.Popen(["bash", "scripts/ci_mac.sh"], cwd=repo)
    try:
        status = run.wait(MAC_CAP_S)
    except subprocess.TimeoutExpired:
        run.terminate()
        run.wait()
        return False, f"it took over {MAC_CAP_S // 60} minutes"
    return status not in COULD_NOT_RUN, COULD_NOT_RUN.get(status, "")


def chain(repo, argv):
    """Leftovers, the full checks, the Mac test runner's, then scripts/release.py with these arguments."""
    subprocess.run([sys.executable, "scripts/leftovers.py", "--end"], cwd=repo)
    head = release.run("git", "rev-parse", "HEAD", cwd=repo)
    if subprocess.run(["bash", "scripts/check_all.sh"], cwd=repo).returncode:
        print("release: failing tests stop it: nothing is public", file=sys.stderr)
        return 1
    failed, why = mac_checks(repo)
    if failed:
        print("release: failing tests on the Mac test runner stop it: nothing is public", file=sys.stderr)
        return 1
    done = subprocess.run([sys.executable, "scripts/release.py", *argv], cwd=repo, env={**os.environ, "TM_GATE_HEAD": head}).returncode
    if why:   # last, where it is seen: it belongs under Not verified in the release notes
        print(f"release: the Mac test runner's checks did not run: {why}")
    return done


def main(argv):
    rehearsing = argv[:1] == ["--rehearse"]
    if len(argv) != 1 + rehearsing:
        sys.exit("usage: release_turn.py [--rehearse] vX.Y.Z")
    repo = Path(__file__).resolve().parent.parent
    if rehearsing and releasing(repo):
        sys.exit("a release is running: rehearse after it")
    release.check_settings()   # before the long run: neither is any test's fault
    if rehearsing:
        return chain(repo, argv)
    release.release_notes(argv[0])
    mark = mark_path(repo)
    mark.write_text(f"{os.getpid()}\n")
    try:
        return chain(repo, argv)
    finally:
        mark.unlink(missing_ok=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
