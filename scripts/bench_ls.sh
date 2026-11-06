#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
# bench_ls.sh DIR [RUNS] — AC-52 (terminum-files.md): over RUNS fresh terminumds (default 20, at
# least 5), the cold first page of a large folder and a cached random page, p50/p95/max, and the
# largest RSS of any run; SORT=recent or ext: in that order. "Cold" means terminumd's listing cache
# is empty (a new process); the OS's disk cache stays warm after the first run, so this is not a
# cold-disk number. Runs a private terminumd on a free port (BENCH_PORT to choose) with its own
# home, state and tmux socket, so the installed one is not disturbed.
set -euo pipefail
dir=$(cd "${1:?dir}" && pwd -P); runs=${2:-20}
[ "$runs" -ge 5 ] || { echo "RUNS must be at least 5 for a p95" >&2; exit 2; }
port=${BENCH_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')}
bin=${TERMINUMD_BIN:-$(dirname "$0")/../terminumd/target/release/terminumd}
tmp_base=${TMPDIR:-/tmp}; tmp_base=${tmp_base%/}; [ ${#tmp_base} -le 40 ] || tmp_base=/tmp   # short: a socket path holds at most 104 bytes (testrun.py)
bench_home=$(mktemp -d "$tmp_base/tmb-XXXXXX")
tsock=tm-bench-$$
pid=""
trap 'set +e; [ -n "$pid" ] && kill $pid 2>/dev/null; tmux -L "$tsock" kill-server 2>/dev/null; rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$tsock"; rm -rf "$bench_home"' EXIT
trap 'exit 130' INT; trap 'exit 143' TERM; trap 'exit 129' HUP   # a signal skips the EXIT trap otherwise
export TM_PORT=$port TM_HOME=$bench_home TM_APP_DIR=$bench_home/state TM_TMUX_SOCKET=$tsock TM_LOG=warn
url="http://127.0.0.1:$port/fb/api/ls?path=$dir${SORT:+&sort=$SORT}"
signin() { # a password over the socket (as `terminum password`), then the sign-in /auth/login gives for it
  curl -s -o /dev/null --unix-socket "$TM_APP_DIR/terminumd.sock" -X POST -H 'Content-Type: application/json' \
       -d '{"password":"bench password"}' http://localhost/password
  curl -s -X POST -H 'Content-Type: application/json' -d '{"password":"bench password"}' http://127.0.0.1:$port/auth/login \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["secret"])'
}
cold=(); warm=(); rss_max=0
for _ in $(seq "$runs"); do
  "$bin" 2>/dev/null & pid=$!
  for _ in $(seq 100); do curl -s -o /dev/null --unix-socket "$TM_APP_DIR/terminumd.sock" http://localhost/health && break; sleep 0.05; done
  tok=$(signin)
  cold+=("$(curl -s -o /dev/null -w '%{time_total}' -H "X-TM-Session: $tok" "$url&limit=500")")
  total=$(curl -s -H "X-TM-Session: $tok" "$url&limit=1" | python3 -c 'import json,sys;print(json.load(sys.stdin)["total"])')
  [ "$total" -gt 0 ] || { echo "$dir is empty: fill it with scripts/make_big_dir.sh" >&2; exit 1; }
  warm+=("$(curl -s -o /dev/null -w '%{time_total}' -H "X-TM-Session: $tok" "$url&offset=$((RANDOM * RANDOM % total))&limit=500")")
  rss=$(ps -o rss= -p $pid); [ "$rss" -gt "$rss_max" ] && rss_max=$rss
  kill $pid; wait $pid 2>/dev/null || true; pid=""
done
q() { # q FRACTION TIMES…: the nearest-rank quantile in ms
  local f=$1; shift
  printf '%s\n' "$@" | sort -n | awk -v f="$f" '{a[NR]=$1} END {i=int(NR*f); if(i<NR*f)i++; if(i<1)i=1; printf "%d", a[i]*1000}'
}
c=$(q 0.95 "${cold[@]}"); w=$(q 0.95 "${warm[@]}")
verdict=$([ "$c" -lt 2500 ] && [ "$w" -lt 50 ] && [ "$rss_max" -lt 153600 ] && echo PASS || echo FAIL)
echo "entries=$total runs=$runs cold_ms p50=$(q 0.5 "${cold[@]}") p95=$c max=$(q 1 "${cold[@]}")" \
     "page_ms p50=$(q 0.5 "${warm[@]}") p95=$w max=$(q 1 "${warm[@]}") rss_max_mb=$((rss_max/1024)) $verdict"
[ "$verdict" = PASS ]
