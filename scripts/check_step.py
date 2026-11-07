#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Runs one check with its own short TMPDIR and TMUX_TMPDIR and fails if it leaves anything there
(spec AC-102): check_all.sh's guard, so a test's leftover fails the run with no false alarm from
another session's. A leftover is a file in the folder, or a process of the check's process group
or with the folder in its environment (a Mac hides the environment of Apple's own programs).
Whatever is left is named, and goes: by its pid, the folder on every exit.

  python3 scripts/check_step.py <command…>"""
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time

SETTLE_S = 15   # what ends by itself gets this long: terminumd's tmutil takes seconds on a busy Mac
LEFT = 3   # the exit status of a check that passed but left something
KILL_AFTER_S = 2   # a leftover still there this long after SIGTERM gets SIGKILL


def own_processes(folder, group):
    """(pid, command) of each process of `group` or whose environment names `folder`."""
    flag = "-E" if sys.platform == "darwin" else "e"
    listed = subprocess.run(["ps", "-ax", flag, "-ww", "-o", "pid=,pgid=,stat=,command="], capture_output=True, text=True, errors="replace").stdout
    found = []
    for line in listed.splitlines():
        fields = line.split(None, 3)
        if len(fields) < 4 or not fields[0].isdigit() or fields[2].startswith("Z") or int(fields[0]) == os.getpid():
            continue
        if fields[1] == str(group) or f"TMPDIR={folder}" in fields[3]:
            found.append((int(fields[0]), fields[3].split(" TMPDIR=")[0][:120]))
    return found


def files_in(folder):
    """What is left, relative: an empty tmux-<uid> is tmux's own and does not count."""
    try:
        os.rmdir(os.path.join(folder, f"tmux-{os.getuid()}"))
    except OSError:
        pass
    return sorted(os.path.relpath(os.path.join(parent, name), folder)
                  for parent, _, names in os.walk(folder) for name in names or [""]
                  if parent != folder or names)


def run(command, folder):
    """The check's (exit status, process group), in a group of its own; a signal here ends it too."""
    env = {**os.environ, "TMPDIR": folder, "TMUX_TMPDIR": folder, "NODE_DISABLE_COMPILE_CACHE": "1"}
    child = subprocess.Popen(command, env=env, start_new_session=True)

    def forward(number, _frame):
        os.killpg(child.pid, number)
    for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(number, forward)
    return child.wait(), child.pid


def main(command):
    if not command:
        sys.exit("usage: check_step.py <command…>")
    folder = os.path.realpath(tempfile.mkdtemp(prefix="tmc.", dir="/tmp"))
    try:
        status, group = run(command, folder)
        end = time.monotonic() + SETTLE_S
        processes = own_processes(folder, group)   # it reads every process's environment: once for a clean step
        while (files_in(folder) or processes) and time.monotonic() < end:
            time.sleep(1)
            processes = own_processes(folder, group)
        left = 0
        for pid, name in processes:
            left += 1
            print(f"LEFT process {pid}: {name}", file=sys.stderr)
            os.kill(pid, signal.SIGTERM)
        if processes:   # an interactive shell ignores SIGTERM; one spinning on a terminal gone, SIGHUP too
            time.sleep(KILL_AFTER_S)
            for pid, _ in own_processes(folder, group):
                os.kill(pid, signal.SIGKILL)
        for name in files_in(folder)[:50]:
            left += 1
            print(f"LEFT {name}", file=sys.stderr)
        return status or (LEFT if left else 0)
    finally:
        shutil.rmtree(folder, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
