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
    /// Closes the pane; `keep` keeps the screen's last rows on the task first. The run is closed when tmux did it.
    Close { words: String, keep: bool },
}

pub fn finished(status: &str) -> bool {
    matches!(status, "DONE" | "FAILED")
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
            Some(0) => "exit 0".to_string(),
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

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
