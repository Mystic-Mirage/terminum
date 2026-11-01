#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Build the Mac package (AC-08, AC-86): one folder, terminum/, that installs itself. Nothing
in it needs an interpreter: the Mac's terminumd is the installer, the server and the
`terminum` command (`terminumd install`; install.sh runs it).

    BUILD, AGENT_ID                   which build this is; which terminumd sources
    agents/<platform>/terminumd       terminumd (the page embedded) for macOS arm64/x86_64 and
                                      Linux x86_64/arm64, the Linux pair for remote hosts' files
    release-signers, LICENSE files    the release key upgrades check; the licenses, NOTICE (marks)
    soundfonts/                       the General MIDI SoundFont MIDI files play with (AC-64,
                                      DEP-06) and its license: fetched once into .toolchain/

The Mac's own terminumd is the one of its architecture (chosen at install). Each macOS
terminumd is signed as io.terminum.terminumd (AC-08): with TM_CODESIGN_IDENTITY, the
maintainer's Developer ID (local.mk), else ad hoc, said. `--stage` leaves the folder for
`make install`; without it a tarball and SHA256SUMS are written too, plus install.sh;
`--release` (release.py) also requires TM_CODESIGN_IDENTITY, a secure timestamp and a Team ID. `--bin FILE` packs an already built terminumd for this Mac
only (scripts/e2e_install.sh); `--out DIR` writes elsewhere than dist/package. `id` prints
the build id of this checkout's sources (the Makefile compiles it into terminumd).

    python3 scripts/package.py [--stage | --release] [--build ID] [--bin FILE] [--out DIR]
    python3 scripts/package.py id
