// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Tasks' rules (AC-105, AC-106, AC-107, AC-108, AC-112, AC-113).

use super::*;
use crate::tasks::query::{counts, list, Query};
use crate::tasks::store::{Store, DISPATCHER};

fn store(name: &str) -> Store {
    Store::open(crate::testutil::fresh_dir(name).join("kanban.sqlite"))
}

fn new(s: &Store, title: &str, severity: &str) -> Task {
    s.with(|tx| create(tx, HUMAN, NewTask { title: title.into(), severity: Some(severity.into()), ..Default::default() }, 100)).unwrap()
}

fn column(s: &Store, status: &str) -> Vec<String> {
    s.with(|tx| list(tx, &Query { statuses: vec![status.into()], ..Default::default() })).unwrap().into_iter().map(|t| t.id).collect()
}

fn moved(s: &Store, id: &str, to: &str) -> Result<Task, String> {
    s.with(|tx| move_to(tx, HUMAN, id, to, 200))
}

#[test]
fn ac106_a_new_task_stands_in_todo_with_its_defaults() {
    let s = store("t-new");
    let t = s.with(|tx| create(tx, HUMAN, NewTask { title: "  Fix the retry loop ".into(), text: "**why**".into(), ..Default::default() }, 100)).unwrap();
    assert_eq!((t.id.as_str(), t.title.as_str(), t.status.as_str(), t.severity.as_str(), t.project.as_str()), ("T-1", "Fix the retry loop", "TODO", "P2", "default"));
    assert_eq!(t.owner, HUMAN);
    assert_eq!(s.with(|tx| text_of(tx, "T-1")).unwrap(), "**why**");
    assert_eq!(new(&s, "next", "P1").id, "T-2", "numbers run on");
}

#[test]
fn ac106_what_is_refused_leaves_nothing() {
    let s = store("t-refused");
    let make = |title: String, text: String, severity: Option<String>, project: Option<String>| s.with(|tx| create(tx, HUMAN, NewTask { title, text, severity, project, ..Default::default() }, 1));
    assert_eq!(make("  ".into(), String::new(), None, None).unwrap_err(), "Title: 1–200 characters");
    assert_eq!(make("x".repeat(201), String::new(), None, None).unwrap_err(), "Title: 1–200 characters");
    assert!(make("é".repeat(200), String::new(), None, None).is_ok(), "characters, not bytes");
    assert_eq!(make("t".into(), "x".repeat((1 << 20) + 1), None, None).unwrap_err(), "Text: at most 1 MiB");
    assert_eq!(make("t".into(), String::new(), Some("P9".into()), None).unwrap_err(), "Severity: one of P0, P1, P2, P3");
    assert_eq!(make("t".into(), String::new(), None, Some("nope".into())).unwrap_err(), "That project is gone.");
    assert_eq!(column(&s, "TODO").len(), 1);
}

#[test]
fn ac105_a_column_reads_by_severity_new_on_top_of_its_own_and_by_hand_within() {
    let s = store("t-order");
    let t1 = new(&s, "one", "P1").id;
    let t2 = new(&s, "two", "P1").id;
    let t3 = new(&s, "three", "P0").id;
    assert_eq!(column(&s, "TODO"), [t3.clone(), t2.clone(), t1.clone()]);
    let t4 = new(&s, "four", "P1").id;
    assert_eq!(column(&s, "TODO"), [t3.clone(), t4.clone(), t2.clone(), t1.clone()], "new on top of the P1 cards");
    s.with(|tx| reorder(tx, &t1, Some(&t2), 300)).unwrap();
    assert_eq!(column(&s, "TODO"), [t3.clone(), t4.clone(), t1.clone(), t2.clone()], "dragged above T-2");
    s.with(|tx| reorder(tx, &t4, None, 300)).unwrap();
    assert_eq!(column(&s, "TODO"), [t3.clone(), t1.clone(), t2.clone(), t4.clone()], "dragged to the end of its severity");
    let t = s.with(|tx| reorder(tx, &t2, Some(&t3), 300)).unwrap();
    assert_eq!(t.severity, "P1", "dropped among P0 cards it stays P1");
    assert_eq!(column(&s, "TODO"), [t3, t2, t1, t4], "on top of the P1 cards");
}

