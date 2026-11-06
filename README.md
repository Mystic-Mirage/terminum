# terminum

**The self-hosted, tmux-native browser IDE for running dozens of coding agents at once, from your
desk or your phone.**

[![terminum on a monitor, a laptop and a phone: one Claude Code session on all three](docs/images/terminum-devices.jpg)](https://terminum.app/)

No app, no account, no vendor relay: terminum runs on your Mac or Linux server, and your devices
reach it over your own network.

## Features

### Local or remote: no difference

- **Same agents:** Claude Code and Codex on your Mac, your Linux server or any ssh host, in one
  session list.
- **Same files:** browse and edit files on any host, no sftp client needed.
- **Same viewer:** JSON as a tree, CSV and TSV as tables; PDF, audio, video and MIDI play in place.
- **Same clipboard:** paste screenshots, drop files (and folders from a Mac's Finder); they land on
  the session's host.
- **Zero server setup:** only tmux needed; reconnects by itself.

### Keep track of every agent

- **Plan and ETA:** the agent keeps its plan via MCP; the list shows its percent done.
- **Agent notes:** findings and follow-ups saved per session, never lost in the scroll.
- **Cross-session alerts:** an agent waiting for you shows up in every session.
- **Status at a glance:** working, waiting, done, subagents.

These come with `terminum hooks install` (see [the guide](docs/USAGE.md)).

### Talk to agents precisely

- **Quote & Ask:** select text, comment, sent as a numbered quote.
- **Bookmarks and reminders:** mark a line now, come back to it later.
- **Rendered chat:** Markdown, tables and Mermaid drawn in the output.

### Any screen, nothing lost

- **Phone, iPad, desktop:** the full IDE in the browser, touch-ready.
- **Survives reboots:** sessions, windows and splits come back with their last 300 lines.
- **Real terminal:** tmux emulates; vim, htop and agents look native.

### Yours to bend

- **Open source, AGPL-3.0:** released almost daily, made to be forked.

## Install

### On a Mac

```bash
brew install tmux        # tmux 3.2+; macOS 14+
curl -fsSL https://github.com/extractumio/terminum/releases/latest/download/install.sh | sh
```

macOS then asks on the screen whether terminum's terminals may read Desktop, Documents,
Downloads and your cloud folders: Allow, once. Then:

```bash
echo 'export PATH="$HOME/.terminum/bin:$PATH"' >> ~/.zshrc   # the install's tip
export PATH="$HOME/.terminum/bin:$PATH"
terminum password        # set the password every browser signs in with
terminum open            # open it: http://127.0.0.1:48080/
terminum hooks install   # optional: exact states from Claude Code and Codex
```

`terminum upgrade` installs the latest signed release (`terminum rollback` goes back); the page
shows a chip when one is out.

### On a Ubuntu/Debian server

```bash
sudo apt install tmux curl openssh-client
curl -fsSL https://github.com/extractumio/terminum/releases/latest/download/install.sh | sh
echo 'export PATH="$HOME/.terminum/bin:$PATH"' >> ~/.bashrc
export PATH="$HOME/.terminum/bin:$PATH"
terminum password
terminum hooks install   # optional: exact states from Claude Code and Codex
```

terminumd runs as a systemd user service (`sudo loginctl enable-linger $USER` keeps it, and the
terminals, after you log out). Open the page from your computer through
`ssh -L 48080:127.0.0.1:48080 <server>` at `http://127.0.0.1:48080/`, or with `tailscale serve` on
the server.

### A remote host's tmux

Install tmux on the host (Linux or macOS) and make sure the machine running terminum reaches it
with `ssh`; then, there:

```bash
terminum hosts add devbox 'alex@devbox.example' --files
```

Its tmux sessions and, with `--files`, its files show on the page; terminum copies its helper
there and keeps it at terminum's version.

### Over Tailscale

To open terminum on your phone or another computer, serve it on your tailnet and sign in at
`https://<machine>.<tailnet>.ts.net/`:

```bash
tailscale serve --bg --https=443 http://127.0.0.1:48080
```

This takes the machine's tailnet name. To keep that name for another service, give terminum a name of
its own with a Tailscale Service: see [Over Tailscale](docs/USAGE.md#over-tailscale) in the guide.

## Learn more

- [The guide](docs/USAGE.md): every command, feature, remote host and setting.
- [Architecture](docs/ARCHITECTURE.md): every part on one diagram, a Mac, a Linux server and the browsers.
- [Set it up with a coding agent](skills/terminum-setup/SKILL.md): a skill Claude Code, Codex and the like follow to install, check and repair terminum for you.
- [SECURITY.md](SECURITY.md): the threat model.
- [All the docs](docs/README.md): specs, contributing, releases, and which fact lives where.

Licensed under AGPL-3.0 or a commercial license (`LICENSE`,
`LICENSE-COMMERCIAL.md`); contributions come under the agreement in `CONTRIBUTING.md`. The coding
agents' marks in `web/static/agents/` are their owners' trademarks, outside both licenses
(`NOTICE`).
