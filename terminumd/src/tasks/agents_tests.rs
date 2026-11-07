// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;
use crate::tasks::store::{Store, DEFAULT_PROJECT, HUMAN};

fn open(name: &str) -> Store {
    Store::open(crate::testutil::fresh_dir(name).join("kanban.sqlite"))
}

fn opus() -> Given {
    Given { name: "Opus reviewer".into(), kind: "claude".into(), command: "claude --model opus".into(), effort: String::new(), at_once: 1 }
}

fn project(tx: &Transaction, id: &str) -> Result<(), String> {
    tx.execute("INSERT INTO projects (id, name, color, dir, place) VALUES (?1, 'api', 0, '/srv/api', 0)", [id]).map(|_| ()).map_err(sql)
}

#[test]
fn ac137_kept_listed_changed_and_deleted() {
    let path = crate::testutil::fresh_dir("agents-kept").join("kanban.sqlite");
    let s = Store::open(path.clone());
    let id = s.with(|tx| add(tx, &opus())).unwrap();
    drop(s);
    let s = Store::open(path);
    let listed = s.with(list).unwrap();
    assert_eq!(listed.len(), 1, "across a restart");
    assert_eq!((listed[0].id.as_str(), listed[0].command.as_str()), (id.as_str(), "claude --model opus"), "kept as written");
    let codex = Given { kind: "codex".into(), command: "codex --search".into(), ..opus() };
    s.with(|tx| change(tx, &id, &codex)).unwrap();
    let now = &s.with(list).unwrap()[0];
    assert_eq!((now.kind.as_str(), now.command.as_str()), ("codex", "codex --search"));
    s.with(|tx| remove(tx, &id)).unwrap();
    assert!(s.with(list).unwrap().is_empty());
    assert_eq!(s.with(|tx| remove(tx, &id)).unwrap_err(), "That agent is gone.");
    assert_eq!(s.with(|tx| change(tx, &id, &opus())).unwrap_err(), "That agent is gone.");
}

#[test]
fn ac137_refused_with_words() {
    let cases: Vec<(Given, &str)> = vec![
        (Given { name: "".into(), ..opus() }, "An agent needs a name."),
        (Given { at_once: 0, ..opus() }, "At once is 1 to 100."),
        (Given { at_once: 101, ..opus() }, "At once is 1 to 100."),
        (Given { name: "n".repeat(41), ..opus() }, "An agent's name is 40 characters at most."),
        (Given { name: "a\tb".into(), ..opus() }, "An agent's name is one line, with no control or invisible characters."),
        (Given { kind: "gemini".into(), ..opus() }, "An agent is Claude Code or Codex."),
        (Given { command: "".into(), ..opus() }, "An agent needs a command."),
        (Given { command: "x".repeat(1001), ..opus() }, "An agent's command is 1000 characters at most."),
        (Given { command: "claude\nrm -rf x".into(), ..opus() }, "An agent's command is one line, with no control or invisible characters."),
        (Given { command: "claude\u{1b}[2J".into(), ..opus() }, "An agent's command is one line, with no control or invisible characters."),
        (Given { command: "claude \u{202e}xr- mr".into(), ..opus() }, "An agent's command is one line, with no control or invisible characters."),
        (Given { command: "claude\u{200b}".into(), ..opus() }, "An agent's command is one line, with no control or invisible characters."),
    ];
    for (given, words) in cases {
        assert_eq!(given.check().unwrap_err(), words);
    }
    assert!(opus().check().is_ok());
    let trimmed = Given::of(&json!({"name": "  ", "kind": "claude", "command": " claude "}));
    assert_eq!((trimmed.name.as_str(), trimmed.command.as_str()), ("", "claude"), "what a page sends is trimmed");
}

#[test]
fn ac137_the_store_down_is_said() {
    assert!(Store::absent().with(|tx| add(tx, &opus())).is_err());
}

#[test]
fn ac137_a_name_stands_once_in_the_pool() {
    let s = open("agents-name");
    let a = s.with(|tx| add(tx, &opus())).unwrap();
    assert_eq!(s.with(|tx| add(tx, &opus())).unwrap_err(), "The pool already has an agent named Opus reviewer.");
    s.with(|tx| change(tx, &a, &opus())).unwrap();
}

