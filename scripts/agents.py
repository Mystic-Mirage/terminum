#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Build terminumd for remote hosts (AC-86): one script on the Mac and on the runner. On a host
terminumd runs as the files agent (`terminumd --agent`).

Apple targets build with cargo; Linux targets build static (musl) with cargo-zigbuild and
zig from the `ziglang` wheel, both pinned and kept in ./.toolchain. Output goes to
dist/agents/<agent id>/<platform>/terminumd and is reused while the agent id (a hash of
terminumd's sources and manifests) and the build it names (`--version`, its BUILD file beside
it) are unchanged.

    python3 scripts/agents.py id                       print the agent id
    python3 scripts/agents.py toolchain                install the pinned toolchain (once)
    python3 scripts/agents.py build [PLATFORM ...]     build (default: all four)
"""
import argparse
import hashlib
import os
import platform as host_platform
import re
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TERMINUMD = REPO / "terminumd"
# the runner keeps it in its home (TM_TOOLCHAIN): checkout cleans the repository every run
TOOLCHAIN = Path(os.environ.get("TM_TOOLCHAIN") or REPO / ".toolchain")
DIST = REPO / "dist/agents"
ZIG = "0.15.2"            # the ziglang wheel (zig as linker for the Linux targets)
ZIGBUILD = "0.24.0"       # cargo-zigbuild (0.24 drops rustc's -Wl,-O1, which zig 0.12+ warns about)
TARGETS = {"macos-aarch64": "aarch64-apple-darwin", "macos-x86_64": "x86_64-apple-darwin",
           "linux-x86_64": "x86_64-unknown-linux-musl", "linux-aarch64": "aarch64-unknown-linux-musl"}


class Failed(Exception):
    pass


def agent_id(repo=REPO):
    """12 hex of sha256 over terminumd's sources and manifests: equal ids speak the same API."""
    h = hashlib.sha256()
    root = repo / "terminumd"
    for f in sorted([*(root / "src").rglob("*.rs"), root / "Cargo.toml", root / "Cargo.lock"]):
        if f.is_file():
            h.update(str(f.relative_to(repo)).encode() + b"\0" + f.read_bytes() + b"\0")
    return h.hexdigest()[:12]


# the compiler's and linker's `warning:` lines of the builds run here (package.py: a release
# refuses them, an install says them); shown as they come. Not cargo install's (the toolchain's
# own notes), nor cargo's count line ("warning: `terminumd` (bin) generated 2 warnings").
WARNINGS = []
COUNT = re.compile(r"warning: .+ generated \d+ warnings?\b")
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def run(cmd, env=None, cwd=TERMINUMD):
    print("+", " ".join(map(str, cmd)), flush=True)
    build = list(map(str, cmd[:2])) in (["cargo", "build"], ["cargo", "zigbuild"])
    if build and sys.stdout.isatty():   # read through a pipe, cargo would drop its colors
        env = {**(env or os.environ), "CARGO_TERM_COLOR": "always"}
    p = subprocess.Popen(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, errors="replace")
    for line in p.stdout:
        print(line, end="", flush=True)
        plain = ANSI.sub("", line).strip()
        if build and plain.startswith("warning:") and not COUNT.match(plain):
            WARNINGS.append(plain)
    if p.wait() != 0:
        raise Failed(f"{cmd[0]} exited {p.returncode}")


def tool_env():
    """PATH with the pinned toolchain first (cargo-zigbuild finds zig through `python3 -m ziglang`)."""
    env = dict(os.environ)
    env["PATH"] = os.pathsep.join([str(TOOLCHAIN / "bin"), str(TOOLCHAIN / "venv/bin"), env.get("PATH", "")])
    return env


def toolchain(platforms):
    """Rust targets, then zig and cargo-zigbuild for the Linux targets, pinned, in ./.toolchain."""
    targets = [TARGETS[p] for p in platforms]
    if shutil.which("rustup"):
        run(["rustup", "target", "add", *targets])
    if any(p.startswith("linux-") for p in platforms):
        if not (TOOLCHAIN / "venv/bin/python3").exists():
            run([sys.executable, "-m", "venv", str(TOOLCHAIN / "venv")], cwd=REPO)
        run([str(TOOLCHAIN / "venv/bin/python3"), "-m", "pip", "install", "--quiet", f"ziglang=={ZIG}"], cwd=REPO)
        if zigbuild_version() != ZIGBUILD:  # missing, or an older pin kept in the toolchain's folder
            run(["cargo", "install", "--locked", "--force", "cargo-zigbuild", "--version", ZIGBUILD, "--root", str(TOOLCHAIN)], cwd=REPO)


