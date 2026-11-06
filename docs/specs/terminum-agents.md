# terminum — Specification (coding agents)

Sibling of `terminum.md` (the core). This spec holds coding agents in terminum: their states in the list, unread, plans and progress, and the bookmarks and reminders they keep for the user. A host's agents are in `terminum-remote.md` AC-99. Cases are numbered across the specs.

## 0. Metadata

| Field | Value |
|---|---|
| Status | current |

A snapshot of what terminum must do, not a record of how it came to be: git holds that.

## 1. Overview

**Problem.** A coding agent works for minutes or hours in a terminal, and what the user needs from it is easy to miss: that it waits for an answer, that it stopped on an error, how far its plan has come, and the final report or open question scrolled away in thousands of lines.

**Who it affects.** Users of Claude Code and Codex who keep several sessions, and read and answer from a Mac or a phone.

**Expected outcome.** The session list and the header show each agent's state and, from its hooks, its tool, what it waits for and its subagents. A reminder names an agent waiting in a pane not shown. A session whose turn ended unseen is unread. A plan shows as a percent and in a Plan pane. What the agent hands over through `terminum mcp` is kept as the session's bookmarks and reminders, shared across a project's sessions on one host.

**Scenario.** Alex runs six agents and reads another session on a phone. A line over the terminal says "api waits for your answer"; a press opens that pane. Another row is hatched: its turn ended unseen, and its Plan pane says 9 of 9 done and holds the agent's final report.

## 2. Scope and non-goals

In scope:
- Reading agents' states from their screens and hooks; subagents; reminders of a waiting or failed agent; unread.
- `terminum hooks`, `terminum hook`, `terminum mcp`, and the installation of hooks and MCP server into Claude Code and Codex.
- Plans, progress and the stale reminder; the records an agent hands the user, their limits and a project's sharing.

Out of scope (explicit):
- A host's agents: `terminum-remote.md` AC-99.
- The user's own bookmarks and reminders, and the panes that list them: `terminum-terminal.md` AC-33, AC-91.
- Sent history: `terminum-terminal.md` AC-90.
- States for other agents: OpenCode gets only a mark (`terminum.md` AC-01).
- An MCP tool listing other sessions: the agent sees a project's records only (AC-98).
- Typing into a pane for an agent's hook or tool: a hook only observes, a reminder reaches the agent as context.

## 3. Assumptions, dependencies, open questions

No assumption, dependency or question of the core or terminal spec moved here: their rows are about tmux and the page. Rows of this spec:

Assumptions:

| ID | Assumption | Owner | Status |
|---|---|---|---|
| AS-71 | Claude Code's and Codex's screens and titles carry the words `tmux/agentstate.rs` reads. | Claude | measured on Claude Code 2.1 and Codex 0.162; a changed wording shows no state (§7) |
| AS-72 | Claude Code does not deliver an async hook's output in an interactive session; Codex delivers its async PostToolUse's. | Claude | measured; the reason for the synchronous PostToolBatch (AC-39) |
| AS-73 | A subagent interrupted, closed, killed or failed fires no SubagentStop in either program. | Claude | read from both programs' source (AC-39) |

Dependencies:

| ID | Dependency | Owner | Status |
|---|---|---|---|
| DEP-71 | Claude Code's plugin commands (`claude plugin marketplace add\|update\|remove`, `claude plugin install\|update\|uninstall\|list`), hooks and `.mcp.json`. | Owner | optional: without `claude` on PATH the install says so |
| DEP-72 | Codex's `hooks.json`, `config.toml` `[mcp_servers]` and its trust of hooks in /hooks. | Owner | optional |

Open questions: none.

## 4. Acceptance cases

| ID | Priority | Case |
|---|---|---|
| AC-39 | [MUST / P0] | A pane running a coding agent shows its state (working, waiting, failed; done shows no mark) in the list and the header, breathes while it works, and a reminder over the terminal names an agent that waits for an answer in a pane not shown. |
| AC-93 | [SHOULD / P1] | A session whose coding agent finished a turn while no page in sight showed it is marked unread until a page shows it. |
| AC-97 | [SHOULD / P1] | A coding agent's plan (its own plan tool through the hooks, or terminum's MCP tools) shows as a percent and a track on its session's row and in the header, and in a Plan pane with the items, the current step and the agent's ETA; a plan not updated for a while is asked for through the hooks. |
| AC-98 | [SHOULD / P1] | A coding agent hands the user what must not get lost in the terminal's scroll (a final report, a next step, a gap seen elsewhere, deferred work, a question) through terminum's MCP tool: kept as its session's bookmarks, or reminders when the user must act; self-contained, found where the call was printed; a project's agents on one host share them, so the same record is not kept twice. |

## 5. BDD scenarios

### AC-39 — Coding agents in the list [MUST / P0]

