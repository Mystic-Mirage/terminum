// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! One run of several tasks (AC-138): what each reads, moves and frees.

use super::*;
use crate::tasks::ops::{self, NewTask};
use crate::tasks::store::{Store, HUMAN};

fn store(name: &str) -> Store {
    Store::open(crate::testutil::fresh_dir(name).join("kanban.sqlite"))
}

fn task(tx: &Transaction, title: &str) -> ops::Task {
    ops::create(tx, HUMAN, NewTask { title: title.into(), ..Default::default() }, 1).unwrap()
}

/// A run of `titles` on agent `a`, its prompt taken: its tasks IN PROGRESS, in pane %7.
fn working(tx: &Transaction, a: &str, titles: &[&str]) -> (Run, Vec<ops::Task>) {
    let tasks: Vec<ops::Task> = titles.iter().map(|t| task(tx, t)).collect();
    let keys: Vec<String> = tasks.iter().map(|t| t.key.clone()).collect();
    let id = add(tx, &Run { agent_id: a.into(), agent_name: a.to_uppercase(), kind: "claude".into(), command: "claude".into(), folder: "/w".into(), ..Default::default() }, &keys, 2).unwrap();
    for t in &tasks {
        ops::link(tx, HUMAN, &t.id, Some(&format!("tmux:local:t:%{a}")), 2).unwrap();
    }
    let r = change(tx, &id, |r| { r.state = "working".into(); r.pane_key = Some(format!("tmux:local:t:%{a}")); }).unwrap();
    crate::dispatcher::move_all(tx, &r, "IN PROGRESS", "The agent took it.").unwrap();
    (by_id(tx, &id).unwrap(), tasks)
}

#[test]
fn ac138_a_run_reads_as_all_its_tasks() {
    store("runs-reads").with(|tx| {
        let (r, t) = working(tx, "a", &["one", "two", "three"]);
        assert_eq!(r.ids, [t[0].id.clone(), t[1].id.clone(), t[2].id.clone()]);
        assert_eq!(r.open, r.ids);
        assert_eq!(r.status, "IN PROGRESS");
        assert_eq!(r.names(), format!("{}, {}, {}", t[0].id, t[1].id, t[2].id));
        for one in &t {
            assert_eq!(last_of(tx, &one.key).unwrap().unwrap().id, r.id, "each task finds its run");
        }
        // one finishes: the others go on, none of the session's rules stands in the way
        ops::move_to(tx, HUMAN, &t[0].id, "DONE", 3)?;
        finished(tx, &t[0].key)?;
        let r = by_id(tx, &r.id)?;
        assert_eq!(r.status, "IN PROGRESS");
        assert_eq!(r.open, [t[1].id.clone(), t[2].id.clone()]);
        assert!(!r.result, "two are yet to give theirs");
        assert!(r.busy());
        // an automatic act leaves what is done
        crate::dispatcher::move_task(tx, &r, "FAILED", "It failed.")?;
        assert_eq!(ops::get(tx, &t[0].id)?.status, "DONE");
        assert_eq!(ops::get(tx, &t[1].id)?.status, "FAILED");
        assert_eq!(by_id(tx, &r.id)?.status, "FAILED", "how the last ended");
        Ok(())
    }).unwrap();
}

#[test]
fn ac138_its_result_is_all_of_theirs() {
    store("runs-result").with(|tx| {
        let (r, t) = working(tx, "a", &["one", "two"]);
        finished(tx, &t[0].key)?;
        assert!(!by_id(tx, &r.id)?.result);
        finished(tx, &t[1].key)?;
        assert!(by_id(tx, &r.id)?.result);
        reopened(tx, &t[1].key)?;
        assert!(!by_id(tx, &r.id)?.result, "run again, its result is yet to come");
        Ok(())
    }).unwrap();
}

