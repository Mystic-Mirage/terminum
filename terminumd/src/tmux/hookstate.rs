// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a coding agent's hooks said about a pane (AC-39, `terminum hook`): its state, the tool
//! it runs, what it waits for, its subagents. A hint with a lease, not the whole truth: an
//! interrupt in Claude Code fires no Stop and a crash no SessionEnd, so a working state no event
//! renewed for LEASE (a waiting one for WAIT_LEASE) gives way to the screen's reading again
//! (agentstate.rs), as does a pane whose program changed. A turn that stopped while its
//! subagents run (in the background) is working until the last of them stops (or BG_LEASE
//! passes without an event), through an idle notification and the next prompts: its end is
//! then, for reminders and unread. Hook events come out of order (they
//! run in the background): a working event of a turn that stopped changes nothing. A record that
//! went with a host's link or a restart comes back with its session's next event; a session seen
//! to end makes none. Codex has no
//! failure event and no question event: its done and its questions are the screen's.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;

use super::agentstate::State;

/// A working state no event renewed for this long is the screen's to say again.
pub const LEASE: Duration = Duration::from_secs(30);
/// A waiting state lasts this long without an event (an approval answered by Esc fires none).
pub const WAIT_LEASE: Duration = Duration::from_secs(120);
/// A turn stopped with subagents running stays working this long without an event.
pub const BG_LEASE: Duration = Duration::from_secs(600);
/// Panes kept at most (one per pane that ran an agent).
const MOST: usize = 500;
/// How long after a Stop a counted subagent may yet show on the screen.
pub const GHOST_AFTER: Duration = Duration::from_secs(5);
/// How often the screen is asked again while such a subagent seems to run, with no page in sight.
const GHOST_EVERY: Duration = Duration::from_secs(10);
/// Ended sessions remembered (their late events make no record).
const ENDED_MOST: usize = 500;

#[derive(Clone, Debug)]
struct Rec {
    agent: &'static str,
    session: String,
    state: Option<State>,
    tool: String,
    why: String,
    subs: BTreeSet<String>,
    at: Instant,
    turn: String,
    stopped: Option<String>,
    command: Option<String>,
    /// its turn ended (a Stop, a failure, its last background subagent): not told yet
    ended: bool,
    /// the folder the agent itself works in, as its own (not a subagent's) last event said:
    /// the paths it prints are relative to it, not to the pane's (AC-35)
    cwd: String,
    /// this turn's end was counted: a screen-read end of it is no second one (AC-39)
    counted: bool,
    /// it put a question to the user that outlives the turn (Codex's request_user_input_async)
    asked: bool,
    /// its turn ended leaving it to the user: it waits until the next prompt (AC-39)
    waits: bool,
    /// when its Stop came with subagents counted: the screen says from GHOST_AFTER on if they run
    stopped_at: Option<Instant>,
}

/// What the list shows of a pane's hooks.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub agent: &'static str,
    pub state: State,
    pub tool: String,
    pub why: String,
    pub subagents: usize,
    /// its turn ended and it waits for the user: the turn is over, not busy
    pub turn_over: bool,
    /// a Stop with subagents counted, past GHOST_AFTER: the screen decides if they run
    pub check_subs: bool,
}

#[derive(Default)]
pub struct Hooks {
    /// Serialize identity acceptance and the socket handler's synchronous plan/turn effects.
    pub(crate) dispatch: Mutex<()>,
    recs: Mutex<HashMap<String, Rec>>,
    /// when each agent's hooks last spoke (terminum hooks / status: installed and running)
    pub last: Mutex<HashMap<&'static str, std::time::SystemTime>>,
    /// panes whose program changed since `take_changed` was last asked (their plans go)
    changed: Mutex<Vec<String>>,
    /// the latest sessions that ended ("agent session"), oldest first
    ended: Mutex<std::collections::VecDeque<String>>,
}

/// The coding agents with hooks (`terminum hooks`), by the name they come with.
pub const AGENTS: [&str; 2] = ["claude", "codex"];