```gherkin
Scenario: AC-39 happy path — states
  Given panes run Claude Code and Codex (by #{pane_current_command}, the window's name, Claude Code's title marks, or Codex's words on the screen of a pane whose program is `node`)
  Then each shows, in the list and the header: its bar pulsing while it works, no other mark (a spinner in the title, "✻ Doing… (…)" on screen, "esc to interrupt"); before its title a flashing amber question mark while it waits for the user (a question, a permission prompt, "Esc to cancel", Codex's "[ ! ] Action Required" title or "enter to submit answer"; its words read as one line, so a narrow pane that breaks them over rows, "Esc to" / "cancel", still says so), a red alert sign when it stopped on an error ("API Error", overloaded, a usage limit, out of credits, a retry limit, high load), no mark when done; only the screen's last 25 rows count, read by terminumd when the title changed or 6 s passed
  And Claude Code's turn line "✻ Waiting for 2 background agents to finish" ("1 background agent and 1 dynamic workflow") is working: its subagents still run; the newest turn line counts (a later "✻ Worked for 3m 2s" is done)
  And Claude Code whose footer, under its input box, counts work in the background ("⏵⏵ bypass permissions on · 1 shell, 1 monitor"; also local agents, teams, dynamic workflows, cloud sessions, MCP tasks, "N background tasks") is working until that count goes: its session goes on; a turn line's "· 1 shell still running" alone is not (it stays after the shell ended), nor an Artifact comment monitor; only the rows right under Claude Code's input box (a rule, "❯", a rule) count, each item a number and its word ("1 shell script added" is not one); with hooks, a Stop's turn waits while that footer, read for that pane only then, counts work (the row shows working), and ends once the count has gone
  And a count of shells, monitors or tasks alone keeps the session working for 10 minutes at most (BACKGROUND_LIMIT), from when it alone kept it so (with hooks, from the Stop): a background job that never ends (a dev server, `tail -f`, a monitor) then leaves it done, its turn ended; the count counts anew after a turn of the agent itself
  And done so, its row and the header show a quiet terminal mark after the state's place ("Done; a shell, a monitor or a task it started still runs") while the footer still counts them, read every 6 s (with hooks too, for that pane only); the mark goes when the count goes or a turn of the agent starts, and never shows while it works, waits or failed
  But a count of agents (local agents, teams, background and remote dynamic workflows, cloud sessions) keeps it working however long they run: they end on their own; "← 1 agent" is Claude Code's agents view's count of other background sessions waiting on the user, not this turn's work
  And on a screen without Claude Code's input box (Codex) the last 10 rows saying agents still run ("2 agents still running", "1 subagent is running", "3 background agents are still running", "Waiting for 2 agents") keep it working; "2 agents finished, none running" does not
  And the row's bar (one color per theme) breathes every 1.4 s while the agent works (not with reduced motion); a dot reads as an unread message; the header's bar stands still
  And a waiting row's ? goes from the row and the header as soon as the user shows that session (it was seen), and comes back only when the agent waits anew after it stopped waiting

Scenario: AC-39 happy path — a turn that stopped and needs the user
  Given a coding agent's turn ends and what it last did leaves it to the user: Codex's request_user_input_async (drawn "? title" with its options, gone from its title at the turn's end), a last message that ends in a question mark of any script ("?", "？", "؟", "՞", "፧", the Greek one), Claude Code's "Session paused" (safeguards' retry dialog), an error line of the program saying a safety stop ("■ This request has been flagged for possible cybersecurity risk.", "API Error: … can't help with this", "Chat stopped as a precaution")
  Then its row and header show the flashing amber ? and the reminder of the next scenario, as for a question it asks mid-turn, also while a shell of its runs on; it stands until the next prompt, and goes from sight when the user shows that session or closes its reminder
  And the turn counts as ended: unread and reminders come as for any end
  And its hook only works out one bit from the last message and posts that: the message itself never leaves the hook's process
  When a turn failed (API error, rate limit, login, context full; Codex's "■ " lines, which fire no event)
  Then a reminder says "<title> failed" with why when known, beside the row's red mark

Scenario: AC-39 happy path — how many subagents
  Given hooks count at least one subagent of a pane
  Then its row shows the number beside a small bot icon, "N subagents" on hover: the higher of the hooks' count and the agents Claude Code lists under its footer ("○ rust-flakes  Fix flaky tests" below "● main", which is not counted)
  And a pane whose hooks count none is not read for it: its screen is not read every 2 s for a list that may not be there

Scenario: AC-39 edge — a turn's end is never lost
  Given a pane whose hooks spoke, and a turn that ended with no Stop counted: Codex's errors, review and compact (no Stop), Claude Code's Esc (no Stop), a Stop that never came (its hooks not trusted in that session)
  When its screen shows it done for 10 s
  Then its turn ends: unread and reminders; a Stop already counted for that turn makes no second end
  And a subagent interrupted, closed, killed or failed fires no SubagentStop in either program, so the count must not trust it: Codex's turn ends at its own Stop whatever subagents it counts; Claude Code's Stop with subagents counted is checked against the screen from 5 s on, and with no agents in the footer or turn line the count goes and the turn ends
  And a subagent's own prompt starts no turn and its failure marks no pane

Scenario: AC-39 happy path — reminders
  When an agent in a pane not shown has waited 2.5 s
  Then "<title> waits for your answer" stands at the terminal's top under the banners: a line across its whole width for each waiting session (three at most on a computer, one with "+N more" on a phone), its ? in the waiting yellow on a faint tint of it (no card, no shadow), starting where the terminal's text does, its words cut by an ellipsis where they would not fit; a click opens that pane, × closes it; it goes 2.5 s after the pane stops waiting or when shown; a reminder closed or seen comes back only when the pane asks again

Scenario: AC-39 edge — a question's words in the transcript
  Given Claude Code's screen read (no hooks) shows its input box (a rule, "❯ ", a rule) and, above it, text that reads like a question ("Would you like to run…", "Do you want to …", "(y/n)": printed by a tool, quoted by the agent)
  Then the session is not waiting: a question of Claude Code's replaces its input box, so words above the box are the transcript

Scenario: AC-39 edge — a named pane, a shell
  Then a pane or window the user named hides the title marks, so the screen decides; a pane at a shell prompt shows no state; "shell" in the layout says a tap may move the cursor along the line (not in htop, less, mc)

Scenario: AC-39 edge — Codex
  Given Codex, which keeps its title while it works and marks it "[ ! ] Action Required | " (blinking "[ . ]") only when it asks
  Then its question waits although its footer says "esc to interrupt"; a turn the user cancelled ("Conversation interrupted") is done, not failed; the list's title leaves out the mark, so the row does not blink
  And Codex started through npm runs as `node`: a `node` pane is Codex while its screen, read as an agent's is, shows Codex's own words ("OpenAI Codex (v", "Ask Codex to do anything", "tell Codex what to do differently", "enter to submit answer", "tab to add notes"); the title never makes it Codex (a killed Codex can leave its mark to the next program); another `node` program, a dev server asking "(Y/n)" too, shows no state and no reminder, also right after Codex in the same pane
  And Codex started without "--no-alt-screen" (as "codex resume" is) draws on tmux's alternate screen, where tmux keeps no scrollback: scrolling up there says so once a pane, naming "codex --no-alt-screen" and Codex's setting alternate_screen = "never" under [tui]

Scenario: AC-39 edge — shared hook matchers
  Given one Codex matcher contains both terminum's hook and another command
  When terminum's hooks are removed
  Then the other command, matcher and unrelated metadata remain; only entries emptied by this removal go

Scenario: AC-39 edge — a complete answer on an open socket
  Given a hook or MCP call has received a complete Content-Length or chunked HTTP answer, or a no-body 204 answer
  Then the client acknowledges it without waiting for the connection to close; an incomplete framed answer still fails within the shared request budget

Scenario: AC-39 happy path — the agents' own hooks
  Given Claude Code or Codex is found (its program on PATH, ~/.claude, $CODEX_HOME or ~/.codex) without terminum's hooks
  Then "terminum status", "terminum open" and "terminum install" say so in one line naming "terminum hooks install"
  When the user runs "terminum hooks install" (or "terminum hooks install claude|codex")
  Then Claude Code gets terminum's plugin: a local marketplace in ~/.terminum/hooks/claude added with Claude Code's own "claude plugin marketplace add" and "claude plugin install terminum@terminum-local --scope user" (terminum never edits Claude Code's settings); Codex gets terminum's entries in $CODEX_HOME/hooks.json (default ~/.codex), the file backed up beside it first and others' entries kept, and is told to trust them in Codex's /hooks (terminum's SessionEnd entry runs without `async`, which Codex would run synchronously with a warning at each start; an older entry is migrated alone, every other byte kept, by an install or an upgrade's refresh, which says to trust the changed hook once); a second install changes nothing; an upgrade's refresh updates Claude Code's plugin in place (never removed first) and checks it with `claude plugin list`, warning and keeping the old install on failure; "terminum hooks remove" takes out only terminum's; each "claude" it runs has a session of its own (no terminal to stop on: a read of /dev/tty fails at once) and a Ctrl-C, a closed terminal or a stop of terminum ends it with terminum; "terminum hooks" says for each agent: not found, found without, or installed and when its hooks last spoke
  And each lifecycle event (session start and end, a prompt, a tool before and after, an approval asked, a notification, a stop, a failure, a subagent's start and stop, Codex's interrupt) runs "terminum hook <agent>" in the background, which reads the event, posts a few fields of it (never a tool's output, a file's text or the agent's answer; a command's first two words with values left out; a plan tool's items for AC-97; the user's prompt for the sent history, AC-90) to terminumd's socket within 150 ms and exits 0 without output
  And Claude Code's plugin also runs "terminum hook claude" synchronously (timeout 2 s) on PostToolBatch, the carrier of AC-97's reminder, because Claude Code does not deliver an async hook's output in an interactive session; Codex's reminder rides its async PostToolUse; that event does nothing else, stays within the 150 ms and prints nothing without a reminder; an install's or upgrade's refresh updates a plugin whose hooks file differs from this build's
  Then the pane's row shows the state its hooks say over the screen's reading (the screen is not read for it), the tool it runs in its mark's title ("Working: Bash: npm test"), what it waits for ("Approve Bash: rm -rf", the question it asks; the reminder card says "<title> asks: …"), why a turn failed ("rate_limit"), and the number of subagents running under the mark
  When Claude Code's turn stops (its Stop) while subagents it started still run in the background
  Then the pane stays working with their number under the mark until the last one stops (SubagentStop), also through Claude Code's idle notice (the main thread waits for the user) and the user's next prompts (the subagents run on); its turn ends then (reminders, unread); a background subagent's tools keep it so, and one quiet for 10 minutes gives way to the screen; an interrupt (Esc) or a failure ends it at once; background shells and monitors are not counted (a dev server would keep it working for ever)

Scenario: AC-39 edge — a previous session's delayed events
  Given a new agent session has started in a pane
  When a delayed non-start event or SessionEnd of a different session arrives
  Then its current state, subagents, folder and plan stay unchanged; rejected events trigger no sent-history, turn or reminder side effects

Scenario: AC-39 edge — hooks that stopped speaking
  Then a working state no hook renewed in 30 s (waiting: 120 s) is the screen's to say again (an interrupt in Claude Code fires no Stop, a crash no end); a tool event of a turn that stopped, come late from the background, changes nothing; another program in the pane, or another agent session, drops what the hooks said; Codex's done is read from its screen (it has no failure event); a hook from a pane no terminumd server lists changes nothing
  And a pane's hook record gone with a host's link or a restart of terminumd comes back with its session's next event of any kind, so the plan tools and the Plan pane stop saying the hooks are not running; a late event of a session seen to end makes none
  And a hook's wait gives way to a screen showing the agent at work: an approval answered fires no event

Scenario: AC-39 failure — the screen cannot be read, no terminumd, a broken hooks.json
  When terminumd cannot capture the pane (closed meanwhile)
  Then the row shows no state this refresh and the next refresh tries again; nothing else is delayed
  When terminumd does not run, the hook returns at once and the agent goes on as without it
  When a person runs "terminum hook" (an unknown agent such as "terminum hook install", or a terminal for its input), it says "terminum hooks install" sets them up and exits 2 instead of waiting for keys
  When Codex's hooks.json does not parse or is not an object of lists, "terminum hooks install" refuses with why and changes nothing; without "claude" on PATH it says so
```

