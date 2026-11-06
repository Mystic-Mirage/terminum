#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
# e2e_install.sh — AC-08, AC-16 end to end, in a temporary HOME: terminumd runs as a plain
# process (TM_NO_LAUNCHD=1) on a free port with a private tmux socket, so the live install,
# launchd and the user's tmux are never touched. Packages are staged with scripts/package.py
# from an already built terminumd (TERMINUMD_BIN, default terminumd/target/release/terminumd):
# builds e2e-a and e2e-b wrap it with TM_BUILD_ID, e2e-c is a build that never comes up. Each
# package is installed the way install.sh does it: its terminumd run with `install` (for
# e2e-c, the real terminumd run under that name: `exec -a` keeps the package's path).
# Checks: install (and the tmux hint without tmux on PATH), status, open, upgrade A → B,
# rollback, a broken build rolled back, the port in use, uninstall keeping state and logs.
# Exits 0 when every check passed.
set -uo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd -P)
real=${TERMINUMD_BIN:-$repo/terminumd/target/release/terminumd}
[ -x "$real" ] || { echo "no terminumd at $real: run make terminumd (or set TERMINUMD_BIN)" >&2; exit 1; }
real=$(cd "$(dirname "$real")" && pwd -P)/$(basename "$real")
tmp_base=${TMPDIR:-/tmp}; tmp_base=${tmp_base%/}; [ ${#tmp_base} -le 40 ] || tmp_base=/tmp   # short: a socket path holds at most 104 bytes (testrun.py)
tmp=$(mktemp -d "$tmp_base/tme-XXXXXX")
tsock=tm-e2e-install-$$
port=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
export HOME=$tmp/home TM_NO_LAUNCHD=1 TM_NO_OPEN=1 TM_PORT=$port TM_TMUX_SOCKET=$tsock NO_COLOR=1
unset TM_HOME TM_APP_DIR TM_LAUNCH_AGENTS TM_BUILD_ID TMUX
mkdir -p "$HOME"
root=$HOME/.terminum
sock=$root/state/terminumd.sock
holder=""
fail=0

cleanup() {
  [ -n "$holder" ] && kill "$holder" 2>/dev/null
  [ -x "$root/bin/terminum" ] && "$root/bin/terminum" uninstall >/dev/null 2>&1
  { read -r pid _ < "$root/state/terminumd.pid"; } 2>/dev/null && kill "$pid" 2>/dev/null
  tmux -L "$tsock" kill-server 2>/dev/null
  rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$tsock"
  rm -rf "$tmp"
}
trap cleanup EXIT
trap 'exit 130' INT; trap 'exit 143' TERM; trap 'exit 129' HUP   # a signal skips the EXIT trap otherwise

check() { # name, then a command that succeeds when the check passes
  local name=$1; shift
  if "$@"; then echo "ok   $name"; else echo "FAIL $name"; fail=1; fi
}
has() { grep -qF -- "$2" <<<"$1"; }
health_build() {
  curl -s -m 2 --unix-socket "$sock" http://localhost/health \
    | python3 -c 'import json,sys; print(json.load(sys.stdin).get("build", ""))' 2>/dev/null
}
is_build() { [ "$(health_build)" = "$1" ]; }
down() { [ -z "$(health_build)" ]; }
link() { [ "$(readlink "$root/builds/$1")" = "$2" ]; }
mode() { [ "$(stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1")" = "$2" ]; }   # GNU stat, else BSD (GNU -f is the file system)
# AC-08: launchd's path holds a copy of build $1's terminumd, its files linked beside it
daemon_is() { [ -f "$root/daemon/terminumd" ] && [ ! -L "$root/daemon/terminumd" ] && cmp -s "$root/daemon/terminumd" "$root/builds/$1/terminumd" \
  && [ "$(readlink "$root/daemon/agents")" = "../builds/$1/agents" ] && [ "$(readlink "$root/daemon/soundfonts")" = "../builds/$1/soundfonts" ]; }

plat=$(python3 "$repo/scripts/agents.py" platform)
install_pkg() { # build-id: its package's terminumd runs `install`, as install.sh runs it
  bash -c 'exec -a "$0" "$1" install' "$tmp/pkg-$1/terminum/agents/$plat/terminumd" "$real"
}

stage() { # build-id, terminumd file
  python3 "$repo/scripts/package.py" --stage --build "$1" --bin "$2" --out "$tmp/pkg-$1" >/dev/null \
    || { echo "FAIL staging $1"; exit 1; }
}
for b in e2e-a e2e-b; do
  printf '#!/bin/bash\nTM_BUILD_ID=%s exec -a "$0" "%s" "$@"\n' "$b" "$real" > "$tmp/$b"
  chmod 755 "$tmp/$b"
  stage "$b" "$tmp/$b"
done
printf '#!/bin/sh\n[ "$1" = --version ] && { echo e2e-c; exit 0; }\necho "error: a broken build" >&2\nexit 1\n' > "$tmp/e2e-c"
chmod 755 "$tmp/e2e-c"
stage e2e-c "$tmp/e2e-c"
check "the package's terminumd says its build" [ "$("$tmp/pkg-e2e-a/terminum/agents/$plat/terminumd" --version)" = e2e-a ]
check "the package holds no script" [ -z "$(find "$tmp/pkg-e2e-a/terminum" -name '*.py' -o -name terminum -type f)" ]
check "the package holds the General MIDI SoundFont and its license (AC-64)" [ -s "$tmp/pkg-e2e-a/terminum/soundfonts/GeneralUser-GS.sf2" -a -s "$tmp/pkg-e2e-a/terminum/soundfonts/GeneralUser-GS-LICENSE.txt" ]

# AC-08: install, with tmux not on PATH
# a folder holding tmux (on Linux /usr/bin, with bash) is replaced by links to all of it but tmux
notmux="" shadow=$tmp/notmux
mkdir -p "$shadow"
IFS=: read -r -a dirs <<<"$PATH"
for d in "${dirs[@]}"; do
  if [ -x "$d/tmux" ]; then
    for f in "$d"/*; do [ "${f##*/}" = tmux ] || ln -sf "$f" "$shadow/"; done
    d=$shadow
  fi
  notmux=${notmux:+$notmux:}$d
done
out=$(PATH=$notmux install_pkg e2e-a 2>&1); rc=$?
echo "$out" | sed 's/^/     | /'
check "install exits 0" [ $rc = 0 ]
check "install says to set the password first" has "$out" "Installed e2e-a. Set the password every browser signs in with: terminum password; then open http://127.0.0.1:$port/"
check "install says how other devices reach it" has "$out" "tailscale serve --bg --https=443 http://127.0.0.1:$port (it takes this machine's name"
check "install says tmux is needed" has "$out" "tmux is needed for terminals: "
check "terminumd reports e2e-a on the socket" is_build e2e-a
check "current → e2e-a" link current e2e-a
check "terminumd runs from daemon/, a copy of e2e-a's" daemon_is e2e-a
check "the installed build has the SoundFont beside its terminumd" [ -s "$root/builds/e2e-a/soundfonts/GeneralUser-GS.sf2" ]
check "~/.terminum is 0700" mode "$root" 700
check "the socket is 0600" mode "$sock" 600
check "the log is 0600" mode "$root/logs/terminumd.log" 600
check "the command is the build's terminumd" [ "$(readlink "$root/bin/terminum")" = "$root/builds/e2e-a/terminumd" ]

out=$("$root/bin/terminum" status 2>&1)
check "status names the build and port" has "$out" "running, build e2e-a"
check "status names the page" has "$out" "http://127.0.0.1:$port/"
check "status says no password is set" has "$out" "password:   not set"
out=$("$root/bin/terminum" open --print 2>&1)
check "open without a password says how to set one" has "$out" "run terminum password, then terminum open"
out=$(TM_WEB_PASSWORD='install check password' "$root/bin/terminum" password 2>&1)
check "password sets it" has "$out" "Password set"
out=$("$root/bin/terminum" open --print 2>&1)
check "open --print gives the page" has "$out" "http://127.0.0.1:$port/ (sign in with your password)"

# AC-16: upgrade, rollback, a broken build
out=$(install_pkg e2e-b 2>&1); rc=$?
check "upgrade A → B" [ $rc = 0 ]
check "upgrade says so" has "$out" "Upgraded e2e-a → e2e-b"
check "terminumd reports e2e-b" is_build e2e-b
check "current → e2e-b, previous → e2e-a" eval 'link current e2e-b && link previous e2e-a'
check "daemon/ is e2e-b's now, at the same path" daemon_is e2e-b
out=$("$root/bin/terminum" rollback 2>&1); rc=$?
check "rollback" [ $rc = 0 ]
check "terminumd reports e2e-a again" is_build e2e-a
check "current → e2e-a, previous → e2e-b" eval 'link current e2e-a && link previous e2e-b'
check "daemon/ is e2e-a's after the rollback" daemon_is e2e-a
check "the command stays the newest build's" [ "$(readlink "$root/bin/terminum")" = "$root/builds/e2e-b/terminumd" ]
out=$("$root/bin/terminum" restart 2>&1); rc=$?
check "restart" [ $rc = 0 ]
check "restart names the build and port" has "$out" "Restarted terminumd: build e2e-a, port $port"
out=$(install_pkg e2e-c 2>&1); rc=$?
echo "$out" | sed 's/^/     | /'
check "a broken build fails the upgrade" [ $rc != 0 ]
check "and says it rolled back" has "$out" "; rolled back to e2e-a — see ~/.terminum/logs/"
check "the failure names the build" has "$out" "Upgrade to e2e-c failed ("
check "terminumd runs e2e-a" is_build e2e-a
check "current → e2e-a, previous → e2e-b after the failed upgrade" eval 'link current e2e-a && link previous e2e-b'
check "daemon/ is e2e-a's after the failed upgrade" daemon_is e2e-a

# AC-08 edge: the port is in use
read -r pid _ < "$root/state/terminumd.pid"
kill "$pid"; for _ in $(seq 50); do down && break; sleep 0.1; done
python3 -c 'import socket, sys, time
s = socket.socket(); s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen(); time.sleep(120)' "$port" &
holder=$!
sleep 0.3
out=$(install_pkg e2e-a 2>&1); rc=$?
check "install with the port taken fails" [ $rc != 0 ]
check "and names the port" has "$out" "port $port is in use"
out=$("$root/bin/terminum" status 2>&1)
check "status says terminumd is down and why" has "$out" "not running — terminumd does not answer: port $port is in use"
kill "$holder"; wait "$holder" 2>/dev/null; holder=""
out=$(install_pkg e2e-a 2>&1); rc=$?
check "install once the port is free" [ $rc = 0 ]
check "terminumd reports e2e-a once the port is free" is_build e2e-a

# AC-16: uninstall keeps state and logs, tmux untouched
tmux -L "$tsock" ls >/dev/null 2>&1; tmux_before=$?
echo '{}' > "$root/state/keep.json"
out=$("$root/bin/terminum" uninstall 2>&1); rc=$?
check "uninstall" [ $rc = 0 ]
check "terminumd stopped" down
check "builds removed" [ ! -e "$root/builds" ]
check "daemon/ removed" [ ! -e "$root/daemon" ]
check "command removed" [ ! -e "$root/bin/terminum" ]
check "state kept" [ -f "$root/state/keep.json" ]
check "logs kept" [ -f "$root/logs/terminumd.log" ]
tmux -L "$tsock" ls >/dev/null 2>&1; tmux_after=$?
check "tmux untouched" [ "$tmux_before" = "$tmux_after" ]

[ $fail = 0 ] && echo PASS || { echo FAIL; exit 1; }
