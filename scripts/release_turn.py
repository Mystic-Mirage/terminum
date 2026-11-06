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

MARK = "terminum-release"
MAC_CAP_S = 3600   # a runner gone quiet must not hold a release


def mark_path(cwd):
    common = subprocess.run(["git", "rev-parse", "--path-format=absolute", "--git-common-dir"], cwd=cwd, capture_output=True, text=True, check=True).stdout.strip()
    return Path(common) / MARK


def releasing(cwd):
    """True while a release of this repository runs: its mark names a live process."""
    try:
        os.kill(int(mark_path(cwd).read_text().strip()), 0)
    except (OSError, ValueError):
        return False
    return True


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
    return status not in (0, 2, 3), {0: "", 3: "the runner is busy, or its lock is stale"}.get(status, "the runner could not run them")


def main(argv):
    rehearsing = argv[:1] == ["--rehearse"]
    argv = argv[1:] if rehearsing else argv
    if len(argv) != 1:
        sys.exit("usage: release_turn.py [--rehearse] vX.Y.Z")
    repo = Path(__file__).resolve().parent.parent
    if rehearsing and releasing(repo):
        sys.exit("a release is running: rehearse after it")
    release.check_settings()   # before the long run: neither is any test's fault
    if not rehearsing:
        release.release_notes(argv[0])
    mark = mark_path(repo)
    if not rehearsing:
        mark.write_text(f"{os.getpid()}\n")
    try:
        subprocess.run([sys.executable, "scripts/leftovers.py", "--end"], cwd=repo)
        head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=repo, capture_output=True, text=True, check=True).stdout.strip()
        if subprocess.run(["bash", "scripts/check_all.sh"], cwd=repo).returncode:
            print("release: failing tests stop it: nothing is public", file=sys.stderr)
            return 1
        failed, why = mac_checks(repo)
        if failed:
            print("release: failing tests on the Mac test runner stop it: nothing is public", file=sys.stderr)
            return 1
        done = subprocess.run([sys.executable, "scripts/release.py", *(["--rehearse"] if rehearsing else []), argv[0]],
                              cwd=repo, env={**os.environ, "TM_GATE_HEAD": head}).returncode
        if why:   # last, where it is seen: it belongs under Not verified in the release notes
            print(f"release: the Mac test runner's checks did not run: {why}")
        return done
    finally:
        if not rehearsing:
            mark.unlink(missing_ok=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
