// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What the session list shows of tasks: open ones by session, for their rows' marks (AC-113), and
//! by project, for the dialog of Delete Project (AC-110). Told to pages only when they change.

use std::collections::{BTreeMap, HashMap};

use parking_lot::Mutex;
use serde_json::json;

use super::ops::rows;
use super::store::Store;
use crate::app::Shared;
use crate::page::Page;

#[derive(Default, PartialEq)]
struct Held {
    counts: HashMap<String, i64>,
    sessions: BTreeMap<String, Vec<String>>,
}

#[derive(Default)]
pub struct Marks {
    held: Mutex<Held>,
    /// one read at a time: a slow one never stands over a newer; `held` stays free meanwhile
    turn: Mutex<()>,
}

impl Marks {
    /// Reads them from the store: whether they changed. Off the async workers.
    pub fn refresh(&self, store: &Store) -> bool {
        let _turn = self.turn.lock();
        let read = store.with(|tx| {
            let counts = rows(tx, "SELECT project, count(*) FROM tasks WHERE archived_at IS NULL GROUP BY project", [], |r| Ok((r.get(0)?, r.get(1)?)))?.into_iter().collect();
            let mut sessions: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (key, id) in rows(tx, "SELECT session_key, 'T-' || number FROM tasks WHERE session_key IS NOT NULL AND archived_at IS NULL AND status <> 'DONE' ORDER BY number", [], |r| Ok((r.get(0)?, r.get(1)?)))? {
                sessions.entry(key).or_default().push(id);
            }
            Ok(Held { counts, sessions })
        });
        // a passing failure says nothing about the tasks: the last numbers stand
        let Ok(read) = read else { return false };
        let mut held = self.held.lock();
        let changed = *held != read;
        *held = read;
        changed
    }

    fn message(&self) -> String {
        let held = self.held.lock();
        json!({"t": "taskmarks", "marks": held.sessions, "counts": held.counts}).to_string()
    }
}

/// A page that connects learns them.
pub fn hello(app: &Shared, page: &Page) {
    page.send(app.task_marks.message());
}

/// Every page learns them (after a change).
pub fn tell(app: &Shared) {
    app.pages.tell(&app.task_marks.message());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::ops::{create, move_to, NewTask};
    use crate::tasks::store::HUMAN;

    #[test]
    fn ac113_marks_hold_open_tasks_and_say_when_they_changed() {
        let store = Store::open(crate::testutil::fresh_dir("marks").join("kanban.sqlite"));
        let marks = Marks::default();
        assert!(!marks.refresh(&store), "nothing yet");
        let key = "tmux:local:terminum:%7";
        let t = store.with(|tx| create(tx, HUMAN, NewTask { title: "a".into(), session: Some(key.into()), ..Default::default() }, 1)).unwrap();
        assert!(marks.refresh(&store));
        assert!(!marks.refresh(&store), "the same again is no change");
        let told: serde_json::Value = serde_json::from_str(&marks.message()).unwrap();
        assert_eq!(told, json!({"t": "taskmarks", "marks": {key: [t.id]}, "counts": {"default": 1}}));
        store.with(|tx| move_to(tx, HUMAN, &t.id, "DONE", 2)).unwrap();
        assert!(marks.refresh(&store));
        let told: serde_json::Value = serde_json::from_str(&marks.message()).unwrap();
        assert_eq!(told, json!({"t": "taskmarks", "marks": {}, "counts": {"default": 1}}), "DONE leaves its row, and still counts in its project");
    }

    #[test]
    fn ac113_a_store_that_fails_keeps_the_last_numbers() {
        let marks = Marks::default();
        marks.held.lock().counts.insert("api".into(), 3);
        assert!(!marks.refresh(&Store::absent()));
        assert_eq!(marks.held.lock().counts["api"], 3);
    }
}
