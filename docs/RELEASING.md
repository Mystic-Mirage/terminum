# Releasing

How releases are built, signed and published. When a release may happen is in
[CLAUDE.md](../CLAUDE.md) §10; how users install is in the [README](../README.md).

## Signed releases

Installs and upgrades accept a release only if its `SHA256SUMS` is signed by the
maintainer's release key (an ed25519 ssh key; `ssh-keygen -Y`, built into macOS). Its public
half is in `release-signers` and in `scripts/install.sh`, and every package carries it, so
`terminum upgrade` checks with the key of the build already installed. The key is the
maintainer's one release key, shared with iterm-enhancer: `~/.config/iterm-enhancer/release-key`
(`TM_RELEASE_KEY` names another), its passphrase in the login Keychain; terminum signs with it
in its own namespace, `terminum-release`, so a signature of one product never verifies as the
other's. On a Mac without that key, once, in your own terminal:

```bash
make signing-key        # the key (TM_RELEASE_KEY); writes the public half
git add release-signers scripts/install.sh && git commit -m "Release key"
ssh-add --apple-use-keychain ~/.config/iterm-enhancer/release-key \
  && ssh-add -d ~/.config/iterm-enhancer/release-key.pub              # keep it in the Keychain, not loaded
```

Then a release is one command, with no prompt: `make release TAG=vX.Y.Z` refuses unless the
tree is clean, the version is above every `v*` tag and `HEAD` is `origin/main`; it installs
the UI's dependencies with `npm ci --ignore-scripts`, builds terminumd with `cargo --locked`
for every platform below ("Builds for remote hosts"; `make toolchain` once: zig and
cargo-zigbuild pinned in `.toolchain/`; a release without the Linux builds fails, since a Linux
server installs from the same package), packs `terminum.tar.gz` and the same bytes as
`terminum-macos.tar.gz` (the name installs before 0.22 ask for: keep it until a release notes
a floor version below which `terminum upgrade` is no longer supported), signs `SHA256SUMS` through ssh-agent (the key loaded
for two minutes with the Keychain's passphrase, then removed), checks the signature, and only
then, if `HEAD` has not moved since the checks (another session's `make install` fast-forwards a clean `main`), tags `HEAD`, pushes the tag and publishes the GitHub release once all its files are
attached. A failure before signing leaves nothing public.

Each macOS terminumd is signed with the maintainer's Developer ID as `io.terminum.terminumd`
(it embeds `terminumd/macos/Info.plist`), so macOS keeps the folder access the user grants
across upgrades (spec AC-08). The identity is named, never guessed: `TM_CODESIGN_IDENTITY` in
the uncommitted `local.mk` beside the Makefile:

```make
TM_CODESIGN_IDENTITY := Developer ID Application: <name> (<team id>)
```

`make release` refuses without it, signs with Apple's secure timestamp (online) and checks
each binary's Team ID; codesign gets 60 s, so a keychain dialog waiting for "Always Allow"
fails the build instead of hanging it. No hardened runtime and no notarization: the tarball
arrives by curl, which sets no quarantine. Without `local.mk`, `make install` signs ad hoc and
says so; macOS then asks again for protected folders after an upgrade.

A build's compiler or linker `warning:` is said, by a release and by `make install`, and stops neither.

Release notes go in `docs/releases/vX.Y.Z.md`, committed before `main` is pushed; the release
command refuses an empty or unreadable file and a symlink out of the tree.

Who else could ship code to users is in [SECURITY.md](../SECURITY.md) ("The installer and
releases"). Keep the key and its passphrase backed up: a new key makes existing installs refuse
upgrades until they are installed again with the one-line installer. A leaked key is replaced
for both products: a new key in both repositories'
`release-signers` and `install.sh`, a release of each, and every install of each installed
again with its one-line installer.

## Before a release

`make release` runs the full checks first (`make check`, once per tree): failing tests stop it, and nothing else that is about quality does.

`make release` first runs `scripts/leftovers.py --end`, which removes what old test runs left and never
stops the release (spec AC-102): `make leftovers` only says what it would remove, `make leftovers END=1`
removes it. `/docs-review` is in CLAUDE.md §10.

## Builds for remote hosts

`make agents` builds terminumd for `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` (static Linux builds with the
pinned zig); `make package` puts them into `dist/package` (`terminum/agents/<platform>/terminumd`,
`BUILD`, `AGENT_ID`, `release-signers`, licenses; no scripts) next to `install.sh`, which runs
the package's terminumd for the Mac's architecture with `install`: the macOS build is the
installer, the server and the `terminum` command at once. The helper a host gets is the build
for its platform (`uname -sm`), named by its agent id.

## CI

Two places, both the owner's: GitHub Actions on the owner's Linux runner for every push
(`.github/workflows/ci.yml`, label `terminum`), and the macOS checks on a Mac test runner of the
owner's network over ssh (`make test-mac`, `scripts/ci_mac.sh`; `TM_MAC_RUNNER` is its ssh name,
in the maintainer's `local.mk`): no GitHub runner on a Mac, whose user can do more than a
CI job should. `make test-mac` is evidence, not a gate: run it before a release and after a
change to macOS-only code (launchd, signing, folders, Finder).

The Linux job runs `make test`, clippy with warnings as errors, `e2e_install.sh`, the Files
panel's checks and the page's live checks (`e2e_live.mjs`, Chromium and WebKit) against a
private terminumd and tmux, and `security_check.sh`. The page's own checks (`e2e_all.mjs`)
measure the layout of Safari and Chrome on the Mac, the page's clients: Linux's fonts and
WebKit differ (columns, mouse cells, motion, TIFF), so they run on a Mac (`make test-mac`, and
before every merge on the maintainer's). Timing checks scale by the load per core (`web/test/check.mjs`),
since the runner's machine is shared. A real `systemctl --user` run is not part of it:
the runner has no user manager, and giving it one (linger) would let a job leave a unit running
outside the runner's confinement; the systemd half is covered by its unit tests (a stub
systemctl) and checked by hand on a disposable machine before a release that changes it.

### The runner (once)

An Ubuntu 24.04 machine; the runner under an unprivileged user of its own (no sudo, no docker,
its home closed to others), as a systemd service confined (`ProtectHome=tmpfs`, `PrivateTmp`, its
home bound in) and capped in memory (`MemoryMax=2G`, `OOMPolicy=continue`: a job that runs out
fails alone), labels `terminum`. Packages: `tmux build-essential` and the browsers' system
libraries (`npx -y playwright-core@<ui's version> install-deps chromium webkit`, as root); Rust,
Node and the browsers install per job into the runner's home. rustc alone peaks near 1.3 GB
building the tests (a 1536M cap stalls it); each run prints
that peak and the job's cap hits. It builds with one rustc and no debug info (the workflow's
env), since two rustc with it outgrew a 2 GB machine. The registration token travels on stdin,
never on a command line:

```bash
gh api -X POST repos/extractumio/terminum/actions/runners/registration-token --jq .token \
  | ssh <runner machine> 'read -r ACTIONS_RUNNER_INPUT_TOKEN; export ACTIONS_RUNNER_INPUT_TOKEN; …'
```

The maintainer's recipe for it lives with the machine's own records, outside this repository.

The repository's settings keep everyone else's code off the runner: Actions › General ›
"Require approval for all external contributors" (`gh api -X PUT
repos/extractumio/terminum/actions/permissions/fork-pr-contributor-approval -f
approval_policy=all_external_contributors`), the runner registered to this repository alone,
and the workflow's `permissions: {}`. Never approve a fork's run (CLAUDE.md §10).
