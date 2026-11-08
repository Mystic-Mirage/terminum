#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
# The full run of CLAUDE.md §8 on this tree, once, the release's gate: each step's output in .checks/<id>.<step>.log,
# and on a full pass .checks/<id>.ok, where <id> names the content of every tracked and new
# file (not the commit: committing a checked tree keeps its pass). Started again on the same
# tree it says when it passed and runs nothing (--force runs again), so a follow-up never pays
# for a run the tree already had. A tree changed during the run (another session, an edit)
# records no pass. It starts on a quiet machine (scripts/load_gate.py, AC-146): exit 75 when the
# load stays high for 15 minutes; TM_GATE_PASSED=1 (make release, which waited already) skips that.
# While iterating, run only the changed area's checks (cargo test <module>, node web/test/e2e_all.mjs <check…>).
#
#   make check        or   bash scripts/check_all.sh [--force | --passed]   --passed: 0 if it passed here
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
tree_id() { python3 -I scripts/check_changed.py --tree-id; }   # the targeted checks' id: one rule
id=$(tree_id) || { echo "check: no tree id (not a git checkout?)" >&2; exit 2; }
mkdir -p .checks
failed=()
step() {   # step <name> <command…>: its output to the log, one line here; what it leaves fails it (check_step.py)
  local name=$1; shift
  local log=".checks/$id.$name.log" t0=$SECONDS
  if python3 -I scripts/check_step.py "$@" >"$log" 2>&1; then echo "ok   $name ($((SECONDS - t0)) s)"
  else echo "FAIL $name ($((SECONDS - t0)) s): $log"; tail -n 15 "$log" | sed 's/^/     /'; failed+=("$name"); fi
}
if [ "${1:-}" = --passed ]; then   # as below: a pass without Docker counts while Docker still cannot run
  [ -f ".checks/$id.ok" ] && { [ ! -f ".checks/$id.docker-skip" ] || ! python3 scripts/check_docker.py --can-run; }; exit
fi
if [ "${1:-}" != --force ] && [ -f ".checks/$id.ok" ]; then
  if [ -f ".checks/$id.docker-skip" ] && python3 scripts/check_docker.py --can-run; then   # a pass without Docker, and it works now
    echo "check: passed on $id without Docker, which works now: its step runs"
    step docker python3 scripts/check_docker.py "$id"
    [ ${#failed[@]} -eq 0 ] || { rm -f ".checks/$id.ok"; echo "check: FAIL on $id: docker"; exit 1; }
    exit 0
  fi
  [ ! -f ".checks/$id.docker-skip" ] || echo "SKIP docker: $(cat ".checks/$id.docker-skip")"
  echo "check: passed already on $id at $(cat ".checks/$id.ok") (--force runs it again)"; exit 0
fi
[ -n "${TM_GATE_PASSED:-}" ] || python3 -I scripts/load_gate.py || exit   # 75: the machine stayed busy
find .checks -name '*.log' ! -name "$id.*" -delete   # earlier trees' logs; their passes stay
# the commit-time scan (CLAUDE.md §2) only knows other machines' names from the patterns file
[ -n "$(git config core.hooksPath)" ] || echo "WARN no git hooks: make hooks (security_check before each commit)"
[ -n "${TM_PRIVATE_PATTERNS:-}" ] || [ -f "${XDG_CONFIG_HOME:-$HOME/.config}/terminum/private-patterns" ] \
  || echo "WARN no private patterns: ~/.config/terminum/private-patterns (other machines' names)"
step test make test
step clippy bash -c 'cd terminumd && cargo clippy --locked --all-targets -- -D warnings'
step panel bash -c 'cd ui && node test/e2e_panel.mjs'
step live bash -c 'cd terminumd && cargo build --locked && cd .. && node web/test/e2e_live.mjs'
# every e2e script but Docker's, which take built binaries as arguments: their own step
step e2e bash -c 'rc=0; for f in scripts/e2e_*.py; do case $f in *remote_sshd*) ;; *) python3 "$f" || { echo "FAIL $f"; rc=1; } ;; esac; done; exit $rc'
step docker python3 scripts/check_docker.py "$id"
[ ! -f ".checks/$id.docker-skip" ] || echo "SKIP docker: $(cat ".checks/$id.docker-skip")"
step install env TERMINUMD_BIN=terminumd/target/debug/terminumd bash scripts/e2e_install.sh
if [ "$(uname -s)" = Darwin ]; then step page node web/test/e2e_all.mjs
else echo "SKIP page (web/test/e2e_all.mjs measures a Mac's browsers: make test-mac)"; fi
step security env -u TM_PORT -u TM_APP_DIR bash scripts/security_check.sh
if [ ${#failed[@]} -gt 0 ]; then echo "check: FAIL on $id: ${failed[*]}"; exit 1; fi
now=$(tree_id)
if [ "$now" != "$id" ]; then echo "check: the tree changed during the run ($id → $now): no pass recorded"; exit 1; fi
date '+%Y-%m-%d %H:%M' > ".checks/$id.ok"
echo "check: PASS on $id (remote changes also: scripts/e2e_remote.py; install changes: scripts/e2e_install.sh; macOS-only code: make test-mac)"
