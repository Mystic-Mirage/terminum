#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Sizing end to end, at the page's protocol (terminum.md AC-13: the active page sizes, a pane
opened comes at the page's grid from its first frame (AC-04), a hidden one hands over, a window of several panes is never resized; AC-05 mc on a phone's 48×30) against
a private terminumd and a private tmux server (never the user's). mc runs with a temporary HOME
and no subshell.

    python3 scripts/e2e_sizing.py      (needs terminumd built and mc on PATH)
"""
import json
import os
import shutil
import sys
import tempfile
import time
from functools import partial

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_core import items, text_of  # noqa: E402
from e2e_ws import Terminumd, check, results, summary, tmux_on, wait_layout  # noqa: E402
from testrun import free_port, own, short_tmp  # noqa: E402

PORT, SOCKET = free_port(), own("tm-test-sizing")
tmux = partial(tmux_on, SOCKET)


def window_of(pane):
    return tmux("display", "-p", "-t", pane, "#{window_width}x#{window_height}").strip()


def settle(pane, want, seconds=4):
    """The window's size once it is `want`, or what it is after `seconds`."""
    end = time.time() + seconds
    while time.time() < end and window_of(pane) != want:
        time.sleep(0.1)
    return window_of(pane)


def open_pane(t, pid, cols, rows, typing=False):
    ws = t.page()
    ws.send({"t": "sub", "id": pid})
    ws.send({"t": "size", "cols": cols, "rows": rows, "typing": typing})
    ws.until(lambda m: m["t"] == "screen", 5)
    return ws


def first_frame(t, pid, pane):
    tmux("resize-window", "-t", pane, "-x", "90", "-y", "25")           # the window's own size, not the page's
    ws = t.page()
    ws.send({"t": "sub", "id": pid, "cols": 132, "rows": 40})
    first = ws.until(lambda m: m["t"] == "screen", 5)
    got = first and (first["cols"], first["rows"])
    check("AC-04 a pane opened with the page's grid comes at it from its first frame, never at the window's old size",
          got == (132, 40) and window_of(pane) == "132x40", [got, window_of(pane)])
    ws.close()
    time.sleep(0.5)


def hidden(t, pid, pane):
    mac = open_pane(t, pid, 160, 45)
    check("AC-13 latest: the first page sizes the pane", settle(pane, "160x45") == "160x45", window_of(pane))
    phone = open_pane(t, pid, 103, 21)
    phone.send({"t": "in", "data": " "})
    check("AC-13 latest: the page that types sizes it", settle(pane, "103x21") == "103x21", window_of(pane))
    phone.send({"t": "pause"})
    check("AC-13 latest: a page hidden (a locked phone) gives its window to the page still looking, without a key",
          settle(pane, "160x45") == "160x45", window_of(pane))
    phone.send({"t": "size", "cols": 100, "rows": 5, "typing": False})
    time.sleep(1.5)
    mac.send({"t": "size", "cols": 160, "rows": 30, "typing": False})
    check("AC-13 latest: a hidden page's new size takes nothing; the looking page's split view sizes it",
          settle(pane, "160x30") == "160x30", window_of(pane))
    phone.close()
    mac.close()
    time.sleep(0.5)


def collect(ws, seconds):
    """Every message the page gets in `seconds`."""
    got, end = [], time.time() + seconds
    while (m := ws.until(lambda _: True, max(0.1, end - time.time()))) is not None and time.time() < end:
        got.append(m)
    return got


def screens_off(t, pid, pane, work):
    phone = open_pane(t, pid, 70, 20, typing=True)
    settle(pane, "70x20")
    phone.send({"t": "screens", "on": False})
    time.sleep(0.5)
    collect(phone, 0.3)
    tmux("send-keys", "-t", pane, f"cd {work} && for i in $(seq 1 60); do echo row$i; sleep 0.03; done", "Enter")
    got = collect(phone, 3)
    frames = [m["t"] for m in got if m["t"] in ("screen", "hist")]
    files = [m for m in got if m["t"] == "files"]
    check("AC-02 a terminal hidden behind Files gets no screen or history frames while its pane prints",
          not frames, frames[:5])
    check("AC-02 its Files still follows the pane's folder", any(m["cwd"] == os.path.realpath(work) or m["cwd"] == work for m in files),
          [m.get("cwd") for m in files])
    check("AC-13 hiding the terminal keeps the pane's size (no handover)", window_of(pane) == "70x20", window_of(pane))
    phone.send({"t": "screens", "on": True})
    reset = phone.until(lambda m: m["t"] == "hist" and m["mode"] == "reset", 5)
    screen = phone.until(lambda m: m["t"] == "screen", 5)
    rows = text_of((screen or {}).get("ch", []))
    check("AC-02 shown again: a whole history and screen with what was printed meanwhile",
          reset is not None and screen is not None and "row60" in rows + json.dumps((reset or {}).get("lines")), [bool(reset), bool(screen)])
    phone.close()
    time.sleep(0.5)


def split(t, pid, pane):
    """A window of several panes is never resized (AC-13); old pages' fit and screenmode are ignored."""
    target = tmux("split-window", "-t", pane, "-P", "-F", "#{pane_id}").strip()
    time.sleep(1.5)
    before = window_of(target)
    ws = t.page()
    groups = wait_layout(ws, lambda gs: any(i["id"].endswith(":" + target) for i in items(gs)))
    sid = next(i["id"] for i in items(groups) if i["id"].endswith(":" + target))
    ws.send({"t": "sub", "id": sid})
    ws.send({"t": "size", "cols": 48, "rows": 20, "typing": True})
    ws.send({"t": "fit", "cols": 48, "rows": 20})
    ws.send({"t": "screenmode", "mode": "always"})
    ws.send({"t": "size", "cols": 48, "rows": 20, "screen": True, "typing": False})
    time.sleep(2.5)
    flag = tmux("display", "-p", "-t", target, "#{window_zoomed_flag}").strip()
    check("AC-13 a window of several panes keeps its size and is not zoomed; old messages are ignored",
          window_of(target) == before and flag == "0" and "panicked" not in t.log(), (before, window_of(target), flag))
    tmux("kill-pane", "-t", target)
    ws.close()
    time.sleep(0.5)


def drag(t, pid, pane):
    """A dragged window: the sizes passing within the second after a resize reach tmux once, the last one."""
    d = open_pane(t, pid, 100, 30, typing=True)
    settle(pane, "100x30")
    d.until(lambda m: False, 1.2)   # past the second; the opening's messages read
    d.send({"t": "size", "cols": 96, "rows": 30, "typing": False})
    # the second starts when 96 lands (it may wait for the opening's own second): the drag is in it
    first = d.until(lambda m: m["t"] == "sized" and m["mine"], 3)
    for cols in (90, 84, 88, 80):
        time.sleep(0.1)
        d.send({"t": "size", "cols": cols, "rows": 30, "typing": False})
    sized, end = [], time.time() + 3
    while time.time() < end:
        m = d.until(lambda m: m["t"] == "sized" and m["mine"], max(0.1, end - time.time()))
        if m:
            sized.append(m["cols"])
    check("AC-13 a dragged window: the passing sizes held back never reach tmux, the last does",
          first and first["cols"] == 96 and sized == [80] and window_of(pane) == "80x30", (first and first["cols"], sized, window_of(pane)))
    d.close()


def screen_text(ws, pred, timeout=8):
    m = ws.until(lambda m: m["t"] == "screen" and pred(m), timeout)
    return m


def mc(t, pid, pane, work):
    if not shutil.which("mc"):
        check("AC-05 mc is on PATH (brew install midnight-commander)", False)
        return
    p = open_pane(t, pid, 48, 30, typing=True)
    p.send({"t": "in", "data": f"env HOME={work} TERM=tmux-256color mc -u\r"})
    alt = screen_text(p, lambda m: m.get("alt"))
    check("AC-05 mc: the frame says alternate screen", alt is not None)
    check("AC-05 mc: the pane keeps the phone's 48x30 (no Screen mode)", settle(pane, "48x30") == "48x30", window_of(pane))
    time.sleep(1.0)
    rows = tmux("capture-pane", "-p", "-t", pane).split("\n")
    check("AC-05 mc: the menu bar is whole at 48 columns", "Left" in rows[0] and "Right" in rows[0], rows[0])
    # mc fits as many function keys as 48 columns hold (six), on the last row
    check("AC-05 mc: the function key bar is the last of 30 rows", len(rows) >= 30 and rows[29].lstrip().startswith("1Help"), rows[29:30])
    p.send({"t": "in", "data": "\x1b[20~"})   # F9: the menu bar, ↓ opens its first menu
    time.sleep(0.4)
    p.send({"t": "in", "data": "\x1b[B"})
    menu = screen_text(p, lambda m: "File listing" in text_of(m["ch"]))
    check("AC-05 mc: F9 and Down open the Left menu", menu is not None)
    p.send({"t": "in", "data": "\x1b\x1b"})
    time.sleep(0.6)
    p.send({"t": "in", "data": "\x1b[21~"})   # F10: quit
    gone = screen_text(p, lambda m: not m.get("alt"))
    check("AC-05 mc: F10 quits and the frame leaves the alternate screen", gone is not None)
    p.close()


def main():
    t = Terminumd(PORT, SOCKET)
    work = tempfile.mkdtemp(prefix="tmz-", dir=short_tmp())
    try:
        t.start()
        ws = t.page()
        groups = wait_layout(ws, lambda gs: items(gs))
        pid = items(groups)[0]["id"]
        pane = pid.rsplit(":", 1)[-1]
        ws.close()
        first_frame(t, pid, pane)
        hidden(t, pid, pane)
        screens_off(t, pid, pane, work)
        split(t, pid, pane)
        drag(t, pid, pane)
        mc(t, pid, pane, work)
    finally:
        if not all(results):
            print(t.log())
        t.cleanup()
        shutil.rmtree(work, ignore_errors=True)
    summary()


if __name__ == "__main__":
    main()