#[test]
fn ac138_an_agent_is_busy_until_its_tasks_end_and_its_turn_is_over() {
    store("runs-busy").with(|tx| {
        assert!(busy_with(tx, "a", "")?.is_none());
        let (r, t) = working(tx, "a", &["one", "two"]);
        assert_eq!(busy_with(tx, "a", "")?.map(|b| b.id).as_deref(), Some(r.id.as_str()));
        assert!(busy_with(tx, "a", &r.id)?.is_none(), "its own run is no other");
        assert!(busy_with(tx, "b", "")?.is_none(), "another agent is free");
        ops::move_to(tx, HUMAN, &t[0].id, "DONE", 3)?;
        ops::move_to(tx, HUMAN, &t[1].id, "FAILED", 3)?;
        assert!(busy_with(tx, "a", "")?.is_some(), "still in its turn");
        change(tx, &r.id, |r| r.turn_ended = true)?;
        assert!(busy_with(tx, "a", "")?.is_none(), "free, its idle session waiting to close");
        // a human's own prompt there: working again, whatever the tasks say
        change(tx, &r.id, |r| r.turn_ended = false)?;
        assert!(busy_with(tx, "a", "")?.is_some());
        Ok(())
    }).unwrap();
}

#[test]
fn ac138_closing_its_session_frees_its_agent() {
    store("runs-closed").with(|tx| {
        let (r, _) = working(tx, "a", &["one"]);
        change(tx, &r.id, |r| r.state = "closed".into())?;
        assert!(busy_with(tx, "a", "")?.is_none());
        assert!(holding(tx, &r.task)?.is_none());
        Ok(())
    }).unwrap();
}

#[test]
fn ac138_giving_up_fails_what_is_open_and_lets_the_window_be() {
    store("runs-give-up").with(|tx| {
        let (r, t) = working(tx, "a", &["one", "two"]);
        ops::move_to(tx, HUMAN, &t[0].id, "DONE", 3)?;
        let after = give_up(tx, &t[1].key)?;
        assert_eq!(after.state, "left");
        assert_eq!(after.pane_key, r.pane_key, "its window stays");
        assert_eq!(ops::get(tx, &t[0].id)?.status, "DONE", "what is done stays");
        assert_eq!(ops::get(tx, &t[1].id)?.status, "FAILED");
        assert!(busy_with(tx, "a", "")?.is_none());
        assert_eq!(give_up(tx, &t[1].key).unwrap_err(), "It has no run to give up.");
        Ok(())
    }).unwrap();
}

#[test]
fn ac138_marks_and_agents_are_each_task_s() {
    store("runs-marks").with(|tx| {
        let (r, t) = working(tx, "a", &["one", "two"]);
        let agents = card_agents(tx)?;
        assert_eq!(agents[&t[1].key]["name"], "A", "the card stays on it, as the run knew it");
        change(tx, &r.id, |r| r.state = "starting".into())?;
        assert_eq!(card_marks(tx)?[&t[1].key], "starting");
        change(tx, &r.id, |r| r.state = "working".into())?;
        ops::move_to(tx, HUMAN, &t[0].id, "DONE", 3)?;
        finished(tx, &t[0].key)?;
        ops::move_to(tx, HUMAN, &t[1].id, "DONE", 3)?;
        let marks = card_marks(tx)?;
        assert!(!marks.contains_key(&t[0].key), "it gave its result");
        assert_eq!(marks[&t[1].key], "no_result");
        Ok(())
    }).unwrap();
}

#[test]
fn ac138_its_budget_is_theirs_together_or_none() {
    store("runs-budget").with(|tx| {
        let (r, t) = working(tx, "a", &["one", "two"]);
        ops::set_budget(tx, HUMAN, &t[0].id, Some(30), 3)?;
        assert_eq!(by_id(tx, &r.id)?.budget, None, "one has none");
        ops::set_budget(tx, HUMAN, &t[1].id, Some(10), 3)?;
        assert_eq!(by_id(tx, &r.id)?.budget, Some(40 * 60));
        Ok(())
    }).unwrap();
}

#[test]
fn ac113_a_session_s_other_task_is_refused_unless_one_run_gave_both() {
    store("runs-free").with(|tx| {
        let (_, t) = working(tx, "a", &["one", "two"]);
        let key = t[0].session.clone().unwrap_or_else(|| "tmux:local:t:%a".into());
        let other = task(tx, "another");
        ops::link(tx, HUMAN, &other.id, Some(&key), 3)?;
        assert!(ops::move_to(tx, HUMAN, &other.id, "IN PROGRESS", 3).unwrap_err().contains("already works on"));
        Ok(())
    }).unwrap();
}
