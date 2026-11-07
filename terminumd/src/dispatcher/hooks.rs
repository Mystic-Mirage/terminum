// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What an agent's hook event means to its run (`terminum-dispatcher.md` AC-121 to AC-124): acted on
//! at once, written when it changes. A hook's fields are data: any process of the user can post one,
//! so only a shape-checked ID is kept, and its words are cleaned.

use serde_json::Value;

use super::runs::{self, Run};
use super::{move_task, store};
use crate::app::Shared;
use crate::plans::rules;
use crate::tasks::ops;

/// The shape a session ID must have to reach a command line (§9 Resume): no flag.
pub fn good_id(id: &str) -> bool {
    id.len() <= 100 && id.starts_with(|c: char| c.is_ascii_alphanumeric()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// What the event changes in `r`: whether it moved the task, whether it was activity. `typed`: this
/// prompt is what terminum typed.
pub(super) fn apply(tx: &rusqlite::Transaction, r: &mut Run, ev: &Value, typed: bool) -> Result<(bool, bool), String> {
    let event = ev["event"].as_str().unwrap_or("");
    let session = ev["session"].as_str().filter(|s| good_id(s));
    let mut moved = false;
    let mut active = !r.nudge_turn;                    // the turn our request started is no new work (AC-123)
    match event {
        "SessionStart" => {
            r.agent_session = session.map(str::to_string).or(r.agent_session.take());
            active = false;
        }
        "UserPromptSubmit" if r.is_starting() => {
            r.state = "working".into();
            r.prompt = None;
            r.new_work();
            r.agent_session = session.map(str::to_string).or(r.agent_session.take());
            let project = ops::project_row(tx, &r.project)?.0;
            ops::set_owner(tx, &r.task, &ops::agent_user(tx, &r.kind, &project)?, ops::now())?;
            if r.status != "IN PROGRESS" {
                move_task(tx, r, "IN PROGRESS", "The agent took it.")?;
                moved = true;
            }
        }
        "UserPromptSubmit" if typed => active = false,
        "UserPromptSubmit" => {
            // a human's own prompt: new work (AC-122)
            active = true;
            r.new_work();
            if r.status != "IN PROGRESS" {
                move_task(tx, r, "IN PROGRESS", "Dispatcher moved it: new work in its session")?;
                moved = true;
            }
        }
        "Stop" => {
            r.turn_ended = true;
            r.open_tools = 0;
            r.nudge_turn = false;
        }
        "StopFailure" => {
            r.turn_ended = true;
            r.open_tools = 0;
            active = true;
            if !r.held && !super::plan::finished(&r.status) {
                let why: String = rules::clean(ev["error"].as_str().unwrap_or("an error")).chars().take(60).collect();
                move_task(tx, r, "FAILED", &format!("The agent's turn failed: {why}"))?;
                moved = true;
            }
        }
        "PreToolUse" => r.open_tools += 1,
        "PostToolUse" | "PostToolUseFailure" => r.open_tools = (r.open_tools - 1).max(0),
        "SessionEnd" => active = false,
        _ => {}
    }
    Ok((moved, active))
}

/// Pane `key`'s agent spoke: its run, if Dispatcher started it. A pane with none costs a set's lookup.
pub fn heard(app: &Shared, key: &str, ev: &Value) {
    if !app.dispatcher.has_run(key) {
        return;
    }
    app.dispatcher.heard_facts(key, ev);
    let (app, key, ev) = (app.clone(), key.to_string(), ev.clone());
    tokio::spawn(async move {
        let typed = ev["event"] == "UserPromptSubmit" && ev["prompt"].as_str().is_some_and(|p| app.dispatcher.was_typed(&key, p));
        let (k, e) = (key.clone(), ev.clone());
        let done = store(&app, move |tx| {
            let Some(mut r) = runs::in_pane(tx, &k)? else { return Ok(None) };
            let was = r.clone();
            let (moved, active) = apply(tx, &mut r, &e, typed)?;
            if r != was {
                runs::save(tx, &r)?;
            }
            Ok(Some((r.task_id.clone(), moved, active)))
        }).await;
        match done {
            Ok(Some((task, moved, active))) => {
                if active {
                    app.dispatcher.touched(&key);
                }
                if moved {
                    crate::tasks::api::told(&app, &task);
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