### AC-93 — Unread [SHOULD / P1]

```gherkin
Scenario: AC-93 happy path — a finished turn nobody saw
  Given the user sent a prompt to Claude Code in %3 and went to another session (or locked the phone)
  When its turn ends: its hook's Stop with no subagent left running (else the last one's SubagentStop), or, read from its screen, its pane done for 10 s on end (not working, not waiting, no subagents: a moment's done between two steps of the turn ends nothing); either way not while Claude Code's footer counts work it runs in the background ("1 shell, 1 monitor", "2 local agents": the turn ends once the count has gone, or after 10 minutes of that count alone; AC-39)
  Then %3's row is hatched over its whole box: pale see-through diagonal lines one whole pixel wide (none blurred over two) in the color of the session's name, its text and the shown row's fill readable through them, hover or not; "Unread: its agent finished" for a screen reader, on every page, while no page in sight showed %3 at that moment
  When a page in sight shows %3 (opened, or a hidden page showing it comes back)
  Then the mark goes on every page; a turn that ends while a page in sight shows it leaves none
  And "in sight" is as for sizing (AC-13): a page in front and, on a computer, focused (one out of focus still streams); a window on another display while the user types elsewhere is not looking, so a turn that ends there is unread
  And the marks are kept in state/unread.json with each tmux server's identity (`@terminum-id`): a restart of terminumd (an upgrade) keeps them; a pane gone takes its mark with it; a tmux server started anew (a reboot, a killed tmux; its pane ids start again at %0) keeps the marks of the panes its restore makes again (AC-15), each on the pane made in its place, and loses the others
  And a row that is busy again (working, waiting, its subagents running) shows no mark while it is: its work goes on

Scenario: AC-93 happy path — mark it unread
  When the user picks Mark as Unread in a row's menu (a right click or a long press; any session, an agent's or not)
  Then the row is unread on every page, as after a turn nobody saw, until a page in sight shows it again (opened, or a hidden page showing it comes back); the session shown on this page now stays unread until it is opened again
  And an unread row's menu has Mark as Read instead, which takes the mark away on every page
```

