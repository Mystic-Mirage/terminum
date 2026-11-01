// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a coding agent's hooks said about a pane (AC-39, `terminum hook`): its state, the tool
//! it runs, what it waits for, its subagents. A hint with a lease, not the whole truth: an
//! interrupt in Claude Code fires no Stop and a crash no SessionEnd, so a working state no event
//! renewed for LEASE (a waiting one for WAIT_LEASE) gives way to the screen's reading again
//! (agentstate.rs), as does a pane whose program changed. A turn that stopped while its
//! subagents run (in the background) is working until the last of them stops (or BG_LEASE
//! passes without an event), through an idle notification and the next prompts: its end is
//! then, for reminders and unread. Hook events come out of order (they
//! run in the background): a working event of a turn that stopped changes nothing. Codex has no
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
}

/// What the list shows of a pane's hooks.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub agent: &'static str,
    pub state: State,
    pub tool: String,
    pub why: String,
    pub subagents: usize,
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
}

/// The coding agents with hooks (`terminum hooks`), by the name they come with.
pub const AGENTS: [&str; 2] = ["claude", "codex"];

/// `name` as one of AGENTS.
pub fn agent(name: &str) -> Option<&'static str> {
    AGENTS.iter().copied().find(|a| *a == name)
}

fn s(ev: &Value, k: &str) -> String {
    ev[k].as_str().unwrap_or("").to_string()
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
            return Some(recs.remove(key).is_some());
        }
        if recs.len() >= MOST && !recs.contains_key(key) {
            return None;
        }
        if recs.get(key).is_some_and(fresh) || name == "SessionStart" {
            recs.remove(key);
        }
        // only a start or a prompt makes a record: a late event of an ended session makes none
        if !recs.contains_key(key) && name != "SessionStart" && name != "UserPromptSubmit" {
            return Some(false);
        }
        let r = recs.entry(key.to_string()).or_insert_with(|| Rec { agent, session: session.clone(), state: None, tool: String::new(),
            why: String::new(), subs: BTreeSet::new(), at: now, turn: String::new(), stopped: None, command: None, ended: false, cwd: String::new() });
        let cwd = s(ev, "cwd");
        let moved = cwd.starts_with('/') && s(ev, "sub").is_empty() && r.cwd != cwd;
        if moved {
            r.cwd = cwd;                                  // the list shows it (its folder, AC-51)
        }
        let before = (r.state, r.tool.clone(), r.why.clone(), r.subs.len());
        let stopped = !turn.is_empty() && r.stopped.as_deref() == Some(turn.as_str());
        let working = |r: &mut Rec| { r.state = Some(State::Working); r.why.clear(); };
        match name.as_str() {
            "SessionStart" => r.state = Some(State::Done),
            // subagents in the background run on into the next turn: a prompt does not end them
            "UserPromptSubmit" => { working(r); r.tool.clear(); r.stopped = None; r.turn = turn.clone(); }
            "PreToolUse" if !stopped => {
                if s(ev, "tool").starts_with("AskUserQuestion") {
                    r.state = Some(State::Waiting);
                    r.why = s(ev, "message");
                } else {
                    working(r);
                    r.tool = s(ev, "tool");
                }
            }
            "PostToolUse" | "PostToolUseFailure" | "PostToolBatch" if !stopped => { working(r); }
            "PermissionRequest" if !stopped => { r.state = Some(State::Waiting); r.why = format!("Approve {}", s(ev, "tool")).trim().to_string(); }
            "Notification" => match ev["kind"].as_str().unwrap_or("") {
                "permission_prompt" | "agent_needs_input" | "elicitation_dialog" if !stopped => { r.state = Some(State::Waiting); r.why = s(ev, "message"); }
                // the main thread idles while its subagents work: not done until the last stops
                "idle_prompt" if r.subs.is_empty() => r.state = Some(State::Done),
                _ => {}
            },
            // its subagents still run (in the background): working until the last one stops
            "Stop" if !r.subs.is_empty() => { r.tool.clear(); r.stopped = Some(turn.clone()); }
            "Stop" | "Interrupt" => { r.state = Some(State::Done); r.tool.clear(); r.subs.clear(); r.stopped = Some(turn.clone()); r.ended = true; }
            "StopFailure" => { r.state = Some(State::Failed); r.why = s(ev, "error"); r.tool.clear(); r.subs.clear(); r.stopped = Some(turn.clone()); r.ended = true; }
            "SubagentStart" if !stopped => { r.subs.insert(s(ev, "sub")); working(r); }
            "SubagentStop" => {
                r.subs.remove(&s(ev, "sub"));
                if r.stopped.is_some() && r.subs.is_empty() && r.state == Some(State::Working) {
                    r.state = Some(State::Done);
                    r.ended = true;
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
        let lease = if r.stopped.is_some() && !r.subs.is_empty() { BG_LEASE } else { LEASE };
        let held = match state {
            State::Working => now.duration_since(r.at) < lease,
            State::Waiting => now.duration_since(r.at) < WAIT_LEASE,
            State::Done => r.agent == "claude",
            State::Failed => true,
        };
        held.then(|| View { agent: r.agent, state, tool: r.tool.clone(), why: r.why.clone(), subagents: r.subs.len() })
    }

    /// Whether pane `key`'s turn ended since this was last asked (its reminders, unread).
    pub fn take_ended(&self, key: &str) -> bool {
        self.recs.lock().get_mut(key).is_some_and(|r| std::mem::take(&mut r.ended))
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