/// `name` as one of AGENTS.
pub fn agent(name: &str) -> Option<&'static str> {
    AGENTS.iter().copied().find(|a| *a == name)
}

/// A session in `Hooks::ended`.
fn ended_key(agent: &str, session: &str) -> String {
    format!("{agent} {session}")
}

fn s(ev: &Value, k: &str) -> String {
    ev[k].as_str().unwrap_or("").to_string()
}

impl Rec {
    /// How long a working state holds: longer once a Stop waits for its background subagents.
    fn working_lease(&self) -> Duration {
        if self.stopped.is_some() && !self.subs.is_empty() { BG_LEASE } else { LEASE }
    }
}

impl Hooks {
    /// Applies event `ev` (cli/hook.rs's fields) to pane `key`; true when what the list shows changed.
    #[cfg(test)]
    pub fn apply(&self, key: &str, ev: &Value, now: Instant) -> bool {
        self.apply_event(key, ev, now).unwrap_or(false)
    }

    /// None rejects an event's identity; Some says whether its accepted state changed.
    pub fn apply_event(&self, key: &str, ev: &Value, now: Instant) -> Option<bool> {
        let agent = ev["agent"].as_str().and_then(agent)?;
        let name = s(ev, "event");
        let session = s(ev, "session");
        let turn = s(ev, "turn");
        let mut recs = self.recs.lock();
        let start = name == "SessionStart" || name == "UserPromptSubmit";
        let fresh = |r: &Rec| r.agent != agent || (!session.is_empty() && r.session != session);
        if !start && recs.get(key).is_some_and(fresh) {
            return None;
        }
        self.last.lock().insert(agent, std::time::SystemTime::now());
        if name == "SessionEnd" {
            let mut ended = self.ended.lock();
            if ended.len() >= ENDED_MOST {
                ended.pop_front();
            }
            ended.push_back(ended_key(agent, &session));
            return Some(recs.remove(key).is_some());
        }
        if recs.len() >= MOST && !recs.contains_key(key) {
            return None;
        }
        if recs.get(key).is_some_and(fresh) || name == "SessionStart" {
            recs.remove(key);
        }
        // a start or a prompt makes a record, and any event of a session not seen to end (its record went
        // with a host's link that dropped, or with a restart): a late event of an ended session makes none
        if !recs.contains_key(key) && !start && (session.is_empty() || self.ended.lock().contains(&ended_key(agent, &session))) {
            return Some(false);
        }
        let r = recs.entry(key.to_string()).or_insert_with(|| Rec { agent, session: session.clone(), state: None, tool: String::new(),
            why: String::new(), subs: BTreeSet::new(), at: now, turn: String::new(), stopped: None, command: None, ended: false, counted: false, asked: false, waits: false, stopped_at: None, cwd: String::new() });
        let cwd = s(ev, "cwd");
        let moved = cwd.starts_with('/') && s(ev, "sub").is_empty() && r.cwd != cwd;
        if moved {
            r.cwd = cwd;                                  // the list shows it (its folder, AC-51)
        }
        let before = (r.state, r.tool.clone(), r.why.clone(), r.subs.len());
        let stopped = !turn.is_empty() && r.stopped.as_deref() == Some(turn.as_str());
        let working = |r: &mut Rec| { r.state = Some(State::Working); r.why.clear(); };
        let end = |r: &mut Rec, state| {
            r.state = Some(state);
            r.tool.clear();
            r.subs.clear();
            r.stopped = Some(turn.clone());
            (r.ended, r.counted) = (true, true);
        };
        match name.as_str() {
            "SessionStart" => r.state = Some(State::Done),
            // subagents in the background run on into the next turn: a prompt does not end them
            // a subagent's own prompt starts no turn of the pane
            "UserPromptSubmit" if !s(ev, "sub").is_empty() => {}
            "UserPromptSubmit" => {
                working(r);
                r.tool.clear();
                r.stopped = None;
                r.stopped_at = None;
                r.turn = turn.clone();
                (r.counted, r.asked, r.waits) = (false, false, false);
            }
            "PreToolUse" if !stopped => {
                let tool = s(ev, "tool");
                if tool.starts_with("request_user_input_async") {   // it answers at once, the question stands
                    r.asked = true;
                    r.why = s(ev, "message");
                } else if tool.starts_with("AskUserQuestion") || tool.starts_with("request_user_input") {
                    r.state = Some(State::Waiting);
                    r.why = s(ev, "message");
                } else {
                    working(r);
                    r.tool = s(ev, "tool");
                }
            }
            "PostToolUse" | "PostToolUseFailure" | "PostToolBatch" if !stopped => {
                r.state = Some(State::Working);
                if !r.asked { r.why.clear(); }
            }
            "PermissionRequest" if !stopped => { r.state = Some(State::Waiting); r.why = format!("Approve {}", s(ev, "tool")).trim().to_string(); }
            "Notification" => match ev["kind"].as_str().unwrap_or("") {
                "permission_prompt" | "agent_needs_input" | "elicitation_dialog" | "elicitation_url_dialog" | "worker_permission_prompt" if !stopped => {
                    r.state = Some(State::Waiting);
                    r.why = s(ev, "message");
                }
                // the main thread idles while its subagents work: not done until the last stops
                "idle_prompt" if r.subs.is_empty() => r.state = Some(State::Done),
                _ => {}
            },
            // it asks the user (a tool that outlives the turn, or its last message's end): whatever it counts
            "Stop" if r.asked || ev["asks"] == true => { r.waits = true; end(r, State::Waiting); }
            // Claude Code's subagents may still run (in the background): working until the last one stops,
            // or the screen says none does (an interrupted, killed or failed one fires no SubagentStop).
            // Codex's root stops for the user: a child's result does not wake it
            "Stop" if !r.subs.is_empty() && agent == "claude" => { r.tool.clear(); r.stopped = Some(turn.clone()); r.stopped_at = Some(now); }
            "Stop" | "Interrupt" => end(r, State::Done),
            // a subagent's failure is its own: the agent goes on
            "StopFailure" if !s(ev, "sub").is_empty() => { r.subs.remove(&s(ev, "sub")); }
            "StopFailure" => { end(r, State::Failed); r.why = s(ev, "error"); }
            "SubagentStart" if !stopped => { r.subs.insert(s(ev, "sub")); working(r); }
            "SubagentStop" => {
                r.subs.remove(&s(ev, "sub"));
                if r.stopped.is_some() && r.subs.is_empty() && r.state == Some(State::Working) {
                    r.state = Some(State::Done);
                    (r.ended, r.counted) = (true, true);
                }
            }
            // a background subagent's tools: it still works
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure" | "PostToolBatch" if !r.subs.is_empty() => {}
            _ => return Some(moved),
        }
        r.at = now;
        Some(moved || before != (r.state, r.tool.clone(), r.why.clone(), r.subs.len()))
    }

