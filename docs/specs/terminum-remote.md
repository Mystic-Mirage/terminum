# terminum — Specification (remote hosts)

Sibling of `terminum.md` (the core). This spec holds remote hosts: their tmux sessions over ssh, their files through an agent, helpers that follow the Mac's build, and the builds and releases. Cases are AC-81…AC-87 and AC-99. A case of another spec is written `AC nn` (a space); `AC-nn` is this spec's own.

## 0. Metadata

| Field | Value |
|---|---|
| Version | 0.8.8 |
| Date | 2026-11-03 |
| Status | draft |
| Author | Claude (Fable 5.1) for the owner |

Change log:

| Version | Date | Change |
|---|---|---|
| 0.8.8 | 2026-11-03 | CI on Linux failed v0.30.0: a link whose ssh ended with its reason ("Permission denied (publickey).") could say "ssh: no answer from the host": the reason was taken once stdout ended, before ssh's words on stderr were read. It now waits for stderr's end, 1 s at most (a child left holding it), when no `%exit` came; a clean `%exit` waits for nothing, as an ssh ControlPersist master keeps stderr (AC-81). |
| 0.8.7 | 2026-11-03 | Issue #8: on a macOS host (zsh) Files never enabled: the helper's copy ran in the login shell, and zsh's nomatch aborted its `rm -f …/terminusd-agent-*` where no old helper was ("no matches found"), so a copy that had landed was reported failed; a Linux host's bash passed the glob on. Every command terminumd runs for the helper on a host (the probe, the copy, its start, its removal) now runs as `exec /bin/sh -c '…'`, as the agents' scripts already did; fish and csh hosts too (the noticed item of AC-82, AC-83) (AC-83). |
| 0.8.6 | 2026-11-03 | Owner, from 0.8.5's review: an agents off given during a long agents on was written over: the on, done later on the same recipe, saved "enabled" after the off and installed the hooks the off had taken out. One agents on or off runs at a time per host, the last asked winning: a later one stops the one running and runs after it; one still waiting never begins. Pragmatic review applied: a killed ssh left its script running on the host (sshd does not end it), so the script now ends its process group when its ssh's input closes (terminumd keeps it open); a host's removal and the refresh after a helper update take their turn too (AC-99). |
| 0.8.5 | 2026-11-03 | An audit of 7e5c220: setting up agents could go on over another recipe saved meanwhile: the recipe read at the start is used for every step and checked before the install and the save (a stale operation saves nothing). Agents off is saved and the link closed first, then the hooks go within 90 s ("Turned off here; …" when that fails). A refused -R was retried only when the ssh ended: it is retried after 10 minutes, then 20, 40, 80, 160 while refused. ssh answers are bounded (1 MB, the last 4 KB of stderr, a 4 KB ready line), only that ssh ended past them. The CLI waits the sum of the server's stages (enable 365 s, agents on 635, off 155, files-remove 65) and says when it gave up. The limits say what the code does: helper copy 300 s, network loss about 30 s, `RemoteCommand=none` not passed (AC-81, AC-82, AC-85, AC-99). |
| 0.8.4 | 2026-11-01 | Owner, from a review of the open bookmarks: a host whose sshd sent no banner kept the link waiting without end and the row then said "tmux exited": the control client's ssh has `ConnectTimeout=10` (as Files' and the agents' ssh), unless the user's ssh configuration sets its own (`ssh -G`, read once per recipe off the async workers: a slow ProxyJump keeps its longer wait), and a link that ends before tmux answered with nothing said says "ssh: no answer from the host" (AC-81). A recipe edited by hand in settings.json dropped an attached host at once, also for a typo; asked to make it more reliable: an attached host keeps its link while the new recipe is invalid, or until the new one answers over ssh, and says why (AC-87). |
| 0.8.3 | 2026-10-29 | Owner: a host group's menu (⋯) near the list's end went past the window's bottom (Off, Attach tmux sessions… and the agents' item cut off); it opens under its button where it fits, else above it, always inside the window, scrolling in a window shorter than it (AC-81). |
| 0.8.2 | 2026-10-26 | Owner: the first "terminum hosts agents <host> on" printed only "terminumd answers (see: terminum status)": the CLI waited 5 s, the install takes longer. It says what it does and waits up to 5 minutes (AC-99). |
| 0.8.1 | 2026-10-26 | Found over a real sshd (Alpine, Debian): `DisableForwarding yes` refuses the Files tunnel's own `-L` too, so only a refusal of `-R` alone (`AllowStreamLocalForwarding local`) keeps Files working (AC-99, AS-84); a login shell that prints made the helper's id check fail on every enable (files and agents): its last line is the answer. AC-99's Definition of Done filled; a macOS host over real sshd not verified. |
| 0.8.0 | 2026-10-26 | Owner: coding agents' progress on a host (AC-99): Claude Code's and Codex's hooks and `terminum mcp` in a host's tmux reach the Mac's terminumd, so the host's rows and the Plan pane show them as for local panes (terminal spec AC 97, AC 98). A per-host `agents` setting (CLI `terminum hosts agents <host> on\|off`, the host menu), installed over ssh with per-agent result lines; the events ride on the files helper's ssh as a reverse Unix-socket forward (`-R`), files staying off unless enabled; a refused forward keeps files working; matching on the host server's full socket path; per-Mac link names; content-based refresh after AC-84; link loss drops the host's hook state. Pragmatic review applied (its must-change list 1–9). Assumption AS-84 to verify. |
| 0.7.3 | 2026-10-26 | An audit's finding fixed: the orphan sweep's answer option was named by terminumd's pid and the connection's number, which two Macs (or two host names of one server) can share, so one sweep could read or remove another's answer; it is named at random per sweep (`@tm-orphans-<16 hex>`) (AC-81). |
| 0.7.2 | 2026-10-24 | Owner, asked whether a host's picture could be a way in: terminumd passed a host's Content-Type and Content-Security-Policy on as they came, so a host not to be trusted (or a helper replaced there) could have a picture's answer be HTML without the `sandbox` policy; a raw answer now carries terminumd's type, by the file's name, and its `sandbox` policy, every other forwarded answer (an error too) is JSON; `scripts/e2e_remote.py` reads a host's PNG and SVG through the header (AC-82; terminum.md 0.18.15, AC-09). |
| 0.7.1 | 2026-10-21 | Owner, on what 0.7.0 left: a host with busybox's ps (no -U, no -p) is swept, through /proc; where tmux rewrites a client's command line (FreeBSD, OpenBSD) one of this server's socket with no terminal counts. Pragmatic review applied: no terminal read from /proc where ps cannot say (else none: kept), only this server's socket (AC-81). |
| 0.7.0 | 2026-10-21 | Owner: a Linux host (tmux 3.4) held 12 control clients of terminum's, up to 4 days old, each left by a dropped link: once ssh's end goes, tmux keeps the client's pipes and unsent output and the client never exits (`detach-client` cannot end it, a signal does). Now, once each connection of a host, terminumd ends those of its control clients there that are orphaned: their parent is init, or a shell whose parent is init. Pragmatic review applied: orphaned proved by the parent, never by the absence of sshd (Tailscale SSH, dropbear, a test's fake ssh are live without one); the shape matched by its arguments (`tmux … -C … -f ignore-size …`), not a prefix; quiet and in the background (`run-shell -b`), every failing `ps` skipping its client, a ppid that is not one number too. Owner, then: those stuck before tmux listed them are found by `ps` too (Linux names a client `tmux: client`), those whose command line tmux rewrote (FreeBSD, OpenBSD) by tmux's list (control clients with `ignore-size`); the script answers the pids it ended in a user option of the server (`@tm-orphans-<terminumd's pid>`, read and removed), so the log names exactly those; checked over real sshd and tmux 3.4, where it showed that a server started by terminum's command keeps that command line: the server's pid is never a candidate and `ps` takes only a process named `tmux: client`. Pragmatic review applied: `read -r` and a pid that is not a number skipped (a backslash ended line joined the next), a listed pid still a tmux process, shells by name (`ssh` is none), `ps -ww`, the answer option named per connection and removed before and after; the script itself run under dash, busybox ash and macOS sh against a stand-in `ps` (AC-81). |
| 0.6.0 | 2026-10-16 | Owner: OQ-81 amended: the host's other tmux sessions stay hidden unless the user attaches them by name (AC 89, `hosts.<name>.attach`); each attached session is a further ssh connection. |
| 0.5.0 | 2026-10-15 | Owner: the helper is `terminumd-agent` (was `terminusd-agent`): a host with only the old one has it started, sees its old build and replaces it (AC-84); install and removal delete the old files. |
| 0.4.0 | 2026-10-15 | Owner: Add host reads as `ssh <what you type>`; Name optional (the destination's host); "Browse its files" ticked in Add host enables files once attached (AC-81, AC-83). |
| 0.3.0 | 2026-10-15 | Owner: OQ-81 resolved: a host is one ssh string; terminumd creates the session `terminum` on the host when it is missing and holds it; windows and panes are made inside it; no session option. terminumd named. |
| 0.2.0 | 2026-10-15 | Pragmatic review: `-F` dropped from the recipe allowlist (a config file can hold ProxyCommand), a destination starting with `-` refused, the remote command quoted for the host's shell, `new-session -A` without `-D` (it would detach the host's other clients), paste on a host through the agent's file. |
| 0.1.0 | 2026-10-15 | Initial draft from iterm-enhancer 0.42 (AC 37, 38, 39, 42, 48): terminumd is the tmux control client over ssh; a host is an ssh destination the user gave terminum; the files agent as before. |

## 1. Overview

**Problem.** Work happens on remote hosts in tmux over ssh. In iterm-enhancer the host had to be found from the ssh process of an iTerm2 gateway; a dropped integration printed protocol into the terminal; reconnecting was iTerm2's.

**Who it affects.** Developers with tmux sessions on Linux and macOS hosts reached by ssh.

**Expected outcome.** A host is added once by its ssh string. terminumd attaches to its tmux in control mode over ssh, in a session `terminum` it creates when missing, shows its sessions beside the local ones, types into them, reconnects after a drop with back-off, and, once the user enables it, browses the host's files through a helper it installs itself and keeps up to date. The helper and terminumd are built for four platforms from one script and released signed.

**Scenario, before and after.**
- Before: Alex types `tms cc devbox work` in iTerm2, waits for windows to open, and after the Wi-Fi drops finds `%output` lines in a terminal.
- After: Alex adds `devbox` once in the page. Its session "terminum" is in the list in two seconds, also on the phone. After a drop the group says "reconnecting (4 s)" and comes back; nothing printed anywhere.

## 2. Scope and non-goals

In scope:
- Hosts (add, remove, reconnect), remote sessions in the list and the terminal, files through the agent, enabling a host's files, helper updates, upload to a host, builds for four platforms, signed releases on the owner's runner.

Out of scope (explicit):
- Hosts without tmux (plain ssh shells): a local pane running `ssh host` is a local pane; its files are not browsed.
- Password prompts over the control connection: ssh must authenticate without a prompt (keys, agent, ControlMaster); a host that needs one is reported.
- Mosh, et, other gateways.

## 3. Assumptions, dependencies, open questions

Assumptions:

| ID | Assumption | Owner | Status |
|---|---|---|---|
| AS-81 | `ssh -T -o BatchMode=yes <options> -- <destination> 'tmux -u -C new-session -A -f ignore-size -s terminum'` gives a control-mode client over ssh whose stdout is line-based and whose `%output` arrives within the link's latency; `pause-after` flow control (`refresh-client -f pause-after=2`) keeps a slow link from backing up. | Claude | verified 2026-10-15 with a stand-in ssh on this Mac (`scripts/fake_ssh.py`, `scripts/e2e_remote.py`); a real sshd not measured |
| AS-82 | The agent (`terminumd --agent --socket <path>`) is iterm-enhancer's `fbd --agent` moved: a Unix socket on the host, a token on stdin, exits on stdin EOF. | Claude | verified in iterm-enhancer |
| AS-83 | The hosts in the owner's use are Ubuntu and Debian x86_64 and arm64 and macOS arm64. | Owner | stated |
| AS-84 | OpenSSH's `-R <host path>:<Mac path>` (AC-99): (a) every refusal (`AllowStreamLocalForwarding no`, `DisableForwarding yes`, Alpine's stock `AllowTcpForwarding no`, a missing folder, a stale file) prints the same line: with `ExitOnForwardFailure=no` `Warning: remote port forwarding failed for listen path <path>` (hidden by `LogLevel=ERROR`), exit 0, the command runs; with `=yes` `Error: …`, exit 255, the command never runs; the client cannot tell them apart, so the helper checks its `--link` socket itself; `DisableForwarding yes` also refuses the Files tunnel's own `-L` (Files fail with it), and `AllowStreamLocalForwarding no` forbids both directions by sshd's definition, so only a refusal of `-R` alone (`AllowStreamLocalForwarding local`) keeps Files working; (b) sshd binds the socket (0600, the user's) before the remote command starts (10 of 10 also with `=no`), never creates its folder, and leaves the file after the connection ends; (c) a stale file is refused (sshd: `Address in use`) and the client's `StreamLocalBindUnlink=yes` does not help: a fresh random name per connection; (d) `-o ControlPath=none` makes no mux master, yet a user config's `RemoteForward` still applies and, failing on the second login, ends the ssh under `ExitOnForwardFailure=yes` (hence `=no`); a mux client's `-R` outlives the client under ControlPersist; a user ControlPath of 104 bytes or more fails the ssh; `ClearAllForwardings=yes` drops the command line's `-R` too (never used); (e) the Mac-side target needs a `/` (a bare name: `Bad remote forwarding specification`, exit 255) and at most 104 bytes; a request takes about 0.7–1.0 ms on loopback; (f) a login shell's rc output (lines above the interactive guard of Debian's `.bashrc`) precedes the command's stdout, `exec /bin/sh -c` included, and `-tt` adds `\r` to each line: output is read after a marker line. Dropbear has no stream-local forwarding, so it refuses Files' `-L` as well as `-R`. | Claude | verified 2026-10-26 against real sshd: OpenSSH 9.7p1 (Alpine 3.20) and 9.2p1 (Debian 12), client OpenSSH 9.9p2 (macOS); the `-L` refusal under `DisableForwarding yes` seen 2026-10-26 (`scripts/e2e_remote_sshd_agents.py`, Alpine: Files "unreachable", connection closed); Dropbear not tested; a macOS host's sshd not verified (Remote Login off on the Mac at hand) |

Dependencies:

| ID | Dependency | Owner | Status |
|---|---|---|---|
| DEP-81 | tmux ≥ 3.2 on the host (DEP 02). | Owner | available |
| DEP-82 | zig and cargo-zigbuild pinned in `./.toolchain` for Linux musl builds; the owner's self-hosted Linux runner. | Claude | available (inherited) |

Open questions:

| ID | Question | Owner | Status | Decision |
|---|---|---|---|---|
| OQ-81 | Does a host attach all its tmux sessions or one named session? | Owner | resolved | One session per host, named `terminum`: the user gives only the ssh string; terminumd runs `tmux -C new-session -A -s terminum` on the host, creating it when missing, and makes windows and panes inside it; other sessions on the host are not shown. Owner, 2026-10-15. Amended 2026-10-16: the user may attach other sessions of that tmux by name (AC 89); they are counted on the host's group, never listed or touched until attached. |
| OQ-82 | Should the files helper be installed with the host, without the Enable click? | Owner | open | Proposed: keep the click (nothing is copied to a host without it), as iterm-enhancer's AC 38. |

## 4. Acceptance cases

| ID | Priority | Case |
|---|---|---|
| AC-81 | [MUST / P0] | The user adds a host by its ssh string and sees its `terminum` session in the list within 5 s, opens and types into its panes, and after a dropped link the host reconnects with back-off and says so; nothing is printed into any terminal. |
| AC-82 | [SHOULD / P1] | The user browses, reads, edits, creates, renames and trashes a host's files as if local, with live refresh, once the host's files are enabled. |
| AC-83 | [SHOULD / P1] | The user enables a host's files with one click the first time its pane is shown; nothing is installed on a host without that click; a host's helper can be removed with one action. |
| AC-84 | [SHOULD / P1] | After the Mac's terminum is upgraded, the helper on every attached host follows by itself and the page says so. |
| AC-85 | [SHOULD / P1] | Pasted images and uploaded files reach a host's pane as paths on that host. |
| AC-86 | [SHOULD / P1] | The maintainer builds terminumd for macOS arm64 and x86_64 and Linux x86_64 and arm64 the same way on the Mac and on the owner's runner, and publishes signed releases from the Mac. |
| AC-87 | [SHOULD / P1] | Hosts are their ssh destinations: two hosts reporting the same name are two hosts; a host's identity and options live in settings and the `terminum hosts` command manages them. |
| AC-99 | [SHOULD / P1] | Once the user enables coding agents' progress on a host, Claude Code and Codex in that host's tmux report status, plans and notes to the host's rows and the Plan pane as local panes do (AC 97, AC 98); a host that cannot forward says why and keeps its files. |

## 5. BDD scenarios

### AC-81 — A host's tmux sessions [MUST / P0]

```gherkin
Scenario: AC-81 happy path — add a host
  When the user presses "Add host" under the session list and types "alex@devbox.example -p 2222" after its "ssh" label (what follows `ssh` in a shell: a Host of ~/.ssh/config as is, a leading "ssh" dropped) and leaves Name empty (or runs "terminum hosts add devbox 'alex@devbox.example -p 2222'")
  Then terminumd records it in settings.json hosts (destination and allowlisted options: -p, -l, -i, -J, -4, -6, -C; anything else refused with "option not allowed: -o" (also -F: a config file can hold ProxyCommand); a destination starting with "-" refused), starts "ssh -T -o BatchMode=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=2 -p 2222 -- alex@devbox.example 'tmux -u -C new-session -A -f ignore-size -s terminum'" (`--` before the destination: ssh reads everything after the destination as the command) (the remote command quoted for the host's shell; -A creates the session when missing and attaches otherwise, without -D, so the host's other clients stay attached) and within 5 s the list shows the group "devbox" in cornflower blue with the windows and panes of that session, every row naming the host in violet; other sessions on the host are not shown until the user attaches them by name (OQ-81, AC 89)
  And an empty Name is the destination's host ("alex@devbox.example" → "devbox", "192.0.2.5" as is); a name so made that another host has is refused with "devbox is already a host: give this one a name"; a typed Name of an existing host replaces its recipe
  And [+] on the group, splits and "New session" make windows and panes inside "terminum" on the host (AC 07)
  And opening a pane, typing, Screen mode, sizing, widgets, bookmarks and search work as for a local pane; the key is "tmux:devbox:default:%N"

