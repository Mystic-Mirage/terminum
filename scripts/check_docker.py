#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""The full run's Docker step (spec AC-104): remote hosts over real ssh, Alpine and Debian. On a Mac with
Apple's chips and a working Docker; else it says why it skipped, and .checks/<id>.docker-skip keeps that, so
a later `make check` of the same tree runs this step once Docker works. Only failing tests fail it: a missing
toolchain, image or package is a skip. A container its script left fails it (check_step.py cannot see one).

  check_docker.py <tree id>     run
  check_docker.py --can-run     exit 0 if it could run now"""
import os
from pathlib import Path
import subprocess
import sys

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
import agents  # noqa: E402
from testrun import container_name  # noqa: E402

PLATFORMS = ("macos-aarch64", "linux-aarch64")
RUNS = (("e2e_remote_sshd.py", (), "test-sshd"), ("e2e_remote_sshd_agents.py", ("alpine",), "rr9-alpine"),
        ("e2e_remote_sshd_agents.py", ("debian",), "rr9-debian"))


def why_not():
    """Why this step cannot run on this machine now, or nothing."""
    if agents.this_platform() != "macos-aarch64":
        return "not a Mac with Apple's chips"
    if not (agents.TOOLCHAIN / "bin").is_dir():
        return "no pinned toolchain: make toolchain"
    try:   # a wedged daemon hangs, and a Mac has no `timeout`
        subprocess.run(["docker", "info"], capture_output=True, timeout=10, check=True)
    except (OSError, subprocess.SubprocessError):
        return "Docker does not answer"
    return ""


def container_left(name):
    listed = subprocess.run(["docker", "ps", "-aq", "--filter", f"name=^/{name}$"], capture_output=True, text=True)
    if not listed.stdout.strip():
        return False
    subprocess.run(["docker", "rm", "-f", name], capture_output=True)
    return True


def run_all(mac, linux):
    """(failed, why not all ran)."""
    failed, why = False, ""
    for script, args, kind in RUNS:
        run = subprocess.Popen([sys.executable, f"scripts/{script}", mac, linux, *args], cwd=REPO)
        try:
            status = run.wait()
        except BaseException:
            run.terminate()
            run.wait()
            raise
        print(f"{script} {' '.join(args)}: exit {status}", flush=True)
        name = container_name(kind, run.pid)
        if container_left(name):
            print(f"LEFT container {name}: removed")
            failed = True
        if status == os.EX_TEMPFAIL:
            why = why or f"{script} could not start its host"
        elif status:
            failed = True
    return failed, why


def main(argv):
    if argv == ["--can-run"]:
        return 1 if why_not() else 0
    if len(argv) != 1:
        sys.exit(__doc__.split("\n\n")[1])
    skip = REPO / ".checks" / f"{argv[0]}.docker-skip"
    skip.unlink(missing_ok=True)
    why = why_not()
    failed = False
    if not why:
        subprocess.run(["make", "ui"], cwd=REPO, check=True)   # terminumd embeds it
        # named by the agent id, not this commit's TM_BUILD: a tree that keeps terminumd's sources keeps its builds
        built = agents.build(PLATFORMS, build_id=f"agent-{agents.agent_id()}")
        mac, linux = (str(built[p]) for p in PLATFORMS)
        failed, why = run_all(mac, linux)
    if why:
        print(f"SKIP docker: {why}")
        skip.write_text(why + "\n")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
