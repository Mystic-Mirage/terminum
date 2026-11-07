#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""The docs' shape, reported by `make test` and blocking nothing (spec AC-103; what it cannot
judge is /docs-review's): each spec's sections in order and its size; each acceptance case unique across the specs, tagged
alike wherever it stands, with scenarios and a line of proof; every AC-nn cited in docs or code
exists, in the spec named beside it; relative links in docs lead somewhere."""
from pathlib import Path
import re
import subprocess
import sys

REPO = Path(__file__).resolve().parent.parent
SECTIONS = ["Metadata", "Overview", "Scope and non-goals", "Assumptions, dependencies, open questions",
            "Acceptance cases", "BDD scenarios", "Flow and sequence diagrams", "Current behavior and gaps",
            "Recommendation and ownership", "Technical details and specifications", "Delivery stages",
            "Checklist with Definition of Done"]
TAGS = {"[MUST / P0]", "[SHOULD / P1]", "[NICE TO HAVE / later]"}
BUILD_DIRS = {"target", "node_modules", "dist", ".toolchain", ".worktrees", ".checks"}
AC = re.compile(r"\bAC-(\d{2,3})\b")
CITED = re.compile(r"\b(terminum(?:-[a-z]+)?\.md)\b[^\n.;|]{0,12}?\bAC-(\d{2,3})\b")
LINK = re.compile(r"(?<!!)\[[^\]\n]*\]\(([^)\s#]+)(?:#[^)\s]*)?\)")


def sections(text):
    """{number: (title, body)} of the `## N. Title` headings outside code fences."""
    found, number, fenced = {}, None, False
    for line in text.splitlines():
        if line.startswith("```"):
            fenced = not fenced
        heading = None if fenced else re.fullmatch(r"## (\d+)\. (.+)", line)
        if heading:
            number = int(heading[1])
            found[number] = (heading[2], [])
        elif number is not None:
            found[number][1].append(line)
    return {n: (title, "\n".join(body)) for n, (title, body) in found.items()}


def check_spec(name, text):
    """(problems, {ac: tag}, dropped) of one spec."""
    problems = []
    parts = sections(text)
    if [parts.get(i, ("",))[0] for i in range(len(SECTIONS))] != SECTIONS:
        problems.append(f"{name}: sections must be {', '.join(f'{i}. {t}' for i, t in enumerate(SECTIONS))}")
        return problems, {}, set()
    if re.search(r"^\| \d+\.\d+\.\d+ \||^Change log", parts[0][1], re.M):
        problems.append(f"{name}: a change log in section 0: a spec is a snapshot, git holds its history")
    tags, dropped = {}, set()
    for number, rest in re.findall(r"^\| AC-(\d{2,3}) \|(.*)$", parts[4][1], re.M):
        if number in tags or number in dropped:
            problems.append(f"{name}: AC-{number} twice in section 4")
        if "[DROPPED" in rest:
            dropped.add(number)
            continue
        tag = rest.split("|")[0].strip()
        if tag not in TAGS:
            problems.append(f"{name}: AC-{number} has the tag {tag!r}; allowed: {', '.join(sorted(TAGS))}")
        tags[number] = tag
    headings = dict(re.findall(r"^### AC-(\d{2,3}) — .*?(\[[^\]]+\])\s*$", parts[5][1], re.M))
    cases = re.split(r"^### AC-(\d{2,3}) — .*$", parts[5][1], flags=re.M)[1:]
    for number, block in zip(cases[::2], cases[1::2]):
        if "Scenario" not in block:
            problems.append(f"{name}: AC-{number} has no scenario")
    for number, tag in tags.items():
        if number not in headings:
            problems.append(f"{name}: AC-{number} has no scenarios in section 5")
        elif headings[number] != tag:
            problems.append(f"{name}: AC-{number} is {tag} in section 4, {headings[number]} in section 5")
        if not re.search(rf"\bAC-{number}\b", parts[11][1]):
            problems.append(f"{name}: AC-{number} has no line of proof in section 11")
    for number in headings:
        if number not in tags:
            problems.append(f"{name}: AC-{number} has scenarios but no row in section 4")
    return problems, tags, dropped


def tracked(root):
    """The tree's own files; outside git (the Mac test runner gets a copy) all but build output."""
    listed = subprocess.run(["git", "ls-files", "-co", "--exclude-standard"], cwd=root, capture_output=True, text=True)
    if listed.returncode == 0:
        return [root / line for line in listed.stdout.splitlines() if (root / line).is_file()]
    return [path for path in root.rglob("*") if path.is_file() and not BUILD_DIRS & set(path.relative_to(root).parts)]


def check_all(root):
    problems, owner = [], {}
    specs = sorted((root / "docs" / "specs").glob("terminum*.md"))
    if not specs:
        return [f"no specs in {root / 'docs' / 'specs'}"]
    for spec in specs:
        found, tags, dropped = check_spec(spec.name, spec.read_text())
        problems += found
        for number in set(tags) | dropped:
            if number in owner:
                problems.append(f"AC-{number} in both {owner[number]} and {spec.name}")
            owner[number] = spec.name
    for path in tracked(root):
        if path.suffix not in {".md", ".rs", ".py", ".sh", ".mjs", ".js", ".ts"} \
                or path.is_relative_to(root / "docs" / "releases"):
            continue
        text = path.read_text(errors="replace")
        rel = path.relative_to(root)
        for number in sorted(set(AC.findall(text))):
            if number not in owner:
                problems.append(f"{rel}: cites AC-{number}, which no spec has")
        for spec_name, number in CITED.findall(text):
            if owner.get(number, spec_name) != spec_name:
                problems.append(f"{rel}: says {spec_name} AC-{number}; it is in {owner[number]}")
        if path.suffix == ".md":
            for target in LINK.findall(text):
                if not re.match(r"[a-z]+:", target) and not (path.parent / target).exists():
                    problems.append(f"{rel}: the link {target} leads nowhere")
    return problems


def warnings(root):
    found = []
    tests = "\n".join(path.read_text(errors="replace") for path in tracked(root)
                      if re.search(r"(^|/)(test|tests)/|_tests?\.|/e2e_|/bench_", str(path.relative_to(root))) and path.suffix in {".rs", ".py", ".sh", ".mjs"})
    for spec in sorted((root / "docs" / "specs").glob("terminum*.md")):
        parts = sections(spec.read_text())
        staged = AC.findall(parts.get(10, ("", ""))[1])  # not built yet: nothing to test
        for number in re.findall(r"^### AC-(\d{2,3}) — ", parts.get(5, ("", ""))[1], re.M):
            if number not in staged and not re.search(rf"\bAC-{number}\b", tests):
                found.append(f"{spec.name}: no test cites AC-{number}")
    return found


def main():
    problems = check_all(REPO)
    for problem in problems:
        print(f"FAIL {problem}")
    for warning in warnings(REPO):
        print(f"WARN {warning}")
    print(f"docs: {len(problems)} findings")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
