#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""`make release`: leftovers cleared, the full checks, then scripts/release.py (spec AC-104). While it
runs, landings still push but leave the primary checkout where it is, so a release is never stopped by
a tree changing under it. Failing tests are what stops a release; a problem clearing leftovers is said."""
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release  # noqa: E402

MARK = "terminum-release"


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


def main(argv):
    if len(argv) != 1:
        sys.exit("usage: release_turn.py vX.Y.Z")
    repo = Path(__file__).resolve().parent.parent
    release.release_notes(argv[0])  # before the long run: a missing file is no test's fault
    mark = mark_path(repo)
    mark.write_text(f"{os.getpid()}\n")
    try:
        subprocess.run([sys.executable, "scripts/leftovers.py", "--end"], cwd=repo)
        if subprocess.run(["bash", "scripts/check_all.sh"], cwd=repo).returncode:
            print("release: failing tests stop it: nothing is public", file=sys.stderr)
            return 1
        return subprocess.run([sys.executable, "scripts/release.py", argv[0]], cwd=repo).returncode
    finally:
        mark.unlink(missing_ok=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
