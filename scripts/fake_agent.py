#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""A coding agent as Dispatcher sees it, for web/test/e2e_livedispatcher.mjs: the measured input box of
Claude Code or Codex (terminum-dispatcher.md AS-125), hooks by `terminum hook`, finish_task by `terminum
mcp`; no model. What it does with a prompt is in the prompt's words:
FINISHES, WORKS, SUBAGENT, TOKENS, ASKS, LIMIT, DIES, CLEAN, SILENT; any other stops at once. `--stubborn`: SIGTERM is ignored. `--start trust`: a question before its box; `--start
none`: never a box; `--start fast`: exits at once with 7."""
import json
import os
import re
import signal
import subprocess
import sys
import termios
import tty

RULE = "\x1b[37m" + "─" * 60 + "\x1b[0m"


def main():
    args = sys.argv[1:]
    cli = args[0]
    opt = lambda name, default: args[args.index(name) + 1] if name in args else default
    kind, start, session = opt("--kind", "claude"), opt("--start", ""), opt("--resume", f"fake-{os.getpid()}")
    typed = ""

    def hook(event, **more):
        subprocess.run([cli, "hook", kind], input=json.dumps({"session_id": session, "cwd": os.getcwd(), "hook_event_name": event, **more}), text=True, capture_output=True, timeout=8)

    def draw(body):
        sys.stdout.write("\x1b[2J\x1b[H" + "\r\n".join(body) + "\r\n")
        sys.stdout.flush()

    def box():
        if kind == "codex":
            shown = f"\x1b[1m›\x1b[0m \x1b[2mAsk Codex to do anything\x1b[0m" if not typed else f"\x1b[1m›\x1b[0m {typed}"
            draw(["  OpenAI Codex", "", shown, "", "  ? for shortcuts"])
        else:
            draw(["  Claude Code", "", RULE, f"\x1b[39m❯ {typed.replace(chr(10), ' ')}\x1b[7m \x1b[0m", RULE, "  ? for shortcuts"])

    def finish(task):
        rpc = lambda i, m, p=None: json.dumps({"jsonrpc": "2.0", **({} if i is None else {"id": i}), "method": m, **({"params": p} if p else {})})
        lines = [rpc(1, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "fake", "version": "1"}}), rpc(None, "notifications/initialized"),
                 rpc(2, "tools/call", {"name": "finish_task", "arguments": {"id": task, "result": "All done."}})]
        subprocess.run([cli, "mcp"], input="\n".join(lines) + "\n", text=True, capture_output=True, timeout=8)

    if "--stubborn" in args:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
    if start == "fast":
        sys.exit(7)
    if start == "none":
        draw(["something it does not know"])
        subprocess.run(["sleep", "600"])
        return
    fd = sys.stdin.fileno()
    saved = termios.tcgetattr(fd)
    tty.setraw(fd)
    sys.stdout.write("\x1b[?2004h")
    try:
        if start == "trust":
            draw([" Accessing workspace:", "", " Quick safety check", "", " ❯ No, exit", "   Yes, I trust this folder", "", " Enter to confirm · Esc to cancel"])
            os.read(fd, 16)
        box()
        if kind == "claude":
            hook("SessionStart", source="resume" if "--resume" in args else "startup")
        while True:
            data = os.read(fd, 65536).decode("utf8", "replace")
            if not data:
                return
            # what is pasted (between the markers) is text, line ends too; a bare CR is Enter
            ends = False
            for at, part in enumerate(re.split(r"\x1b\[20[01]~", data)):
                if at % 2:
                    typed += part.replace("\r", " ").replace("\n", " ")
                else:
                    ends = ends or "\r" in part
                    typed += part.split("\r")[0]
            if not ends:
                box()
                continue
            prompt, typed = typed, ""
            box()
            if "SILENT" in prompt:
                continue
            if kind == "codex":
                hook("SessionStart", source="startup")
            hook("UserPromptSubmit", prompt=prompt)
            several = re.search(r"tasks: ((?:T-\d+(?:, )?)+)", prompt)   # a press of several names them all (AC-138)
            named = re.search(r"T-\d+", prompt)
            tasks = re.findall(r"T-\d+", several[1]) if several else [named[0]] if named else [""]
            if "LIMIT" in prompt:
                hook("StopFailure", error="rate_limit")
            elif "DIES" in prompt:
                sys.exit(3)
            elif "CLEAN" in prompt:
                hook("SessionEnd", reason="other")
                sys.exit(0)
            elif "ASKS" in prompt:
                hook("Notification", notification_type="permission_prompt", message="Approve Bash")
            elif "SUBAGENT" in prompt:
                hook("SubagentStart", agent_id="sub-1", agent_type="reviewer")
            elif "TOKENS" in prompt:
                path = os.path.join(os.getcwd(), f"transcript-{os.getpid()}.jsonl")
                with open(path, "w", encoding="utf8") as f:
                    f.write(json.dumps({"type": "assistant", "message": {"id": "m1", "usage": {"input_tokens": 1200, "output_tokens": 34, "cache_read_input_tokens": 5}}}) + "\n")
                hook("Stop", last_assistant_message="done", transcript_path=path)
            elif "WORKS" in prompt:
                hook("PreToolUse", tool_name="Bash", tool_input={"command": "sleep"})   # a long tool: no Stop
            elif "FINISHES" in prompt:
                hook("PostToolUse", tool_name="Read", tool_input={}, tool_response={})
                for task in tasks:
                    finish(task)
                hook("Stop", last_assistant_message="done")
            else:
                hook("Stop", last_assistant_message="I did part of it.")
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, saved)


if __name__ == "__main__":
    main()
