#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
# Removes a finished worktree, from anywhere: what scripts/worktree.py remove refuses, this refuses
# (work still in it, a process in it). Usage: scripts/delete_worktree.sh <slug>
set -eu
exec python3 "$(dirname "$0")/worktree.py" remove "$@"