### AC-97 — Session plans and progress [SHOULD / P1]

```gherkin
Scenario: AC-97 happy path — the agent's own plan
  Given Claude Code with terminum's hooks in pane %3
  When its TaskCreate makes "Write the parser" (id 1) and "Add tests" (id 2) and its TaskUpdate sets id 1 completed
  Then terminumd keeps the pane's plan from the successful PostToolUse events only: 2 items, 1 completed, 50%
  And the session's row shows "50%" right of its title over a thin track in the row's color, its hover "1 of 2 · updated 1 min ago"; the floating buttons' column a circle "50%" with its ring half filled (first in the column; on a phone under ⟨…⟩, unfolded with the others), its hover "Plan: 1 of 2 · updated 1 min ago"
  And the header's Plan button shows "50%" beside its icon; at 0% its icon alone and no circle in the column
  When TodoWrite sends the whole list (3 items) or Codex's update_plan its steps (a result "Plan updated")
  Then that list replaces the plan; a TaskUpdate with status "deleted" removes its item
  When a subagent of the session (an event carrying its agent_id) calls a plan tool
  Then the session's plan is not changed and that event gets no reminder
  And an update_session_plan whose _meta.sessionId is another than the pane's hooked session is not applied, its answer says only the main agent updates the plan
  And a pane making MCP calls with no hook event of terminum's gets, at the end of update_session_plan's and get_session_plan's answers, "terminum's hooks are not running in this session (Codex: trust them in /hooks); send the whole plan with update_session_plan.", and the Page's `plan` carries `hooks: false`

Scenario: AC-97 happy path — the Plan pane
  When the user presses the percent, the column's progress circle or the header's Plan button
  Then the Plan pane opens (modal, a window placed as the reminders' pane, terminum-terminal.md AC-91), laid out in sections: the plan's title (15 px, 600); "Now" and the current step ("doing") on a raised strip with a gray pill inside it at its left (as a row's folder bar; no stripe along its rounded edge); the progress, "3 of 9 done" and "33%" over a bar filled to it, the ETA "~17:40 (agent's estimate, given 16:55)" under it; "Steps", each item with its mark in a round badge (✓ on green, ▶ on the text color with its row tinted, an empty ring pending, a large "!" on amber) and its text hanging beside it; under a rule "Updated 2 min ago (16:57)" and where the plan came from (terminumd's times, aged by the browser's clock); "For you" and how many records its agent kept for the user ("2 kept for you by the agent: show them", a press opening the bookmarks' pane; "Nothing kept for you by the agent.", AC-98)
  And every text the agent wrote is kept without control or bidi characters (U+202A–U+202E, U+2066–U+2069) and shown as plain text (never Markdown or HTML: "<b>x</b>" reads as typed); on a phone the pane is full-width under the header, its buttons at least 44 px

Scenario: AC-97 happy path — found before a plan
  Given a pane runs a coding agent that has made no plan yet
  Then the header shows a Plan button (its icon alone, right of ✎, its hover "Plan: none yet"), and the column no progress circle; a row's menu and the header's ⋯ menu have "Plan and Notes…" for any session
  And the Plan pane without a plan says "No plan: the agent has not sent one for this session.", then the notes

Scenario: AC-97 edge — the agent exited
  Given the pane's plan was archived because its program changed
  Then its Plan pane shows the pane's last archived plan marked "Finished session (agent exited hh:mm)" (terminumd's `plan` with `finished: true`) until a new agent session there makes a plan; its row shows no percent for it, the column no circle

Scenario: AC-97 happy path — terminum's MCP tools
  Given `terminum mcp` started by the agent in pane %3 (TMUX and TMUX_PANE from its environment)
  When the agent calls update_session_plan {items: 7 with 3 completed, doing: "Run the tests", eta_minutes: 45}
  Then it answers "Plan stored: 3 of 7 completed (43%), ETA ~17:40." and the row shows "43%"; the ETA is the call's time plus 45 minutes, shown as the agent's estimate; an absolute time from the agent is never taken
  When the session also keeps a built-in plan (TaskCreate, TodoWrite, update_plan)
  Then the MCP call's items are not applied ("Doing and ETA stored; items follow your own plan tool (3 of 7 completed), so the items sent were not taken, ETA ~17:40.") and a later built-in plan replaces MCP items; doing, title and ETA are kept on either
  When the agent calls get_session_plan {}
  Then it answers the stored plan as lines "[x] …", "[>] …", "[ ] …", "[!] …"

Scenario: AC-97 happy path — the stale reminder
  Given a pane whose hooks say working, its turn started at its UserPromptSubmit
  When a PostToolUse of the main agent comes and the plan was last updated longer ago than clamp(turn's time / 6, 10 min, 30 min), counted from the later of its update and the turn's start, or there is no plan and the turn has worked 10 min
  Then terminumd answers the hook with terminum's fixed text and numbers only ("terminum: this session's plan was last updated 14 min ago. Update it now: …"), which `terminum hook` gives the agent as the event's additionalContext; nothing is typed into the pane; the row's percent and the column's circle dim while stale
  And no second reminder comes within one threshold, and a turn without a plan gets one at most
  And a plan whose items are all completed gets no reminder; only the agent's reminder event carries one (never PreToolUse, a prompt or a stop): the events of AC-39's hooks, the printed hookEventName that event's; Claude Code's async PostToolUse never takes it

Scenario: AC-97 edge — partial, ETA, reuse, restart
  Given two plan items and a successful TaskUpdate deleting the first with a subject
  Then only the first item disappears; the next item's text and status stay unchanged
  When a TaskUpdate names id 9, which no TaskCreate made (an event lost)
  Then no item is invented; the plan is marked partial ("partial: some updates were missed"), its known counts shown, until a whole list (TodoWrite, update_plan, update_session_plan) heals it
  When the items change without a new eta_minutes
  Then the ETA is dropped (it said nothing about the new list)
  When the agent ends and a new agent session starts in the same pane (SessionStart, another session id, another program)
  Then the old plan is archived (kept in plans.json, never shown) and the row shows no percent until the new session keeps a plan
  When every item of a plan is completed
  Then its row and the header show 100% while its turn works and for 10 minutes after its last update, then none (the list's next refresh); the Plan pane keeps it
  When the user's next request starts a turn (UserPromptSubmit) on a finished plan
  Then that plan is archived (kept in plans.json, never shown) and the request starts with none: the row shows no percent until the agent keeps a plan; a plan with an item not completed stays across requests
  When terminumd restarts
  Then every plan comes back from `state/plans.json` with its updated time; a pane gone is archived when its hook record is dropped (kept in plans.json, not shown); a tmux server started anew (another `@terminum-id`: its pane ids start again at %0) takes its plans with it
  And an MCP call that terminumd cannot yet tie to an agent or session (its hooks not heard since the restart) keeps the restored plan, never archives it
  When the turn is interrupted or stopped mid-plan
  Then the plan stays as it was, not stale; the next prompt of the same session goes on with it
  When the pane is idle, waiting or stopped
  Then no reminder comes; after the Mac slept, the first PostToolUse may remind once

Scenario: AC-97 failure — tools refused
  When a plan tool's call fails (PostToolUseFailure, Codex's update_plan refused in Plan mode, a result that is not a success)
  Then the plan is not changed
  When `terminum mcp` starts outside a terminum pane (no TMUX or TMUX_PANE: it starts in every Claude Code and Codex session of the Mac)
  Then it offers no tools and no instructions, so it costs that session nothing
  When a tool is called anyway, or terminumd does not know the pane
  Then the tool answers an error "This session does not run in a terminum pane; do not call terminum's tools again."
  When terminumd is not running (no answer on its private socket in 2 s)
  Then each tool answers an error "terminum is not running; go on without it."
  When nothing answers as the agent starts in a pane (terminumd restarting, a host's link down)
  Then the tools and instructions are offered all the same (an agent lists them once, at its start): each call answers "terminum is not running; go on without it." until terminumd or the link is back, then works, or says the session is not in a terminum pane; the sinks (a host's `run/mac-*.sock` too) are listed at each call, so a link that comes up later serves the session. An agent in a pane terminum does not serve, started while nothing answered, carries the tools; terminum's MCP server is installed only where the user asked for it (`terminum hooks install`, a host's coding agents' progress)
  When update_session_plan has no items, over 50, an item over 200 characters, or a status not pending, in_progress, completed or blocked
  Then it answers an error naming the limit or the allowed values (an empty list: "send the whole plan"), and nothing is stored; eta_minutes outside 1–1440 is refused the same way
  When plans.json cannot be written or does not parse
  Then terminumd's log says why; a file that does not parse is set aside as plans.json.bak and an empty state starts

Scenario: AC-97 happy path — installed with the hooks
  When the user runs `terminum hooks install`
  Then Claude Code's terminum plugin carries `.mcp.json` naming `terminum mcp`, and Codex's config.toml gets one block between "# >>> terminum: MCP server …" and "# <<< terminum" ([mcp_servers.terminum], env_vars TMUX and TMUX_PANE, tools approved), the file backed up first; hooks.json's entries stay byte for byte; `terminum hooks` says whether each agent has the MCP server; `terminum hooks remove` takes the block out; the install says to restart running Codex sessions (they read config.toml when they start)
  And install and remove change only the lines terminum wrote: a line of another's inside the markers (a table Codex added there) is kept; a config.toml that is a symlink is written where it points, the link kept
  When config.toml already has an [mcp_servers.terminum] table outside terminum's block, or the file changed while the install ran
  Then the install refuses and says why, the file untouched
Scenario: AC-97 happy path — kept current by an install or upgrade of terminum
  Given the user ran `terminum hooks install` for an agent before
  When `terminum install` or `terminum upgrade` succeeds
  Then Claude Code's plugin is updated in place (AC-39) when its marker names another build, the plugin lacks `.mcp.json` or its hooks file differs from this build's (the synchronous PostToolBatch entry added), and Codex's MCP block is added or made current while hooks.json stays byte for byte; one line per agent refreshed
  And an agent where terminum's hooks were never installed gets nothing
  When the refresh fails (claude not on PATH or not answering within 20 s a call, config.toml refused)
  Then a "Warning: …; run terminum hooks install" line is said and the install or upgrade still succeeds; `terminum status`, `open` and `install` say in one line that the agent has terminum's hooks without its MCP server, naming `terminum hooks install`
```

