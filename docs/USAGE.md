# Using terminum

The full guide: install, commands, the page, your own tmux, other devices, remote hosts and
configuration. The overview is in the [README](../README.md).

## What it is

- **terminumd**, one Rust server on `127.0.0.1:48080`: the tmux control client of a private
  tmux server (`tmux -L terminum`, started by terminumd with `~/.terminum/tmux.conf`) and of each
  remote host over ssh; it serves the page and the Files panel and never emulates a terminal:
  screens are tmux's own grid (`capture-pane`), only the rows that changed are sent.
- **The page** (`web/static`): the session list (tmux sessions, windows, panes; coding agents'
  states), the terminal in **Wrap**, **Grid** or **Fit**, **Screen mode** for full-screen
  programs (the pane is sized to the view, keys and the mouse reach the program), widgets for
  code, Markdown, diagrams and tables in the output, folds, bookmarks, search, hot keys for
  phones, paste and upload of files.
- **The Files panel** (`ui/`): the tree of the shown pane's folder, a viewer and editor with
  highlighting and rendered Markdown (its ```mermaid fences drawn as diagrams, "Source" shows
  their text), JSON and YAML as a tree, CSV and TSV as a table (Source shows the text),
  pictures, PDFs in the browser's own viewer, videos and sounds in its player (AU and AIFF
  decoded where the browser does not play them; MIDI rendered by terminumd with the General
  MIDI SoundFont that comes with terminum, GeneralUser GS, or yours: `~/.terminum/soundfont.sf2`
  or `midi.soundfont` in settings.json), create, rename, Trash, live refresh; Esc in View hides
  it and goes back to the terminal, Esc in the tree hides it and leaves the keys there (Enter,
  Esc, ↓, Enter… looks at one file after another); on a wide window it is docked beside the
  terminal, open or closed for each session, its width set by dragging its border with the
  terminal; on a phone it is a view in its place.
- **Sizing**: the pane follows the page you use: the one that opens it, comes to the front or
  types there sizes it to its view (45×14 to 240×70 cells); a window of several panes (Claude
  Code's teammates split it) keeps its layout. Text is never scaled: a pane narrower or wider than
  your view re-flows to it, lines tmux or the program wrapped joined again (a full-screen program
  such as mc is drawn as its grid, scaled when wider). Switching sessions shows the new one at once
  at your text size.
- **Sessions outlive everything**: terminals live in tmux; terminumd runs as a launchd agent
  and comes back after a crash; after a reboot, or when a tmux server was killed, the sessions,
  windows, splits and folders are restored: terminum's own, each host's and the sessions of your
  own tmux you attached (`"restore": true`). Each session's last 300 lines are kept as well
  (`"lastLines"`, up to 2000, 0 for none): after the restart, **Show Last 300 Lines** in its row's
  menu shows how it ended (the terminal's lines; a full-screen program's own screen is not kept).
  They are kept on disk in `~/.terminum/state/lastlines/` (private, out of Time Machine) until the
  pane closes; `"lastLines": 0` deletes them.

## Install

```bash
curl -fsSL https://github.com/extractumio/terminum/releases/latest/download/install.sh | sh
```

On a Mac it needs macOS 14+ and tmux 3.2+ (`brew install tmux`); on a Ubuntu/Debian server,
systemd, tmux 3.2+, curl and openssh-client (`sudo apt install tmux curl openssh-client`).
Nothing else. The installer checks the
release's signature, installs into `~/.terminum/` (`builds/<build>`, `current`, the command
`~/.terminum/bin/terminum`, which is terminumd itself, and `daemon/terminumd`, the copy launchd
runs), loads the launchd agent `io.terminum.terminumd` and says how to set the password. launchd holds `127.0.0.1:48080` from
then on, so no other program can take the page's port while terminumd restarts or is upgraded.

During the install macOS asks on the Mac's screen, once per folder, whether programs in
terminum's terminals may read Desktop, Documents, Downloads and each cloud folder
(`~/Library/CloudStorage`: Dropbox, Google Drive, OneDrive…): terminumd opens each one itself, so the prompts come while you are
at the Mac rather than when an agent first reads a folder while you are away. Allow covers
every shell and agent in terminum's tmux and stays across upgrades, since launchd always runs
`daemon/terminumd` (signed as `io.terminum.terminumd`). The install waits 5 s for the answers
and says which folders macOS still asks about; `terminum folders` asks again and waits. A
refused folder can be allowed again in System Settings › Privacy & Security › Files and Folders.

iCloud Drive (`~/Library/Mobile Documents`) stays closed to programs in terminum's terminals, as
to any program without Full Disk Access: macOS refuses it ("Operation not permitted") and never
asks, so terminum does not ask for it, and does not need Full Disk Access for anything else.
Keep work in Documents or Desktop (also when "Desktop & Documents Folders" sync to iCloud: their
own Allow covers them) or in a cloud folder of `~/Library/CloudStorage`.

macOS judges a terminal's program by the program that started terminum's tmux server, and that
server outlives upgrades. `terminum status`, the install and `terminum folders` say when it was
started by another program (a build from before this one, a build run by hand, `tmux -L
terminum` typed in Terminal): restart it once at a quiet moment, by a reboot or `tmux -L
terminum kill-server`, which ends the programs in it; terminum makes the sessions again with
their windows, splits, folders and projects (AC-15), and the programs are started again by
hand (`claude --continue`, `codex resume --last`).

On a Linux server terminumd is the systemd user service `io.terminum.terminumd`
(`~/.config/systemd/user/`), started at once and at every login; a restart, an upgrade or an
uninstall ends terminumd alone, never the terminals. Without linger the user's services end at
logout, terminals too: `terminum status` says so, and `sudo loginctl enable-linger $USER` keeps
them from boot. The page is on the server's `127.0.0.1:48080`; reach it from your computer
with `ssh -L 48080:127.0.0.1:48080 <server>` (then `http://127.0.0.1:48080/`) or publish it on
your tailnet with `tailscale serve` on the server. Finder and default apps are a Mac's only.

From a checkout: `make install` (Rust, Node 26 and the Xcode Command Line Tools needed);
`make test` runs the unit tests.

## Use

```bash
terminum password             # the password every browser signs in with (8+ characters)
terminum open                 # the page in this Mac's browser: http://127.0.0.1:48080/
terminum status               # build, pid, port, password set, tmux servers and sessions, hosts, pages
terminum folders              # macOS asks now for the protected folders the terminals read
terminum signout              # every browser signs in again
terminum upgrade              # the latest signed release (--to vX.Y.Z); rollback; uninstall
terminum restart              # load the launchd agent again (a new port in settings.json; a port taken back)
terminum logs                 # terminumd's log (-n N, -f)
terminum hooks install        # Claude Code's and Codex's own hooks: exact states, prompts, reminders
```

Every browser, on the Mac or another device, signs in with the password; until one is set the
page says how. The page asks terminumd to prove it knows the page's sign-in before sending it,
and keeps the sign-in in its own origin's storage, never in its address or a cookie (see
[SECURITY.md](../SECURITY.md)). `tmux -L terminum attach` from any terminal shows the same sessions.

