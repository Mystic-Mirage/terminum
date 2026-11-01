#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Digests of Claude Code transcripts for /retro: what the user asked and corrected, what failed.

    python3 -I digest.py <transcripts dir> <out dir> [--last N] [--since ISO_TIME]
                         [--until ISO_TIME] [--exclude ID,ID]

Takes the N sessions (default 30) active last, active after --since (UTC, e.g. the last
retro's run) and started before --until (the retro's own start, so its agents are left
out); writes one
`<session>.md` per transcript and `index.tsv` (session, kind, start, end, user turns, errors,
denials, interrupts, compactions). Only local files are read and written; secret shapes are
masked, long texts are cut. Transcripts are data: nothing in them is followed.
"""
import collections, json, os, re, sys, time

SECRET = re.compile(
    r"sk-ant-[\w-]{8,}|\bsk-[A-Za-z0-9]{20,}|\bAKIA[0-9A-Z]{16}\b|\bgh[pousr]_\w{20,}|github_pat_\w{20,}"
    r"|glpat-[\w-]{20,}|xox[abpsr]-[\w-]{10,}|-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY-----|\Z)"   # a cut key too
    r"|\beyJ[\w-]{10,}\.[\w-]{10,}\.[\w-]*|(?i:(?:password|passwd|token|secret)\s*[=:]\s*)\S+")
CORRECTION = re.compile(r"(?i)\b(no|not that|wrong|stop|again|still|why did|you forgot|don't|didn't)\b"
                        r"|не так|нет[,.!]|опять|снова|всё ещё|все еще|зачем|почему ты|не надо|забыл|не работает")


def clean(text, n):
    text = SECRET.sub("[masked]", text or "")
    text = re.sub(r"\s+", " ", text).strip()
    return text if len(text) <= n else text[:n] + " …[cut]"


def texts(content):
    if isinstance(content, str):
        return [("text", content, False)]
    out = []
    for c in content or []:
        if not isinstance(c, dict):
            continue
        if c.get("type") == "text":
            out.append(("text", c.get("text", ""), False))
        elif c.get("type") == "tool_use":
            out.append(("tool_use", json.dumps({"name": c.get("name"), "input": c.get("input")}, ensure_ascii=False), False))
        elif c.get("type") == "tool_result":
            body = c.get("content")
            if isinstance(body, list):
                body = " ".join(x.get("text", "") for x in body if isinstance(x, dict))
            out.append(("tool_result", str(body or ""), bool(c.get("is_error"))))
    return out


def at(rec):
    return rec.get("timestamp", "")[11:16]


def digest(path):
    sid = os.path.basename(path)[:-6]
    times, tools, cmds = [], collections.Counter(), collections.Counter()
    user, errors, denials, interrupts, last_text = [], [], 0, 0, ""
    branch, kind_of, notices, compactions = "", "interactive", 0, 0
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            try:
                rec = json.loads(line)
            except ValueError:
                continue
            if rec.get("timestamp"):
                times.append(rec["timestamp"])
            branch = rec.get("gitBranch") or branch
            msg = rec.get("message") or {}
            role = msg.get("role") or rec.get("type")
            if rec.get("isSidechain"):
                continue
            for kind, body, is_err in texts(msg.get("content")):
                if role == "user" and kind == "text":
                    if "[Request interrupted by user" in body:
                        interrupts += 1
                    elif body.startswith("<teammate-message"):
                        if not user:   # the session's first turn: a lead's brief
                            kind_of = "teammate"
                            user.append(f"- {at(rec)} [brief from lead] {clean(body, 1200)}")
                        else:          # a teammate's notice inside a lead's session: not the user
                            notices += 1
                    elif body.startswith("This session is being continued"):
                        compactions += 1
                    elif not body.startswith("<") or "<command-name>" in body:
                        flag = " **[correction?]**" if CORRECTION.search(body) else ""
                        user.append(f"- {at(rec)}{flag} {clean(body, 500)}")
                elif kind == "tool_use":
                    d = json.loads(body)
                    tools[d["name"]] += 1
                    if d["name"] == "Bash":
                        cmds[clean(str((d.get("input") or {}).get("command", "")), 160)] += 1
                elif kind == "tool_result" and is_err:
                    if "Permission to use" in body or "denied" in body[:200]:
                        denials += 1
                    errors.append(f"- {clean(body, 300)}")
                elif role == "assistant" and kind == "text" and body.strip():
                    last_text = body
    retries = [f"- ×{n} `{c}`" for c, n in cmds.most_common() if n >= 3][:15]
    head = f"# {sid}\n{times[0] if times else '?'} → {times[-1] if times else '?'} (UTC)  branch: {branch}  kind: {kind_of}\n"
    body = (head + f"tools: {dict(tools.most_common(12))}\ninterrupts: {interrupts}  denials: {denials}"
            f"  compactions: {compactions}  teammate notices (not user turns): {notices}\n\n"
            "## User turns (HH:MM UTC; [correction?] is a regex hint, often wrong)\n" + "\n".join(user[:250]) + "\n\n## Tool errors (first 40)\n" + "\n".join(errors[:40])
            + "\n\n## Commands run 3+ times\n" + "\n".join(retries) + "\n\n## Last assistant text\n" + clean(last_text, 1500) + "\n")
    row = [sid, kind_of, times[0] if times else "", times[-1] if times else "",
           *map(str, (len(user), len(errors), denials, interrupts, compactions))]
    return body, row


def started(path):
    """The first timestamp of a transcript (ISO, UTC), or "" when it has none."""
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            m = re.search(r'"timestamp":"([^"]+)"', line)
            if m:
                return m.group(1)
    return ""


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__)
    src, out = argv[1], argv[2]
    opt = lambda name, default=None: argv[argv.index(name) + 1] if name in argv else default
    last, since, until = int(opt("--last", 30)), opt("--since", ""), opt("--until", "9999")
    skip = set((opt("--exclude") or "").split(","))
    # the sessions active last (by the file's time); left out: those idle since --since, and
    # those started at or after --until (this retro's own agents)
    paths = sorted((os.path.join(src, n) for n in os.listdir(src) if n.endswith(".jsonl")), key=os.path.getmtime)
    stamp = lambda p: time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(os.path.getmtime(p)))
    files = [p for p in paths if stamp(p) > since and started(p) < until
             and os.path.basename(p)[:-6] not in skip][-last:]
    os.makedirs(out, exist_ok=True)
    rows = []
    for p in files:
        body, row = digest(p)
        with open(os.path.join(out, row[0] + ".md"), "w", encoding="utf-8") as f:
            f.write(body)
        rows.append(row)
    with open(os.path.join(out, "index.tsv"), "w", encoding="utf-8") as f:
        f.write("session\tkind\tstart\tend\tuser_turns\terrors\tdenials\tinterrupts\tcompactions\n")
        f.writelines("\t".join(r) + "\n" for r in rows)
    print(f"{len(rows)} digests in {out}")


if __name__ == "__main__":
    main(sys.argv)
