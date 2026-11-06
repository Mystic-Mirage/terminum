#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
# The macOS checks on a Mac test runner of the owner's network (TM_MAC_RUNNER: its ssh name,
# in local.mk), over ssh, as the owner's Mac pool runs tests: no GitHub runner on a Mac. The tree
# (no builds, dependencies, local files or state) goes to <runner>:~/ci/terminum; there run
# make test, the launchd test under its own label, the Files panel's and the page's browser
# checks (WebKit on macOS is the iPhone's engine). Nothing is installed there: the runner's own
# terminum, if any, is never touched. `make release` runs it after the full checks.
# Exit: 0 passed; 1 tests failed; 2 they could not run (no answer, a link that dropped, setup); 3 busy.
#
#   make test-mac            or   TM_MAC_RUNNER=mac-runner make test-mac
set -euo pipefail
host=${TM_MAC_RUNNER:?"set TM_MAC_RUNNER to the ssh name of the Mac test runner (local.mk: TM_MAC_RUNNER = name)"}
repo=$(cd "$(dirname "$0")/.." && pwd)
# one run at a time on the runner: the lock (beside the tree) is taken before the tree is
# replaced, so a second run never changes the files of one under way
rc=0
ssh -o BatchMode=yes -o ConnectTimeout=10 "$host" 'mkdir -p ci/terminum && { mkdir ci/terminum.lock 2>/dev/null || exit 3; }' || rc=$?
case $rc in
    0) ;;
    3) echo "ci_mac: another run uses $host:~/ci/terminum (or a stale ~/ci/terminum.lock: remove it)" >&2; exit 3 ;;
    255) echo "ci_mac: $host does not answer over ssh (asleep, off, or another network)" >&2; exit 2 ;;
    *) echo "ci_mac: $host:~/ci/terminum could not be made" >&2; exit 2 ;;
esac
# until the run starts the lock is this script's to give back; then the run's (it may outlive an interrupted ssh)
running=0
stage=""
trap '[ -z "$stage" ] || rm -rf "$stage"; [ $running = 1 ] || ssh -o BatchMode=yes -o ConnectTimeout=10 "$host" "rmdir ci/terminum.lock" \
    || echo "ci_mac: $host:~/ci/terminum.lock was left: remove it" >&2' EXIT
stage=$(mktemp -d /tmp/tm-ci-src.XXXXXX)
python3 "$repo/scripts/ci_source.py" "$repo" "$stage" || exit 2
# Preserve the runner's existing caches and local configuration; selection of outbound files
# has already happened in the source staging step above.
rsync -a --delete \
    --exclude .git --exclude target --exclude node_modules --exclude ui/dist --exclude dist \
    --exclude .toolchain --exclude local.mk --exclude CLAUDE.local.md --exclude MEMORY.md --exclude project-memory \
    --exclude '.env*' --exclude workspaces.json --exclude .claude --exclude .codex \
    "$stage/" "$host:ci/terminum/" || exit 2
running=1
# a login shell for Homebrew's PATH; no tmux there would skip every check of a real tmux
rc=0
ssh -o BatchMode=yes -o ServerAliveInterval=30 -o ServerAliveCountMax=4 "$host" 'bash -lc '"'"'
    set -euo pipefail
    trap "rmdir ~/ci/terminum.lock 2>/dev/null || true" EXIT
    cd ~/ci/terminum
    export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH" NO_COLOR=1
    command -v tmux >/dev/null || { echo "ci_mac: no tmux on the runner: its tmux checks would be skipped (brew install tmux)" >&2; exit 4; }
    # the Rust of the Linux CI, for this run only: the default toolchain of the runner stays as it is
    export RUSTUP_TOOLCHAIN=1.98.1
    rustup toolchain install "$RUSTUP_TOOLCHAIN" --profile minimal >/dev/null || exit 4
    echo "== $(hostname) $(sw_vers -productVersion), load $(sysctl -n vm.loadavg)"
    make test
    (cd terminumd && TM_LAUNCHD_TEST=1 cargo test --locked --test launchd)
    (cd ui && npx playwright-core install chromium webkit >/dev/null) || exit 4
    (cd ui && node test/e2e_panel.mjs)
    (cd terminumd && cargo build --locked)
    node web/test/e2e_all.mjs
    node web/test/e2e_live.mjs
'"'" || rc=$?
# the run ended (ssh says how): its lock goes, whatever its own trap did; a link that dropped
# leaves it (the run may go on there)
if [ "$rc" = 255 ]; then
    echo "ci_mac: ssh to $host failed: if no run is under way there, remove ~/ci/terminum.lock" >&2
else
    ssh -o BatchMode=yes -o ConnectTimeout=10 "$host" 'rmdir ci/terminum.lock 2>/dev/null || true' \
        || echo "ci_mac: $host:~/ci/terminum.lock was left: remove it" >&2
fi
case $rc in
    0) exit 0 ;;
    255|4) echo "ci_mac: the checks could not run on $host" >&2; exit 2 ;;
    *) echo "ci_mac: tests failed on $host" >&2; exit 1 ;;
esac