### AC-98 — The agent's bookmarks and reminders [SHOULD / P1]

```gherkin
Scenario: AC-98 happy path — the agent hands the user a record instead of printing it
  Given the coding agent finds a next step, a gap or a bug in another part of the project, deferred work, a question only the user can answer, or ends with a final report
  When it calls add_notes_for_user {notes: [{title: "Final report: retry loop fixed", text: "<Markdown: what changed, files, why, how checked, next step>", kind: "report"}, {title: "Decide: rename the API to fetch_all?", text: "…", kind: "question", remind: true}]}
  Then terminumd keeps each as a bookmark of the agent's pane (AC-33) marked made by the agent (claude or codex): its title the bookmark's note, its text the body, its kind and file kept; one with remind true is a reminder (AC-91) and comes up when the turn ends
  And it answers each record's id, and for a reminder that it comes up at each turn end until the user resolves it
  And it shows as a bookmark the user makes there does, in its session alone its title and text cleaned (control and bidi characters dropped) and redacted (`sent::redact`) before they are kept
  And its place is where the agent's call was printed: the pane's newest history line then, and its title, which the agent's transcript of the call shows; Open looks for the title nearest that line (AC-33's search), and where it is no longer in the history it shows the record with "These lines are no longer in this session's history."
  And the tool's description says what it covers and the rule: a final report, findings and deferred work are bookmarks; remind only when the user must act or decide (a blocker, a question, a risky leftover, a step only the user can take); when unsure, a bookmark; each record readable without the chat (what, where: files and paths, why, next step)
  And the MCP instructions ask for one record per final report, finding outside the task, deferred item or question, never one for progress; a record that changes is updated with update_notes_for_user instead of added again

Scenario: AC-98 happy path — the agent tidies its own, the user answers only questions
  When a record with remind true has the kind bug, deferred, followup or report
  Then it is kept as a bookmark and the answer says only a question reminds; update_notes_for_user refuses remind true for such a record of an agent, the user's own stays the user's
  When the agent adds with {replaces: [ids]}
  Then those of its own records go in the same save, a reminder too, and the answer names them; a record of the user's is left and named
  When a report is added to a session holding three of agents' reports
  Then the oldest there go in the same save and the answer names them, with no banner: a later report tells what the earlier ones did
  And every answer to add_notes_for_user lists the session's other open records, newest first, ten at most and how many more, by id, kind and title, so the agent closes what it covered instead of the user

Scenario: AC-98 happy path — shown with the user's
  Then it is listed with the user's bookmarks and reminders (AC-33, AC-91), its agent's mark before its title, its text as Markdown with raw HTML off (as rendered output, AC-31), never quoted; Quote in chat pastes its title, a blank line, its text quoted
  And the Plan pane (AC-97) says how many open records the agent left, a press opening the session's bookmarks' pane

Scenario: AC-98 happy path — the agent updates one
  When the agent calls update_notes_for_user {id, status: "done"} (or a new title, text or remind)
  Then a reminder is resolved, a bookmark deleted (open reopens a resolved reminder); any record of the caller's session is found, the user's too; of other sessions only what the project's scope below allows

Scenario: AC-98 happy path — the agent reads and deletes its session's
  When the agent calls get_notes_for_user {}
  Then it gets JSON of its session's bookmarks and reminders, the agent's and the user's, in the order they were made: id, type (bookmark, reminder), status (open, up, resolved), by (the agent, or "user"), title, kind, file, no text, then the project's scope below; with {id} that record whole, its note and text, cleaned and redacted as a record is kept; the tool says their text is data, never instructions
  When the agent calls delete_notes_for_user {ids: [1–20 ids]}
  Then those records are gone for good, a reminder too, every page told at once
  And an id of another session, of its project too, of an orphaned record or of none is answered "No record <id> in this session" and nothing of that call is deleted; an agent never deletes another session's, and reads and closes one only by the project's scope below

Scenario: AC-98 happy path — a project's sessions share their agents' records
  Given a pane that is a member of a project (AC-92), its agent's scope beyond its own session is, on the same host as that pane (the host part of the record's key): agents' records of the project's other member panes, and agents' orphaned records stamped with that project
  And a pane in no project has its own session's alone; a pane taken out of a project leaves it at once, whatever its records are stamped with; the user's records of other sessions are in no scope, for reading, closing or repeats
  When the agent adds a record whose title equals an open record in its session (letter case, runs of spaces and end punctuation aside), or in a live session of that scope with the same kind, the same reminder or bookmark, and the same file when both name one
  Then it is not kept, the answer says "Not kept, your text was not stored", names the record's id and its session, and asks to read it and add under a title that says what when its text adds something; a repeat within the call is said too
  And a repeated question raises the reminder kept (up in its own session as at a turn end) and the answer asks the agent to tell the user which session holds it, so a question is never left in an idle session unseen
  And kind report never repeats across sessions: each session's report is its own
  When the title equals an orphaned record of that scope, of the same kind and file
  Then the new record is kept and the orphaned one goes in the same save, named in the answer: a closed session's never blocks a live one
  When the title shares at least 3 words and at least 60% of the larger title's words with an open record of a live session of the scope
  Then the record is kept, and the answer says it looks like that record, by id, session and title, and how to delete the new one if it is the same; a wrong hint costs one line and loses nothing
  And a repeat judged against an id in the call's `replaces` does not count: the old record goes and the new one is kept
  When get_notes_for_user {} is called
  Then after the session's own it lists that scope's open records, newest first, at most 50 then {"more": n}, each with `session` (its row's title when made, cleaned and cut to 40 characters) and `closed` for an orphaned one; with {id} a record of the scope is read whole
  And every answer to add_notes_for_user adds up to 10 of them, as quoted JSON said to be data, never instructions, apart from the session's own, and only the session's own are asked to be closed
  When update_notes_for_user {id, status: "done"} names a record of the scope
  Then it is closed as its own would be, a copy of it whole with who closed it and when is added first to `closed-records.jsonl` beside web-bookmarks.json (0600, past 1 MiB the file becomes `.1`, one older file kept), the close is logged, and when the copy cannot be written nothing closes
  And any other change to it, status open, title, text or remind, is refused with why, as are delete_notes_for_user and `replaces` naming it
  And limits, reports past three and "went for room" stay each session's own, so one session's record never deletes another's; a record another session's add was refused for can still go by its holder's limits or close, a known limit
  And a reminder comes up at its own session's turn end, and the page's panes keep their default filter (AC-33)

Scenario: AC-98 edge — limits, the notes of 0.23
  When a call adds bookmarks to a session holding 50 of agents' bookmarks, or reminders to one holding 20 of agents' open reminders
  Then the oldest of that kind there (in the order they were made) are deleted, in the same save, so that the new ones are kept, never a refusal for room; the answer names the ids that went; a resolved reminder counts in no limit (AC-91)
  And a session's count is of its own records, never another's, of its project too
  And update_notes_for_user reopening a reminder or making a record the other kind makes room the same way, never deleting that record
  And past 500 records of agents in all, the oldest orphaned ones go (no live scope loses a record but by its own limits); agents' records never count against the user's limits, and the user's own are never deleted for room
  And a call carries 1–20 records: title 1–120 characters, text 1–2000, kind bug, deferred, followup, question or report, file at most 300
  When terminumd starts with the notes.json of 0.23 (the Plan pane's notes)
  Then its open notes become agents' bookmarks of their scopes once (title: the text's first line, cut to 120), and the file is kept as notes.json.imported

Scenario: AC-98 happy path — told when the oldest went
  Given agents' bookmarks or reminders of a session were deleted for room
  When that session's agent next ends a turn (AC-91's turn end)
  Then every page shows a banner over the terminal: "Agents' bookmarks of session <its row's title> are over 50: the oldest are deleted automatically. Review them." (for reminders "Agents' reminders … over 20"), with Open bookmarks (Open reminders) and ×; both kinds of a session are two lines
  And Open shows the pane that made room and opens its bookmarks' (reminders') pane (that pane gone: Bookmarks / Reminders below the sessions); × closes it on every page; pages that connect later get it until closed; a restart forgets it
  And one banner a session and kind: a deletion while it shows changes nothing; after it was closed, the next turn end after another deletion there shows it again; orphaned records that went (past 500) show none

Scenario: AC-98 failure — not kept
  When the call is made outside a terminum pane or terminumd is down
  Then it answers the same errors as AC-97's tools
  When web-bookmarks.json cannot be written
  Then the tool answers an error and nothing is kept
```