"""
import argparse
import hashlib
import os
import shutil
import subprocess
import time
import sys
import tarfile
import urllib.request
from pathlib import Path

import agents

REPO = agents.REPO
OUT = REPO / "dist/package"
NAME = "terminum"
TARBALL = "terminum.tar.gz"
# the same bytes under the name releases before Linux servers had: their installs ask for it
MAC_TARBALL = "terminum-macos.tar.gz"
SIGN_ID = "io.terminum.terminumd"  # macos/Info.plist's CFBundleIdentifier
COPY = ["release-signers", "LICENSE", "LICENSE-COMMERCIAL.md", "NOTICE", "README.md"]
# GeneralUser GS 2.0.3 (S. Christian Collins, License v2.0: free use in software), pinned by its
# commit and checked by SHA-256 (DEP-06): {name in the package: (path in its repository, sha256)}
SOUNDFONT_URL = "https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/97049183643d5fc5a9322a69c5b09efb667c6c3a/"
SOUNDFONT = {
    "GeneralUser-GS.sf2": ("GeneralUser-GS.sf2", "9575028c7a1f589f5770fccc8cff2734566af40cd26ed836944e9a5152688cfe"),
    "GeneralUser-GS-LICENSE.txt": ("documentation/LICENSE.txt", "7b32efefdf95ce38a043799f0659853ddc00fbaa14d8c50f0aca16b9b8b405be"),
}
# every source that goes into a build (its id); build output never counts
SOURCES = ["terminumd/src", "terminumd/assets", "terminumd/Cargo.toml", "terminumd/Cargo.lock",
           "terminumd/build.rs", "terminumd/macos",
           "ui/src", "ui/public", "ui/build.mjs", "ui/package.json", "ui/package-lock.json", "web/static",
           "release-signers"]


def build_id(repo=REPO):
    """`<commit>-<hash of every source that goes into a build>`: a change anywhere gives a
    new id; the same sources give the same id."""
    h = hashlib.sha256()
    for rel in SOURCES:
        root = repo / rel
        if root.is_file():
            files = [root]
        elif root.is_dir():
            files = sorted(p for p in root.rglob("*") if p.is_file() and "__pycache__" not in p.parts)
        else:
            continue
        for f in files:
            h.update(str(f.relative_to(repo)).encode() + b"\0" + f.read_bytes() + b"\0")
    r = subprocess.run(["git", "-C", str(repo), "rev-parse", "--short", "HEAD"], capture_output=True, text=True)
    return f"{r.stdout.strip() or 'nogit'}-{h.hexdigest()[:8]}"


def build_macos(build, platforms):
    """terminumd for these platforms (the Mac's, or a Linux machine's own), compiled with this
    build id (the installer checks it)."""
    env = agents.tool_env()
    env.update(TM_BUILD=build, TM_AGENT_ID=agents.agent_id(), TM_BUILD_DATE=time.strftime("%Y-%m-%d"))
    out = {}
    for platform in platforms:
        target = agents.TARGETS[platform]
        agents.run(["cargo", "build", "--locked", "--release", "--target", target], env=env)
        out[platform] = agents.TERMINUMD / "target" / target / "release/terminumd"
    return out


def binaries(build, given, stage, release=False):
    """{platform: terminumd}: `given` for this Mac only; for `make install` (stage) this
    Mac's own build; for a release both Mac builds; the Linux pair when the toolchain is there
    (a release without it fails: a Linux server installs from the same package)."""
    if given:
        return {agents.this_platform(): Path(given)}
    here = agents.this_platform()
    if here.startswith("linux-") and not stage:
        raise agents.Failed("a release is made on a Mac: its macOS builds are signed there")
    found = build_macos(build, [here] if stage else ["macos-aarch64", "macos-x86_64"])
    try:
        found.update(agents.build(["linux-x86_64", "linux-aarch64"], build))   # a Linux install checks its build
    except agents.Failed as e:  # a developer without the cross toolchain: Mac only, said loudly
        if release:
            raise agents.Failed(f"a release needs the Linux builds: {e}")
        print(f"warning: no Linux helpers in this package ({e}); remote hosts' files cannot be enabled. "
              "Run make toolchain.", file=sys.stderr)
    warned(release)
    return found


def warned(release):
    """The builds' warnings (CLAUDE.md §8: none ships): a release refuses them unless
    TM_ALLOW_WARNINGS=1 (said); an install says them."""
    if not agents.WARNINGS:
        return
    lines = "\n  ".join(dict.fromkeys(agents.WARNINGS))
    if release and os.environ.get("TM_ALLOW_WARNINGS") != "1":
        raise agents.Failed(f"the builds printed {len(agents.WARNINGS)} warning(s); a release ships none "
                            f"(fix them, or TM_ALLOW_WARNINGS=1 to release anyway):\n  {lines}")
    print(f"warning: the builds printed {len(agents.WARNINGS)} warning(s)"
          f"{' (TM_ALLOW_WARNINGS=1: released anyway)' if release else '; a release refuses them'}:\n  {lines}", file=sys.stderr)


def is_macho(path):
    """A Mach-O file (thin or universal); the e2e checks pack shell wrappers, never signed."""
    with open(path, "rb") as f:
        return f.read(4) in (b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe", b"\xca\xfe\xba\xbe")


def signer(release):
    """The identity macOS terminumd is signed with: TM_CODESIGN_IDENTITY, never one picked
    from the keychain by itself (it may be another team's, its key may ask in a dialog)."""
    identity = os.environ.get("TM_CODESIGN_IDENTITY", "").strip()
    if not identity and release:
        raise agents.Failed("TM_CODESIGN_IDENTITY is not set: a release is signed with the maintainer's "
                            "Developer ID (docs/RELEASING.md); not packed")
    if not identity:
        print("warning: TM_CODESIGN_IDENTITY is not set: terminumd is signed ad hoc; macOS asks again "
              "for protected folders after an upgrade", file=sys.stderr)
    return identity or "-"


def codesign(binary, identity, release):
    """Signs a macOS terminumd as SIGN_ID; a release's must carry a Team ID."""
    cmd = ["codesign", "--force", "-s", identity, "-i", SIGN_ID, "--timestamp" if release else "--timestamp=none", str(binary)]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        raise agents.Failed("codesign took over 60 s (a keychain dialog waiting for an answer?): not packed")
    if r.returncode != 0:
        raise agents.Failed(f"codesign {binary.name}: {r.stderr.strip()}: not packed")
    if release:
        info = subprocess.run(["codesign", "-dv", str(binary)], capture_output=True, text=True).stderr
        if f"Identifier={SIGN_ID}\n" not in info or "TeamIdentifier=" not in info or "TeamIdentifier=not set" in info:
            raise agents.Failed(f"{binary} is not signed by a team as {SIGN_ID}: not packed")


def soundfont():
    """The SoundFont's files in .toolchain/soundfonts, fetched once and each checked by its SHA-256."""
    folder = REPO / ".toolchain/soundfonts"
    folder.mkdir(parents=True, exist_ok=True)
    for name, (path, sha) in SOUNDFONT.items():
        f = folder / name
        if not f.is_file() or hashlib.sha256(f.read_bytes()).hexdigest() != sha:
            print(f"fetching {name}", file=sys.stderr)
            data = urllib.request.urlopen(SOUNDFONT_URL + urllib.request.quote(path), timeout=300).read()
            if hashlib.sha256(data).hexdigest() != sha:
                raise SystemExit(f"{name}: its SHA-256 is not the pinned one: not packed")
            f.write_bytes(data)
    return folder


def stage(build, out, given=None, only_stage=False, release=False):
    pkg = out / NAME
    shutil.rmtree(pkg, ignore_errors=True)
    pkg.mkdir(parents=True)
    for rel in COPY:
        shutil.copy2(REPO / rel, pkg / rel)
    identity = None
    for platform, path in binaries(build, given, only_stage, release).items():
        dest = pkg / "agents" / platform / "terminumd"
        dest.parent.mkdir(parents=True)
        shutil.copy2(path, dest)
        os.chmod(dest, 0o755)
        if platform.startswith("macos-") and (release or is_macho(dest)):
            identity = identity or signer(release)
            codesign(dest, identity, release)
    (pkg / "soundfonts").mkdir()
    for name in SOUNDFONT:
        shutil.copy2(soundfont() / name, pkg / "soundfonts" / name)
    (pkg / "BUILD").write_text(build + "\n")
    (pkg / "AGENT_ID").write_text(agents.agent_id() + "\n")
    return pkg


def anonymous(info):
    """No owner names or ids and one time in the tarball: it says nothing of who built it."""
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    info.mtime = 0
    return info


def archive(pkg, out):
    tar = out / TARBALL
    with tarfile.open(tar, "w:gz") as t:
        t.add(pkg, arcname=NAME, filter=anonymous)
    shutil.copy2(tar, out / MAC_TARBALL)
    shutil.copy2(REPO / "scripts/install.sh", out / "install.sh")
    # the signed list names its release, so an older one cannot be served as the latest
    sums = f"# release {(pkg / 'BUILD').read_text().strip()}\n" + "".join(
        f"{hashlib.sha256((out / f).read_bytes()).hexdigest()}  {f}\n" for f in (TARBALL, MAC_TARBALL, "install.sh"))
    (out / "SHA256SUMS").write_text(sums)
    return [tar, out / MAC_TARBALL, out / "install.sh", out / "SHA256SUMS"]


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if argv[:1] == ["id"]:
        return print(build_id())
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--stage", action="store_true", help="only the folder (make install)")
    p.add_argument("--release", action="store_true", help="a release: Developer ID required (release.py)")
    p.add_argument("--build", default=os.environ.get("TM_BUILD"))
    p.add_argument("--bin", help="an already built terminumd for this Mac only")
    p.add_argument("--out", type=Path, default=OUT)
    a = p.parse_args(argv)
    if not a.build:
        sys.exit("no build id: set TM_BUILD or --build")
    if a.bin and not Path(a.bin).is_file():
        sys.exit(f"no terminumd at {a.bin}")
    out = a.out.resolve()
    try:
        pkg = stage(a.build, out, a.bin, a.stage, a.release)
        print(pkg)
        if not a.stage:
            for f in archive(pkg, out):
                print(f)
    except agents.Failed as e:
        sys.exit(str(e))
    return pkg


if __name__ == "__main__":
    main()
