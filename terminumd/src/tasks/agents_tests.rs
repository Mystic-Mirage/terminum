// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;
use crate::tasks::store::{Store, DEFAULT_PROJECT};

fn store(name: &str) -> Store {
    Store::open(crate::testutil::fresh_dir(name).join("kanban.sqlite"))
}

fn opus() -> Given {
    Given { name: "Opus reviewer".into(), kind: "claude".into(), host: None, dir: String::new(), command: "claude --model opus".into() }
}

const HOSTS: &[String] = &[];

#[test]
fn ac137_kept_listed_changed_and_deleted() {
    let s = store("agents-kept");
    let id = s.with(|tx| add(tx, DEFAULT_PROJECT, &opus(), HOSTS)).unwrap();
    let listed = s.with(list).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].id.as_str(), listed[0].project.as_str(), listed[0].command.as_str()), (id.as_str(), "default", "claude --model opus"), "kept as written");
    let codex = Given { kind: "codex".into(), command: "codex --search".into(), dir: "~/work".into(), ..opus() };
    s.with(|tx| change(tx, &id, &codex, HOSTS)).unwrap();
    let now = &s.with(list).unwrap()[0];
    assert_eq!((now.kind.as_str(), now.dir.as_str(), now.command.as_str()), ("codex", "~/work", "codex --search"));
    s.with(|tx| remove(tx, &id)).unwrap();
    assert!(s.with(list).unwrap().is_empty());
    assert_eq!(s.with(|tx| remove(tx, &id)).unwrap_err(), "That agent is gone.");
    assert_eq!(s.with(|tx| change(tx, &id, &opus(), HOSTS)).unwrap_err(), "That agent is gone.");
}

#[test]
fn ac137_refused_with_words_and_nothing_kept() {
    let s = store("agents-refused");
    let long_command = format!("claude {}", "x".repeat(1000));
    let cases: Vec<(Given, &str)> = vec![
        (Given { name: "  ".into(), ..opus() }, "An agent needs a name."),
        (Given { name: "n".repeat(41), ..opus() }, "An agent's name is 40 characters at most."),
        (Given { name: "a\tb".into(), ..opus() }, "An agent's name is one line, with no control characters."),
        (Given { kind: "gemini".into(), ..opus() }, "An agent is Claude Code or Codex."),
        (Given { command: "".into(), ..opus() }, "An agent needs a command."),
        (Given { command: long_command, ..opus() }, "An agent's command is 1000 characters at most."),
        (Given { command: "claude\nrm -rf x".into(), ..opus() }, "An agent's command is one line, with no control characters."),
        (Given { command: "claude\u{1b}[2J".into(), ..opus() }, "An agent's command is one line, with no control characters."),
        (Given { dir: "work/api".into(), ..opus() }, "A folder starts with / or ~."),
        (Given { dir: "/a\nb".into(), ..opus() }, "A folder is one line of 1000 characters at most."),
        (Given { host: Some("devbox.example".into()), ..opus() }, "The server devbox.example is not one of this terminum's hosts."),
    ];
    for (given, words) in cases {
        let trimmed = Given::of(&json!({"name": given.name, "kind": given.kind, "host": given.host, "dir": given.dir, "command": given.command}));
        assert_eq!(s.with(|tx| add(tx, DEFAULT_PROJECT, &trimmed, HOSTS)).unwrap_err(), words);
    }
    assert!(s.with(list).unwrap().is_empty());
    assert_eq!(s.with(|tx| add(tx, "gone", &opus(), HOSTS)).unwrap_err(), "That project is gone.");
}

#[test]
fn ac137_a_name_stands_once_in_its_project() {
    let s = store("agents-name");
    let a = s.with(|tx| add(tx, DEFAULT_PROJECT, &opus(), HOSTS)).unwrap();
    assert_eq!(s.with(|tx| add(tx, DEFAULT_PROJECT, &opus(), HOSTS)).unwrap_err(), "Default already has an agent named Opus reviewer.");
    s.with(|tx| change(tx, &a, &opus(), HOSTS)).unwrap();
    let other = s.with(|tx| {
        tx.execute("INSERT INTO projects (id, name, color, dir, place) VALUES ('p1', 'api', 0, '', 0)", []).map_err(sql)?;
        add(tx, "p1", &opus(), HOSTS)
    }).unwrap();
    assert_ne!(a, other, "another project may use the name");
}

#[test]
fn ac137_a_known_host_is_kept() {
    let s = store("agents-host");
    let hosts = vec!["devbox.example".to_string()];
    let id = s.with(|tx| add(tx, DEFAULT_PROJECT, &Given { host: Some("devbox.example".into()), ..opus() }, &hosts)).unwrap();
    assert_eq!(s.with(list).unwrap()[0].host.as_deref(), Some("devbox.example"));
    s.with(|tx| remove(tx, &id)).unwrap();
}

#[test]
fn ac137_a_project_deleted_takes_its_agents() {
    let s = store("agents-gone");
    s.with(|tx| {
        tx.execute("INSERT INTO projects (id, name, color, dir, place) VALUES ('p1', 'api', 0, '', 0)", []).map_err(sql)?;
        add(tx, "p1", &opus(), HOSTS)?;
        add(tx, DEFAULT_PROJECT, &opus(), HOSTS)?;
        tx.execute("DELETE FROM projects WHERE id = 'p1'", []).map_err(sql)
    }).unwrap();
    let left = s.with(list).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].project, "default");
}