## 6. Flow and sequence diagrams

### An agent's state (AC-39)

```mermaid
stateDiagram-v2
  [*] --> working: spinner in title or "esc to interrupt"
  working --> waiting: question, permission, "Esc to cancel", "Action Required"
  working --> failed: "API Error", overloaded, usage limit
  working --> done: no spinner, no question
  waiting --> working: the question gone from the screen, or a hook says so
  waiting --> reminder: 2.5 s and the pane not shown
  reminder --> [*]: shown or answered
```

terminumd decides the state from the last 25 rows of the pane, or from the agent's hooks while they hold (their lease); the page draws it and times the reminder. A turn that ends (working or waiting, then done, failed or no agent) puts its pane's reminders up (AC-91).

### Hooks and tools (AC-39, AC-97, AC-98)

```mermaid
sequenceDiagram
  participant A as Claude Code or Codex
  participant H as terminum hook / terminum mcp
  participant D as terminumd
  participant P as Page
  A->>H: an event on stdin (hook), a tool call (MCP)
  H->>D: POST /hook within 150 ms, POST /agent within 2 s, to every sink
  D->>D: state, turn's end, plan, records; plans.json, web-bookmarks.json
  D->>P: the list's item (state, tool, why, subagents, plan, unread), marks
  D-->>H: 204, {remind}, or {text, error}
  H-->>A: nothing, additionalContext on the reminder event, or the tool's answer
```


