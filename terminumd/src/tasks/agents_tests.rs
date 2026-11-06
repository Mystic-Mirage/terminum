// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;
use crate::tasks::store::{Store, DEFAULT_PROJECT};

fn open(name: &str) -> Store {
    Store::open(crate::testutil::fresh_dir(name).join("kanban.sqlite"))
}

fn opus() -> Given {
    Given { name: "Opus reviewer".into(), kind: "claude".into(), host: None, dir: String::new(), command: "claude --model opus".into() }
}

fn project(tx: &Transaction, id: &str, host: Option<&str>) -> Result<(), String> {
    tx.execute("INSERT INTO projects (id, name, color, host, dir, place) VALUES (?1, 'api', 0, ?2, '/srv/api', 0)", (id, host)).map(|_| ()).map_err(sql)
}

#[test]
fn ac137_kept_listed_changed_and_deleted() {
    let path = crate::testutil::fresh_dir("agents-kept").join("kanban.sqlite");
    let s = Store::open(path.clone());
    let id = s.with(|tx| add(tx, DEFAULT_PROJECT, &opus())).unwrap();
    drop(s);
    let s = Store::open(path);
    let listed = s.with(list).unwrap();
    assert_eq!(listed.len(), 1, "across a restart");
    assert_eq!((listed[0].id.as_str(), listed[0].project.as_str(), listed[0].command.as_str()), (id.as_str(), "default", "claude --model opus"), "kept as written");
    let codex = Given { kind: "codex".into(), command: "codex --search".into(), dir: "~/work".into(), ..opus() };
    s.with(|tx| change(tx, &id, &codex)).unwrap();
    let now = &s.with(list).unwrap()[0];
    assert_eq!((now.kind.as_str(), now.dir.as_str(), now.command.as_str()), ("codex", "~/work", "codex --search"));
    s.with(|tx| remove(tx, &id)).unwrap();
    assert!(s.with(list).unwrap().is_empty());
    assert_eq!(s.with(|tx| remove(tx, &id)).unwrap_err(), "That agent is gone.");
    assert_eq!(s.with(|tx| change(tx, &id, &opus())).unwrap_err(), "That agent is gone.");
}

#[test]
fn ac137_refused_with_words() {
    let cases: Vec<(Given, &str)> = vec![
        (Given { name: "".into(), ..opus() }, "An agent needs a name."),
        (Given { name: "n".repeat(41), ..opus() }, "An agent's name is 40 characters at most."),
        (Given { name: "a\tb".into(), ..opus() }, "An agent's name is one line, with no control or invisible characters."),
        (Given { kind: "gemini".into(), ..opus() }, "An agent is Claude Code or Codex."),
        (Given { command: "".into(), ..opus() }, "An agent needs a command."),
        (Given { command: "x".repeat(1001), ..opus() }, "An agent's command is 1000 characters at most."),
        (Given { command: "claude\nrm -rf x".into(), ..opus() }, "An agent's command is one line, with no control or invisible characters."),
        (Given { command: "claude\u{1b}[2J".into(), ..opus() }, "An agent's command is one line, with no control or invisible characters."),
        (Given { command: "claude \u{202e}xr- mr".into(), ..opus() }, "An agent's command is one line, with no control or invisible characters."),
        (Given { command: "claude\u{200b}".into(), ..opus() }, "An agent's command is one line, with no control or invisible characters."),
        (Given { dir: "work/api".into(), ..opus() }, "A folder starts with / or ~."),
        (Given { dir: "/a\nb".into(), ..opus() }, "A folder is one line of 1000 characters at most, with no control or invisible characters."),
        (Given { host: Some("devbox.example".into()), ..opus() }, "The server devbox.example is not one of this terminum's hosts."),
    ];
    for (given, words) in cases {
        assert_eq!(given.check(&[]).unwrap_err(), words);
    }
    assert!(opus().check(&[]).is_ok());
    let trimmed = Given::of(&json!({"name": "  ", "kind": "claude", "command": " claude "}));
    assert_eq!((trimmed.name.as_str(), trimmed.command.as_str(), trimmed.host), ("", "claude", None), "what a page sends is trimmed");
}

#[test]
fn ac137_a_project_gone_is_said_and_the_store_down_too() {
    let s = open("agents-gone-project");
    assert_eq!(s.with(|tx| add(tx, "gone", &opus())).unwrap_err(), "That project is gone.");
    assert!(s.with(list).unwrap().is_empty());
    assert!(Store::absent().with(|tx| add(tx, DEFAULT_PROJECT, &opus())).is_err());
}

#[test]
fn ac137_a_name_stands_once_in_its_project() {
    let s = open("agents-name");
    let a = s.with(|tx| add(tx, DEFAULT_PROJECT, &opus())).unwrap();
    assert_eq!(s.with(|tx| add(tx, DEFAULT_PROJECT, &opus())).unwrap_err(), "Default already has an agent named Opus reviewer.");
    s.with(|tx| change(tx, &a, &opus())).unwrap();
    let other = s.with(|tx| { project(tx, "p1", None)?; add(tx, "p1", &opus()) }).unwrap();
    assert_ne!(a, other, "another project may use the name");
}

#[test]
fn ac137_an_empty_folder_is_the_projects_on_its_server() {
    let s = open("agents-server");
    let away = Given { host: Some("devbox.example".into()), ..opus() };
    assert!(away.check(&["devbox.example".to_string()]).is_ok(), "a known server");
    assert_eq!(s.with(|tx| add(tx, DEFAULT_PROJECT, &away)).unwrap_err(), "Default's folder is on This Mac: an agent on another server needs its own folder.");
    s.with(|tx| add(tx, DEFAULT_PROJECT, &Given { name: "far".into(), host: away.host.clone(), dir: "/srv/api".into(), ..opus() })).unwrap();
    s.with(|tx| { project(tx, "far", Some("devbox.example"))?; add(tx, "far", &away) }).unwrap();
    assert_eq!(s.with(|tx| add(tx, "far", &Given { name: "here".into(), ..opus() })).unwrap_err(), "api's folder is on devbox.example: an agent on another server needs its own folder.");
}

#[test]
fn ac137_a_project_deleted_takes_its_agents() {
    let s = open("agents-gone");
    s.with(|tx| {
        project(tx, "p1", None)?;
        add(tx, "p1", &opus())?;
        add(tx, DEFAULT_PROJECT, &opus())?;
        tx.execute("DELETE FROM projects WHERE id = 'p1'", []).map_err(sql)
    }).unwrap();
    let left = s.with(list).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].project, "default");
}