Scenario: AC-81 happy path — the link drops and comes back
  When the network drops
  Then within about 30 s (ssh's ServerAliveInterval 15 s × ServerAliveCountMax 2 unanswered) the control client ends, the group "devbox" says "reconnecting (2 s)" on a line of its own under its name (a long state wraps there, never cut), a shown pane keeps its last screen with "Connection to devbox lost" over it, typing is refused with that message; terminumd reconnects after 1, 2, 4 … 30 s
  When ssh succeeds again
  Then the group is back within 5 s, the shown pane's screen is captured anew (its history too), and the notice goes; the tmux session on the host never noticed

Scenario: AC-81 edge — a dropped link left its control client on the host
  Given the link dropped while the host stayed up, and the host's tmux kept terminum's control client of it ("tmux -u -C new-session -A -f ignore-size -s terminum", its ssh end gone, its parent now init)
  When terminumd connects to the host again
  Then it ends that client (SIGTERM; detach-client cannot) once for that connection, logged as event=clients.orphans with exactly the pids ended (the script answers in a server option named at random for that sweep); terminum's clients are those tmux lists as control clients with ignore-size (still a tmux process: a pid not reused), and (Linux) the user's processes named "tmux: client" with terminum's command line, also one stuck before tmux listed it
  And where tmux rewrote a client's command line (FreeBSD, OpenBSD: "tmux: client (<socket>)") one of this server's socket with no terminal counts too (an interactive client has one; a command's or another program's control client of this server left orphaned is ended as well, an accepted risk); on a host whose ps cannot say (busybox: no -U, no -p) processes, their names, parents and terminals are read from /proc
  And the tmux server is never one, though a server terminum's command started keeps that command line; a control client whose parent lives (this connection's, another Mac's, through any ssh server), one of another shape, one under a subreaper are left alone; a read that fails leaves its process

Scenario: AC-81 edge — the pane's folder and programs
  Then a remote pane's folder is the host's path (for Files and "cd"); the program is #{pane_current_command} on the host; the agent state is read from its screen as locally

Scenario: AC-81 failure — ssh fails
  When ssh exits before "%begin" (unknown host key, a password needed, tmux missing on the host), or the host's sshd sends no banner within ssh's 10 s (`ConnectTimeout`, unless ~/.ssh/config sets one for that host: "Connection timed out during banner exchange"; ssh ending before tmux answered with nothing said: "ssh: no answer from the host")
  Then the group "devbox" shows ssh's last line under its name, wrapped in full and in the error color (BatchMode: "Permission denied (publickey)", "tmux: command not found"), nothing is retried faster than the back-off, and the log has ssh's full output; "Retry now" in the group's menu retries at once (the menu opens under its button, above it where the window ends first, always whole inside the window); while the host is not attached (connecting, reconnecting, failed, off) its group has no "+ New window": there is no tmux to make it in
```

### AC-82 — Files on a host [SHOULD / P1]

```gherkin
Scenario: AC-82 happy path — browse and edit
  Given devbox's files are enabled (AC-83) and the page shows its pane in "/home/alex/api"
  Then terminumd runs "ssh devbox -L <app>/agents/devbox.sock:<remote socket> -- ~/.terminum/bin/terminumd-agent --agent --socket <remote socket>" (one per host, token on stdin, back-off 1, 2, 4 … 30 s), and within 3 s Files shows "devbox:/home/alex/api" with a green dot
  And expanding, opening, editing, saving, creating, renaming and trashing work as locally (AC 52–AC 59): every request carries X-TM-Host: devbox and goes to that host's agent unchanged (token and Host replaced, Origin dropped), never to a local file; a change on the host shows within 2 s
  And "Reveal in Finder" and "Open with default app" are not offered; terminumd refuses them with "Not available for remote files"; a local and a remote file with one path never share a tab

Scenario: AC-82 edge — the agent connection drops, an older agent
  When the ssh of the agent closes
  Then the agent exits (stdin closed), the dot turns amber, terminumd reconnects with back-off; unsaved edits stay; a save meanwhile fails with "Not saved: devbox not reachable — save again"
  When the agent reports an agent id other than terminumd's
  Then terminumd installs the matching agent from the running build (AC-84) and reconnects; a build without an agent for that platform says "helper outdated · terminum hosts enable devbox"

Scenario: AC-82 failure — the agent stalls or answers badly
  Then a request that gets no bytes for 30 s, or no end within 120 s, fails and frees its connection; bodies are capped at 64 MB; a lagged event stream reconnects with a host-scoped rescan; a dialog answered after the page switched hosts does nothing ("The panel switched to another pane — nothing changed"); a document saves to the host it came from

Scenario: AC-82 security — the agent
  Then the agent listens only on a Unix socket with a random name in a 0700 folder on the host (removed on exit), takes its token on stdin (never a command line), exits on stdin EOF or SIGHUP, writes only under $HOME and /tmp on the host; on the Mac the forwarded socket is in the app folder (0700) and only terminumd connects to it; answers are untrusted (capped, streamed, Origin not forwarded; their type and policy terminumd's: a raw file's by its name with the `sandbox` policy, every other answer JSON)
```

### AC-83 — Enable a host's files [SHOULD / P1]

```gherkin
Scenario: AC-83 happy path — first time
  Given devbox is attached and its files never enabled or declined
  When the page shows a pane of devbox and the user opens Files
  Then Files says "devbox is a remote host. Browse its files? This copies a 6 MB helper to ~/.terminum/bin on devbox." with [Enable] and [Not now]
  When the user clicks Enable
  Then terminumd reads the platform over the host's ssh recipe, copies the agent the package carries for it to "~/.terminum/bin/terminumd-agent-<agent id>", links "~/.terminum/bin/terminumd-agent" to it, checks it through a trial tunnel, records "files": "enabled" in settings.json, and Files shows the host's folder within seconds; the agent logs to "~/.terminum/logs/agent.log" on the host (1 MB, one older kept)

Scenario: AC-83 edge — a host whose login shell is not sh
  Given devbox's login shell is zsh (macOS's since 10.15), fish or csh
  When terminumd probes it, copies or removes the helper, or starts it
  Then each command runs as "exec /bin/sh -c '…'" whatever the login shell, so zsh's nomatch never aborts a removal of old helpers none of which is there ("no matches found: …/terminusd-agent-*"), and fish or csh never read POSIX sh