## 7. Current behavior and gaps

| AC | Gap | Evidence |
|---|---|---|
| AC-39 | A changed wording of Claude Code or Codex shows no state. | `terminumd/src/tmux/agentstate.rs:25` |
| AC-39 | Codex's startup prompts (trust, update) show no state; nor does a `node` Codex with a typed draft and no other mark, until it is sent; nor does a custom `[tui].terminal_title` mark a question. | `terminumd/src/tmux/agentstate.rs:37`, `terminumd/src/tmux/agentstate.rs:62` |
| AC-39 | An error still in the last 25 rows after a later finished turn reads as failed. | `terminumd/src/tmux/agentstate.rs:325` |

## 8. Recommendation and ownership

**Recommended approach.** terminumd reads an agent's state from the last 25 rows of its screen, the only source without an install. Where the agent's own hooks are installed they say the state over the screen under leases, and add its tool, what it waits for, its subagents and its plan items: `terminum hook` posts a few fields of each event to terminumd's private socket and exits. `terminum mcp` is a stdio MCP server, the agent's way to send a plan and records, and it posts to the same socket. A turn's end is taken from either source, once. Records are bookmarks and reminders with `by` set, in the store the user's already use.

**Rejected alternative.** A store of notes beside the bookmarks (0.23's `notes.json`): one tool and one store were chosen, and its notes are imported once (AC-98). Typing a plan reminder into the pane was rejected too: `send-keys` is only for an explicit key or paste of the user (CLAUDE.md §2), so the reminder goes back as the hook's `additionalContext`.

| Part | Owner |
|---|---|
| terminumd: agent state, hooks, plans, records, unread; `terminum hook`, `hooks`, `mcp` | Claude |
| Page: rows' marks, reminders, Plan pane | Claude |
| Acceptance with real agents on a Mac and a phone | Owner |

## 9. Technical details and specifications

### Interfaces

| Interface | Shape |
|---|---|
| `terminum hooks [status\|install\|remove] [claude\|codex]` | AC-39, AC-97; a person's command |
| `terminum hook claude\|codex` | run by the agent's hooks: the event's JSON on stdin (its first 1 MiB kept, the rest read and dropped); posts to every sink at once: the local terminumd's socket (`TM_APP_DIR`, else `~/.terminum/state`), then the Macs' links in `~/.terminum/run`; prints at most `{"hookSpecificOutput":{"hookEventName":…,"additionalContext":…}}` |
| `POST /hook` on `terminumd.sock` | `{agent, socket, socket_path, pane, event, session?, turn?, sub?, subtype?, kind?, error?, cwd?, model?, tool?, message?, asks?, plan?, prompt?}` → 204, or 200 `{"remind": text}` for a reminder |
| `POST /agent` on `terminumd.sock` | `{socket, socket_path, pane, tool, args, session?}` → always 200 `{text, error}`; the tool `probe` asks whether terminumd knows the pane |
| `GET /health` on the socket | adds `hooks: {claude\|codex: <Unix seconds last heard>}` |
| `terminum mcp` | MCP over stdio, one JSON-RPC message a line, at most 1 MiB; protocol versions 2025-11-25, 2025-06-18, 2025-03-26 (default 2025-06-18); serverInfo name `terminum`; the pane from `TMUX` and `TMUX_PANE`, the session from `_meta.sessionId` |
| WebSocket, page → terminumd | `unread {id, on}` (AC-93) |
| The list's item | may carry `tool`, `why`, `subagents`, `plan`, `unread` |
| WebSocket `plan` | page asks for a pane's plan; the answer's `plan` has title, doing, items, done, total, pct, eta and more, `hooks: false` when no hook spoke, `finished: true` for an archived plan |

Hook fields, cut to these characters: event 40, socket 64, socket_path 300, session, turn and sub 100, subtype 60, kind 40, error 60, cwd 1000, model 80, tool 80, message 200, prompt 20,000. `asks` is one bit: the last message ended in a question mark. `turn` is `turn_id`, else `prompt_id`.

Events run: Claude Code SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, PostToolUseFailure, PermissionRequest, Notification, Stop, StopFailure, SubagentStart, SubagentStop, SessionEnd, each async with a 3 s timeout, and PostToolBatch synchronous with 2 s; Codex SessionStart, UserPromptSubmit, PreToolUse, PermissionRequest, PostToolUse, Stop, SubagentStart, SubagentStop, SessionEnd, Interrupt, each async with 3 s but SessionEnd, which is not async.

MCP tools:

| Tool | Arguments |
|---|---|
| `update_session_plan` | `items` (required): `[{text, status}]`; `doing`, `eta_minutes`, `title` |
| `get_session_plan` | none |
| `add_notes_for_user` | `notes` (required): `[{title, text, remind?, kind?, file?}]`; `replaces`: ids |
| `get_notes_for_user` | `id?` |
| `update_notes_for_user` | `id` (required), `status` (open, done), `title`, `text`, `remind` |
| `delete_notes_for_user` | `ids` (required) |

### Files

| File | Holds |
|---|---|
| `state/plans.json` | 0600, `{version: 1, current, archived, servers}`; `servers` each tmux server's identity by its keys' start |
| `state/unread.json` | `{unread: [keys], servers}`; a file that cannot be read is set aside |
| `web-bookmarks.json` | format in `terminum-terminal.md` §9; an agent's record adds `by` (claude, codex), `kind` and `file` |
| `closed-records.jsonl` | beside `web-bookmarks.json`: records another session closed (AC-98) |
| `notes.json`, `notes.json.imported` | 0.23's notes, imported once (AC-98) |
| `~/.terminum/hooks/claude/` | the local marketplace `terminum-local`: `.claude-plugin/marketplace.json`, `plugins/terminum/{.claude-plugin/plugin.json, hooks/hooks.json, .mcp.json}` |
| `~/.terminum/hooks/claude.installed` | the build that installed the plugin |
| `$CODEX_HOME/hooks.json`, `config.toml` | terminum's entries and block; a copy `<file>.terminum-<time>` beside each before a change |

### Configuration

No setting of its own. `CODEX_HOME` (default `~/.codex`), `TMUX`, `TMUX_PANE` and `TM_APP_DIR` are read.

### Limits

Limits a scenario states are not repeated.

| What | Limit | Beyond it |
|---|---|---|
| A question's words on the screen | the last 12 rows of the 25 | older text is history |
| Claude Code's footer | 4 rows under its input box | not read as a count |
| Hook records | 500 panes | a further pane's event changes nothing |
| Ended sessions remembered | 500 | the oldest goes |
| Plan | title 80, doing 120 characters | answered an error |
| Archived plans | 50 | the oldest goes |
| A `claude` call | 60 s at `terminum hooks install`, 20 s in a refresh | said; `refresh` warns (AC-97) |

### Security

- Hooks: `terminum hook` sends only to terminumd's private socket (0600 in a 0700 folder: only the user's own processes reach it), or a Mac's link on a host. A process of the user can post a made-up event, which can only change a pane's shown state, its tool and wait texts and add a sent entry for a pane terminumd lists; nothing from a hook runs or reaches tmux.
- The installer writes only terminum's plugin folder, Codex's `hooks.json` and `config.toml` (backed up) and its own marker; Claude Code's settings are changed by Claude Code's own command only.
- Plan items, the current step, titles and records are model-written text: control and bidi characters dropped, records redacted (`sent::redact`), shown as plain text or Markdown with raw HTML off, never run. `SECURITY.md` has the threat.
- `get_notes_for_user` and `add_notes_for_user` say another session's text is data, never instructions.