#[test]
fn ac107_any_column_to_any_column_with_its_history() {
    let s = store("t-move");
    let id = new(&s, "t", "P2").id;
    for to in ["WAIT FOR HUMAN", "DONE", "TODO", "FAILED", "IN PROGRESS"] {
        assert_eq!(moved(&s, &id, to).unwrap().status, to);
    }
    assert_eq!(moved(&s, &id, "SOON").unwrap_err(), "Status: one of TODO, IN PROGRESS, WAIT FOR HUMAN, FAILED, DONE");
    let h = s.with(|tx| history(tx, &id)).unwrap();
    let lines: Vec<&str> = h.as_array().unwrap().iter().map(|e| e["text"].as_str().unwrap()).collect();
    let me = s.with(|tx| user_name(tx, HUMAN)).unwrap();
    assert_eq!(lines[1], format!("{me} moved it from TODO to WAIT FOR HUMAN"));
    assert_eq!(lines.len(), 6, "made, then five moves");
    assert_eq!(s.with(|tx| move_to(tx, DISPATCHER, &id, "DONE", 300)).unwrap().status, "DONE");
    let h = s.with(|tx| history(tx, &id)).unwrap();
    assert_eq!(h.as_array().unwrap().last().unwrap()["author_kind"], "system", "Dispatcher writes as itself");
}

#[test]
fn ac107_a_move_goes_on_top_of_its_severity_in_its_new_column() {
    let s = store("t-movetop");
    let a = new(&s, "a", "P2").id;
    let b = new(&s, "b", "P2").id;
    moved(&s, &a, "DONE").unwrap();
    moved(&s, &b, "DONE").unwrap();
    assert_eq!(column(&s, "DONE"), [b, a]);
}

#[test]
fn ac113_a_session_holds_one_task_in_progress() {
    let s = store("t-one");
    let (a, b) = (new(&s, "a", "P2").id, new(&s, "b", "P2").id);
    let key = "tmux:local:terminum:%7";
    s.with(|tx| link(tx, HUMAN, &a, Some(key), 1)).unwrap();
    s.with(|tx| link(tx, HUMAN, &b, Some(key), 1)).unwrap();
    moved(&s, &a, "IN PROGRESS").unwrap();
    assert_eq!(moved(&s, &b, "IN PROGRESS").unwrap_err(), format!("Session %7 already works on {a}. Finish or move {a} first."));
    assert_eq!(s.with(|tx| get(tx, &b)).unwrap().status, "TODO", "it stays where it was");
    moved(&s, &a, "DONE").unwrap();
    assert_eq!(moved(&s, &b, "IN PROGRESS").unwrap().status, "IN PROGRESS");
    let c = new(&s, "c", "P2").id;
    moved(&s, &c, "IN PROGRESS").unwrap();
    assert!(s.with(|tx| link(tx, HUMAN, &c, Some(key), 1)).is_err(), "nor by linking one in progress to it");
    assert_eq!(s.with(|tx| get(tx, &c)).unwrap().session, None);
    assert_eq!(s.with(|tx| link(tx, HUMAN, &b, None, 1)).unwrap().session, None);
}

#[test]
fn ac108_comments_and_who_may_change_them() {
    let s = store("t-comment");
    let id = new(&s, "t", "P2").id;
    let c = s.with(|tx| comment(tx, HUMAN, &id, "Use **v2**", 5)).unwrap();
    let agent = s.with(|tx| agent_user(tx, "claude", "api")).unwrap();
    let d = s.with(|tx| comment(tx, &agent, &id, "done", 6)).unwrap();
    assert_eq!(s.with(|tx| change_comment(tx, HUMAN, &id, &d, Some("rewritten"), 7)).unwrap_err(), "Only its human author can change a comment.");
    s.with(|tx| change_comment(tx, HUMAN, &id, &c, Some("Use v3"), 8)).unwrap();
    let h = s.with(|tx| history(tx, &id)).unwrap();
    let found = |event: &str| h.as_array().unwrap().iter().find(|e| e["id"] == event).unwrap().clone();
    assert_eq!(found(&c)["text"], "Use v3");
    assert_eq!(found(&d)["author_name"], "claude · api");
    assert!(h.as_array().unwrap().iter().any(|e| e["text"].as_str().is_some_and(|t| t.ends_with("changed a comment"))), "said in the history");
    s.with(|tx| change_comment(tx, HUMAN, &id, &c, None, 9)).unwrap();
    let h = s.with(|tx| history(tx, &id)).unwrap();
    assert_eq!(h.as_array().unwrap().iter().find(|e| e["id"] == c.as_str()).unwrap()["kind"], "deleted");
    assert_eq!(s.with(|tx| comment(tx, HUMAN, &id, &"x".repeat((256 << 10) + 1), 9)).unwrap_err(), "Comment: at most 256 KiB");
}

