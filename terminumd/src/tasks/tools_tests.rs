// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! An agent's rights on tasks (AC-115).

use super::*;
use crate::tasks::store::HUMAN;

const KEY: &str = "tmux:local:terminum:%7";
const OTHER: &str = "tmux:local:terminum:%8";

fn who(project: Option<&str>) -> Who {
    Who { key: KEY.into(), agent: "claude".into(), title: "api".into(), project: project.map(str::to_string), dir: None }
}

fn fresh(name: &str) -> (Store, std::path::PathBuf) {
    let dir = crate::testutil::fresh_dir(name);
    let s = Store::open(dir.join("kanban.sqlite"));
    s.with(|tx| tx.execute("INSERT INTO projects (id, name, color, dir, place) VALUES ('api', 'api', 1, '', 0), ('web', 'web', 2, '', 1)", []).map_err(ops::sql)).unwrap();
    (s, dir)
}

fn task(s: &Store, project: &str, session: Option<&str>) -> String {
    s.with(|tx| ops::create(tx, HUMAN, NewTask { title: format!("t in {project} {session:?}"), text: "body".into(), project: Some(project.into()), session: session.map(str::to_string), ..Default::default() }, 1)).unwrap().id
}

fn call(s: &Store, dir: &Path, w: &Who, tool: &str, args: Value) -> Result<String, String> {
    run(s, dir, w, tool, &args).map(|(t, _)| t)
}

#[test]
fn ac115_an_agent_does_all_on_its_own() {
    let (s, dir) = fresh("tt-own");
    let w = who(Some("api"));
    let id = task(&s, "api", Some(KEY));
    let got = call(&s, &dir, &w, "get_task", json!({"id": id})).unwrap();
    assert!(got.starts_with(DATA), "said to be data: {got}");
    assert!(got.contains("\"text\":\"body\""));
    assert_eq!(call(&s, &dir, &w, "move_task", json!({"id": id, "to": "WAIT FOR HUMAN"})).unwrap(), format!("{id} is in WAIT FOR HUMAN."));
    call(&s, &dir, &w, "add_comment", json!({"id": id, "text": "note"})).unwrap();
    call(&s, &dir, &w, "finish_task", json!({"id": id, "result": "**fixed**"})).unwrap();
    let (status, history) = s.with(|tx| Ok((ops::get(tx, &id)?.status, ops::history(tx, &id)?))).unwrap();
    assert_eq!(status, "DONE");
    let lines = history.as_array().unwrap();
    assert!(lines.iter().any(|e| e["kind"] == "comment" && e["text"] == "**fixed**" && e["author_name"] == "claude · api"), "its result is its comment, its author the agent");
}

#[test]
fn ac115_others_of_the_project_are_read_and_commented_never_moved() {
    let (s, dir) = fresh("tt-others");
    let w = who(Some("api"));
    let id = task(&s, "api", Some(OTHER));
    assert!(call(&s, &dir, &w, "get_task", json!({"id": id})).is_ok());
    assert!(call(&s, &dir, &w, "add_comment", json!({"id": id, "text": "seen"})).is_ok());
    assert_eq!(call(&s, &dir, &w, "move_task", json!({"id": id, "to": "DONE"})).unwrap_err(), format!("{id} belongs to session %8. You can read it and comment."));
    assert!(call(&s, &dir, &w, "finish_task", json!({"id": id, "result": "x"})).is_err());
    assert_eq!(s.with(|tx| ops::get(tx, &id)).unwrap().status, "TODO");
}

#[test]
fn ac115_another_project_is_out_of_scope() {
    let (s, dir) = fresh("tt-scope");
    let w = who(Some("api"));
    let id = task(&s, "web", None);
    assert_eq!(call(&s, &dir, &w, "get_task", json!({"id": id})).unwrap_err(), format!("{id} is outside this session's project."));
    assert!(call(&s, &dir, &w, "add_comment", json!({"id": id, "text": "x"})).is_err());
    assert!(call(&s, &dir, &w, "take_task", json!({"id": id})).is_err());
    let listed = call(&s, &dir, &w, "list_tasks", json!({})).unwrap();
    assert!(!listed.contains(&format!("\"{id}\"")), "{listed}");
}