- **Under the list**: New session, Bookmarks / Reminders, Add host and Attach tmux sessions
  fold into one row of their icons with ▸ on a computer; click it and they unfold (▾) until you
  click it again; the page remembers which.
- **Quick shell**: the button in the header (or "Quick shell below" / "on the left" in the menu)
  opens a shell of its own under the shown pane, in its folder, for quick commands; drag its edge
  to size it, ⇄ moves it, × closes it. It is a tmux window marked with its pane, listed under it.
- **Coding agents**: with `terminum hooks install` Claude Code (a plugin of terminum's, added
  with `claude plugin`) and Codex (entries in `~/.codex/hooks.json`; trust them once in Codex's
  `/hooks`) tell terminum when they work, wait for an approval or an answer, stop, fail or run
  subagents; without them terminum reads their screens. `terminum hooks` says what is set up,
  `terminum hooks remove` takes terminum's hooks out. An install or upgrade of terminum refreshes them (and the
  MCP server) where you installed them, never elsewhere; `terminum uninstall` takes them out.
- **MCP server**: `terminum hooks install` also adds `terminum mcp`, terminum's MCP server, so
  the agent can keep the session's plan, progress and notes for you (AC-97, AC-98): in Claude
  Code through the same plugin (its `.mcp.json`), in Codex as one block in
  `~/.codex/config.toml` between `# >>> terminum: MCP server …` and `# <<< terminum` (the file
  backed up first; nothing else in it changes, lines of others inside the markers are kept, a
  symlinked file is written where it points). An `[mcp_servers.terminum]` of your own there, or a
  file changed while the install ran, is refused, not overwritten. Restart running Codex sessions
  to give them the server. Codex still runs terminum's hooks only once trusted in its `/hooks`;
  `terminum hooks remove` takes the block out. The server starts in every Claude Code and Codex
  session on the Mac, but outside a terminum pane it offers no tools and no instructions, so it
  costs those sessions nothing.