#[test]
fn ac137_a_project_deleted_leaves_the_pool() {
    let s = open("agents-gone");
    s.with(|tx| {
        project(tx, "p1")?;
        add(tx, &opus())?;
        tx.execute("DELETE FROM projects WHERE id = 'p1'", []).map_err(sql)
    }).unwrap();
    assert_eq!(s.with(list).unwrap().len(), 1);
}

#[test]
fn ac138_an_effort_goes_on_its_kind_s_command_line() {
    assert_eq!(command_line("claude", "claude", ""), "claude");
    assert_eq!(command_line("claude", "claude --model opus", "high"), "claude --model opus --effort high");
    assert_eq!(command_line("codex", "codex", "low"), "codex -c model_reasoning_effort=low");
    let with = |kind: &str, command: &str, effort: &str| Given { kind: kind.into(), command: command.into(), effort: effort.into(), ..opus() }.check();
    assert!(with("claude", "claude", "max").is_ok());
    assert_eq!(with("codex", "codex", "max").unwrap_err(), "codex has no effort max.");
    assert_eq!(with("claude", "claude", "$(rm -rf)").unwrap_err(), "claude has no effort $(rm -rf).", "a word of a fixed list, never free text");
    assert!(with("claude", "claude; notify", "high").unwrap_err().contains("; | & or #"), "the flag would go to the other program");
    assert!(with("claude", "claude; notify", "").is_ok(), "a command is the human's own words");
}

#[test]
fn ac138_each_agent_has_its_own_color() {
    let s = open("agents-colors");
    s.with(|tx| {
        add(tx, &opus())?;
        let second = add(tx, &Given { name: "b".into(), ..opus() })?;
        add(tx, &Given { name: "c".into(), ..opus() })?;
        remove(tx, &second)?;
        add(tx, &Given { name: "d".into(), ..opus() })?;
        let colors: Vec<i64> = list(tx)?.iter().map(|a| a.color).collect();
        assert_eq!(colors, [0, 2, 3]);
        Ok(())
    }).unwrap();
}

#[test]
fn ac138_a_busy_agent_is_not_deleted() {
    open("agents-busy").with(|tx| {
        let id = add(tx, &opus())?;
        let t = crate::tasks::ops::create(tx, HUMAN, crate::tasks::ops::NewTask { title: "one".into(), ..Default::default() }, 1)?;
        let like = crate::dispatcher::runs::Run { agent_id: id.clone(), agent_name: "Opus reviewer".into(), kind: "claude".into(), command: "claude".into(), folder: "/srv".into(), ..Default::default() };
        let run = crate::dispatcher::runs::add(tx, &like, std::slice::from_ref(&t.key), 1)?;
        let refused = remove(tx, &id).unwrap_err();
        assert!(refused.contains("Opus reviewer has a session open:") && refused.contains(&t.id), "{refused}");
        crate::dispatcher::runs::change(tx, &run, |r| r.state = "closed".into())?;
        remove(tx, &id)
    }).unwrap();
}

#[test]
fn ac140_default_stands_at_home() {
    let s = open("agents-place");
    let (name, host, dir) = s.with(|tx| crate::tasks::ops::project_place(tx, DEFAULT_PROJECT)).unwrap();
    assert_eq!((name.as_str(), host, dir.as_str()), ("Default", None, "~"));
}

#[test]
fn ac139_a_folder_is_one_line_from_the_root_or_home() {
    assert!(crate::tasks::projects::check_folder("~/work").is_ok() && crate::tasks::projects::check_folder("/srv/api").is_ok() && crate::tasks::projects::check_folder("").is_ok());
    assert_eq!(crate::tasks::projects::check_folder("work/api").unwrap_err(), "A folder starts with / or ~.");
    assert_eq!(crate::tasks::projects::check_folder("/a\nb").unwrap_err(), "A folder is one line of 1000 characters at most, with no control or invisible characters.");
}

#[test]
fn ac137_what_an_agent_wrote_stands_under_its_new_name() {
    open("agents-rename").with(|tx| {
        let id = add(tx, &opus())?;
        let user = crate::tasks::ops::pool_user(tx, &id, "Opus reviewer")?;
        change(tx, &id, &Given { name: "Opus critic".into(), ..opus() })?;
        assert_eq!(crate::tasks::ops::user_name(tx, &user)?, "Opus critic");
        assert_eq!(crate::tasks::ops::pool_user(tx, &id, "Opus reviewer")?, user, "a run's old copy of the name renames nothing");
        assert_eq!(crate::tasks::ops::user_name(tx, &user)?, "Opus critic");
        Ok(())
    }).unwrap();
}
