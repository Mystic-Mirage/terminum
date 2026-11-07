// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What an agent's hook event means to its run (`terminum-dispatcher.md` AC-121 to AC-124): acted on
//! at once, written when it changes. A hook's fields are data: only a shape-checked ID is kept.

use serde_json::Value;

use super::runs::{self, Run};
use super::{heard_change, move_task, say, write};
use crate::app::Shared;
use crate::tasks::ops;

/// The shape a session ID must have to reach a shell (§9 Resume).
pub fn good_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 100 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// What the event changes in `run`; what is to be said on its task. Pure of tmux; `typed`: this prompt
/// is what terminum typed.
pub(super) fn apply(tx: &rusqlite::Transaction, r: &mut Run, ev: &Value, typed: bool, active: &mut bool) -> Result<bool, String> {
    let event = ev["event"].as_str().unwrap_or("");
    let session = ev["session"].as_str().unwrap_or("");
    let quiet = r.nudge_turn;                          // the turn our request started is no new work (AC-123)
    let mut moved = false;
    match event {
        "SessionStart" => {
            r.hooked = true;
            if good_id(session) {
                r.agent_session = Some(session.to_string());
            }
        }
        "UserPromptSubmit" if matches!(r.state.as_str(), "starting" | "asking" | "typed") => {
            r.state = "working".into();
            r.prompt = None;
            r.hooked = true;
            r.idle_for = 0;
            r.turn_ended = false;
            r.wait_moved = false;
            if good_id(session) {
                r.agent_session = Some(session.to_string());
            }
            let project: String = tx.query_row("SELECT p.name FROM projects p JOIN tasks t ON t.project = p.id WHERE t.id = ?1", [&r.task], |x| x.get(0)).map_err(ops::sql)?;
            let owner = ops::agent_user(tx, &r.kind, &project)?;
            tx.execute("UPDATE tasks SET owner = ?1 WHERE id = ?2", (&owner, &r.task)).map_err(ops::sql)?;
            if r.status != "IN PROGRESS" {
                move_task(tx, r, "IN PROGRESS", "The agent took it.")?;
                moved = true;
            }
        }
        "UserPromptSubmit" if typed => {}
        "UserPromptSubmit" => {
            // a human's own prompt: new work (AC-122)
            *active = true;
            r.nudge_turn = false;
            r.nudged = false;
            r.no_result_wait = false;
            r.wait_moved = false;
            r.turn_ended = false;
            r.held = false;
            r.closing_for = 0;
            if r.status != "IN PROGRESS" {
                move_task(tx, r, "IN PROGRESS", "Dispatcher moved it: new work in its session")?;
                moved = true;
            }
        }
        "Stop" => {
            r.turn_ended = true;
            r.open_tools = 0;
            r.nudge_turn = false;
            *active = !quiet;
        }
        "StopFailure" => {
            r.turn_ended = true;
            r.open_tools = 0;
            *active = true;
            if !r.held && !super::plan::finished(&r.status) {
                let why = ev["error"].as_str().unwrap_or("an error");
                move_task(tx, r, "FAILED", &format!("The agent's turn failed: {why}"))?;
                moved = true;
            }
        }
        "PreToolUse" => {
            r.open_tools += 1;
            *active = !quiet;
        }
        "PostToolUse" | "PostToolUseFailure" => {
            r.open_tools = (r.open_tools - 1).max(0);
            *active = !quiet;
        }
        "SessionEnd" => {}
        _ => *active = !quiet,
    }
    Ok(moved)
}

/// Pane `key`'s agent spoke: its run, if Dispatcher started it.
pub fn heard(app: &Shared, key: &str, ev: &Value) {
    let (app, key, ev) = (app.clone(), key.to_string(), ev.clone());
    tokio::spawn(async move {
        let typed = ev["event"] == "UserPromptSubmit" && ev["prompt"].as_str().is_some_and(|p| app.dispatcher.was_typed(&key, p));
        let (k, e) = (key.clone(), ev.clone());
        let done = write(&app, move |tx| {
            let Some(mut r) = runs::in_pane(tx, &k)? else { return Ok(None) };
            let mut active = false;
            let moved = apply(tx, &mut r, &e, typed, &mut active)?;
            runs::save(tx, &r)?;
            Ok(Some((r.task_id.clone(), moved, active)))
        }).await;
        match done {
            Ok(Some((task, moved, active))) => {
                if active {
                    app.dispatcher.touched(&key);
                }
                if moved {
                    heard_change(&app, &task);
                }
            }
            Ok(None) => return,
            Err(e) => tracing::warn!(event = "dispatcher.hook", error = %e),
        }
        // an end: the exit code decides, at a walk now (AS-127)
        if ev["event"] == "SessionEnd" && ev["reason"] != "clear" {
            app.dispatcher.soon.notify_one();
        }
    });
}

/// Says on the task that words did not reach its session, once for what was missed.
pub(super) fn not_delivered(tx: &rusqlite::Transaction, r: &Run, why: &str) -> Result<(), String> {
    say(tx, r, &format!("Not delivered: {why}"))
}
