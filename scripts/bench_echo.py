#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Key echo latency (terminum.md AC-20): types 200 keys into a shell in a private tmux
server through a private terminumd's page socket and times each one from sending it to the
frame that shows it. Budget: p50 < 25 ms, p95 < 60 ms on loopback; exits non-zero naming
the number and the budget when one is missed.

    python3 scripts/bench_echo.py [--keys 200]
"""
import argparse
import os
import math
import statistics
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import Terminumd  # noqa: E402
from testrun import free_port, own  # noqa: E402

PORT, SOCKET = free_port(), own("tm-test-bench-echo")


def text_of(ch):
    return "\n".join("".join(r[0] if isinstance(r[0], str) else "".join(r[0]) for r in line["r"]) for _, line in ch)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--keys", type=int, default=200)
    a = ap.parse_args()
    t = Terminumd(PORT, SOCKET, env={"TM_LOG": "warn"})
    times, missed = [], 0
    try:
        t.start()
        ws = t.page()
        g = ws.until(lambda m: m["t"] == "layout" and m["groups"] and m["groups"][0]["items"], 5)["groups"]
        ws.send({"t": "sub", "id": g[0]["items"][0]["id"]})
        ws.send({"t": "size", "cols": 200, "rows": 60, "typing": False})
        ws.until(lambda m: m["t"] == "screen", 5)
        time.sleep(1)
        ws.send({"t": "in", "data": "clear\r"})
        time.sleep(0.5)
        while True:                             # drain what came meanwhile
            try:
                ws.recv(0.2)
            except Exception:
                break
        typed = ""
        for i in range(a.keys):
            ch = "abcdefghijklmnopqrstuvwxyz"[i % 26]
            if len(typed) >= 150:               # a fresh line keeps the row short
                ws.send({"t": "in", "data": "\x15"})
                ws.until(lambda m: m["t"] == "screen", 2)
                typed = ""
            typed += ch
            t0 = time.perf_counter()
            ws.send({"t": "in", "data": ch, "k": i + 1})
            m = ws.until(lambda m: m["t"] == "screen" and typed in text_of(m["ch"]), 2)
            if m is None:
                print(f"key {i + 1}: no echo within 2 s")
                missed += 1
                continue
            times.append((time.perf_counter() - t0) * 1000)
            time.sleep(0.02)
    finally:
        t.cleanup()
    if not times:
        print("no echo measured")
        sys.exit(1)
    times.sort()
    p50, p95 = statistics.median(times), times[math.ceil(len(times) * 0.95) - 1]
    print(f"keys {len(times)}/{a.keys}, missed {missed}  p50 {p50:.1f} ms  p95 {p95:.1f} ms  max {times[-1]:.1f} ms")
    failed = [f"p50 {p50:.1f} ms over 25 ms" for _ in [0] if p50 >= 25] + [f"p95 {p95:.1f} ms over 60 ms" for _ in [0] if p95 >= 60]
    if missed:
        failed.append(f"{missed} of {a.keys} keys had no echo within 2 s")
    for f in failed:
        print("BUDGET MISSED:", f)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
