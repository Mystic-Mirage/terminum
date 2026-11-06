# terminum — build and install from a checkout (users install the package:
# curl … install.sh | sh, then the terminum command; README).
#   make install    build the package from this checkout and install it (first install or
#                   upgrade; a build that does not come up healthy is rolled back); from the primary
#                   checkout only: a worktree's is refused (CONTRIBUTING.md "Worktrees"); a clean main behind
#                   origin/main is fast-forwarded first, one with uncommitted changes refused (CLAUDE.md §9)
#   make upgrade    the same
#   make rollback   back to the previous build
#   make uninstall  stop terminumd, remove the launchd agent, builds and command (keeps state, logs)
#   make toolchain  the pinned cross toolchain (zig, cargo-zigbuild) for the Linux builds
#   make agents     terminumd for the four platforms (AC-86)
#   make package    the release package in dist/package (tarball, install.sh, SHA256SUMS)
#   make release TAG=vX.Y.Z   tag HEAD, build, sign (key via ssh-agent and the Keychain) and
#                   publish; only when the maintainer asks for it (CLAUDE.md §10)
#   make rehearse TAG=vX.Y.Z  what make release does, stopping before anything is signed or public
#   make land SLUG=x  a worktree's targeted checks, then merged, pushed, the checkout updated, the tree deleted
#   make leftovers  what old test runs left (AC-102); END=1 removes it, as make release does first
#   make signing-key       the maintainer's release key (once)
#   make test       unit tests (Rust, scripts, UI, page)
#   make check      the full run, the release's gate, once per tree (scripts/check_all.sh; --force: CHECK_ARGS=--force)
#   make test-mac   the macOS checks on the LAN's Mac test runner over ssh (scripts/ci_mac.sh)
#   make installed  whether the running terminumd is this tree's build (exit 1 if another)
#   make hooks      the git hooks of scripts/hooks (security_check before a commit and on its message)

ifeq ($(shell id -u),0)
$(error Run make as your user, not root: terminum lives in your home folder)
endif

# the maintainer's own settings, never committed: TM_MAC_RUNNER (make test-mac), TM_CODESIGN_IDENTITY, the Developer ID
# that signs the macOS terminumd (scripts/package.py; without it, ad hoc)
-include local.mk
export TM_CODESIGN_IDENTITY
export TM_MAC_RUNNER
export TM_LAND_INSTALL

# one id per set of sources, compiled into terminumd (option_env!("TM_BUILD")) and the UI
BUILD := $(shell python3 scripts/package.py id)
export TM_BUILD := $(BUILD)
# the day it was built, shown under the session list
export TM_BUILD_DATE := $(shell date +%Y-%m-%d)
# which terminumd sources: an agent on a remote host works with this terminumd when the ids match
export TM_AGENT_ID := $(shell python3 scripts/agents.py id)

.PHONY: all build ui terminumd test check test-mac primary-checkout install install-stage installed hooks upgrade rollback uninstall package agents toolchain land leftovers release rehearse signing-key

# this machine's platform (a Mac's own architecture, right under Rosetta too; or Linux's), the
# package's terminumd for it
PLATFORM := $(shell python3 scripts/agents.py platform)
# the installed command: the newest build's terminumd
TERMINUM := $${TM_HOME:-$$HOME/.terminum}/bin/terminum

all: terminumd
build: terminumd

# exactly the lockfile, no install scripts
ui/node_modules: ui/package.json ui/package-lock.json
	cd ui && npm ci --ignore-scripts --no-fund --no-audit
	touch $@

ui: ui/node_modules
	cd ui && npm run -s build

terminumd: ui
	cd terminumd && cargo build --locked --release

test: ui
	-python3 scripts/check_docs.py
	cd terminumd && cargo test --locked
	# the release tooling (developer scripts); the command's own tests are in cargo test
	python3 -m unittest discover -s scripts/tests
	cd ui && npx tsc -p . && npm test
	node --test web/test/*.test.mjs

check:
	bash scripts/check_all.sh $(CHECK_ARGS)

test-mac:
	bash scripts/ci_mac.sh

# the machine has one install, so a worktree's build never replaces it: install from the primary
# checkout after the merge (TM_INSTALL_FROM_WORKTREE=1: the maintainer's own try of a branch)
primary-checkout:
	@gd=$$(git rev-parse --absolute-git-dir 2>/dev/null); cd=$$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null); \
	if [ -n "$$gd" ] && [ "$$gd" != "$$cd" ] && [ -z "$$TM_INSTALL_FROM_WORKTREE" ]; then \
	echo "make install: refused in a worktree ($$(git rev-parse --show-toplevel)): the machine has one install;"; \
	echo "  here make build and the checks; after the merge, make install from the primary checkout"; exit 2; fi

# freshen first and the rest in a make of its own: this one's TM_BUILD was read before main moved
install: primary-checkout
	python3 scripts/worktree_fresh.py freshen
	$(MAKE) --no-print-directory install-stage

install-stage: primary-checkout ui
	python3 scripts/package.py --stage
	dist/package/terminum/agents/$(PLATFORM)/terminumd install

# several checkouts (worktrees, sessions) share one install: say whose build runs
installed:
	@got=$$($(TERMINUM) status 2>/dev/null | sed -n 's/.*running, build \([^,]*\),.*/\1/p'); \
	behind=$$(git rev-list --count HEAD..origin/main 2>/dev/null || echo 0); \
	if [ "$$got" = "$(BUILD)" ]; then echo "installed: this tree ($(BUILD))$$([ "$$behind" -gt 0 ] && echo ", $$behind commits behind origin/main as last fetched")"; \
	else echo "installed: ANOTHER build ($${got:-none running}), this tree is $(BUILD)"; exit 1; fi

hooks:
	git config core.hooksPath "$$(cd "$$(git rev-parse --path-format=absolute --git-common-dir)/.." && pwd -P)/scripts/hooks"
	@echo "hooks: the primary checkout's scripts/hooks for every checkout of this repository, never a branch's own (git config --unset core.hooksPath removes them)"

upgrade: install

rollback:
	$(TERMINUM) rollback

uninstall:
	$(TERMINUM) uninstall

package: ui
	python3 scripts/package.py

# Remote hosts (AC-86): the pinned cross toolchain once, then terminumd for macOS
# arm64/x86_64 and Linux x86_64/arm64
toolchain:
	python3 scripts/agents.py toolchain

agents: ui
	python3 scripts/agents.py build

land:
	@test -n "$(SLUG)" || { echo "usage: make land SLUG=<worktree>"; exit 2; }
	python3 scripts/land.py "$(SLUG)"

leftovers:
	python3 scripts/leftovers.py $(if $(END),--end)

release:
	@test -n "$(TAG)" || { echo "usage: make release TAG=v0.3.0"; exit 2; }
	python3 scripts/release_turn.py "$(TAG)"

# its own target, no variable of release's: a mistyped variable would publish
rehearse:
	@test -n "$(TAG)" || { echo "usage: make rehearse TAG=v0.3.0"; exit 2; }
	python3 scripts/release_turn.py --rehearse "$(TAG)"

# the maintainer's release key, once (run it in your own terminal: it asks for a passphrase)
signing-key:
	python3 scripts/signing_key.py
