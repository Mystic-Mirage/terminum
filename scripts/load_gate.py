#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""A full run on a quiet machine (spec AC-140). Before a release's full run, and before a task's first
timed check, the load average is looked at: over the limit it waits, a look each minute, and once the
load is low it starts 3 minutes later, those minutes looked at too. After 15 minutes a release gives up;
a task's checks run anyway, saying so. TM_GATE_LOAD sets the limit (half the cores: a guess).

  load_gate.py           make check by hand: exit 0 to go, 75 when still busy after 15 minutes
  load_gate.py --task    a task's timed checks: waits the same way, also while a release runs; exit 0"""
import fcntl
import os
from pathlib import Path
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
from worktree import common_dir, releasing  # noqa: E402

LOOK_S = 60
HOLD_S = 180
LIMIT_S = 900
BUSY = 75


def limit():
    try:
        return float(os.environ["TM_GATE_LOAD"])
    except (KeyError, ValueError):
        return (os.cpu_count() or 2) / 2


def fmt(number):
    return f"{number:.1f}".removesuffix(".0")


def wait(busy=lambda: None, load=lambda: os.getloadavg()[0], say=print, clock=time.monotonic, sleep=time.sleep):
    """None once the machine is quiet, else why it was still busy after LIMIT_S. busy(): why the machine
    is busy besides its load (a release, another full run), or None."""
    top = limit()

    def look():
        now = load()
        return now, busy() or (f"load {fmt(now)} over {fmt(top)}" if now > top else None)

    now, why = look()
    if why is None:
        say(f"gate: load {fmt(now)}, at most {fmt(top)}: starting")
        return None
    say(f"gate: {why}: waiting, a look each minute, {LIMIT_S // 60} minutes at most")
    began, quiet, last = clock(), None, why
    while True:
        if quiet is None and clock() - began >= LIMIT_S:   # once quiet, its 3 minutes may end past the 15
            return f"{last} for {LIMIT_S // 60} minutes"
        sleep(LOOK_S)
        now, why = look()
        if why:
            quiet, last = None, why
            say(f"gate: {why}: still waiting")
            continue
        quiet = clock() if quiet is None else quiet
        if clock() - quiet >= HOLD_S:
            say(f"gate: load {fmt(now)}, at most {fmt(top)} for {HOLD_S // 60} minutes: starting")
            return None
        say(f"gate: load {fmt(now)}, at most {fmt(top)}: starting once it stays so for {HOLD_S // 60} minutes")


class Turn:
    """One full run at a time across trees (a release behind a rehearsal); the kernel frees it with its holder."""
    def __init__(self, cwd):
        self.path, self.file = common_dir(cwd) / "terminum-gate.turn", None

    def take(self):
        """None once this run holds the turn, else why it waits."""
        if self.file is None:
            self.file = self.path.open("w")
        try:
            fcntl.flock(self.file, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return "another full run holds the turn (a release or a rehearsal in another tree)"
        return None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        if self.file:
            self.file.close()


def for_task(cwd=".", say=print, **kw):
    """A task's timed checks: wait as a release would, also while one runs, then go whatever happened."""
    why = wait(busy=lambda: "a release is running" if releasing(cwd) else None, say=say, **kw)
    if why:
        say(f"gate: running under {why.rsplit(' for ', 1)[0]}: a timing failure may be the load's")


def main(argv):
    if argv == ["--task"]:
        for_task()
        return 0
    if argv:
        sys.exit("usage: load_gate.py [--task]")
    why = wait()
    if why:
        print(f"check: the machine is busy: {why}: nothing ran", file=sys.stderr)
        return BUSY
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
