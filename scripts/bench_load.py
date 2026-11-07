#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""terminumd's memory and CPU under load (terminum.md AC-20): 60 panes in 2 sessions of a
private tmux server, two pages each showing a pane that prints a line a second; `ps`
samples terminumd every second. Budgets: RSS under 60 MB and CPU under 2 % loaded; with no
output and no page, CPU under 0.5 %. Also times a list refresh: `list-panes` of 60 panes in the
format terminumd asks for (titles.rs LIST_PANES_FORMAT).
Exits non-zero naming the number and the budget when one is missed.

    python3 scripts/bench_load.py [--minutes 5] [--idle-seconds 60]
"""
import argparse
import ast
import os
import re
import statistics
import subprocess
import sys
import time
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd, tmux_on  # noqa: E402
from testrun import free_port, own  # noqa: E402

PORT, SOCKET = free_port(), own("tm-test-bench-load")
tmux = partial(tmux_on, SOCKET)
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def list_panes_format(source=None):
    """titles.rs's LIST_PANES_FORMAT, its macro filled in; raises when the source no longer says it so."""
    if source is None:
        with open(os.path.join(ROOT, "terminumd/src/tmux/titles.rs"), encoding="utf-8") as f:
            source = f.read()
    literal = r'"(?:[^"\\]|\\.)*"'
    text = lambda rust: ast.literal_eval(rust.replace("\\\n", ""))   # Rust's line continuation; its other escapes are Python's
    head, tail = re.search(rf"LIST_PANES_FORMAT: &str = concat!\(({literal}), agent_name_format!\(\), ({literal})\);", source, re.S).groups()
    name = re.search(rf"\(\) => \{{ ({literal}) \}};", source).group(1)
    return text(head) + text(name) + text(tail)


def sample(pid, seconds):
    rss, cpu = [], []
    end = time.time() + seconds
    while time.time() < end:
        out = subprocess.run(["ps", "-o", "rss=,%cpu=", "-p", str(pid)], capture_output=True, text=True).stdout.split()
        if len(out) == 2:
            rss.append(int(out[0]) / 1024)
            cpu.append(float(out[1]))
        time.sleep(1)
    return rss, cpu


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--minutes", type=float, default=5)
    ap.add_argument("--idle-seconds", type=float, default=60)
    a = ap.parse_args()
    t = Terminumd(PORT, SOCKET, env={"TM_LOG": "warn"})
    failed = []
    try:
        t.start()
        time.sleep(1)
        tmux("new-session", "-d", "-s", "work")
        for s in ("main", "work"):
            while int(tmux("display", "-p", "-t", s, "#{session_windows}").strip() or 0) < 30:
                tmux("new-window", "-d", "-t", f"{s}:")
        time.sleep(3)
        pages = []
        for k, s in enumerate(("main", "work")):
            ws = t.page()
            g = ws.until(lambda m: m["t"] == "layout" and sum(len(x["items"]) for x in m["groups"]) >= 60, 15)
            item = next(i for x in g["groups"] if x["label"] == s for i in x["items"])
            ws.send({"t": "sub", "id": item["id"]})
            ws.send({"t": "size", "cols": 120, "rows": 40, "typing": False})
            ws.send({"t": "in", "data": "while true; do date; sleep 1; done\r"})
            pages.append(ws)
        panes = sum(1 for _ in tmux("list-panes", "-a", "-F", "x").splitlines())
        t0 = time.perf_counter()
        tmux("list-panes", "-a", "-F", list_panes_format())
        print(f"panes {panes}; one list-panes {1000 * (time.perf_counter() - t0):.1f} ms (tmux side)")
        stop = time.time() + a.minutes * 60
        rss, cpu = [], []
        while time.time() < stop:
            for ws in pages:                    # keep reading: a page that stops is dropped
                try:
                    while True:
                        ws.recv(0.05)
                except Exception:
                    pass
            r, c = sample(t.proc.pid, 5)
            rss += r
            cpu += c
        print(f"loaded: RSS max {max(rss):.1f} MB, CPU mean {statistics.mean(cpu):.2f} %")
        if max(rss) >= 60:
            failed.append(f"RSS {max(rss):.1f} MB over 60 MB")
        if statistics.mean(cpu) >= 2:
            failed.append(f"CPU {statistics.mean(cpu):.2f} % over 2 %")
        for ws in pages:
            ws.close()
        for s in ("main", "work"):
            tmux("send-keys", "-t", s, "C-c", "")
        time.sleep(3)
        _, idle = sample(t.proc.pid, a.idle_seconds)
        print(f"idle: CPU mean {statistics.mean(idle):.2f} %")
        if statistics.mean(idle) >= 0.5:
            failed.append(f"idle CPU {statistics.mean(idle):.2f} % over 0.5 %")
    finally:
        t.cleanup()
    for f in failed:
        print("BUDGET MISSED:", f)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
