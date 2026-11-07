// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a walk does with one run (`terminum-dispatcher.md` AC-123, AC-124, AC-125): a pure rule over
//! the run's clocks and what was just read, so every case is a test with no tmux and no waiting.

use super::runs::Run;

/// WAIT FOR HUMAN after an agent's question stands this long.
pub const WAIT: i64 = 120;
/// The request after a turn with no result, and WAIT FOR HUMAN this long after it.
pub const NUDGE: i64 = 300;
pub const AFTER_NUDGE: i64 = 600;
/// DONE, marked no result, after this long with no activity.
pub const NOTHING: i64 = 3600;
/// A session closes this long after DONE or FAILED.
pub const CLOSE: i64 = 7200;

/// A subagent silent and idle this long gets the main agent one request, then what can be done (AC-127).
pub const SUB_TELL: i64 = 600;
pub const SUB_END: i64 = 1200;

/// What a walk's reading of a session's memory says, by two walks in a row (AC-128).
#[derive(Clone, Debug, PartialEq)]
pub struct Memory {
    pub gib: f64,
    pub end_gib: f64,
    pub warn_due: bool,
    pub end_due: bool,
}

impl Memory {
    /// This walk's reading against the limits, 0 being off; `over`: the walks in a row each was passed.
    pub fn read(gib: f64, warn_gib: f64, end_gib: f64, over: &mut (u32, u32)) -> Memory {
        over.0 = if warn_gib > 0.0 && gib > warn_gib { over.0 + 1 } else { 0 };
        over.1 = if end_gib > 0.0 && gib > end_gib { over.1 + 1 } else { 0 };
        Memory { gib, end_gib, warn_due: over.0 >= 2, end_due: over.1 >= 2 }
    }
}

/// What this walk read of the run's pane.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// the pane still stands, and carries this run's mark
    pub pane: bool,
    pub marked: bool,
    /// what the window command recorded
    pub exit: Option<i32>,
    /// the agent's process still runs under the pane's shell
    pub alive: bool,
    /// its agent waits for an answer: what it asks
    pub waiting: Option<String>,
    /// its turn failed, as the list says: why
    pub failed: Option<String>,
    /// activity since the last walk (§9)
    pub active: bool,
    /// a program other than the agent and its shell runs in the pane
    pub program: Option<String>,
    /// another of the session's tasks is not DONE or FAILED
    pub others_open: bool,
    /// its tree's memory, if `ps` said
    pub memory: Option<Memory>,
    /// seconds to add to the clocks: none over a gap (AC-131)
    pub dt: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Moves the task, with the words on its history.
    Move { to: &'static str, words: String },
    /// Says this on the task.
    Say(String),
    /// Asks the agent once for its result, through the gate.
    Nudge,
    /// Gives the agent the last warning: seconds left of the budget (AC-126).
    Warn(i64),
    /// Ends the agent: the task was moved first, so its end counts for nothing (AC-123).
    End,
    /// Tells the agent how much memory its session uses, once (AC-128).
    MemoryWarn(f64),
    /// Closes the pane; `keep` keeps the screen's last rows on the task first. The run is closed when tmux did it.
    Close { words: String, keep: bool },
}

/// When the last warning goes: 3% of the budget left, at least 2 minutes (AC-126).
pub fn warn_at(total: i64) -> i64 {
    (total * 3 / 100).max(120)
}

pub fn finished(status: &str) -> bool {
    matches!(status, "DONE" | "FAILED")
}

/// The task fails first, so the end Dispatcher causes counts for nothing; then the agent ends.
fn end_failed(run: &mut Run, acts: &mut Vec<Act>, words: String) {
    run.state = "gone".into();
    acts.push(Act::Move { to: "FAILED", words });
    acts.push(Act::End);
}