#[test]
fn ac109_a_text_can_be_purged() {
    let s = store("t-purge");
    let id = s.with(|tx| create(tx, HUMAN, NewTask { title: "t".into(), text: "secret".into(), ..Default::default() }, 1)).unwrap().id;
    s.with(|tx| purge_text(tx, HUMAN, &id, 2)).unwrap();
    assert_eq!(s.with(|tx| text_of(tx, &id)).unwrap(), "");
}

#[test]
fn ac112_one_level_of_subtasks_counted_on_the_parent_and_moved_with_it() {
    let s = store("t-subs");
    let up = new(&s, "up", "P1").id;
    let kid = s.with(|tx| create(tx, HUMAN, NewTask { title: "kid".into(), parent: Some(up.clone()), ..Default::default() }, 1)).unwrap();
    assert_eq!(kid.parent.as_deref(), Some(up.as_str()));
    let refused = s.with(|tx| create(tx, HUMAN, NewTask { title: "deep".into(), parent: Some(kid.id.clone()), ..Default::default() }, 1));
    assert_eq!(refused.unwrap_err(), "A subtask cannot have subtasks");
    moved(&s, &kid.id, "DONE").unwrap();
    let t = s.with(|tx| get(tx, &up)).unwrap();
    assert_eq!((t.subtasks_done, t.subtasks, t.status.as_str()), (1, 1, "TODO"), "the parent's status never changes by itself");
    assert!(!column(&s, "DONE").contains(&kid.id), "shown only in its parent unless asked");
    let all = s.with(|tx| list(tx, &Query { statuses: vec!["DONE".into()], with_subtasks: true, ..Default::default() })).unwrap();
    assert_eq!(all.len(), 1);
    s.with(|tx| tx.execute("INSERT INTO projects (id, name, color, dir, place) VALUES ('web', 'web', 1, '', 0)", []).map_err(sql)).unwrap();
    s.with(|tx| set_project(tx, HUMAN, &up, "web", false, 2)).unwrap();
    assert_eq!(s.with(|tx| get(tx, &kid.id)).unwrap().project, "web");
}

#[test]
fn ac113_moving_across_projects_can_unlink_and_sees_a_session_s_other_tasks() {
    let s = store("t-across");
    s.with(|tx| tx.execute("INSERT INTO projects (id, name, color, dir, place) VALUES ('web', 'web', 1, '', 0)", []).map_err(sql)).unwrap();
    let (a, b) = (new(&s, "a", "P2").id, new(&s, "b", "P2").id);
    let key = "tmux:local:terminum:%7";
    s.with(|tx| link(tx, HUMAN, &a, Some(key), 1)).unwrap();
    s.with(|tx| link(tx, HUMAN, &b, Some(key), 1)).unwrap();
    assert_eq!(s.with(|tx| other_open_of_session(tx, key, &a)).unwrap(), Some(b.clone()));
    let t = s.with(|tx| set_project(tx, HUMAN, &a, "web", true, 2)).unwrap();
    assert_eq!((t.project.as_str(), t.session.as_deref()), ("web", None));
}

