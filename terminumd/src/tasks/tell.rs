// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A human's move reaches the task's session (AC-114): fixed words with a T-number, a column and
//! a user name, never a title, text or comment, as an agent or a host can write those and this
//! reaches an agent as the user's words. It rides the next hook that carries context; what types
//! into an idle session is Dispatcher's (`terminum-dispatcher.md` §9).

use std::collections::HashMap;

use parking_lot::Mutex;
use rusqlite::OptionalExtension;
use serde_json::Value;

use super::ops;
use super::store::DISPATCHER;
use crate::app::Shared;
use crate::tmux::agentstate::State;

/// Lines waiting for a pane's next context, with their tasks.
#[derive(Default)]
pub struct Lines(Mutex<HashMap<String, Vec<(String, String)>>>);

impl Lines {
    /// The newest few of a pane: a session with no next hook never holds many.
    pub(crate) fn push(&self, key: &str, task: &str, line: String) {
        const KEPT: usize = 20;
        let mut all = self.0.lock();
        let lines = all.entry(key.to_string()).or_default();
        lines.push((task.to_string(), line));
        if lines.len() > KEPT {
            lines.remove(0);
        }
    }

    pub(crate) fn take(&self, key: &str) -> Vec<(String, String)> {
        self.0.lock().remove(key).unwrap_or_default()
    }
}

fn say(app: &Shared, task: &str, text: String) {
    let a = app.clone();
    let id = task.to_string();
    tokio::task::spawn_blocking(move || {
        let done = a.tasks.with(|tx| ops::log(tx, &ops::key_of(tx, &id)?, DISPATCHER, "told", &text, None, ops::now()).map(|_| ()));
        match done {
            Ok(()) => super::api::change_heard(&a, &id),   // an open window shows it
            Err(e) => tracing::warn!(event = "tasks.told", error = %e),
        }
    });
}

/// What a turn's end says of the task its session works on (AC-136), as hookstate read it: a turn
/// that ends in a question or while subagents run is no stop.
fn ended_words(state: State, why: &str) -> Option<String> {
    match state {
        State::Done => Some("The agent stopped with this task open".to_string()),
        State::Failed => Some(format!("The agent stopped: {}", if why.is_empty() { "an error" } else { why })),
        _ => None,
    }
}

/// An agent's event is said on the task in IN PROGRESS of its pane, once in a row: nothing moves
/// by it (terminum-dispatcher.md AC-123 moves only what Dispatcher started); `Ok(None)`: nothing to say.
fn say_stopped(tx: &rusqlite::Transaction, key: &str, words: &str, at: i64) -> Result<Vec<String>, String> {
    let mut said = Vec::new();
    for (row, id) in ops::rows(tx, "SELECT id, 'T-' || number FROM tasks WHERE session_key = ?1 AND status = 'IN PROGRESS' AND archived_at IS NULL", [key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let last: Option<String> = tx.query_row("SELECT text FROM events WHERE task = ?1 ORDER BY at DESC, rowid DESC LIMIT 1", [&row], |r| r.get(0)).optional().map_err(ops::sql)?;
        if last.as_deref() != Some(words) {
            ops::log(tx, &row, DISPATCHER, "told", words, None, at)?;
            said.push(id);
        }
    }
    Ok(said)
}

/// Off the async workers; an open window shows it.
fn said_stopped(app: &Shared, key: &str, words: String) {
    let (a, key) = (app.clone(), key.to_string());
    tokio::task::spawn_blocking(move || match a.tasks.with(|tx| say_stopped(tx, &key, &words, ops::now())) {
        Ok(said) => said.iter().for_each(|id| super::api::change_heard(&a, id)),
        Err(e) => tracing::warn!(event = "tasks.told", error = %e),
    });
}

/// A pane's turn ended: what became of it is said on its task.
pub fn turn_ended(app: &Shared, key: &str) {
    if let Some(words) = app.hooks.ended_as(key).and_then(|(state, why)| ended_words(state, &why)) {
        said_stopped(app, key, words);
    }
}

/// A pane's agent session ended; /clear goes on.
pub fn session_ended(app: &Shared, key: &str, ev: &Value) {
    if ev["event"] == "SessionEnd" && ev["reason"] != "clear" {
        said_stopped(app, key, "The agent's session ended with this task open".to_string());
    }
}

/// The pane's next context, joined to what else is due; each line then is said on its task.
pub fn with_lines(app: &Shared, key: &str, remind: Option<String>) -> Option<String> {
    let lines = app.task_lines.take(key);
    if lines.is_empty() {
        return remind;
    }
    for (task, _) in &lines {
        say(app, task, "Told the session".to_string());
    }
    let words: Vec<String> = remind.into_iter().chain(lines.into_iter().map(|(_, l)| l)).collect();
    Some(words.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac132_what_a_turn_s_end_says() {
        assert_eq!(ended_words(State::Failed, "rate_limit").as_deref(), Some("The agent stopped: rate_limit"));
        assert_eq!(ended_words(State::Failed, "").as_deref(), Some("The agent stopped: an error"));
        assert_eq!(ended_words(State::Done, "").as_deref(), Some("The agent stopped with this task open"));
        assert_eq!(ended_words(State::Waiting, "Which one?"), None, "a question is no stop");
        assert_eq!(ended_words(State::Working, ""), None, "subagents still run");
    }

    #[test]
    fn ac132_it_is_said_on_the_task_in_progress_once_and_moves_nothing() {
        use crate::tasks::ops::{create, move_to, NewTask};
        use crate::tasks::store::{Store, HUMAN};
        let s = Store::open(crate::testutil::fresh_dir("tell-stopped").join("kanban.sqlite"));
        let key = "tmux:local:t:%7";
        let t = s.with(|tx| create(tx, HUMAN, NewTask { title: "a".into(), session: Some(key.into()), ..Default::default() }, 1)).unwrap();
        let said = |w: &str| s.with(|tx| say_stopped(tx, key, w, 2)).unwrap();
        assert!(said("The agent stopped with this task open").is_empty(), "in TODO: nothing to say");
        s.with(|tx| move_to(tx, HUMAN, &t.id, "IN PROGRESS", 2)).unwrap();
        assert_eq!(said("The agent stopped with this task open"), [t.id.clone()]);
        assert!(said("The agent stopped with this task open").is_empty(), "once in a row");
        assert_eq!(said("The agent stopped: rate_limit"), [t.id.clone()], "another word is said");
        assert_eq!(s.with(|tx| ops::get(tx, &t.id)).unwrap().status, "IN PROGRESS", "nothing moved");
    }

    #[test]
    fn ac114_lines_wait_per_pane_and_go_once() {
        let l = Lines::default();
        l.push("a", "T-1", "one".into());
        l.push("a", "T-2", "two".into());
        l.push("b", "T-3", "three".into());
        assert_eq!(l.take("a").len(), 2);
        assert!(l.take("a").is_empty());
        assert_eq!(l.take("b")[0].1, "three");
    }
}