#[test]
fn ac115_a_session_in_default_reaches_only_its_own_and_what_it_made() {
    let (s, dir) = fresh("tt-default");
    let w = who(None);
    let mine = task(&s, "default", Some(KEY));
    let others = task(&s, "default", Some(OTHER));
    let free = task(&s, "default", None);
    let made = call(&s, &dir, &w, "create_task", json!({"title": "Bug: a gap", "severity": "P2"})).unwrap();
    let made = made.trim_start_matches("Kept as ").trim_end_matches(", in TODO.").to_string();
    let listed = call(&s, &dir, &w, "list_tasks", json!({})).unwrap();
    for (id, seen) in [(&mine, true), (&made, true), (&others, false), (&free, false)] {
        assert_eq!(listed.contains(&format!("\"{id}\"")), seen, "{id} in {listed}");
    }
    assert!(call(&s, &dir, &w, "get_task", json!({"id": others})).is_err());
}

#[test]
fn ac110_two_default_sessions_of_one_title_share_nothing() {
    let (s, dir) = fresh("tt-same-title");
    let a = who(None);
    let b = Who { key: OTHER.into(), ..who(None) };
    let made = call(&s, &dir, &a, "create_task", json!({"title": "Debt: a's gap", "severity": "P3"})).unwrap();
    let id = made.trim_start_matches("Kept as ").trim_end_matches(", in TODO.").to_string();
    assert!(call(&s, &dir, &a, "get_task", json!({"id": id})).is_ok());
    assert!(call(&s, &dir, &b, "get_task", json!({"id": id})).is_err(), "the same title gives no reach");
    assert!(!call(&s, &dir, &b, "list_tasks", json!({})).unwrap().contains(&format!("\"{id}\"")));
}

#[test]
fn ac115_reading_makes_no_user_row() {
    let (s, dir) = fresh("tt-read");
    call(&s, &dir, &who(Some("api")), "list_tasks", json!({})).unwrap();
    let agents: i64 = s.with(|tx| tx.query_row("SELECT count(*) FROM users WHERE kind = 'agent'", [], |r| r.get(0)).map_err(ops::sql)).unwrap();
    assert_eq!(agents, 0);
}

#[test]
fn ac115_a_list_is_cut_after_an_agent_s_reach_and_says_so() {
    let (s, dir) = fresh("tt-cut");
    let w = who(None);
    for n in 0..200 {
        s.with(|tx| ops::create(tx, HUMAN, NewTask { title: format!("busy {n}"), severity: Some("P0".into()), ..Default::default() }, 1)).unwrap();
    }
    let mine = task(&s, "default", Some(KEY));
    let listed = call(&s, &dir, &w, "list_tasks", json!({})).unwrap();
    assert!(listed.contains(&format!("\"{mine}\"")), "its own, past the board's first 200");
    for n in 0..200 {
        s.with(|tx| ops::create(tx, HUMAN, NewTask { title: format!("in api {n}"), project: Some("api".into()), ..Default::default() }, 1)).unwrap();
    }
    assert!(call(&s, &dir, &who(Some("api")), "list_tasks", json!({})).unwrap().ends_with("ask by status or severity for others."));
}

#[test]
fn ac115_a_free_task_can_be_taken_one_with_a_session_cannot() {
    let (s, dir) = fresh("tt-take");
    let w = who(Some("api"));
    let free = task(&s, "api", None);
    assert!(call(&s, &dir, &w, "move_task", json!({"id": free, "to": "DONE"})).unwrap_err().contains("take_task first"));
    call(&s, &dir, &w, "take_task", json!({"id": free})).unwrap();
    let t = s.with(|tx| ops::get(tx, &free)).unwrap();
    assert_eq!((t.session.as_deref(), t.status.as_str(), t.owner_name.as_str()), (Some(KEY), "TODO", "claude · api"), "linked, owned and still in TODO");
    let held = task(&s, "api", Some(OTHER));
    assert_eq!(call(&s, &dir, &w, "take_task", json!({"id": held})).unwrap_err(), format!("{held} has a session already."));
}

