// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A task DONE for 2 days leaves the board by itself (AC-118): with its subtasks that are DONE, never
//! while a subtask is open or a run still holds it.

use rusqlite::Transaction;

use super::ops::{self, rows};
use super::store::DISPATCHER;

const AFTER: i64 = 2 * 24 * 3600;
/// In one beat: a store long unwalked empties over several, holding up nobody.
const MOST: i64 = 200;

/// What is due at `now`, archived: their IDs. A move or a return starts the 2 days again.
pub fn due(tx: &Transaction, now: i64) -> Result<Vec<String>, String> {
    let found: Vec<(String, String)> = rows(tx, "SELECT t.id, 'T-' || t.number FROM tasks t WHERE t.archived_at IS NULL AND t.status = 'DONE'
        AND COALESCE((SELECT max(e.at) FROM events e WHERE e.task = t.id AND e.kind IN ('move', 'archive')), t.updated_at) <= ?1
        AND NOT EXISTS (SELECT 1 FROM tasks s WHERE s.parent = t.id AND s.status <> 'DONE' AND s.archived_at IS NULL)
        ORDER BY t.number LIMIT ?2", (now - AFTER, MOST), |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut archived = Vec::new();
    for (key, id) in found {
        if ops::get(tx, &id)?.archived_at.is_some() || crate::dispatcher::runs::holding(tx, &key)?.is_some() {
            continue;
        }
        let kids: Vec<String> = rows(tx, "SELECT 'T-' || number FROM tasks WHERE parent = ?1 AND status = 'DONE' AND archived_at IS NULL", [&key], |r| r.get(0))?;
        for task in kids.into_iter().chain(std::iter::once(id)) {
            ops::archive(tx, DISPATCHER, &task, true, now)?;
            archived.push(task);
        }
    }
    Ok(archived)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::runs::{self, Run};
    use crate::tasks::ops::{NewTask, Task};
    use crate::tasks::store::{Store, HUMAN};

    const DAY: i64 = 24 * 3600;

    fn store(name: &str) -> Store {
        Store::open(crate::testutil::fresh_dir(name).join("kanban.sqlite"))
    }

    fn done(tx: &Transaction, title: &str, parent: Option<&str>, at: i64) -> Task {
        let t = ops::create(tx, HUMAN, NewTask { title: title.into(), parent: parent.map(str::to_string), ..Default::default() }, at).unwrap();
        ops::move_to(tx, HUMAN, &t.id, "DONE", at).unwrap();
        t
    }

    #[test]
    fn ac118_done_for_two_days_archives_itself_and_says_who() {
        store("archive-due").with(|tx| {
            let old = done(tx, "old", None, 100);
            let young = done(tx, "young", None, 100 + DAY);
            assert_eq!(due(tx, 99 + 2 * DAY)?, Vec::<String>::new(), "not yet");
            assert_eq!(due(tx, 100 + 2 * DAY)?, vec![old.id.clone()]);
            assert!(ops::get(tx, &young.id)?.archived_at.is_none());
            let now = ops::get(tx, &old.id)?;
            assert_eq!(now.status, "DONE", "it keeps the status it had");
            assert!(ops::history(tx, &old.id)?.to_string().contains("Archived by Dispatcher"));
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac118_its_done_subtasks_go_with_it_an_open_one_holds_it() {
        store("archive-kids").with(|tx| {
            let parent = done(tx, "parent", None, 100);
            let kid = done(tx, "kid", Some(&parent.id), 100 + DAY);
            let open = ops::create(tx, HUMAN, NewTask { title: "open".into(), parent: Some(parent.id.clone()), ..Default::default() }, 100)?;
            let later = 100 + 3 * DAY + 1;
            assert_eq!(due(tx, later)?, vec![kid.id.clone()], "the parent stays until its subtasks are DONE or archived");
            assert!(ops::get(tx, &parent.id)?.archived_at.is_none());
            ops::archive(tx, HUMAN, &open.id, true, later)?;
            assert_eq!(due(tx, later)?, vec![parent.id.clone()], "the open one was archived by hand");
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac118_a_subtask_done_goes_along_however_young() {
        store("archive-young-kid").with(|tx| {
            let parent = done(tx, "parent", None, 100);
            let kid = done(tx, "kid", Some(&parent.id), 100 + 2 * DAY);
            assert_eq!(due(tx, 100 + 2 * DAY)?, vec![kid.id, parent.id]);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac118_a_return_starts_the_two_days_again() {
        store("archive-return").with(|tx| {
            let t = done(tx, "t", None, 100);
            let at = 100 + 2 * DAY;
            assert_eq!(due(tx, at)?, vec![t.id.clone()]);
            ops::archive(tx, HUMAN, &t.id, false, at)?;
            assert_eq!(due(tx, at + DAY)?, Vec::<String>::new(), "brought back on purpose");
            assert_eq!(due(tx, at + 2 * DAY)?, vec![t.id.clone()]);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac118_a_run_that_holds_it_keeps_it_on_the_board() {
        store("archive-held").with(|tx| {
            let t = done(tx, "one", None, 100);
            let other = ops::create(tx, HUMAN, NewTask { title: "two".into(), ..Default::default() }, 100)?;
            let like = Run { agent_id: "a".into(), kind: "claude".into(), command: "claude".into(), folder: "/srv".into(), project: "default".into(), ..Default::default() };
            let run = runs::add(tx, &like, &[t.key.clone(), other.key.clone()], 100)?;
            assert_eq!(due(tx, 100 + 3 * DAY)?, Vec::<String>::new(), "its run still works on the other");
            runs::change(tx, &run, |r| r.state = "closed".into())?;
            assert_eq!(due(tx, 100 + 3 * DAY)?, vec![t.id.clone()]);
            Ok(())
        }).unwrap();
    }
}