/// The walk's step for `run`: its clocks change here, what is to be done comes back.
pub fn step(run: &mut Run, f: &Facts) -> Vec<Act> {
    let mut acts = Vec::new();
    let free = !run.held;                              // a human's move stands (AC-124)
    let movable = free && !finished(&run.status);
    if !f.pane || !f.marked {
        run.state = "closed".into();
        if f.pane {
            run.pane_key = None;
        }
        let words = if f.pane { "Its pane is no longer this run's: the link is dropped." } else { "Its window was closed." };
        if movable {
            acts.push(Act::Move { to: "FAILED", words: words.to_string() });
        } else {
            acts.push(Act::Say(format!("Session ended: {}", words.to_lowercase())));
        }
        return acts;
    }
    // the agent's end: its exit code tells a clean one from a signal (AS-127)
    if run.state != "gone" && !f.alive {
        run.state = "gone".into();
        run.open_tools = 0;
        let how = match f.exit {
            Some(code) => format!("exit {code}"),
            None => "no exit recorded".to_string(),
        };
        if movable {
            if f.exit == Some(0) {
                acts.push(Act::Move { to: "DONE", words: "The session ended without finish_task.".into() });
            } else {
                acts.push(Act::Move { to: "FAILED", words: format!("The agent's process ended: {how}.") });
            }
        } else {
            acts.push(Act::Say(format!("Session ended: {} ended, {how}.", run.kind)));
        }
    }
    let alive = run.state != "gone";
    if alive && movable && let Some(why) = &f.failed {
        acts.push(Act::Move { to: "FAILED", words: format!("The agent's turn failed: {why}") });
        return acts;
    }
    // a wait for an answer (AC-124)
    match (&f.waiting, alive) {
        (Some(why), true) => {
            run.waited_for += f.dt;
            if run.waited_for >= WAIT && run.status == "IN PROGRESS" && free {
                run.wait_moved = true;
                acts.push(Act::Move { to: "WAIT FOR HUMAN", words: if why.is_empty() { "It asks in its window".into() } else { why.clone() } });
            }
        }
        _ => {
            run.waited_for = 0;
            if run.wait_moved && run.status == "WAIT FOR HUMAN" && alive && f.active && free {
                run.wait_moved = false;
                acts.push(Act::Move { to: "IN PROGRESS", words: "The agent went on.".into() });
            }
        }
    }
    // no activity (AC-123); an answer waited for has no idle clock
    if f.active || !alive {
        run.idle_for = 0;
    } else if f.waiting.is_none() {
        run.idle_for += f.dt;
    }
    if alive && free && f.waiting.is_none() {
        if run.status == "IN PROGRESS" && run.turn_ended {
            if run.idle_for >= NUDGE && !run.nudged {
                run.nudged = true;
                run.nudge_turn = true;
                acts.push(Act::Nudge);
            } else if run.nudged && run.idle_for >= NUDGE + AFTER_NUDGE {
                run.no_result_wait = true;
                acts.push(Act::Move { to: "WAIT FOR HUMAN", words: "The agent stopped without a result".into() });
            }
        }
        let open_for_nothing = run.status == "IN PROGRESS" || (run.status == "WAIT FOR HUMAN" && run.no_result_wait);
        if open_for_nothing && run.idle_for >= NOTHING {
            acts.push(Act::Move { to: "DONE", words: "Dispatcher closed it: no activity for 60 minutes and no result from the agent.".into() });
        }
    }
    // the budget: only IN PROGRESS counts, never a wait for an answer (AC-126)
    if let Some(total) = run.budget.filter(|_| alive && run.status == "IN PROGRESS") {
        if f.waiting.is_none() {
            run.budget_used += f.dt;
        }
        let left = total - run.budget_used;
        if left <= 0 && !run.held {
            if !run.warned {
                acts.push(Act::Say("No warning could be given before the limit.".into()));
            }
            end_failed(run, &mut acts, format!("Time budget of {} minutes used: the agent was ended.", total / 60));
            return acts;
        }
        if !run.warned && left <= warn_at(total) {
            run.warned = true;
            acts.push(Act::Warn(left));
        }
    }
    // what it consumes: two walks in a row, as summed memory counts shared pages many times (AC-128)
    if let Some(m) = f.memory.as_ref().filter(|_| alive && !finished(&run.status)) {
        if m.end_due && free {
            end_failed(run, &mut acts, format!("Memory limit of {} GiB exceeded: {:.1} GiB", m.end_gib, m.gib));
            return acts;
        }
        if m.warn_due && !run.memory_warned {
            run.memory_warned = true;
            acts.push(Act::MemoryWarn(m.gib));
        }
    }
    // closing after DONE or FAILED (AC-125): any activity starts the 2 hours again, a human's at its
    // shell too; only an agent ended, or its turn over, is closed under no one
    if finished(&run.status) {
        if f.active {
            if !run.again_said && run.closing_for > 0 {
                run.again_said = true;
                acts.push(Act::Say("The 2 hours before closing start again: there is activity in the session.".into()));
            }
            run.closing_for = 0;
        } else {
            run.closing_for += f.dt;
        }
        let idle = f.exit.is_some() || (f.alive && run.turn_ended);
        if run.closing_for >= CLOSE && !f.others_open && f.waiting.is_none() && idle {
            if let Some(program) = &f.program {
                run.state = "left".into();
                acts.push(Act::Say(format!("Session left open: {program} runs in it")));
            } else {
                acts.push(Act::Close { words: format!("Session ended: closed 2 hours after {}.", run.status), keep: !run.result });
            }
        }
    }
    acts
}

/// How far a subagent has been dealt with, by how long it has been silent and idle (AC-127). A busy
/// tree, or an open tool, runs neither clock: the caller does not ask.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub enum SubStep {
    #[default]
    Nothing,
    Tell,
    CannotEnd,
}

/// The step `silent` seconds reach; it is done once, when it passes what was `told`.
pub fn subagent_step(silent: i64, told: SubStep) -> Option<SubStep> {
    let reached = if silent >= SUB_END { SubStep::CannotEnd } else if silent >= SUB_TELL { SubStep::Tell } else { SubStep::Nothing };
    (reached > told).then_some(reached)
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