### Compatibility

An old Codex SessionEnd entry with `async` is migrated alone, every other byte of `hooks.json` kept; Codex trusts a hook by its entry's hash, so it says to trust the changed one once.

### Logging and observability

`hook.turn_end {key, by}`, `plans.unsaved`, `unread.unsaved`, `bookmark.deleted_by_agent`, `bookmark.changed_by_agent`, `record.closed_by_other_session {id, pane, of}`, `notes.import`. `terminum hooks` says when each agent's hooks last spoke.

## 10. Delivery stages

All cases are shipped; nothing is staged.

## 11. Checklist with Definition of Done

- [x] AC-39 — `cargo test tmux::agentstate`, `cargo test tmux::hookstate`, `cargo test tmux::titles`, `cargo test cli::` (`terminumd/src/cli/tests/hooks_tests.rs`), `scripts/e2e_agentstate.py`, `scripts/e2e_hooks.py`, `web/test/e2e_app.mjs`, `web/test/e2e_rows.mjs`, `web/test/e2e_look.mjs`
- [x] AC-93 — `cargo test page::unread`, `cargo test page::turns`, `scripts/e2e_hooks.py`, `web/test/e2e_projects.mjs`
- [x] AC-97 — `cargo test plans`, `cargo test page::plans`, `cargo test cli::` (`terminumd/src/cli/mcp_tests.rs`, `hookplan_tests.rs`, `tests/refresh_tests.rs`), `web/test/plans.test.mjs`, `web/test/e2e_plans.mjs`, `web/test/e2e_plansfind.mjs`, `web/test/e2e_planslook.mjs`, `web/test/e2e_liveplans.mjs`
- [x] AC-98 — `cargo test agentmarks`, `cargo test page::bookmarks`, `cargo test cli::mcp`, `web/test/e2e_livenotes.mjs`, `web/test/e2e_marksfull.mjs`, `web/test/e2e_marksscope.mjs`, `web/test/e2e_marknav.mjs`