    /// The pane's hooks, while they hold: None when it has none, its lease ran out, or its
    /// program is no longer the one that ran when they spoke (`command`: the pane's program now).
    /// Codex's done (it may be a failure) is the screen's to say.
    pub fn view(&self, key: &str, command: &str, now: Instant) -> Option<View> {
        let mut recs = self.recs.lock();
        let r = recs.get_mut(key)?;
        match &r.command {
            None => r.command = Some(command.to_string()),       // the program it ran in
            Some(c) if c != command => { recs.remove(key); self.changed.lock().push(key.to_string()); return None; }
            _ => {}
        }
        let state = r.state?;
        let held = match state {
            State::Working => now.duration_since(r.at) < r.working_lease(),
            State::Waiting => r.waits || now.duration_since(r.at) < WAIT_LEASE,
            State::Done => r.agent == "claude",
            State::Failed => true,
        };
        let check_subs = !r.subs.is_empty() && r.stopped_at.is_some_and(|at| now.duration_since(at) >= GHOST_AFTER);
        held.then(|| View { agent: r.agent, state, tool: r.tool.clone(), why: r.why.clone(), subagents: r.subs.len(), turn_over: r.waits, check_subs })
    }

    /// Whether pane `key`'s turn ended since this was last asked (its reminders, unread).
    pub fn take_ended(&self, key: &str) -> bool {
        self.recs.lock().get_mut(key).is_some_and(|r| std::mem::take(&mut r.ended))
    }