#[test]
fn ac115_a_new_task_is_marked_cleaned_and_not_kept_twice() {
    let (s, dir) = fresh("tt-new");
    let w = who(Some("api"));
    let said = call(&s, &dir, &w, "create_task", json!({"title": "Bug: Gap\u{202e} in\u{7} retry", "text": "API_TOKEN=abc123 here", "severity": "P1"})).unwrap();
    let id = said.trim_start_matches("Kept as ").trim_end_matches(", in TODO.").to_string();
    let (t, text) = s.with(|tx| Ok((ops::get(tx, &id)?, ops::text_of(tx, &id)?))).unwrap();
    assert_eq!((t.title.as_str(), t.by_agent, t.session.clone(), t.status.as_str(), t.project.as_str(), t.severity.as_str()), ("Bug: Gap in retry", true, None, "TODO", "api", "P1"));
    assert!(text.contains("[redacted]") && !text.contains("abc123"), "{text}");
    assert_eq!(call(&s, &dir, &w, "create_task", json!({"title": "Bug: gap in  RETRY.", "severity": "P1"})).unwrap_err(), format!("Not kept: {id} already says this. Read it with get_task, and comment if you have more."));
    assert_eq!(call(&s, &dir, &w, "create_task", json!({})).unwrap_err(), "title is required");
}

#[test]
fn ac109_an_agent_s_file_stays_inside_its_folder_or_comes_as_text() {
    let (s, dir) = fresh("tt-file");
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("out.log"), "log").unwrap();
    std::fs::write(dir.join("secret.txt"), "key").unwrap();
    std::os::unix::fs::symlink(dir.join("secret.txt"), work.join("link.txt")).unwrap();
    let mut w = who(Some("api"));
    w.dir = Some(work.display().to_string());
    let id = task(&s, "api", Some(KEY));
    call(&s, &dir, &w, "attach_file", json!({"task": id, "path": work.join("out.log")})).unwrap();
    let list = s.with(|tx| files::of_task(tx, &id)).unwrap();
    assert_eq!(list[0].name, "out.log");
    assert!(call(&s, &dir, &w, "attach_file", json!({"task": id, "path": dir.join("secret.txt")})).unwrap_err().contains("outside this session's folder"));
    assert!(call(&s, &dir, &w, "attach_file", json!({"task": id, "path": work.join("link.txt")})).unwrap_err().contains("outside"), "judged on resolved paths");
    call(&s, &dir, &w, "attach_file", json!({"task": id, "path": "out.log"})).unwrap();
    assert_eq!(s.with(|tx| files::of_task(tx, &id)).unwrap().len(), 2, "a relative path is the session's");
    let got = call(&s, &dir, &w, "get_file", json!({"task": id, "name": "out.log"})).unwrap();
    assert!(got.contains(&list[0].id));
    w.dir = None;
    assert!(call(&s, &dir, &w, "attach_file", json!({"task": id, "path": "/etc/hosts"})).unwrap_err().starts_with("From a host"));
    call(&s, &dir, &w, "attach_file", json!({"task": id, "name": "note.txt", "text": "hello"})).unwrap();
    let big = "x".repeat(TEXT_FILE_MOST + 1);
    assert!(call(&s, &dir, &w, "attach_file", json!({"task": id, "name": "big.txt", "text": big})).unwrap_err().contains("at most"));
    assert!(call(&s, &dir, &w, "get_file", json!({"task": id, "name": "note.txt"})).unwrap_err().contains("host"));
}

#[test]
fn ac145_what_is_left_to_do_says_its_kind_and_severity() {
    let ((s, dir), w) = (fresh("tools-left"), who(Some("api")));
    assert_eq!(call(&s, &dir, &w, "create_task", json!({"title": "a gap", "severity": "P2"})).unwrap_err(), "Start the title with Bug:, Feat: or Debt:.");
    assert_eq!(call(&s, &dir, &w, "create_task", json!({"title": "Feat: a gap"})).unwrap_err(), "Give it a severity: P0, P1, P2 or P3.");
    for title in ["Bug: drops a BOM", "Feat: paging", "Debt: two copies of a rule"] {
        assert!(call(&s, &dir, &w, "create_task", json!({"title": title, "severity": "P2"})).unwrap().starts_with("Kept as"), "{title}");
    }
}
