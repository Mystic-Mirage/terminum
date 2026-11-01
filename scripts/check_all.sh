#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
# The checks of CLAUDE.md §8 on this tree, once: each step's output in .checks/<id>.<step>.log,
# and on a full pass .checks/<id>.ok, where <id> names the content of every tracked and new
# file (not the commit: committing a checked tree keeps its pass). Started again on the same
# tree it says when it passed and runs nothing (--force runs again), so a follow-up never pays
# for a run the tree already had. A tree changed during the run (another session, an edit)
# records no pass. While iterating, run only the changed area's checks (cargo test <module>,
# node web/test/e2e_all.mjs <check…>).
#
#   make check        or   bash scripts/check_all.sh [--force]
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
tree_id() {
  python3 -I - <<'PY'
import hashlib, subprocess
files = subprocess.run(["git", "ls-files", "-co", "--exclude-standard", "-z"], capture_output=True, check=True).stdout.split(b"\0")
h = hashlib.sha256()
for name in sorted(filter(None, files)):
    try:
        h.update(name + b"\0" + open(name, "rb").read() + b"\0")
    except (FileNotFoundError, IsADirectoryError):   # deleted, not yet staged; a submodule
        h.update(name + b"\0-\0")
print(h.hexdigest()[:16])
PY
}
id=$(tree_id) || { echo "check: no tree id (not a git checkout?)" >&2; exit 2; }
mkdir -p .checks
if [ "${1:-}" != --force ] && [ -f ".checks/$id.ok" ]; then
  echo "check: passed already on $id at $(cat ".checks/$id.ok") (--force runs it again)"; exit 0
fi
find .checks -name '*.log' ! -name "$id.*" -delete   # earlier trees' logs; their passes stay
# the commit-time scan (CLAUDE.md §2) only knows other machines' names from the patterns file
[ -n "$(git config core.hooksPath)" ] || echo "WARN no git hooks: make hooks (security_check before each commit)"
[ -n "${TM_PRIVATE_PATTERNS:-}" ] || [ -f "${XDG_CONFIG_HOME:-$HOME/.config}/terminum/private-patterns" ] \
  || echo "WARN no private patterns: ~/.config/terminum/private-patterns (other machines' names)"
failed=()
step() {   # step <name> <command…>: its output to the log, one line here
  local name=$1; shift
  local log=".checks/$id.$name.log" t0=$SECONDS
  if "$@" >"$log" 2>&1; then echo "ok   $name ($((SECONDS - t0)) s)"
  else echo "FAIL $name ($((SECONDS - t0)) s): $log"; tail -n 15 "$log" | sed 's/^/     /'; failed+=("$name"); fi
}
step test make test
step clippy bash -c 'cd terminumd && cargo clippy --locked --all-targets -- -D warnings'
step panel bash -c 'cd ui && node test/e2e_panel.mjs'
step live bash -c 'cd terminumd && cargo build --locked && cd .. && node web/test/e2e_live.mjs'
if [ "$(uname -s)" = Darwin ]; then step page node web/test/e2e_all.mjs
else echo "SKIP page (web/test/e2e_all.mjs measures a Mac's browsers: make test-mac)"; fi
step security env -u TM_PORT -u TM_APP_DIR bash scripts/security_check.sh
if [ ${#failed[@]} -gt 0 ]; then echo "check: FAIL on $id: ${failed[*]}"; exit 1; fi
now=$(tree_id)
if [ "$now" != "$id" ]; then echo "check: the tree changed during the run ($id → $now): no pass recorded"; exit 1; fi
date '+%Y-%m-%d %H:%M' > ".checks/$id.ok"
echo "check: PASS on $id (remote changes also: scripts/e2e_remote.py; install changes: scripts/e2e_install.sh; macOS-only code: make test-mac)"