    /// How pane `key`'s turn ended, once `take_ended` said it did: its state and why.
    pub fn ended_as(&self, key: &str) -> Option<(State, String)> {
        self.recs.lock().get(key).and_then(|r| Some((r.state?, r.why.clone())))
    }

    /// Whether pane `key`'s hooks already counted this turn's end, so its screen's is no second one.
    /// Not for a turn its hooks left without an end (no Stop, or one that waited on subagents).
    pub fn end_counted(&self, key: &str) -> bool {
        self.recs.lock().get(key).is_some_and(|r| r.counted)
    }

    /// The screen shows no subagent running after pane `key`'s Stop: those counted fired no
    /// SubagentStop (interrupted, killed, failed). They go, its turn ends.
    pub fn drop_ghosts(&self, key: &str) {
        if let Some(r) = self.recs.lock().get_mut(key).filter(|r| !r.subs.is_empty() && r.stopped_at.is_some()) {
            r.subs.clear();
            r.stopped_at = None;
            r.state = Some(State::Done);
            r.ended = true;
            r.counted = true;
        }
    }

    /// How long until the screen must say if a Stop's counted subagents run, also with no page in
    /// sight, when the list is read seldom: that is when an end must not go unseen.
    pub fn subs_due_in(&self, now: Instant) -> Option<Duration> {
        self.recs.lock().values().filter(|r| !r.subs.is_empty()).filter_map(|r| r.stopped_at)
            .map(|at| (at + GHOST_AFTER).checked_duration_since(now).unwrap_or(GHOST_EVERY)).min()
    }

    /// How long until a hook's lease on a state runs out, also with no page in sight: a turn with
    /// no Stop ends by the read after it, which the list's slow beat must not leave waiting (AC-39).
    pub fn lease_ends_in(&self, now: Instant) -> Option<Duration> {
        self.recs.lock().values().filter_map(|r| {
            let lease = match r.state? {
                State::Working => r.working_lease(),
                State::Waiting if !r.waits => WAIT_LEASE,
                _ => return None,
            };
            (r.at + lease).checked_duration_since(now)
        }).min()
    }

    /// The panes whose turn ended and was not told yet, taken.
    pub fn take_all_ended(&self) -> Vec<String> {
        self.recs.lock().iter_mut().filter_map(|(k, r)| std::mem::take(&mut r.ended).then(|| k.clone())).collect()
    }

    /// The folder pane `key`'s agent works in, as its hooks last said (None: no hooks spoke).
    pub fn cwd(&self, key: &str) -> Option<String> {
        self.recs.lock().get(key).map(|r| r.cwd.clone()).filter(|c| !c.is_empty())
    }

    /// Whether pane `key`'s hooks last said it works.
    pub fn working(&self, key: &str) -> bool {
        self.recs.lock().get(key).is_some_and(|r| r.state == Some(State::Working))
    }

    /// Whether pane `key`'s agent has hooks that spoke (its prompts come from them).
    pub fn has(&self, key: &str) -> bool {
        self.recs.lock().contains_key(key)
    }

    /// The panes whose program changed since this was last asked.
    pub fn take_changed(&self) -> Vec<String> {
        std::mem::take(&mut *self.changed.lock())
    }

    /// Pane `key`'s agent and its session, as its hooks last said.
    pub fn session(&self, key: &str) -> Option<(&'static str, String)> {
        self.recs.lock().get(key).map(|r| (r.agent, r.session.clone()))
    }

    /// Forgets the panes of a server (`prefix`, "tmux:<host>:<socket>:") that are gone.
    pub fn retain(&self, prefix: &str, live: &[String]) {
        let live: std::collections::HashSet<&str> = live.iter().map(String::as_str).collect();
        self.recs.lock().retain(|k, _| !k.starts_with(prefix) || live.contains(k.as_str()));
    }
}

#[cfg(test)]
#[path = "hookstate_tests.rs"]
mod tests;
