#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Typing regressions on private tmux servers, including a local fake SSH host.

    python3 scripts/e2e_typing_review.py

Checks queued input destinations and Unicode editing keys through a real daemon.
"""
import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
from testrun import free_port, own  # noqa: E402

from e2e_ws import Terminumd, check, summary, unix_request, wait_for, wait_layout

HERE = Path(__file__).resolve().parent


def tmux(socket, *args):
    return subprocess.run(["tmux", "-f", "/dev/null", "-L", socket, *args],
                          capture_output=True, text=True, check=True).stdout



def recorder(socket, pane, output, ready):
    # With tty echo off, the first 512-byte part never prints: typing's next part waits
    # PART_WAIT (250 ms), leaving a deterministic interval for two destinations to queue.
    command = f"stty -echo -icanon min 1 time 0; touch {shlex.quote(str(ready))}; exec cat > {shlex.quote(str(output))}"
    tmux(socket, "respawn-pane", "-k", "-t", pane, command)
    wait_for(ready.exists, bool)


def unicode_keys():
    """The actual cursor module's keys for a DOM row with astral Unicode before its cursor."""
    source = r'''
import { cursorCell, arrowsTo } from './web/static/cursor.js';
const text = { data: 'a😀b' }, caret = { data: ' ' }, row = {};
const cur = { closest: () => row };
globalThis.NodeFilter = { SHOW_TEXT: 4 };
globalThis.document = { createTreeWalker(root) {
  const nodes = root === cur ? [caret] : [text, caret];
  let i = 0;
  return { nextNode: () => nodes[i++] ?? null };
} };
const screen = { querySelector: () => cur, querySelectorAll: () => [row] };
const from = cursorCell(screen);
process.stdout.write(JSON.stringify({ from, keys: arrowsTo(from, { row: 0, col: 1 }) }));
'''
    result = subprocess.run(["node", "--input-type=module", "-e", source],
                            cwd=HERE.parent, capture_output=True, text=True, check=True)
    return json.loads(result.stdout)


def main():
    t = Terminumd(free_port(), own("tm-review-typing"))
    remote_socket = own("tm-review-rt")
    remote = Path(t.root) / "remote"
    remote.mkdir()
    fake_pids = Path(t.root) / "fake-ssh-pids"
    t.env.update(HOME=t.root, TM_SSH=str(HERE / "fake_ssh.py"),
                 FAKE_SSH_HOME=str(remote), FAKE_SSH_TMUX_SOCKET=remote_socket,
                 FAKE_SSH_PIDS=str(fake_pids))
    ws = None
    try:
        t.start()
        ws = t.page()
        st, body = unix_request(t.sock, "POST", "/hosts/add",
                                {"name": "typingbox", "recipe": "alex@devbox.example"})
        assert st == 200, body
        groups = wait_layout(ws, lambda gs: any(g.get("host") == "typingbox" and g.get("items") for g in gs), 10)
        assert groups, "Private fake host did not attach"
        local_pane = tmux(t.tmux_socket, "list-panes", "-t", "main", "-F", "#{pane_id}").strip()
        remote_pane = tmux(remote_socket, "list-panes", "-t", "terminum", "-F", "#{pane_id}").strip()
        assert local_pane == remote_pane, "The regression requires equal server-local pane ids"
        local_key = f"tmux:local:{t.tmux_socket}:{local_pane}"
        remote_key = next(i["id"] for g in groups if g.get("host") == "typingbox" for i in g["items"])
        local_out, remote_out = Path(t.root) / "local.out", Path(t.root) / "remote.out"
        recorder(t.tmux_socket, local_pane, local_out, Path(t.root) / "local.ready")
        recorder(remote_socket, remote_pane, remote_out, Path(t.root) / "remote.ready")
        ws.send({"t": "sub", "id": local_key})
        assert ws.until(lambda m: m.get("t") == "screen" and m.get("sid") == local_key, 5)
        read = lambda p: p.read_text() if p.exists() else ""
        ws.send({"t": "in", "data": "a" * 4096})
        partial = wait_for(lambda: read(local_out), lambda s: len(s) >= 512, every=0.02)   # while the first send is in flight
        assert len(partial) < 4096, "The first send must still be in flight when other input queues"
        ws.send({"t": "in", "data": "LOCAL_REVIEW\r"})
        ws.send({"t": "sub", "id": remote_key})
        ws.send({"t": "in", "data": "REMOTE_REVIEW\r"})
        wait_for(lambda: read(local_out), lambda s: "LOCAL_REVIEW" in s)
        wait_for(lambda: read(remote_out), lambda s: "REMOTE_REVIEW" in s)
        local_text, remote_text = read(local_out), read(remote_out)
        check("AC-03 queued local and fake-remote input sharing a pane number stay separate",
              "REMOTE_REVIEW" not in local_text and "LOCAL_REVIEW" not in remote_text,
              f"local bytes={len(local_text)}, remote bytes={len(remote_text)}")

        # A real bash/readline accepts the actual cursor module's generated keys through
        # the daemon; browser hit-testing is covered separately by webReview.
        pane = tmux(t.tmux_socket, "new-window", "-d", "-t", "main:", "-P", "-F", "#{pane_id}",
                    "-c", t.root, "env PS1='review$ ' LC_CTYPE=en_US.UTF-8 /bin/bash --noprofile --norc").strip()
        key = f"tmux:local:{t.tmux_socket}:{pane}"
        capture = lambda: tmux(t.tmux_socket, "capture-pane", "-p", "-t", pane)
        wait_for(capture, lambda s: "review$" in s)
        ws.send({"t": "sub", "id": key})
        assert ws.until(lambda m: m.get("t") == "screen" and m.get("sid") == key, 5)
        movement = unicode_keys()
        check("AC-40 actual cursor module counts a😀b as three editing characters",
              movement["from"] == {"row": 0, "col": 3} and movement["keys"] == "\x1b[D" * 2)
        ws.send({"t": "in", "data": "printf '%s\\n' a😀b"})
        wait_for(capture, lambda s: "a😀b" in s)
        ws.send({"t": "in", "data": movement["keys"] + "X\r"})
        shown = wait_for(capture, lambda s: "aX😀b" in s)
        check("AC-40 two generated Left keys insert before an emoji in real bash/readline",
              any(line.strip() == "aX😀b" for line in shown.splitlines()))
    except (AssertionError, subprocess.SubprocessError) as e:
        check("Private live typing scenario", False, e)
    finally:
        if ws:
            ws.close()
        t.stop()
        for line in fake_pids.read_text().splitlines() if fake_pids.exists() else []:
            try:
                os.kill(int(line.split()[0]), signal.SIGTERM)
            except ProcessLookupError:
                pass
        t.cleanup()
        subprocess.run(["tmux", "-L", remote_socket, "kill-server"], stderr=subprocess.DEVNULL)
    summary()


if __name__ == "__main__":
    main()
