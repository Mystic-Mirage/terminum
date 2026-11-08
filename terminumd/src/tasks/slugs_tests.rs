// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Tasks' IDs by project (AC-148).

use super::*;
use crate::tasks::ops::{self, NewTask};
use crate::tasks::store::{Store, HUMAN};

fn store(name: &str) -> Store {
    let s = Store::open(crate::testutil::fresh_dir(name).join("kanban.sqlite"));
    s.with(|tx| {
        tx.execute_batch("INSERT INTO projects (id, name, color, dir, place) VALUES ('t', 'Terminum', 0, '/Users/alex/work/terminum/', 0), ('q', 'Q', 1, '~/iquantize', 1);").map_err(sql)?;
        fill(tx)
    }).unwrap();
    s
}

fn make(s: &Store, project: &str) -> String {
    s.with(|tx| ops::create(tx, HUMAN, NewTask { title: "x".into(), project: Some(project.into()), ..Default::default() }, 1)).unwrap().id
}

fn id_of(s: &Store, said: &str) -> Result<String, String> {
    s.with(|tx| ops::get(tx, said)).map(|t| t.id)
}

#[test]
fn ac148_letters_come_from_the_folder_then_the_name() {
    assert_eq!(default_prefix("/Users/alex/work/terminum", "x"), "TERM");
    assert_eq!(default_prefix("~/iquantize/", "x"), "IQUA");
    assert_eq!(default_prefix("/srv/api", "x"), "API");
    assert_eq!(default_prefix("/srv/2024-reports", "x"), "REPO", "letters only");
    assert_eq!(default_prefix("/srv/проект", "Billing"), "BILL", "no Latin letters in the folder: the name's");
    assert_eq!(default_prefix("", "Проект"), "PROJ");
    assert_eq!(check_prefix(" trm2 ").unwrap(), "TRM2");
    for bad in ["", "2TRM", "TOOLONGXX", "T-R", "ТЕРМ"] {
        assert!(check_prefix(bad).is_err(), "{bad}");
    }
}

#[test]
fn ac148_each_project_counts_its_own_and_a_taken_prefix_changes_its_last_letter() {
    let s = store("slug-count");
    assert_eq!((make(&s, "t"), make(&s, "t"), make(&s, "q"), make(&s, "default")), ("TERM-1".into(), "TERM-2".into(), "IQUA-1".into(), "TASK-1".into()));
    s.with(|tx| { tx.execute("INSERT INTO projects (id, name, color, dir, place) VALUES ('t2', 'T2', 2, '/x/terminum', 2)", []).map_err(sql)?; fill(tx) }).unwrap();
    assert_eq!(make(&s, "t2"), "TERA-1");
}

#[test]
fn ac148_a_task_moved_takes_the_next_id_there_and_its_old_one_still_finds_it() {
    let s = store("slug-move");
    let (a, _) = (make(&s, "t"), make(&s, "t"));
    s.with(|tx| ops::change_project(tx, HUMAN, &a, "q", None, 2)).unwrap();
    assert_eq!(id_of(&s, "TERM-1").unwrap(), "IQUA-1");
    assert_eq!(make(&s, "t"), "TERM-3", "an ID a task left is never given again");
    let key: String = s.with(|tx| tx.query_row("SELECT id FROM tasks WHERE slug = 'IQUA-1'", [], |r| r.get(0)).map_err(sql)).unwrap();
    assert_eq!(id_of(&s, &key[..UID_LEN]).unwrap(), "IQUA-1", "the key's start names it");
    assert_eq!(id_of(&s, &key).unwrap(), "IQUA-1");
    assert_eq!(id_of(&s, &key[..10]).unwrap_err(), format!("No task {}", &key[..10]), "only the shown 8 or the whole key");
}

#[test]
fn ac148_new_letters_keep_the_numbers_and_the_old_ids_still_find_them() {
    let s = store("slug-prefix");
    make(&s, "t");
    make(&s, "t");
    s.with(|tx| set_prefix(tx, "t", "trm")).unwrap();
    assert_eq!(id_of(&s, "TERM-2").unwrap(), "TRM-2");
    assert_eq!(make(&s, "t"), "TRM-3");
    assert_eq!(s.with(|tx| set_prefix(tx, "q", "TRM")).unwrap_err(), "TRM names another project's tasks.");
    assert_eq!(s.with(|tx| set_prefix(tx, "q", "TERM")).map(|_| make(&s, "q")).unwrap(), "TERM-3", "freed letters count on above the IDs they had");
    s.with(|tx| set_prefix(tx, "q", "QQ")).unwrap();
    assert_eq!(s.with(|tx| set_prefix(tx, "t", "TERM")).unwrap_err(), "TERM-3 already names another task: choose other letters.", "q's task was TERM-3");
    assert_eq!(s.with(|tx| set_prefix(tx, "default", "X")).unwrap_err(), "Default's tasks keep TASK.");
    assert!(s.with(|tx| set_prefix(tx, "t", "t-1")).is_err());
}
