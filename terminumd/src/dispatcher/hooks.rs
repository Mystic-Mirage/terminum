// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What an agent's hook event means to its run (`terminum-dispatcher.md` AC-121 to AC-124): acted on
//! at once, written when it changes. A hook's fields are data: any process of the user can post one,
//! so only a shape-checked ID is kept, and its words are cleaned.

use std::future::Future;

use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use super::runs::{self, Run};
use super::{move_all, move_one, move_task, store};
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
            let agent = ops::agent_user(tx, &r.kind, &project)?;
            for id in &r.ids {
                ops::set_owner(tx, &ops::key_of(tx, id)?, &agent, ops::now())?;
            }
            move_all(tx, r, "IN PROGRESS", "The agent took it.")?;
            moved = true;
        }
        "UserPromptSubmit" if typed => active = false,
        "UserPromptSubmit" => {
            // a human's own prompt: new work (AC-122); to which of several finished tasks, it does not say
            active = true;
            r.new_work();
            if r.status != "IN PROGRESS" && (r.ids.len() == 1 || !r.open.is_empty()) {
                for id in r.targets() {
                    move_one(tx, id, "IN PROGRESS", "Dispatcher moved it: new work in its session")?;
                }
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
            if !super::plan::finished(&r.status) {
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

/// Hooks' events waiting for their turn: what comes first is applied first.
pub(super) struct Events {
    sender: UnboundedSender<(String, Value)>,
    waiting: Mutex<Option<UnboundedReceiver<(String, Value)>>>,
}

impl Default for Events {
    fn default() -> Events {
        let (sender, waiting) = unbounded_channel();
        Events { sender, waiting: Mutex::new(Some(waiting)) }
    }
}

/// Pane `key`'s agent spoke: its run, if Dispatcher started it. A pane with none costs a lookup.
pub fn heard(app: &Shared, key: &str, ev: &Value) {
    if !app.dispatcher.has_run(key) {
        return;
    }
    app.dispatcher.heard_facts(key, ev);
    let _ = app.dispatcher.events.sender.send((key.to_string(), ev.clone()));
}

/// What one event does to its run.
async fn one_event(app: Shared, key: String, ev: Value) {
    let typed = ev["event"] == "UserPromptSubmit" && ev["prompt"].as_str().is_some_and(|p| app.dispatcher.was_typed(&key, p));
    let (k, e) = (key.clone(), ev.clone());
    let done = store(&app, move |tx| {
        let Some(mut r) = runs::in_pane(tx, &k)? else { return Ok(None) };
        let was = r.clone();
        let (moved, active) = apply(tx, &mut r, &e, typed)?;
        if r != was {
            runs::save(tx, &r)?;
        }
        Ok(Some((r, moved, active)))
    }).await;
    match done {
        Ok(Some((r, moved, active))) => {
            if active {
                app.dispatcher.touched(&key);
            }
            if moved {
                super::told_all(&app, &r);
            }
        }
        Ok(None) => return,
        Err(e) => tracing::warn!(event = "dispatcher.hook", error = %e),
    }
    // an end: the exit code decides, at a walk now (AS-127)
    if ev["event"] == "SessionEnd" && ev["reason"] != "clear" {
        app.dispatcher.soon.notify_one();
    }
}

/// Each item in its turn; one that panics is said and takes no other with it.
async fn in_turn<T, F: Future<Output = ()> + Send + 'static>(mut items: UnboundedReceiver<T>, work: impl Fn(T) -> F) {
    while let Some(item) = items.recv().await {
        if let Err(e) = tokio::spawn(work(item)).await {
            tracing::error!(event = "dispatcher.hook", error = %e);
        }
    }
}

/// Hooks' events, one at a time (AC-131): a Stop never overtakes the prompt before it.
pub async fn hearing(app: Shared) {
    let Some(events) = app.dispatcher.events.waiting.lock().take() else { return };
    in_turn(events, move |(key, ev)| one_event(app.clone(), key, ev)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn ac131_a_slow_event_is_not_overtaken_by_the_next() {
        let (sender, items) = unbounded_channel();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        for item in [("Prompt", 40), ("Stop", 0)] {
            sender.send(item).unwrap();
        }
        drop(sender);
        in_turn(items, move |(name, wait)| {
            let log = log.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(wait)).await;
                log.lock().push(name);
            }
        }).await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(*seen.lock(), vec!["Prompt", "Stop"]);
    }

    #[tokio::test]
    async fn ac131_a_panic_takes_only_its_own_event() {
        let (sender, items) = unbounded_channel();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        for item in [true, false] {
            sender.send(item).unwrap();
        }
        drop(sender);
        in_turn(items, move |bad| {
            let log = log.clone();
            async move {
                assert!(!bad, "a test's own panic");
                log.lock().push(bad);
            }
        }).await;
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(*seen.lock(), vec![false]);
    }

    #[test]
    fn ac121_a_walk_that_read_earlier_keeps_a_pane_opened_since() {
        let d = crate::dispatcher::Dispatcher::default();
        d.watch_pane("gone");
        std::thread::sleep(Duration::from_millis(2));
        let read_at = Instant::now();
        std::thread::sleep(Duration::from_millis(2));
        d.watch_pane("opening");
        d.keep_panes(["live".to_string()].into_iter(), read_at);
        assert!(d.has_run("live"), "what the walk read");
        assert!(d.has_run("opening"), "its first hooks must be heard: the walk could not have read it");
        assert!(!d.has_run("gone"), "older than the read, and no run has it");
    }
}