Scenario: AC-83 happy path — ticked in Add host
  When the user ticks "Browse its files" in Add host (the tick is the click)
  Then "files": "wanted" is recorded, Files shows "Setting up…" instead of the question, and once the host is attached terminumd enables it as Enable does; at success "files": "enabled"
  When that enabling fails
  Then "wanted" is cleared, and Files shows the question with the reason; Enable tries again

Scenario: AC-83 edge — Not now, remove, later
  When the user clicks Not now
  Then "files": "declined" is recorded and the question is not asked again; Files' menu ("Browse files of devbox…") or "terminum hosts enable devbox" enables it any time
  When the user picks "Remove helper from devbox" or runs "terminum hosts files-remove devbox"
  Then the connection closes, "bin/terminumd-agent*" and "logs/agent.log*" are removed from the host's ~/.terminum (the folder when empty; a Mac host keeps its own install), and "files" is cleared
  And an enabled host asks nothing later, also after an upgrade (the agent is updated, AC-84)

Scenario: AC-83 failure — the host cannot take it
  When ssh fails, the platform has no agent in the package, the copy or the probe times out
  Then Files shows ssh's message or "no helper for <system>", nothing is recorded as enabled, and Enable can be tried again; concurrent enables, removes and updates keep every other host's record
```

### AC-84 — Helpers follow the Mac's build [SHOULD / P1]

```gherkin
Scenario: AC-84 happy path — after an upgrade
  Given devbox's files are enabled and its helper is of another build
  When terminumd starts after an upgrade, or devbox attaches
  Then within 30 s, without the user opening Files, terminumd copies the helper of its build over the host's ssh and connects to it; Files on a devbox pane says "updating the helper on devbox…", then a toast "Helper on devbox updated to <build>" and "(helper updated to <build>)" in the header for 60 s

