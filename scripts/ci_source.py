#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Stage only versioned source and non-ignored source additions for the Mac runner."""
import fnmatch
import os
from pathlib import Path
import shutil
import subprocess
import sys

SOURCE_DIRS = {".github", "docs", "scripts", "skills", "terminumd", "ui", "web"}
SOURCE_SUFFIXES = {".rs", ".ts", ".js", ".mjs", ".py", ".sh", ".css", ".html", ".md",
                   ".toml", ".json", ".yml", ".yaml", ".txt", ".svg", ".png", ".jpg", ".gif", ".plist"}
PRIVATE_DIRS = {"project-memory", ".git", ".codex", ".claude", ".ssh", ".aws", ".azure", ".gnupg", ".config", ".docker", ".kube",
                "node_modules", "target", "dist", ".toolchain", "__pycache__"}
PRIVATE_NAMES = {"local.mk", "claude.local.md", "memory.md", "workspaces.json", ".netrc", ".npmrc",
                 ".pypirc", ".pgpass", "tokens.json", "token.json"}
PRIVATE_PATTERNS = (".env*", "secrets.*", "credentials.*", "*.pem", "*.key", "*.p12", "*.pfx", "*.crt", "*.cer")


def approved(path, tracked):
    """Selection uses names only; excluded local files are never opened."""
    parts = Path(path).parts
    if not parts or Path(path).is_absolute() or ".." in parts:
        return False
    for part in parts:
        name = part.lower()
        if name in PRIVATE_DIRS or name in PRIVATE_NAMES or any(fnmatch.fnmatchcase(name, p) for p in PRIVATE_PATTERNS):
            return False
    return tracked or (parts[0] in SOURCE_DIRS and Path(path).suffix.lower() in SOURCE_SUFFIXES)


def source_files(root):
    files = set()
    for tracked, flags in [(True, ["--cached"]), (False, ["--others", "--exclude-standard"])]:
        output = subprocess.check_output(["git", "-C", str(root), "ls-files", "-z", *flags])
        files.update(path for path in output.decode().split("\0") if approved(path, tracked))
    return sorted(files)


def stage(root, destination):
    root = root.resolve()
    for relative in source_files(root):
        source, target = root / relative, destination / relative
        resolved = source.resolve()
        try:
            resolved_relative = resolved.relative_to(root)
        except ValueError:
            continue
        if not approved(str(resolved_relative), True):
            continue
        if not source.is_symlink() and not source.is_file():
            continue  # deleted source and submodules are not files to transfer
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_symlink():
            link = source.readlink()
            link_path = Path(os.path.normpath(str(Path(relative).parent / link)))
            if link.is_absolute() or not approved(str(link_path), True):
                continue  # never export an external/private link or the path it reveals
            target.symlink_to(link)
        else:
            shutil.copy2(resolved, target)


if __name__ == "__main__":
    stage(Path(sys.argv[1]), Path(sys.argv[2]))