def zigbuild_version():
    """The version of the toolchain's cargo-zigbuild, or None when there is none."""
    exe = TOOLCHAIN / "bin/cargo-zigbuild"
    if not exe.exists():
        return None
    r = subprocess.run([str(exe), "--version"], capture_output=True, text=True)
    return r.stdout.split()[-1] if r.returncode == 0 and r.stdout.split() else None


def mac_platform():
    """This Mac's own architecture: under Rosetta, Python would say x86_64 on Apple Silicon."""
    r = subprocess.run(["sysctl", "-n", "hw.optional.arm64"], capture_output=True, text=True)
    return "macos-aarch64" if r.stdout.strip() == "1" or host_platform.machine() == "arm64" else "macos-x86_64"


def this_platform():
    """The platform of the terminumd this machine runs: a Mac's own architecture, or Linux's."""
    return mac_platform() if host_platform.system() == "Darwin" else native()


def native():
    """The platform this machine builds without cross-compiling."""
    system = {"Darwin": "macos", "Linux": "linux"}.get(host_platform.system(), "?")
    machine = {"arm64": "aarch64", "aarch64": "aarch64", "x86_64": "x86_64", "AMD64": "x86_64"}.get(host_platform.machine(), "?")
    return f"{system}-{machine}"


def build(platforms, build_id=None):
    """Build each platform's agent once per agent id and build; returns {platform: path}.
    `build_id`: the build it names (a package's, which its installer checks against BUILD);
    else TM_BUILD, else agent-<id>."""
    aid = agent_id()
    env = tool_env()
    bid = build_id or env.get("TM_BUILD") or f"agent-{aid}"
    env.update(TM_AGENT_ID=aid, TM_BUILD=bid)
    out = {}
    for p in platforms:
        if p not in TARGETS:
            raise Failed(f"unknown platform {p} (one of {', '.join(TARGETS)})")
        dest = DIST / aid / p / "terminumd"
        stamp = dest.with_name("BUILD")  # the build the cached one names: another is built anew
        if not dest.is_file() or not stamp.is_file() or stamp.read_text().strip() != bid:
            target = TARGETS[p]
            if p.startswith("linux-"):  # the same way on the Mac and on the Linux runner
                if zigbuild_version() != ZIGBUILD:
                    raise Failed(f"the Linux toolchain is missing or not cargo-zigbuild {ZIGBUILD}: run make toolchain")
                run(["cargo", "zigbuild", "--locked", "--release", "--target", target], env=env)
            elif native().startswith("macos-"):
                run(["cargo", "build", "--locked", "--release", "--target", target], env=env)
            else:
                raise Failed(f"{p} builds only on macOS")
            dest.parent.mkdir(parents=True, exist_ok=True)
            tmp = dest.with_name(".terminumd.new")  # checked before it is cached under its id
            shutil.copy2(TERMINUMD / "target" / target / "release/terminumd", tmp)
            if p == native():  # asked (no TM_BUILD_ID: it would answer for it), else found in its bytes
                own = {k: v for k, v in os.environ.items() if k != "TM_BUILD_ID"}
                said = lambda flag: subprocess.run([str(tmp), flag], capture_output=True, text=True, env=own).stdout.strip()
                got = (said("--agent-id"), said("--version"))
            else:
                data = tmp.read_bytes()
                got = tuple(x if x.encode() in data else "none" for x in (aid, bid))
            if got != (aid, bid):
                tmp.unlink()
                raise Failed(f"{target} terminumd says agent id {got[0]}, build {got[1]}; expected {aid}, {bid}")
            tmp.replace(dest)
            stamp.write_text(bid + "\n")
        out[p] = dest
    return out



def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("id")
    sub.add_parser("platform", help="this machine's platform, as a package names its terminumd")
    t = sub.add_parser("toolchain")
    t.add_argument("platforms", nargs="*")
    b = sub.add_parser("build")
    b.add_argument("platforms", nargs="*")
    a = p.parse_args(argv)
    try:
        if a.cmd == "id":
            return print(agent_id())
        if a.cmd == "platform":
            return print(this_platform())
        if a.cmd == "toolchain":
            return toolchain(a.platforms or list(TARGETS))
        for plat, path in build(a.platforms or list(TARGETS)).items():
            print(f"{plat}: {path.relative_to(REPO)}")
    except Failed as e:
        sys.exit(str(e))


if __name__ == "__main__":
    main()