- **Sent history**: the speech bubble under the split shows what you sent in the pane's folder
  (its prompts and commands, kept in `~/.terminum/sessions`), newest first: ↑ older, ↓ newer;
  Jump to message shows where it was sent, Insert into chat types it again (without Enter). On a
  wide window small bars on the terminal's right edge mark where your messages (white), the
  agent's answers ended (pale blue), bookmarks (orange) and reminders (pink) stand in the
  history: press one to go there. Settings' Side marks turns them off.
- **The agent's folder**: when Claude Code works in another folder than where it started (a
  worktree), its row in the list, the header's folder and the Files tree follow it, and paths
  it prints open from there. Settings' Follow the agent's folder turns that off for this browser (`page.followAgent`
  in settings.json for every browser that did not choose).
- **Tip of the day**: on a computer the foot of the session list shows a tip, another each day;
  Next shows the next one, × closes it until tomorrow (Settings' Show today's tip brings it back), Turn off (or Settings' Tip of the day) turns
  tips off for this browser.
- **Ask**: select text in the terminal and press the blue speech bubble with a question mark (under
  the selection's orange [+]); type a question and Add to Chat puts the selection as a Markdown
  quote (`> Quote #0007`, then `> ` and each of its lines), a blank line, your question and a blank
  line into the input, without Enter; the view stays where you were reading. In a coding agent's
  input it is typed, so the agent shows it whole, unless the agent's box would then be taller
  than the pane: then it is pasted and the agent folds it.
  The number counts up per session. The text you asked about keeps a light blue wavy line down its left in
  the terminal, as a reminder's quote, on every page of that session.
- **Full-screen programs with the mouse** (Claude Code's fullscreen mode, vim with `set mouse=a`,
  mc): a drag, a double or a triple click is the program's, and it selects in its own way. When
  it copies what it selected to the terminal's clipboard (OSC 52: Claude Code does on the
  release, vim with an OSC 52 yank does on `y`), the page shows Copy, [+] and Ask by the
  selection with its text, as for a selection of yours; they go at the next click or key. Hold
  **Shift** while dragging to select in the browser instead (Safari, Chrome, Firefox): nothing
  reaches the program.
- **Reminders**: pick Reminder in a bookmark's dialog; the pale fuchsia bell comes up when an agent
  of its project ends a turn, its count filled, and the terminal's top says so for 4 seconds
  ("iQuantize: 16 reminders, 3 new"; another session's when you open it next). It opens the same window as the bookmarks' button,
  with the open reminders only, those up first: Show in terminal, Resolve (it ends it, with Undo) and
  Quote in chat. Quote in chat, on a reminder or a bookmark, and Insert into chat on a sent message
  paste it (a bookmark's note and its text) into the session's input, without Enter.
  Bookmarks and reminders made in a project's session show in every session of that project;
  made outside projects, only in their own session. Bookmarks / Reminders below the sessions lists
  them all, each with its project or session (Open all); once their project is gone (deleted, or
  its last session closed), or a session outside projects closes, they are only there, "orphaned".
- **Limits**: you keep up to 300 bookmarks and 100 open reminders (a save past them says so). Coding
  agents' records have their own: 50 bookmarks and 20 open reminders a project (or session); past
  them their oldest go, and a banner over the terminal says so at the end of that agent's turn, with
  a link to review them. A resolved reminder is deleted 30 days after you resolve it.
- **Going through them**: the bookmarks' button (its count: bookmarks and open reminders) opens a
  window with the session's bookmarks and reminders on top, top to bottom ("1 / 52" at each one's
  right), each its note and its
  text's start on one line, and the chosen one's content under them; drag the line between them to
  share the height (a double click: half). Press one, or ↑ ↓ on a computer, to show it; Show in
  terminal (Enter) goes there; Resolve (or Delete) settles one, a reminder resolved and a bookmark
  deleted, with Undo for 5 seconds; Esc closes the window. The bar over the terminal has the same
  Resolve.
- **Kept by the agent**: a coding agent hands you what must not get lost in the scroll (a final
  report, a next step, a gap it saw elsewhere, deferred work, a question) as the session's
  bookmarks, with its mark; a reminder when you must act or decide. Each is written to read without
  the chat; Open goes to where it was made, Insert into chat pastes its title and text.
- **The view** is the pane sized to the browser window you use (in front, or the phone in your
  hand), from 45×14 (an iPhone in portrait) to 240×70 cells; a pane of another width re-flows to
  the window (a full-screen program is scaled to fit). The menu has 20 color schemes (one for every browser, kept in settings.json),
  the Markdown text size and the history lines of new terminals. Option or ⌘ with ↑/↓ pages.
- **Keys of an IDE**: ⌘[ and ⌘] go back and forward through the last 20 sessions you showed,
  ⌘F searches the history, ⌘B opens or closes Files. In a coding agent's input, Backspace or
  Delete over text you selected deletes it, as in a text field.
- **The header** names the shown pane under its title: its folder (the copy button copies its full
  path), tmux's `session:window`, then what runs there.
- **The list**: a group's buttons (copy, import, [+], ⋯) show while the pointer is over its header,
  in the place of its count; each row's Clone (shown while the pointer is over the row; on a phone always)
  opens a window beside it in its folder; a row hatched with pale lines is unread (its agent finished
  a turn nobody saw); a right click, or a finger
  rested on a row and lifted, opens its menu (Clone, Add to Project, Mark as Unread or Read, Diagnostics… (what terminumd and tmux see of the pane, to copy into a bug report: check it first, it holds the pane's folder, host and title), Split Below, Right, Left,
  moves, Move to Session, Close); the header's ⋯ holds the settings. Drag a row, a
  group's or a project's header to reorder: a dashed outline shows where it lands (a finger rests
  a moment first; moving at once scrolls). Projects are named groups
  of sessions in a color of their own, with [+] for a new window in the project's folder, the same
  on every browser; a right click on a project's header, or its ⋯, opens its menu (New Window,
  Rename, moves among the projects, Ungroup, Close Project, which asks first); a tmux session's group
  has the same kind of menu (New Window, a window in that session; moves, Close Group).
  ⌘Z sends the program's undo (Claude Code, zsh, bash, fish); ⌘⇧Z redoes where it can (fish);
  ⌘X cuts the text selected in a coding agent's input box.
- **On a phone**: the floating buttons but the keyboard's two fold into ⟨…⟩ at the top right; a tap
  shows them for 3 s. They hide while your finger scrolls. A selection shows the page's Copy, [+]
  and Ask there instead of iOS's menu. On the input: a finger held still half a second and lifted
  opens Cut, Clear, Paste, Copy, Undo, Redo (what applies); a quick swipe right is →, left is Undo;
  two taps select a word. Dialogs and menus open centered.
- **A screenshot** pasted into the terminal (⌘V) is saved in `~/.terminum/tmp` (on a host, in the
  host's `~/.terminum/tmp`) and its path is pasted: Claude Code takes it as an image.
- **Files and folders dropped from Finder** on a session of this Mac, in this Mac's browser, put
  their own paths at the input, quoted, one per line in a coding agent's box, without Enter;
  nothing is uploaded, at any size (terminumd reads the drag's paths from macOS's drag pasteboard).
- **A file dropped** anywhere else (a host's session, a phone, a drag from a browser or Mail), or
  picked with Upload, is uploaded as a screenshot is and its path pasted, up to 100 MB; give a
  larger file by its path (in Finder ⌥⌘C copies it).
- **Files and folders dragged from Files** onto the terminal put their paths at the input, quoted,
  without Enter; nothing is uploaded. Drag a selected row to put every selected row's path.
- **Files dragged from Finder onto the Files tree** upload into the folder under the pointer (a
  file's row: its folder; below the rows: the root), on this Mac or the pane's host; the folder
  shows while you drag. A name already there asks Keep Both or Skip: nothing is written over.

## Plans, progress and notes

With terminum's hooks installed (`terminum hooks install`, above), a coding agent's plan shows
where it is:

- **Where**: the session's row shows "43%" right of its title over a thin track (hover: "3 of 7 ·
  updated 2 min ago"); the shown session's progress is a circle "43%" with a ring, first in the floating buttons'
  column (on a phone, tap ⟨…⟩ to see it with the others); the header has a Plan button right of ✎ with the percent ("43%"; the icon alone at 0%)
  for a session with a coding agent. The percent is
  the items completed of all, never the agent's own guess; it dims when the plan is stale.
- **The Plan pane**: press the percent on a row (it shows that session), the progress circle
  or the header's Plan button; by keyboard, Tab to the Plan button and press Enter or Space; Esc or × closes
  it. It shows the plan's title, the current step, the items, the agent's ETA ("~17:40 (agent's
  estimate, given 16:55)") and when it was updated, then how many records the agent kept for you
  (a press shows them: the bookmarks' list). The plan's text is shown as plain text.
- **"No plan: the agent has not sent one for this session."**: the agent made no plan yet. Claude
  Code makes tasks only for longer, multi-step work (or when asked: "make a plan with tasks");
  terminum's MCP tools report one too (update_session_plan). A session started before terminum's
  hooks were installed reports nothing until it is restarted.
- **Marks**: ✓ completed, ▶ in progress, ○ pending, ⚠ blocked. "partial: some updates were
  missed" means an update named an item terminum never saw; the next whole list heals it.
- **How agents feed it**: their own plan tool, read from their hooks (Claude Code's tasks and
  TodoWrite, Codex's update_plan where it has one), and terminum's MCP server `terminum mcp`
  (update_session_plan, get_session_plan, add_notes_for_user, update_notes_for_user): the
  current step, an ETA in minutes, the plan itself for an agent without a plan tool (Codex
  0.164 has none), and the records it keeps for you (Kept by the agent, above). A new agent
  session in the pane starts with no plan.
- **The reminder**: when a working agent has not updated its plan for 10–30 minutes (by the
  turn's length), terminum asks it once through the hook's answer (Claude Code: a synchronous
  PostToolBatch hook; Codex: its PostToolUse); nothing is typed into the pane, and an idle,
  waiting or finished plan is never reminded.

### When the Plan pane is empty or wrong

- **No percent at all**: terminum's hooks are not installed (`terminum hooks` says), or Codex
  does not trust them yet (open `/hooks` in Codex and trust terminum's, the changed SessionEnd
  one too); the pane then says "terminum's hooks are not running in this session".
- **The session started before the install or an upgrade**: restart it; agents read their
  plugins and `config.toml` only when they start.
- **Not a terminum pane**: an agent in another terminal, or under sudo, su, a container or a
  nested ssh, is not seen; its MCP tools answer that the session does not run in a terminum pane.
- **terminumd is down**: the tools answer "terminum is not running"; `terminum status`, then
  `terminum restart`.
- **After `terminum rollback`** the hooks and the MCP server keep working: they run
  `~/.terminum/bin/terminum`, which a rollback never moves (it stays the newest build's); an MCP
  tool the older terminumd does not know fails as a tool error, never the agent.
- **"MCP server failed" in Claude Code or Codex** after `terminum uninstall` or `terminum
  upgrade --to` a release older than 0.23.0 (an upgrade does move `~/.terminum/bin/terminum`): the
  older build has no `terminum mcp`. Run `terminum hooks remove` with the newer build first;
  after the fact, delete Codex's block between `# >>> terminum: MCP server …` and `# <<<
  terminum` in `~/.codex/config.toml` and run `terminum hooks install` with the build you kept.
- **A host's agents do not show**: see Remote hosts below.
- **Stored where**: plans in `~/.terminum/state/plans.json`, the agent's records with your
  bookmarks in `~/.terminum/state/web-bookmarks.json` (0.23's `notes.json` is imported once and kept
  as `notes.json.imported`). To clear the plans, stop terminumd first (`launchctl bootout gui/$(id -u)/io.terminum.terminumd`; on Linux
  `systemctl --user stop io.terminum.terminumd`), remove the files, then `terminum restart`;
  terminals live in tmux and are not touched.

## Your own tmux sessions

```bash
terminum tmux attach work                 # or "Attach tmux sessions" under the list: a session of plain `tmux`
terminum tmux attach work --socket dev    # one of `tmux -L dev`
terminum tmux attach work --host devbox   # one of a host's tmux (the import icon on its group)
terminum tmux                             # what is attached and whether it runs
terminum tmux detach work                 # terminum forgets it; it goes on running
```

terminum shows the session as one of its own, never closes a session there, and sizes its
windows to the page that shows them, as any pane; when no page in sight shows one, it gets
tmux's own size back. If that whole tmux server is gone (a reboot, the server
killed), terminum makes the attached sessions again with their windows and folders; a session
you close while the server runs stays closed. When you
add a host whose tmux runs other sessions, terminum offers them; later the import icon on the
host's group does. Each attached session on a host is one more ssh connection.

## A phone or another computer

terminumd listens only on `127.0.0.1:48080`. Other devices reach it through `tailscale serve`,
which gives it an HTTPS address on your tailnet; they sign in with the same password.

### Over Tailscale

Tailscale gives one name per machine, `https://<mac>.<tailnet>.ts.net`, with one service on
each port. Pick where terminum goes:

| Setup | terminum at | Steps |
| --- | --- | --- |
| Nothing else on the Mac's name | `https://<mac>.<tailnet>.ts.net/` | 1–3 below |
| Another service keeps the Mac's name (such as iterm-enhancer) | `https://terminum.<tailnet>.ts.net/` | 1 below, then the Tailscale Service |

1. Set the password, if you have not yet:

   ```bash
   terminum password
   ```

2. Serve it over HTTPS. This replaces what port 443 served: move another service first (below).

   ```bash
   tailscale serve --bg --https=443 http://127.0.0.1:48080
   ```

3. Open `https://<mac>.<tailnet>.ts.net/` on a device in your tailnet and sign in with the
   password.

Without a tagged machine for a Service, another service on the Mac's name can move to a port
of its own instead, such as iterm-enhancer on 8765:

```bash
tailscale serve --https=443 off                            # if it had the address without a port
tailscale serve --bg --https=8443 http://127.0.0.1:8765    # it moves to https://<mac>.<tailnet>.ts.net:8443
tailscale serve status                                     # :443 → 48080 (terminum), :8443 → 8765
```

Notes:

- Give `--https` every time: `tailscale serve --bg 48080` also serves port 443, and replaces
  what is served there without a word.
- `http://<mac>.<tailnet>.ts.net` does not open: `tailscale serve` cannot redirect port 80 to
  HTTPS. Type the `https://` address.
- terminum answers to any `*.ts.net` name. Another name a proxy forwards goes in `allow_hosts`
  in `~/.terminum/state/web.json`.
- A device that comes through a proxy cannot use Finder or type a path into a terminal from the
  Files panel; the rest works as on the Mac.
- `tailscale funnel` (the public internet) is refused: terminum answers it with 403.
- To stop serving it: `tailscale serve --https=443 off`.
- `tailscale serve --bg` and `--service` are kept by tailscaled and come back after a restart
  of the Mac. The program behind each address must be running too: terminumd starts at login;
  iterm-enhancer runs only while iTerm2 is open.

#### A name of its own: a Tailscale Service

`https://terminum.<tailnet>.ts.net` is a Tailscale Service. It leaves the Mac's name to another
service, and it can be served only by a tagged machine (`tag:personal` below):

1. In the tailnet policy (admin console → Access controls), let your devices reach the
   service and let the Mac host it without a manual approval:

   ```json
   "grants": [{ "src": ["tag:personal"], "dst": ["svc:terminum"], "ip": ["tcp:443"] }],
   "autoApprovers": { "services": { "svc:terminum": ["tag:personal"] } }
   ```

2. In admin console → Services, define `terminum` with the endpoint `tcp:443`.
3. Then, on the Mac:

   ```bash
   tailscale serve --service=svc:terminum --https=443 http://127.0.0.1:48080
   ```

   `autoApprovers` approves only a host that advertises after the service is defined. Approve
   the Mac on the service's page when it advertised earlier, or without `autoApprovers`.
4. Open `https://terminum.<tailnet>.ts.net/` and sign in with the password.

5. Give the Mac's name back to the other service if step 2 of the first list took it (it then
   opened terminum under both names), and check:

   ```bash
   tailscale serve --bg --https=443 http://127.0.0.1:8765   # the Mac's name → iterm-enhancer
   tailscale serve status   # <mac>…:443 → 8765; terminum…ts.net (svc:terminum) → 48080
   ```

To remove it: `tailscale serve clear svc:terminum`.

## Remote hosts

```bash
terminum hosts add 'alex@devbox.example -p 2222' --files   # or "Add host" under the list
terminum hosts                                             # each host, its state, files and agents
terminum hosts enable devbox                               # browse its files (copies a helper)
terminum hosts agents devbox on|off                        # coding agents' progress from devbox
terminum hosts off|on|retry|remove|files-remove devbox
```

`hosts agents devbox on` (or the host menu's "Coding agents' progress on devbox…") puts terminum's
hooks and MCP server into Claude Code and Codex on devbox (`~/.claude`, `~/.codex`) and prints one
line per agent: installed, not found, needs Codex trust (/hooks), uses devbox's own terminum,
refused (a terminum entry of another command) or failed and why. Codex asks once to trust them in
`/hooks`; restart agent sessions already running there. devbox's rows and Plan pane then show its
agents as local ones (Plans, progress and notes, above). `off`, or removing the host, takes them
out again; nothing is written to devbox's `~/.claude` or `~/.codex` before you turn it on.

The events ride on the Files helper's ssh as a reverse Unix-socket forward, so devbox's sshd must
allow it: `AllowStreamLocalForwarding yes` (the default) and, on Alpine, `AllowTcpForwarding yes`
(its stock value is `no`). When only this reverse forward is refused (`AllowStreamLocalForwarding
local`), the host menu and `terminum hosts` say "forwarding refused by devbox's sshd", Files keep
working, and the forward is tried again after 10 minutes, then less and less often (up to every 2 h 40 min) while it stays refused. `DisableForwarding yes` or
`AllowStreamLocalForwarding no` refuse the Files tunnel too (it forwards a socket the other way);
Dropbear forwards no Unix socket in either direction, so neither Files nor agents work there:
Files say "forwarding refused by devbox's sshd (…)" and "Files cannot reach the helper". Only agents in devbox's default tmux server are
seen; an account with a ForceCommand or a login file that ends the session shows the ssh's last
lines in the host menu instead.

A host is its ssh string (options `-p -l -i -J -4 -6 -C`); terminumd holds its tmux session
`terminum` over ssh (and the host's other sessions you attach, above), reconnects after a drop, and browses its files through a helper it copies
only after you enable it. A lost network shows as "reconnecting" within about 30 s (ssh's keepalive:
15 s × 2); copying the helper may take up to 300 s, so `terminum hosts enable` waits up to 6 minutes
and `hosts agents … on` up to 10½. The remote command is always on ssh's command line: a
`RemoteCommand` for the host in your `~/.ssh/config` makes ssh refuse to connect: give terminum a
`Host` alias without one.

## Configure

Everything is in `~/.terminum/` and is read again within a second of a change:

| File | What |
|---|---|
| `settings.json` | `port`, `tmux.socket`, `tmux.history`, `size`, `restore`, `lastLines`, `themes` rules (by host and session), `page` defaults (`textSize`, `markdownSize`, `widgets`, `folds`, `followAgent`), `midi.soundfont` (the .sf2 MIDI files play with), `hosts` (each with `attach`: sessions of its tmux), `attach` (sessions of your own tmux by socket) |
| `themes/*.json` | colors, ANSI palette, font; `local.json` and `remote.json` ship as defaults |
| `hotkeys.json` | the hot keys panel's rows (`"F5"`, `"C-r"`, `"text:git status"`, `"Paste"`) |
| `custom.css` | your stylesheet, loaded after the page's own |
| `tmux.conf` | the private tmux server's configuration (written once; yours to edit) |

A wrong value keeps its default and is said once in the page.

## Specs and contributing

The behavior, limits and evidence are in `docs/specs/` (`terminum.md` and its siblings);
contributor rules in [CLAUDE.md](../CLAUDE.md); the release procedure in
[docs/RELEASING.md](RELEASING.md). Licensing: AGPL-3.0 or commercial (`LICENSE`,
`LICENSE-COMMERCIAL.md`); contributions come under the agreement in `CONTRIBUTING.md`. The
coding agents' marks in `web/static/agents/` are their owners' trademarks, outside both
licenses (`NOTICE`).
