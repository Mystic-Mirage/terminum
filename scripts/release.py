#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Publish a release (AC-86) from the maintainer's Mac, the one machine that builds terminumd
for macOS and Linux and holds the release key; only on the maintainer's request (CLAUDE.md
§10). In this order, so that a failure leaves nothing public:

  1. checks: release notes in docs/releases/<tag>.md; a clean tree; a new version is above every v* tag and made at origin/main's
     HEAD, an existing tag must be HEAD; its release must not be published yet
  2. build with dependencies exactly as locked (npm ci, cargo --locked); each macOS terminumd
     signed with the maintainer's Developer ID (TM_CODESIGN_IDENTITY, local.mk) as
     io.terminum.terminumd, so macOS keeps folder grants across upgrades (AC-08)
  3. sign SHA256SUMS through ssh-agent with the maintainer's key (TM_RELEASE_KEY, see
     signing_key.py), loaded for two minutes with the passphrase the login Keychain keeps
     (no prompt) and removed from the agent afterwards; check it against release-signers
  4. tag HEAD and push the tag; a draft release gets the package, install.sh, SHA256SUMS
     and SHA256SUMS.sig, and is published only then

    python3 scripts/release.py v0.15.0
"""
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

import package
import signing_key

# the maintainer's release key as installs carry it; `make signing-key` writes it
SIGNERS = package.REPO / "release-signers"
NAMESPACE = "terminum-release"


def version(text):
    """(0, 15, 0) for "v0.15.0"; None for anything else."""
    m = re.fullmatch(r"v(\d+(?:\.\d+)*)", (text or "").strip())
    return tuple(int(x) for x in m.group(1).split(".")) if m else None


def verify(sums, sig):
    """True if `sums` is signed by the key in release-signers, checked as installs check it."""
    with open(sums, "rb") as source:
        r = subprocess.run(["ssh-keygen", "-Y", "verify", "-f", str(SIGNERS), "-I", NAMESPACE, "-n", NAMESPACE, "-s", str(sig)],
                           stdin=source, capture_output=True)
    return r.returncode == 0


def run(*cmd, cwd=None, env=None):
    r = subprocess.run(cmd, cwd=cwd or package.REPO, env=env, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"{' '.join(cmd[:3])}: {(r.stderr or r.stdout).strip()}")
    return r.stdout.strip()


def gh(*args, check=True):
    r = subprocess.run(["gh", *args], cwd=package.REPO, capture_output=True, text=True)
    if check and r.returncode != 0:
        sys.exit(f"gh {' '.join(args[:2])}: {r.stderr.strip()}")
    return r


def tag_exists(tag):
    return subprocess.run(["git", "rev-parse", "-q", "--verify", f"refs/tags/{tag}"], cwd=package.REPO,
                          capture_output=True).returncode == 0


def check_source(tag):
    """What users run is exactly the public tag: a clean tree at its commit, on main; a new
    version only above every earlier one, from main as it is now. Returns that commit."""
    run("git", "fetch", "--quiet", "--tags", "origin", "main")
    if run("git", "status", "--porcelain"):
        sys.exit("the tree has changes: commit or stash them (a release is built from the tag only)")
    head = run("git", "rev-parse", "HEAD")
    if tag_exists(tag):
        if run("git", "rev-parse", f"{tag}^{{commit}}") != head:
            sys.exit(f"{tag} exists and is not HEAD: check it out, or release a new version")
        if subprocess.run(["git", "merge-base", "--is-ancestor", "HEAD", "origin/main"], cwd=package.REPO).returncode != 0:
            sys.exit(f"{tag} is not on origin/main")
        return head
    if head != run("git", "rev-parse", "origin/main"):
        sys.exit("HEAD is not origin/main: push main (or pull) first")
    newest = max((version(t) for t in run("git", "tag", "-l", "v*").split() if version(t)), default=None)
    if newest and version(tag) <= newest:
        sys.exit(f"{tag} is not above the newest release tag v{'.'.join(map(str, newest))}")
    return head


def check_unmoved(head):
    """The tag goes on HEAD and must be what was built: another session's `make install` fast-forwards
    a clean main (scripts/worktree.py freshen)."""
    if run("git", "rev-parse", "HEAD") != head:
        sys.exit("HEAD moved during the build (another session's make install?): nothing is public; release again")


def check_unpublished(tag):
    """A published release is never changed: its files are what users checked. True if a
    draft left by an earlier run exists (it is completed)."""
    r = gh("release", "view", tag, "--json", "isDraft", "--jq", ".isDraft", check=False)
    if r.returncode == 0 and r.stdout.strip() != "true":
        sys.exit(f"{tag} is already published: release a new version")
    return r.returncode == 0


def sign(sums):
    """Sign SHA256SUMS through ssh-agent, then check the signature as installs will. The key
    is in the agent only for this, at most two minutes."""
    if not signing_key.KEY.is_file():
        sys.exit(f"no release key at {signing_key.KEY}: run make signing-key once")
    pub = signing_key.KEY.with_suffix(".pub")
    sums.with_name("SHA256SUMS.sig").unlink(missing_ok=True)
    # no terminal and no askpass: it fails rather than asks when the Keychain has no passphrase
    r = subprocess.run(["ssh-add", "-t", "120", "--apple-use-keychain", str(signing_key.KEY)], stdin=subprocess.DEVNULL,
                       capture_output=True, text=True, start_new_session=True, timeout=30,
                       env={**os.environ, "SSH_ASKPASS_REQUIRE": "never"})
    if r.returncode != 0:
        sys.exit(f"could not load the release key into ssh-agent ({r.stderr.strip()}); once, in a terminal:\n"
                 f"  ssh-add --apple-use-keychain {signing_key.KEY} && ssh-add -d {pub}")
    try:
        run("ssh-keygen", "-Y", "sign", "-q", "-f", str(pub), "-n", NAMESPACE, str(sums))
    finally:
        subprocess.run(["ssh-add", "-d", str(pub)], capture_output=True)
    if not verify(sums, sums.with_name("SHA256SUMS.sig")):
        sys.exit("the signature does not match release-signers: commit the key's public half (make signing-key)")


def release_notes(tag):
    """The release's notes, read before anything is built; refused when missing, empty,
    unreadable or a link out of the tree."""
    path = package.REPO / "docs/releases" / f"{tag}.md"
    if path.is_symlink():
        sys.exit(f"cannot read release notes {path.relative_to(package.REPO)}: symlink notes are not allowed")
    try:
        path.resolve().relative_to(package.REPO.resolve())
    except (OSError, RuntimeError, ValueError):
        sys.exit(f"cannot read release notes {path.relative_to(package.REPO)}: path must stay inside the repository")
    try:
        notes = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        sys.exit(f"no release notes: write {path.relative_to(package.REPO)} first")
    except (OSError, UnicodeError) as e:
        sys.exit(f"cannot read release notes {path.relative_to(package.REPO)}: {e}")
    if not notes.strip():
        sys.exit(f"release notes {path.relative_to(package.REPO)} are empty")
    return notes


def publish(tag, draft, notes, head="HEAD"):
    """Tag `head`, push the tag (again, if an earlier run stopped after tagging), fill a draft
    release and publish it."""
    footer = ("Install or upgrade: curl -fsSL https://github.com/extractumio/terminum/releases/latest/download/install.sh | sh\n\n"
              f"SHA256SUMS is signed with the release key {signing_key.fingerprint()}.")
    body = (notes.rstrip() + "\n\n" if notes.strip() else "") + footer + "\n"
    with tempfile.TemporaryDirectory(prefix="tm-release-") as folder:
        path = Path(folder) / "notes.md"
        path.write_text(body, encoding="utf-8")
        if not tag_exists(tag):
            run("git", "tag", "-a", tag, "-m", tag, head)
        run("git", "push", "--quiet", "origin", f"refs/tags/{tag}")
        if not draft:
            gh("release", "create", tag, "--verify-tag", "--draft", "--title", tag, "--notes-file", str(path))
        files = [str(package.OUT / f) for f in (package.TARBALL, package.MAC_TARBALL, "install.sh", "SHA256SUMS", "SHA256SUMS.sig")]
        gh("release", "upload", tag, *files, "--clobber")  # into the draft only
        gh("release", "edit", tag, "--notes-file", str(path), "--draft=false", "--latest")
        return gh("release", "view", tag, "--json", "url", "--jq", ".url").stdout.strip()


def main(argv):
    if len(argv) != 1 or not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", argv[0]):
        sys.exit("usage: release.py vX.Y.Z")
    tag = argv[0]
    if "ssh-" not in (SIGNERS.read_text() if SIGNERS.is_file() else ""):
        sys.exit("release-signers holds no release key: run make signing-key and commit it first")
    if not os.environ.get("TM_CODESIGN_IDENTITY", "").strip():
        sys.exit("TM_CODESIGN_IDENTITY is not set: a release is signed with the maintainer's Developer ID (docs/RELEASING.md)")
    head = check_source(tag)
    notes = release_notes(tag)
    draft = check_unpublished(tag)
    run("npm", "ci", "--ignore-scripts", "--no-fund", "--no-audit", cwd=package.REPO / "ui")
    run("npm", "run", "-s", "build", cwd=package.REPO / "ui", env=dict(os.environ, TM_BUILD=tag))
    package.main(["--build", tag, "--release"])
    for platform in ("linux-x86_64", "linux-aarch64"):
        if not (package.OUT / package.NAME / "agents" / platform / "terminumd").is_file():
            sys.exit(f"the package has no {platform} terminumd (run make toolchain): not released")
    sign(package.OUT / "SHA256SUMS")
    check_unmoved(head)
    print(f"{tag}: {publish(tag, draft, notes, head)}")  # only now does anything become public


if __name__ == "__main__":
    main(sys.argv[1:])
