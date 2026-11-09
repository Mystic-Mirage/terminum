// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Subtasks moved and taken out (AC-112).

use super::*;
use crate::tasks::ops::{self, NewTask};
use crate::tasks::store::{Store, HUMAN};

fn store(name: &str) -> Store {
    Store::open(crate::testutil::fresh_dir(name).join("kanban.sqlite"))
}

fn make(s: &Store, parent: Option<&str>) -> String {
    s.with(|tx| ops::create(tx, HUMAN, NewTask { title: "x".into(), parent: parent.map(str::to_string), ..Default::default() }, 1)).unwrap().id
}

fn parent_of(s: &Store, id: &str) -> Option<String> {
    s.with(|tx| ops::get(tx, id)).unwrap().parent
}

#[test]
fn ac112_a_task_goes_under_another_moves_and_comes_out() {
    let s = store("fam-move");
    let (a, b, c) = (make(&s, None), make(&s, None), make(&s, None));
    s.with(|tx| set_parent(tx, HUMAN, &c, Some(&a), 2)).unwrap();
    assert_eq!(parent_of(&s, &c).as_deref(), Some(a.as_str()));
    assert_eq!(s.with(|tx| ops::get(tx, &a)).unwrap().subtasks, 1);
    s.with(|tx| set_parent(tx, HUMAN, &c, Some(&b), 3)).unwrap();
    assert_eq!(parent_of(&s, &c).as_deref(), Some(b.as_str()));
    let said: Vec<String> = s.with(|tx| ops::history(tx, &a)).unwrap().as_array().unwrap().iter().map(|e| e["text"].as_str().unwrap().to_string()).collect();
    assert!(said.contains(&format!("alex moved {c} to {b}")) || said.iter().any(|t| t.ends_with(&format!("moved {c} to {b}"))), "{said:?}");
    s.with(|tx| set_parent(tx, HUMAN, &c, None, 4)).unwrap();
    assert_eq!(parent_of(&s, &c), None);
    assert_eq!(s.with(|tx| ops::get(tx, &b)).unwrap().subtasks, 0);
}

#[test]
fn ac112_what_would_make_a_second_level_or_cross_projects_is_refused() {
    let s = store("fam-refused");
    let (a, b) = (make(&s, None), make(&s, None));
    let kid = make(&s, Some(&a));
    assert_eq!(s.with(|tx| set_parent(tx, HUMAN, &b, Some(&kid), 2)).unwrap_err(), "A subtask cannot have subtasks");
    assert_eq!(s.with(|tx| set_parent(tx, HUMAN, &a, Some(&b), 2)).unwrap_err(), format!("{a} has subtasks of its own: it cannot become a subtask."));
    assert_eq!(s.with(|tx| set_parent(tx, HUMAN, &a, Some(&a), 2)).unwrap_err(), "A task cannot be its own subtask.");
    s.with(|tx| { tx.execute("INSERT INTO projects (id, name, color, dir, place) VALUES ('w', 'web', 0, '/srv/web', 0)", []).map_err(ops::sql)?; crate::tasks::slugs::fill(tx) }).unwrap();
    let far = s.with(|tx| ops::create(tx, HUMAN, NewTask { title: "y".into(), project: Some("w".into()), ..Default::default() }, 1)).unwrap().id;
    assert_eq!(s.with(|tx| set_parent(tx, HUMAN, &b, Some(&far), 2)).unwrap_err(), format!("{far} is in another project: move {b} there first."));
}

#[test]
fn ac112_a_card_s_boxes_leave_out_what_was_archived() {
    let s = store("fam-boxes");
    let a = make(&s, None);
    let (k1, k2) = (make(&s, Some(&a)), make(&s, Some(&a)));
    s.with(|tx| ops::archive(tx, HUMAN, &k2, true, 2)).unwrap();
    let key = s.with(|tx| ops::get(tx, &a)).unwrap().key;
    let got = s.with(|tx| boxes(tx, &[key.as_str()])).unwrap();
    let ids: Vec<&str> = got[&key].iter().map(|b| b["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [k1.as_str()]);
    assert_eq!(s.with(|tx| ops::get(tx, &a)).unwrap().subtasks, 1, "an archived subtask leaves the count");
}
