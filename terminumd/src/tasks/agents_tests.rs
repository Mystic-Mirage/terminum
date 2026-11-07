// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;
use crate::tasks::store::{Store, DEFAULT_PROJECT};

fn open(name: &str) -> Store {
    Store::open(crate::testutil::fresh_dir(name).join("kanban.sqlite"))
}

fn opus() -> Given {
    Given { name: "Opus reviewer".into(), kind: "claude".into(), host: None, dir: String::new(), command: "claude --model opus".into(), effort: String::new() }
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

#[test]
fn ac128_a_project_s_limits_are_checked_and_go_with_it() {
    let s = open("agents-limits");
    s.with(|tx| { project(tx, "p1", None)?; keep_limits(tx, "p1", Some((6.0, 12.0))) }).unwrap();
    assert_eq!(s.with(limits).unwrap()[0].2, 12.0);
    assert_eq!(s.with(|tx| keep_limits(tx, "p1", Some((9.0, 3.0)))).unwrap_err(), "The warning is above the end.");
    assert_eq!(s.with(|tx| keep_limits(tx, "p1", Some((-1.0, 3.0)))).unwrap_err(), "A limit is a number of GiB, 0 for off.");
    s.with(|tx| keep_limits(tx, "p1", Some((9.0, 0.0)))).unwrap();
    s.with(|tx| keep_limits(tx, "p1", None)).unwrap();
    assert!(s.with(limits).unwrap().is_empty(), "back to the global ones");
    assert_eq!(s.with(|tx| keep_limits(tx, "gone", None)).unwrap_err(), "That project is gone.");
}

#[test]
fn ac138_an_effort_goes_on_its_kind_s_command_line() {
    assert_eq!(command_line("claude", "claude", ""), "claude");
    assert_eq!(command_line("claude", "claude --model opus", "high"), "claude --model opus --effort high");
    assert_eq!(command_line("codex", "codex", "low"), "codex -c model_reasoning_effort=low");
    let with = |kind: &str, command: &str, effort: &str| Given { kind: kind.into(), command: command.into(), effort: effort.into(), ..opus() }.check(&[]);
    assert!(with("claude", "claude", "max").is_ok());
    assert_eq!(with("codex", "codex", "max").unwrap_err(), "codex has no effort max.");
    assert_eq!(with("claude", "claude", "$(rm -rf)").unwrap_err(), "claude has no effort $(rm -rf).", "a word of a fixed list, never free text");
    assert!(with("claude", "claude; notify", "high").unwrap_err().contains("; | & or #"), "the flag would go to the other program");
    assert!(with("claude", "claude; notify", "").is_ok(), "a command is the human's own words");
}

#[test]
fn ac138_each_agent_of_a_project_has_its_own_color() {
    let s = open("agents-colors");
    s.with(|tx| {
        project(tx, "api", None)?;
        add(tx, "api", &opus())?;
        let second = add(tx, "api", &Given { name: "b".into(), ..opus() })?;
        let third = add(tx, "api", &Given { name: "c".into(), ..opus() })?;
        remove(tx, &second)?;
        let fourth = add(tx, "api", &Given { name: "d".into(), ..opus() })?;
        let colors: Vec<i64> = of_project(tx, "api")?.iter().map(|a| a.color).collect();
        assert_eq!(colors, [0, 2, 3], "{third} {fourth}");
        Ok(())
    }).unwrap();
}

#[test]
fn ac138_a_busy_agent_is_not_deleted() {
    open("agents-busy").with(|tx| {
        let id = add(tx, DEFAULT_PROJECT, &opus())?;
        let t = crate::tasks::ops::create(tx, crate::tasks::store::HUMAN, crate::tasks::ops::NewTask { title: "one".into(), ..Default::default() }, 1)?;
        let like = crate::dispatcher::runs::Run { agent_id: id.clone(), agent_name: "Opus reviewer".into(), kind: "claude".into(), command: "claude".into(), folder: "/srv".into(), ..Default::default() };
        let run = crate::dispatcher::runs::add(tx, &like, std::slice::from_ref(&t.key), 1)?;
        let refused = remove(tx, &id).unwrap_err();
        assert!(refused.contains("Opus reviewer is busy with") && refused.contains(&t.id), "{refused}");
        crate::dispatcher::runs::change(tx, &run, |r| r.state = "closed".into())?;
        remove(tx, &id)
    }).unwrap();
}
