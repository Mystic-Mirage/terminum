#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
# terminum: install or upgrade with one line (AC-08, AC-86)
#
#   curl -q -fsSL https://github.com/extractumio/terminum/releases/latest/download/install.sh | sh
#
# Downloads the latest release's package, its SHA256SUMS and their signature into a
# temporary folder, checks the signature with the release key below, then the checksum,
# and runs the package's own installer: its terminumd for this machine (a Mac's architecture,
# or Linux on x86_64 or aarch64) with `install` (a versioned build in ~/.terminum/builds, the
# launchd agent or the systemd user service, a health check, rollback when the new build does
# not come up). Nothing is run before the checks pass. Everything happens in main, called on the last line: a
# cut-off download does nothing.
set -eu

# the maintainer's release key (make signing-key writes it; the same as release-signers)
SIGNERS='terminum-release namespaces="terminum-release" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILXEkieNRfwJy2GLquHnQptia7ORU0+2kR4mUr+GB80A'

fail() { echo "install.sh: $*" >&2; exit 1; }

main() {
    os=$(uname -s)
    case $os in
        Darwin) ;;
        Linux)
            for tool in curl ssh-keygen tar; do
                command -v $tool >/dev/null 2>&1 || fail "$tool is needed: sudo apt install curl openssh-client tar"
            done ;;
        *) fail "terminum installs on macOS and Linux, not on $os" ;;
    esac
    [ "$(id -u)" != 0 ] || fail "run it as your user, not root (it installs into your home folder)"
    base="${TM_RELEASE_URL:-https://github.com/extractumio/terminum/releases}/latest/download"
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    echo "Downloading terminum …"
    # a stalled server fails the install instead of hanging it (the same deadline as `terminum upgrade`)
    get() { curl -q -fsSL --connect-timeout 30 --max-time 300 "$1" -o "$2" || fail "download failed: $1"; }
    get "$base/terminum.tar.gz" "$tmp/package.tar.gz"
    get "$base/SHA256SUMS" "$tmp/SHA256SUMS"
    get "$base/SHA256SUMS.sig" "$tmp/SHA256SUMS.sig"
    case $SIGNERS in *ssh-*) ;; *) fail "this installer carries no release key: nothing installed" ;; esac
    printf '%s\n' "$SIGNERS" > "$tmp/signers"
    ssh-keygen -Y verify -f "$tmp/signers" -I terminum-release -n terminum-release \
        -s "$tmp/SHA256SUMS.sig" < "$tmp/SHA256SUMS" >/dev/null 2>&1 \
        || fail "the release is not signed by the release key: nothing installed"
    want=$(awk '$2 == "terminum.tar.gz" { print $1 }' "$tmp/SHA256SUMS")
    if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$tmp/package.tar.gz"); else got=$(shasum -a 256 "$tmp/package.tar.gz"); fi
    got=$(echo "$got" | awk '{ print $1 }')
    [ -n "$want" ] && [ "$want" = "$got" ] || fail "checksum mismatch: nothing installed"
    release=$(awk '$1 == "#" && $2 == "release" { print $3 }' "$tmp/SHA256SUMS")
    [ -n "$release" ] || fail "the signed list names no release: nothing installed"
    installed="${TM_HOME:-$HOME/.terminum}/builds/current/BUILD"
    if [ -f "$installed" ]; then
        current=$(cat "$installed")
        if awk -v incoming="$release" -v current="$current" 'BEGIN {
            if (incoming !~ /^v[0-9]+(\.[0-9]+)*$/ || current !~ /^v[0-9]+(\.[0-9]+)*$/) exit 1
            sub(/^v/, "", incoming); sub(/^v/, "", current)
            n = split(incoming, a, "."); m = split(current, b, ".")
            for (i = 1; i <= n || i <= m; i++) {
                if (a[i]+0 < b[i]+0) exit 0
                if (a[i]+0 > b[i]+0) exit 1
            }
            exit 1
        }'; then
            fail "the release $release is older than the installed $current: nothing installed"
        fi
    fi
    if [ "$os" = Darwin ]; then
        # the Mac's own architecture: under Rosetta, uname -m would say x86_64 on Apple Silicon
        plat=macos-x86_64
        [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ] && plat=macos-aarch64
    else
        case $(uname -m) in
            x86_64|amd64) plat=linux-x86_64 ;;
            aarch64|arm64) plat=linux-aarch64 ;;
            *) fail "no terminum for $(uname -m) Linux (x86_64 and aarch64 only)" ;;
        esac
    fi
    mkdir "$tmp/pkg"
    tar -xzf "$tmp/package.tar.gz" -C "$tmp/pkg" || fail "the package does not unpack: nothing installed"
    installer="$tmp/pkg/terminum/agents/$plat/terminumd"
    [ -f "$installer" ] || fail "the package has no terminumd for $plat: nothing installed"
    "$installer" install
}

main "$@"