Scenario: AC-84 edge — the same build, two Macs
  Then nothing is copied and no message shows when the helper is current; two Macs of different builds each run their own build's helper on one host (named by agent id) and neither copies again on every connect

Scenario: AC-84 failure — the update fails
  When the copy fails (ssh refused, disk full)
  Then Files says "Could not update the helper on devbox: <reason>", the host's files stay down, and terminumd tries again after 10 minutes or at its next start
```

### AC-85 — Paste and upload to a host [SHOULD / P1]

```gherkin
Scenario: AC-85 happy path — an image into a remote pane
  Given the page shows a devbox pane whose files are enabled
  When the user pastes an image
  Then terminumd writes it through the agent to "~/.terminum/tmp/<utc>-<8 hex>.png" on devbox (0700, 0600) and the page pastes that path quoted, no Enter (AC 36)

Scenario: AC-85 failure — not enabled or too slow
  When devbox's files are not enabled
  Then nothing is typed and the toast says "Enable the files of devbox in Files to paste there"
  When the agent does not take the bytes within 30 s plus 1 s per 256 KB
  Then nothing is typed, the toast says why, and no partial file is left under the name
```

### AC-86 — Builds and releases [SHOULD / P1]

```gherkin
Scenario: AC-86 happy path — build on the Mac and on the runner
  When the maintainer runs "make agents"
  Then the pinned toolchain (zig, cargo-zigbuild in ./.toolchain) builds terminumd for aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-musl, aarch64-unknown-linux-musl (static Linux builds); the same script runs read-only on the owner's self-hosted runner for the Linux pair on a tag
  And the package "terminum-macos.tar.gz" holds terminumd for the four targets (the Mac's chosen at install), the installer and the command

Scenario: AC-86 security — signed releases, a public repository
  When the maintainer runs "make release TAG=vX.Y.Z" (only on their request, CLAUDE.md §10)
  Then it refuses unless the tree is clean, the version is above every v* tag and HEAD is origin/main; builds with "npm ci" and "cargo --locked"; signs SHA256SUMS through ssh-agent with the release key (loaded 2 minutes, passphrase from the login Keychain, never prompting); only then tags, pushes the tag and publishes a draft once complete; release notes come from "docs/releases/vX.Y.Z.md" (refused when empty, unreadable or a symlink out of the tree)
  And workflows run only on pushes to main, tags and manual dispatch, read-only, actions pinned; fork pull requests need approval; install.sh and "terminum upgrade" verify the signature with the key they carry before the checksum, and an older signed manifest never replaces a newer install

Scenario: AC-86 failure — a build or check fails
  Then nothing public is made; the command exits non-zero with the step that failed
```

### AC-87 — Hosts are ssh destinations [SHOULD / P1]

```gherkin
Scenario: AC-87 happy path — the command and the file
  When the user runs "terminum hosts"
  Then it lists each host: name, destination, options, "attached"/"reconnecting"/"off", files enabled or not, helper build; "hosts add", "hosts remove <name>", "hosts enable|files-remove <name>", "hosts off|on <name>" (detach without removing) change settings.json and take effect within 2 s

Scenario: AC-87 edge — same name, different hosts
  Given "vm1" and "vm2" both report the host name "ubuntu"
  Then they are two hosts with their own records, agents and keys ("tmux:vm1:…", "tmux:vm2:…"); "ubuntu" only labels them
  When a host's ssh recipe changes, is switched off or becomes invalid
  Then its old helper and coding-agent link stop and its Files registration is removed before another connection can use that name; the replacement probes its own home again

Scenario: AC-87 edge — settings.json edited under an attached host
  Given "devbox" is attached
  When the user edits its recipe in settings.json so that it is invalid ("-o ProxyCommand=x")
  Then its link stays, its group says "settings.json: option not allowed: -o; still connected with the previous recipe" under its name, and typing reaches it
  When the new recipe is valid but different ("-p 2223", "alex@refused.example")
  Then terminumd tries it first (`ssh … tmux -V`, BatchMode, 10 s) with the old link kept ("trying the new recipe; still connected with the previous one"); once it answers the host moves to it (its old helper and agents' link stop first, as above); if it fails the group says "the new recipe failed: <ssh's last line>; still connected with the previous one (terminum hosts retry devbox)", the same recipe is not tried again until Retry or another edit
  And a host that is not attached (connecting, reconnecting, failed) takes a new recipe at once, as a fix of it should; `hosts add`, `remove`, `off` and `on` act at once as before

Scenario: AC-87 failure — a bad record
  Given settings.json names a host with an option not allowed or a destination with control characters
  Then the host is not started, the list shows it with "settings.json: option not allowed: -o" and the other hosts run
```

### AC-99 — Coding agents' progress on a host [SHOULD / P1]

```gherkin
Scenario: AC-99 edge — overlapping helper connections
  Given a replacement helper publishes its Mac link while an old helper exits
  Then publication and conditional cleanup are serialized across helper processes; the old cleanup never removes the replacement link

Scenario: AC-99 happy path — enable
  Given devbox is attached
  When the user picks "Coding agents' progress on devbox…" in the host menu, or runs "terminum hosts agents devbox on"
  Then terminumd copies or updates the helper as for files (AC 83, shared code), links "~/.terminum/bin/terminum-host" to it, creates "~/.terminum/run" (0700), and over ssh in "exec /bin/sh -c '…'" finds claude (known paths, then "$SHELL -lc", then "$SHELL -ic", each without input, 10 s, output between a random marker) and CODEX_HOME
  And runs "~/.terminum/bin/terminum-host hooks install --host-mode" there, with hook and MCP entries calling terminum-host by absolute path; the host's own "~/.terminum/bin/terminum" is never touched
  And records "agents": "enabled" for devbox and shows one result line per agent: "installed", "not found", "needs Codex trust (/hooks)", "uses devbox's own terminum", "refused: a terminum entry of another command", or "failed: <why>"
  And nothing is installed in devbox's ~/.claude or ~/.codex before that action; Add host has no tick for it in v1
  And the whole action runs over the ssh recipe read when it began; when devbox's record changes or goes meanwhile, the install is not started after the helper copy, nothing is saved and it says "the host's record changed meanwhile; nothing was saved, try again"

  And "terminum hosts agents devbox on" first says "Enabling coding agents' progress on devbox… (copying the helper and installing)" and waits up to 5 minutes for the result lines (other host commands: 5 s)

Scenario: AC-99 happy path — the link
  Given devbox's agents are enabled and its files not
  Then terminumd runs the helper's ssh (AC 82) for agents alone with "-o ControlPath=none" and "-R <devbox's $HOME>/.terminum/run/m-<mac id>-<rand8>.sock:<app>/links/<host id>.sock", registers no Files for devbox, and Files still offers Enable
  And the link counts as up only after the helper's ready line; the helper checks "run" (the user's, 0700, not a symlink) and the forwarded socket (a socket of the user's), points "run/mac-<mac id>.sock" at it atomically, and removes only its own Mac's sockets that refuse a connection
  And with files enabled too, the same ssh carries both: no extra connection

Scenario: AC-99 happy path — events and MCP from a host pane
  Given Claude Code runs in a pane of devbox's "terminum" session
  When its hooks fire and it calls update_session_plan through "terminum-host mcp"
  Then the hook posts to the host's own terminumd socket and to every "run/mac-*.sock" in parallel within 150 ms, each event with TMUX's full socket path
  And the Mac's terminumd takes the host from the listener the event came in on, matches the socket path to devbox's "#{socket_path}", and the pane's row shows the status and percent and the Plan pane its plan and notes, as for a local pane (AC 97, AC 98)
  And a prompt the agent was sent is kept under devbox, never under the Mac ("local")

Scenario: AC-99 failure — the host refuses the forward
  Given devbox's sshd refuses "-R" alone ("AllowStreamLocalForwarding local")
  When the helper says "link: refused"
  Then the same ssh keeps running without the link (it asked for "-R" with "ExitOnForwardFailure=no"), devbox's files keep working, the host menu and "terminum hosts" say "forwarding refused by devbox's sshd (AllowTcpForwarding/AllowStreamLocalForwarding/DisableForwarding, or Dropbear)", and the forward is tried again after 10 minutes (that ssh ends then and connects anew, even while files keep it busy), then after 20, 40, 80 and 160 minutes while the host keeps refusing; once the link is up the next refusal starts at 10 minutes again
  When devbox's sshd has "DisableForwarding yes" or "AllowStreamLocalForwarding no", or devbox runs Dropbear
  Then the Files tunnel's own "-L" is refused too: its trial request through the forward gets no answer, and devbox's Files say the same "forwarding refused by devbox's sshd (…)" and "Files cannot reach the helper": a limit of the ssh server that terminumd cannot work around (AS-84 a)

Scenario: AC-99 failure — no ready line
  Given devbox's account runs a ForceCommand, a restricted shell, or a login file that ends the session
  Then no ready line comes, the link is not up, and the host menu shows the last lines of the ssh's stdout and stderr; nothing claims events reach the Mac

Scenario: AC-99 edge — two Macs on one host
  Given two Macs, each with devbox's agents enabled
  Then each has its own "m-<mac id>-*.sock" and "mac-<mac id>.sock"; a hook posts to both; neither removes the other's live socket; one stopped leaves the other's events flowing
  And a refresh after AC 84 reinstalls only when the plugin's files, ".mcp.json" or the hook command differ, never because the two builds differ

Scenario: AC-99 edge — a TMUX_TMPDIR server also named "default"
  Given a user's own tmux on devbox with TMUX_TMPDIR=/x, so its socket is also named "default"
  When an agent in its pane %3 posts an event
  Then its socket path differs from devbox's "#{socket_path}", the event answers 204 (MCP: NOT_IN_PANE), and "tmux:devbox:default:%3" is untouched

Scenario: AC-99 edge — link loss
  When devbox's link drops
  Then devbox's hook state is dropped, its rows fall back to the screen reading (AC 90, AC 91) instead of a "working" kept forever, and the link reconnects with the helper's back-off; events while down are lost

Scenario: AC-99 edge — upgrade
  When AC 84 updates devbox's helper
  Then "terminum-host hooks install --host-mode --refresh" runs there and refreshes by content; a current install changes nothing

Scenario: AC-99 edge — disable or remove the host
  When the user picks the menu item again and turns it off, runs "terminum hosts agents devbox off", or removes devbox
  Then "agents" is cleared and the link stops first ("<app>/links/<host id>.sock" removed: no event from devbox is taken any more), then "hooks remove --host-mode" runs on devbox within 90 s and removes only entries whose command is terminum-host, and the helper is removed only when files are off; a failure there says "Turned off here; could not take terminum's hooks out of devbox: <why>"

Scenario: AC-99 edge — on and off asked while the other runs
  Given "terminum hosts agents devbox on" is still installing (up to 10 minutes)
  When the user turns agents off for devbox (the menu or the CLI)
  Then the on is stopped at once and says "a later agents on or off for this host took over; this one was stopped": its ssh is ended, and its script on devbox with it (the script watches its ssh's input and ends its process group when it closes, as sshd leaves a command running); only then the off runs as above: the on saves nothing after it and installs nothing after its removal
  And one on or off runs at a time per host, the last asked winning: one still waiting when a later one is asked never begins; other hosts' run beside it; removing devbox stops one running and takes the entries out in its turn; the hooks' refresh after a helper update waits for the one running and is stopped by a later one

Scenario: AC-99 edge — a host with its own terminum
  Given devbox runs its own terminum whose hooks are installed
  Then the host-mode install skips each agent with "uses devbox's own terminum" and changes nothing; its hooks reach this Mac through the link once that terminum is of this build

Scenario: AC-99 limits
  Then only panes of devbox's default-named tmux server at its default socket path are served; panes of "tmux -L other", and agents under sudo, su, a container or a nested ssh post nothing the Mac takes
  And when "<devbox's $HOME>/.terminum/run/m-<mac id>-<rand8>.sock" exceeds 100 bytes, agents are refused with "devbox's home path is too long for a socket" and nothing is installed
```

## 6. Flow and sequence diagrams

### A host's two connections (AC-81, AC-82)

```mermaid
flowchart LR
  D["terminumd"] -- "ssh … -- tmux -C attach" --> C["control client on devbox"]
  C --> S["tmux server on devbox"]
  D -- "ssh -L devbox.sock -- terminumd-agent" --> A["agent on devbox"]
  A --> F["files on devbox"]
  P["Page"] -- "X-TM-Host: devbox" --> D
```

Terminals and files are separate ssh processes: the control client exists for every attached host; the agent only once files are enabled (AC-83).

### Reconnect (AC-81)

```mermaid
stateDiagram-v2
  [*] --> attached
  attached --> reconnecting: ssh exits
  reconnecting --> attached: %begin seen
  reconnecting --> reconnecting: back-off 1,2,4…30 s
  reconnecting --> failed: ssh error before %begin
  failed --> reconnecting: back-off or Retry now
  attached --> off: hosts off
  off --> attached: hosts on
```

A shown pane keeps its last screen while reconnecting; typing is refused with the reason.

## 7. Current behavior and gaps

| AC | Today (iterm-enhancer 0.42) | Gap | Evidence |
|---|---|---|---|
| AC-81 | iTerm2 is the -CC client through a gateway session; the bridge sniffs the ssh argv. | terminumd runs ssh and tmux -C itself; a hosts record. | `iterm-enhancer/bridge/fbbridge/resolve.py:18`, `tmuxwatch.py:1` |
| AC-82 | fbd proxies to the agent over a forwarded socket. | Moved into terminumd. | `iterm-enhancer/fbd/src/remote.rs:1`, `fbd/src/agent.rs:1` |
| AC-83 | Enable from the panel; `agentctl.py`. | Port to Rust in terminumd. | `iterm-enhancer/bridge/fbbridge/agentctl.py:1`, `ui/src/host-offer.ts:1` |
| AC-84 | Sweep every 30 s in the bridge. | Port. | `iterm-enhancer/bridge/fbbridge/remote.py:1` |
| AC-85 | `paste.py` saves through a remote script over ssh. | Through the agent's API. | `iterm-enhancer/bridge/fbbridge/web/paste.py:85` |
| AC-86 | `scripts/agents.py`, `release.py`, the runner workflows. | Rename. | `iterm-enhancer/scripts/agents.py:1`, `release.py:1` |
| AC-87 | Hosts keyed by ssh args in `agents.json`. | `settings.json` hosts; a CLI. | `iterm-enhancer/bridge/fbbridge/hosts.py:1` |
| AC-99 | terminumd drops a host's hook and MCP events: `/hook` and `/agent` take only local panes by socket name; a hook prompt is kept under "local". | A link per host (`-R` on the helper's ssh), a router with limits, socket-path matching, host-mode install, the host passed to `sent::keep`. | `terminumd/src/sock.rs:113`, `sock.rs:132`, `terminumd/src/agentapi.rs:34` |
| AC-82 | No ssh of terminumd passes `RemoteCommand=none` (OpenSSH before 7.6 refuses the option); §9 Security says so: a `RemoteCommand` in the user's ssh config makes ssh refuse to start. | — | `terminumd/src/hosts/mod.rs:62` |
| AC-83 | Noticed, not fixed: the probe reads the first two words of stdout, so a host's rc file that prints (bash sources `.bashrc` for sshd commands on Debian) breaks the `uname -sm` parse. | Frame the probe's output with a random marker. | `terminumd/src/hosts/tunnel.rs:124` |
| AC-82, AC-83 | Fixed in 0.8.7 (issue #8: zsh's nomatch): every remote script runs as `exec /bin/sh -c '…'` (`hosts/recipe.rs` `with_command`). | rbash and ForceCommand stay unsupported. | `terminumd/src/hosts/tunnel.rs:137` |

## 8. Recommendation and ownership

**Recommended approach.** terminumd owns both ssh processes per host (control client and agent tunnel), both from the same allowlisted recipe; the agent is terminumd itself in agent mode; the page needs no notion of a gateway. Identity is the record's name; the recipe is checked before any shell sees it.

**Rejected alternative.** One ssh with the agent multiplexed over the control channel — rejected because tmux's control protocol is line-based text and file bodies would need their own framing; two ssh processes with ControlMaster share one connection anyway.

| Part | Owner |
|---|---|
| terminumd hosts, control clients over ssh, agent tunnel | Claude |
| Builds, runner, releases | Claude (maintainer runs releases) |
| Acceptance on the owner's hosts | Owner |

## 9. Technical details and specifications

### Interfaces

`terminum hosts add [<name>] '<destination and options>' [--files]`, `hosts remove|enable|files-remove|off|on <name>`, `hosts` (list). Page messages: `addhost {name, recipe, files}` (an empty name: the destination's host; answered `hostadded {name}` or `error`), `hostaction {name, action}`; the layout carries `host` per group and `state: attached|reconnecting|failed|off`, `files: enabled|declined|wanted|null`.

Control client command: `ssh -T -o BatchMode=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=2 <options> -- <destination> '<remote command>'` where the remote command is `tmux -u -C new-session -A -f ignore-size -s terminum`; `refresh-client -f pause-after=2` after attach.

### Data formats

```json
{"hosts": {"devbox": {"ssh": ["-p", "2222", "alex@devbox.example"], "files": "enabled",
  "helper": "a1b2c3d4e5f6", "theme": "remote"}}}
```

### Configuration

| Key | Default | Example | Effect |
|---|---|---|---|
| `hosts.<name>.ssh` | — | `["-J", "bastion", "alex@devbox.example"]` | recipe (allowlist) |
| `hosts.<name>.attach` | `[]` | `["work"]` | sessions of the host's tmux attached by name (AC 89), each over its own ssh connection |
| `hosts.<name>.files` | `null` | `enabled` | files helper state: `enabled`, `declined`, `wanted` (ticked in Add host, until set up) or `null` |
| `TM_SSH` | `ssh` | `/opt/homebrew/bin/ssh` | the ssh binary |

### Performance

| Operation | Target | Measured by |
|---|---|---|
| Host attach to list | < 5 s | `scripts/e2e_remote.py` |
| Key echo over a LAN host | p50 < 60 ms | `scripts/bench_echo.py --host` |
| Remote file open | < 500 ms | `scripts/e2e_remote.py` |

### Constraints and limits

| Limit | Value | When exceeded |
|---|---|---|
| Hosts | 32 | "At most 32 hosts" |
| Reconnect back-off | 1, 2, 4 … 30 s | — |
| ssh connections per host | 1 + one per attached session (AC 89) + one helper ssh while files or agents are enabled (agents' `-R` rides on it, AC-99) | sessions start one at a time, back-off 1 … 30 s each |
| Agents link (AC-99) | the host's socket path ≤ 100 bytes; 64 KB a request; 50 requests a second in a burst, 20 sustained, per link; 16 connections at once, one request each, its head within 5 s and its body and answer within 5 s; a disable ends its open connections | agents refused with why; 413; 429 (never queued); over 16 closed unread; 408 or closed |
| Refused forward (AC-99) | retried with `-R` after 10 min, then 20, 40, 80, 160 min while refused (that ssh reconnects: files pause a moment) | files keep running without it |
| Agent answers | 64 MB; 30 s idle, 120 s total | request fails, connection freed |
| Helper copy (enable, update, agents on) | 300 s | failure shown, retry later |
| Helper removal (files remove; agents off with files off) | 60 s | failure shown |
| Agents off (AC-99) | saved here first, then the host's hooks removed within 90 s | "Turned off here; could not take terminum's hooks out of devbox: <why>" |
| A host's answers to terminumd's ssh (probe, install, agents) | 1 MB of stdout, the last 4 KB of stderr, a helper's ready line 4 KB | the step fails with ssh's last words |
| Network loss | ~30 s (ServerAliveInterval 15 × ServerAliveCountMax 2) on the control client; ~45 s on the helper's ssh (ServerAliveInterval 15, ssh's default count of 3) | reconnect with back-off |
| `terminum hosts <action>` wait | 5 s + enable 360 s, agents-on 630 s, agents-off 150 s, files-remove 60 s | "no answer from terminumd to /hosts/<action> within <n> s; it may still be working (see: terminum hosts)" |

### Security

The recipe allowlist (no `-o`, no `-F`, no destination starting with `-`, no remote command but tmux or the agent, each shell-quoted for the host; `BatchMode=yes`; the remote command is always on ssh's command line, so a `RemoteCommand` of the user's ssh config makes ssh refuse to start, never run, and `RemoteCommand=none` is not passed: ssh older than OpenSSH 7.6 refuses that option); a host is its record, never a name it reports; the agent's token on stdin; on a host terminumd ends only the user's orphaned tmux control clients (AC-81: listed by that tmux with `ignore-size` and still a tmux process, or on Linux named `tmux: client` with terminum's command line; a parent that is init or a shell of init's; never the server; one with `ignore-size` that init runs on purpose is taken for one, an accepted risk); answers untrusted; nothing copied to a host before Enable; releases signed on the Mac only; a host's other sessions are never typed into or sized until the user attaches them (AC 89), attached by session id with `-E` (no name in the host's shell, no environment copied into them). Denied: `terminum hosts add x 'alex@h -o ProxyCommand=…'` → `option not allowed: -o`.

Agents' link (AC-99): nothing is written to a host's `~/.claude` or `~/.codex` before the user turns agents on, and remove takes off only entries whose command is terminum-host. A process on the host (the user, or root) can, only for that host's rows: post status, plan and prompt events for panes of the host's default server, write and read their plans and notes, and receive reminder text; it cannot reach other routes, local or other hosts' keys, or the Mac's files (a note's file is shown as text). Other users of the host cannot connect: sshd binds the socket 0600 in a 0700 folder; root can, an accepted risk. The Mac-side socket `<app>/links/<host id>.sock` (0600, folder 0700) has two routes only, `POST /hook` and `POST /agent`, with the limits of §9 Constraints; the host is the listener's, never a name in the body; the body's socket path must equal that server's `#{socket_path}`; a prompt is kept under that host. The helper, not shell glue, checks `~/.terminum/run` (the user's, 0700, not a symlink) and the socket's owner before linking, and the hook checks the folder the same way before connecting. A host's notes shown in a project mixing local panes carry the host's name.

### Compatibility

`agents.json` of iterm-enhancer is imported by `terminum import` as hosts with `files: enabled`; tmux keys `tmux-remote:<dest>:…` become `tmux:<name>:…`.

### Logging and observability

```text
2026-10-15T10:01:02.300Z level=info event=host.attached host=devbox sessions=1 panes=8 ms=1840
2026-10-15T10:07:44.010Z level=warn event=host.lost host=devbox reason="ssh exited 255" retry_s=2
```

| Metric | Type | Alert when |
|---|---|---|
| `hosts_attached` | gauge | fewer than configured for 5 min |
| `host_reconnects` | counter | > 10 an hour |

## 10. Delivery stages

| Stage | Cases | What a user can do once it ships | Effort |
|---|---|---|---|
| Stage 1 (fast lane) | AC-81 | Add a host and work in its tmux sessions from every client; survive drops | — |
| Stage 2 | AC-82, AC-83, AC-87 | Browse and edit a host's files after one click; manage hosts | — |
| Stage 3 | AC-84, AC-85, AC-86 | Helpers follow upgrades; paste to a host; releases for four platforms | — |
| Stage 4 | AC-99 | See a host's coding agents' status, plans and notes after one action | — |

## 11. Checklist with Definition of Done

- [x] Full-tree review (2026-10-31, uncommitted tree on `beea7ee`): `scripts/e2e_remote.py` passes 32/32 against a private daemon and local fake SSH. The unchanged `scripts/e2e_remote_agents.py` passes 22/22 three times after the HTTP framing fix (complete replies had timed out waiting for EOF). `scripts/e2e_review.py host` verifies off, changed recipe and invalid-record cleanup; `scripts/e2e_typing_review.py` verifies equal-number local/remote panes keep queued input separate. CI source selection and `ci_mac.sh` are exercised through synthetic repositories and stub SSH/rsync; no real remote transfer or credential read was performed. See `docs/reviews/2026-10-31-full-code-review.html` for full results and limits.

Evidence of 2026-10-15: `scripts/e2e_remote.py` runs a private terminumd whose ssh is `scripts/fake_ssh.py` (this Mac plays the host with a private tmux server and a fake home; no sshd here) → 18/18.

- [x] AC-81 — `scripts/e2e_remote.py` (add, list within 5 s, type, a dropped link reconnecting, typing after, a failing ssh's words) → pass; `scripts/e2e_remote_sshd.py` against a Linux container (Alpine, OpenSSH, tmux 3.4) over real ssh: the session listed in 0.1 s, typing, a killed sshd session reconnecting → pass.
- [x] AC-81 orphans (2026-10-21) — `scripts/e2e_hostattach.py` through `scripts/fake_ssh.py` (macOS: tmux's list): on reconnect, orphaned control clients of terminum's (init's child, a shell's of init's, one of another command line that tmux lists with ignore-size) are ended and the log names exactly them; one of another shape and a live one of terminum's shape under a living parent are left; the answer option is removed; terminumd's new clients type on; `scripts/e2e_remote_sshd.py` (Alpine, OpenSSH 9.7, tmux 3.4, procps): a client planted under a shell of init's is ended after the sshd session is killed and logged as `ended=[<its pid>]`, the server started by terminum's command keeps its pid (a first version that matched `ps` by command line alone ended it: caught here, never installed); `cargo test tmux::orphans` (the script run by /bin/sh against a stand-in `ps`: ended only init's orphan, one under a shell of init's and a listed one whose command line tmux rewrote; left the server, clients under sshd, ssh or a living shell, a listed pid now another program, a line a backslash ends, a pid not a number; the same under dash and busybox ash in containers); dry runs on a Linux host (tmux 3.4, dash, procps): the first version chose exactly its 8 clients of parent init and left the 2 under sshd, the last chose none with only live clients and the server there → pass.
- [x] AC-82 — `cargo test remote::` and `scripts/e2e_remote.py` (listing and reading through the agent, Finder refused) → pass.
- [x] AC-83 — `scripts/e2e_remote.py` (enable copies the helper, remove takes it off) → pass; `scripts/e2e_remote_sshd.py`: the static linux-aarch64 helper copied, the host's home listed and a file read through it, removed → pass.
- [x] AC-84 — `scripts/e2e_remote_sshd.py`: a helper answering another agent id is replaced by this build's at terminumd's next start (1.1 s), the panel's state says `updated` with the build, the host's record names the helper → pass. The 10-minute retry after a failed copy is not exercised.
- [x] AC-85 — `scripts/e2e_remote.py` (a pasted image saved on the host) → pass.
- [ ] AC-86 — `make package` builds macOS arm64 and x86_64; the Linux pair builds with the pinned toolchain (`scripts/agents.py build linux-aarch64 linux-x86_64`, static musl ELF); `scripts/tests/test_release.py` covers the refusals.
- [x] AC-87 — `cargo test hosts::` (allowlist, names) and `scripts/e2e_remote.py` (refusal, record, removal) → pass.
- [x] AC-81 orphans 0.7.1 (2026-10-21) — `scripts/e2e_remote_sshd.py` with procps and with `TM_SSHD_BUSYBOX=1` (busybox's ps only): the planted orphan is ended and logged, the server keeps its pid, 13/13 each; the script run in an Alpine container with busybox against a real orphan: `KILL` of exactly it; `cargo test tmux::orphans` (a BSD client of no terminal ended, one with a terminal left; a ps that fails gives way to /proc and, with none, ends nothing) → pass. A FreeBSD or OpenBSD host itself was not tried: its ps is a stand-in in the test.
- [x] 0.7.2 (2026-10-24) — AC-82 `cargo test remote` (a hostile agent: terminumd's type and sandbox policy, JSON for a listing and an error) and `scripts/e2e_remote.py` 31/31 (a host's PNG and SVG through the header: typed by name, the sandbox policy, the same bytes; `?t=` refused) → pass.
- [ ] AC-99 (2026-10-26) — `cargo test --locked` 632 + 1 pass, `cargo clippy --all-targets -D warnings` clean (link router: other routes 404, body and rate limits, host from the listener; socket-path match with the TMUX_TMPDIR collision; sinks' order and folder checks: foreign owner, symlinked folder, own dead socket removed, another Mac's live one kept; host-mode install on a fake home: paths, refusal, idempotence, content refresh; ssh argv with `-R` and `ControlPath=none`; refusal fallback; the helper's id read from the last line of a printing login shell) → pass. `python3 scripts/e2e_remote_agents.py` against the fake host (enable, a hook and `terminum-host mcp` from a host pane reach the row's percent and the Plan pane, a foreign tmux refused, a refused forward's message, disable removes), the hooks run inside one long-lived agent process as an agent's are: 20/20 in 6 runs in a row (an earlier version ran each hook as the shell's foreground program, so the pane's program changed and AC 97 archived the plan in about half the runs: the test's fault, no terminumd change) → pass. `python3 scripts/e2e_remote_sshd_agents.py alpine|debian` over a real OpenSSH sshd (Alpine 3.20, OpenSSH 9.7p1; debian:12-slim, OpenSSH 9.2p1 with a `.bashrc` that prints), the static linux-aarch64 helper of this build: the host attaches, enabling answers one line per agent (claude, a stub under `~/.nvm`, and codex installed), the link comes up, `terminum-host` is the helper, `~/.terminum/run` 0700 with `mac-<id>.sock` → its `m-<id>-*.sock`, the plugin's synchronous PostToolBatch entry and Codex's block name terminum-host, `terminum-host hook claude` in the host's pane exits 0 silently, the host row's plan 1 of 3, MCP's initialize is terminum's with doing, ETA and a note kept and shown, a refused forward (a second sshd with `AllowStreamLocalForwarding local`) says refused and keeps Files listing, the hook exits 0 with no link, a lost link comes back, drops the host's hook state, sweeps the old socket and events flow again (2 of 3), disabling removes terminum-host and the agents' entries: 19/19 each → pass. Found there: Debian's printing `.bashrc` failed every enable (files and agents) until the helper's id was read from the last line; `DisableForwarding yes` refuses Files' `-L` too (AS-84 a). Not verified: a macOS host over real sshd (Remote Login off on the Mac at hand); Dropbear; a ForceCommand user and two terminumd on one container over real sshd for AC-99; real Claude Code and Codex on a host (their hooks were seen on the Mac: Claude Code 2.1.310 updated its plan 3 s after the PostToolBatch reminder, Codex 0.164 at once after its async PostToolUse one). AS-84 verified 2026-10-26.
- [x] 0.7.3 (2026-10-26) — AC-81 `cargo test tmux::orphans` (each sweep's option its own, random) and `python3 scripts/e2e_remote.py` 32/32 → pass. Not run: `scripts/e2e_remote_sshd.py` (Docker and the Linux helper).
- [x] 0.8.3 (2026-10-29) — AC-81 `web/test/e2e_tmux.mjs` (Chromium and WebKit, 1400 px wide: the window shortened until its group's button, in the list's view, has under 100 px below it: the menu opens above the button, whole inside the window) 102/102 → pass.
- [x] 0.8.4 (2026-11-01) — AC-81 `cargo test tmux::control` (an ssh that ends silently before tmux answered: "ssh: no answer from the host"; its stderr's last line when it said one); AC-87 `scripts/e2e_remote.py` 37/37 against a private terminumd with the fake ssh: settings.json edited by hand under the attached devbox: an invalid recipe keeps the link (typing reaches it), a failing one keeps it and says ssh's words, the old link dropped then takes the new record, a recipe that answers takes over, and back. On the delivered tree (uncommitted, on ed902db): `make test` (Rust 692 + 1, Python 54, Panel unit 47, Page unit 154), Clippy `-D warnings` clean, `cd ui && node test/e2e_panel.mjs` PASS 165, `node web/test/e2e_all.mjs` PASS 1609, `node web/test/e2e_live.mjs` PASS 191, `scripts/e2e_remote.py` 37/37, `scripts/e2e_remote_agents.py` 22/22, `scripts/e2e_hooks.py` 32/32, `scripts/security_check.sh` PASS. Pragmatic review: approve with conditions, applied (a key's value holding '=' inside a word leaked up to it; a held host whose old link dropped stayed on the old recipe; `off` with a typo; Make's `:=`/`?=`; `=>` and `::` not hidden; signal handlers before the child; `alt` decided by terminumd).
- [x] 0.8.5 (2026-11-03) — audit fixes: AC-99 `cargo test hosts::agents` (a record changed during the install: nothing saved, the newer kept; off saved and the link stopped before the host's removal); AC-85 `cargo test hosts::link` (the retry scheduled only for a refused wanted link, backed off 10…160 min, a paused-clock scheduler ends a healthy connection at its time); `cargo test hosts::bounded` (answer, stderr tail and ready line bounds, only its own ssh ended). On the delivered tree (uncommitted, on 7e5c220): `make test` (Rust 725 + 1, Python 69, Panel unit 52, Page unit 175), clippy `-D warnings`, `ui/test/e2e_panel.mjs` 171/171, `web/test/e2e_live.mjs` 233/233, `web/test/e2e_all.mjs` 1691/1691 in Chromium and WebKit, `scripts/security_check.sh`. Pragmatic review: must-fix (a handler cut part-way on close) and should-fix (lanes' order, the -R back-off, the sign-in wait, toasts once and the lost write by pane, the CLI budgets) applied.
- [x] 0.8.6 (2026-11-03) — AC-99 `cargo test hosts::agents` (an off during a long on stops it before it runs; each later one stops the one running; one waiting when a later one is asked never begins; another host's turn runs beside it; a refresh waits for the running one and a later off stops it) and `cargo test hosts::agentsinstall` (the script under /bin/sh as sshd starts it: its ssh's input closed, its slow step and what follows never run; open, it runs to its end); `scripts/e2e_remote_agents.py` 22/22, `scripts/e2e_remote_sshd_agents.py` 19/19 over a real sshd (Alpine, Docker), `scripts/e2e_remote.py` 37/37. On the delivered tree (uncommitted, on 4659553): `make check` (Rust 733 + 1, Python 69, Panel unit 52, Page unit 175, clippy `-D warnings`, `ui/test/e2e_panel.mjs` 171/171, `web/test/e2e_live.mjs` 233/233, `web/test/e2e_all.mjs` 1707/1707 in Chromium and WebKit, `scripts/security_check.sh`).
- [x] 0.8.7 (2026-11-03) — AC-83 `cargo test hosts::tunnel` (what `recipe::with_command`, now the one way any script reaches a host (the agents' scripts too), hands ssh is `exec /bin/sh -c '…'`; run by `zsh -c` as sshd runs a zsh login shell's command, the helper's removal with no old helper to match ends 0 and takes the folders; the bare script in zsh says "no matches found", as issue #8 reported); `scripts/e2e_remote_sshd.py` 13/13 and `scripts/e2e_remote_sshd_agents.py` 19/19 over a real sshd (the helper copied, started and removed through the wrapper), `scripts/e2e_remote.py` 37/37, `scripts/e2e_remote_agents.py` 22/22; `make check` (Rust 734 + 1, the Panel, live and Page browser checks, clippy, security). Not verified: a real macOS host over ssh (the reporter's).
- [x] 0.8.8 (2026-11-03) — AC-81 `cargo test an_ssh_link_that_never_reached_tmux_says_so` (an ssh that closes stdout and writes its reason to stderr 0.3 s later is reported with that reason; the old code said "ssh: no answer from the host", as CI's run of v0.30.0 did); `cargo clippy --all-targets --target aarch64-unknown-linux-musl -D warnings` clean (v0.30.1's Linux failure: a cast `mode_t` → `u32` that is one type on Linux); `make check`.