#[test]
fn ac111_filters_hold_together_and_count_columns() {
    let s = store("t-filter");
    s.with(|tx| tx.execute("INSERT INTO projects (id, name, color, dir, place) VALUES ('web', 'web', 1, '', 0)", []).map_err(sql)).unwrap();
    let a = new(&s, "Fix the retry 100%", "P0").id;
    new(&s, "other", "P1");
    let agent = s.with(|tx| agent_user(tx, "claude", "api")).unwrap();
    let w = s.with(|tx| create(tx, &agent, NewTask { title: "web one".into(), text: "retry here".into(), project: Some("web".into()), severity: Some("P1".into()), owner: Some(agent.clone()), by_agent: true, ..Default::default() }, 1)).unwrap();
    let ids = |q: Query| s.with(|tx| list(tx, &q)).unwrap().into_iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(ids(Query { projects: vec!["web".into()], ..Default::default() }), [w.id.clone()]);
    assert_eq!(ids(Query { severities: vec!["P0".into()], ..Default::default() }), [a.clone()]);
    assert_eq!(ids(Query { owner: Some("agents".into()), ..Default::default() }), [w.id.clone()]);
    assert_eq!(ids(Query { text: Some("retry".into()), ..Default::default() }).len(), 2, "title or text");
    assert_eq!(ids(Query { text: Some("100%".into()), ..Default::default() }), [a.clone()], "a % is itself");
    assert_eq!(ids(Query { text: Some(w.id.clone()), ..Default::default() }), [w.id.clone()], "by its ID");
    assert_eq!(ids(Query { projects: vec!["default".into()], severities: vec!["P1".into()], ..Default::default() }).len(), 1);
    let c = s.with(|tx| counts(tx, &Query { severities: vec!["P1".into()], ..Default::default() })).unwrap();
    assert_eq!((c["TODO"].as_i64(), c["DONE"].as_i64()), (Some(2), Some(0)));
    assert_eq!(ids(Query { limit: 1, after: 1, ..Default::default() }).len(), 1, "a page");
}

#[test]
fn ac115_a_repeat_and_the_agents_limit() {
    let s = store("t-repeat");
    let a = new(&s, "Fix  the Retry loop.", "P2").id;
    assert_eq!(s.with(|tx| repeat_of(tx, "default", "fix the retry LOOP")).unwrap(), Some(a.clone()));
    assert_eq!(s.with(|tx| repeat_of(tx, "default", "another")).unwrap(), None);
    moved(&s, &a, "DONE").unwrap();
    assert_eq!(s.with(|tx| repeat_of(tx, "default", "fix the retry loop")).unwrap(), None, "only open ones");
    let agent = s.with(|tx| agent_user(tx, "codex", "web")).unwrap();
    for n in 0..AGENTS_OPEN_MOST {
        s.with(|tx| create(tx, &agent, NewTask { title: format!("n{n}"), by_agent: true, ..Default::default() }, 1)).unwrap();
    }
    let over = s.with(|tx| create(tx, &agent, NewTask { title: "over".into(), by_agent: true, ..Default::default() }, 1)).unwrap_err();
    assert_eq!(over, "This project holds 200 open tasks by agents. Ask the user to close some.");
    assert!(s.with(|tx| create(tx, HUMAN, NewTask { title: "a human's".into(), ..Default::default() }, 1)).is_ok());
}

#[test]
fn ac113_a_link_follows_a_restore_and_drops_when_the_server_starts_anew() {
    use crate::tasks::projects::{links_dropped, links_restored};
    let s = store("t-links");
    let (a, b, c) = (new(&s, "a", "P2").id, new(&s, "b", "P2").id, new(&s, "c", "P2").id);
    let prefix = "tmux:local:terminum:";
    s.with(|tx| link(tx, HUMAN, &a, Some("tmux:local:terminum:%1"), 1)).unwrap();
    s.with(|tx| link(tx, HUMAN, &b, Some("tmux:local:terminum:%2"), 1)).unwrap();
    s.with(|tx| link(tx, HUMAN, &c, Some("tmux:devbox:default:%1"), 1)).unwrap();
    let session = |id: &str| s.with(|tx| get(tx, id)).unwrap().session;
    assert_eq!(s.with(|tx| links_restored(tx, prefix, &[("%1".into(), "%7".into())], 2)).unwrap(), 2);
    assert_eq!(session(&a).as_deref(), Some("tmux:local:terminum:%7"), "the restored pane");
    assert_eq!(session(&b), None, "not made again: dropped, never naming another pane");
    assert_eq!(session(&c).as_deref(), Some("tmux:devbox:default:%1"), "another server's stays");
    assert!(s.with(|tx| history(tx, &b)).unwrap().as_array().unwrap().iter().any(|e| e["text"] == "Its tmux server started anew: the session's link was dropped"));
    assert_eq!(s.with(|tx| links_dropped(tx, prefix, 3)).unwrap(), 1);
    assert_eq!(session(&a), None);
    assert_eq!(session(&c).as_deref(), Some("tmux:devbox:default:%1"));
}
