// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! settings.json (AC-19): defaults, the spec's messages, unknown keys, the hosts rewrite.

use super::*;

fn tmp(name: &str) -> std::path::PathBuf {
    crate::testutil::fresh_dir(&format!("settings-{name}")).join("settings.json")
}

#[test]
fn missing_or_empty_gives_every_default_and_no_message() {
    let (s, m) = parse("");
    assert_eq!((s.clone(), m.is_empty()), (Settings::default(), true));
    assert_eq!((s.port, s.tmux_socket.as_str(), s.tmux_history, s.restore), (48080, "terminum", 12000, true));
    assert_eq!(s.themes.default, "local");
    assert_eq!(s.themes.hosts.get("*").map(String::as_str), Some("remote"));
    assert_eq!(s.page_json(), json!({"textSize": 13, "markdownSize": 14, "history": 12000, "lastLines": 300, "widgets": true, "folds": true, "colorScheme": null, "followAgent": true, "agentExitDays": 0, "messages": []}));
    let (s, m) = load(Path::new("/nonexistent/terminum/settings.json"));
    assert_eq!((s, m), (Settings::default(), vec![]));
}

#[test]
fn the_spec_example_is_read_whole() {
    let text = r#"{"port": 47901, "tmux": {"socket": "tm-test", "history": 20000},
 "restore": false,
 "themes": {"default": "local", "hosts": {"*": "remote", "devbox": "work"}, "sessions": {"notes": "paper"}},
 "page": {"textSize": 15, "markdownSize": 16, "widgets": false, "folds": false, "colorScheme": "Rosé Pine"},
 "hosts": {"devbox": {"ssh": ["-p", "2222", "alex@devbox.example"], "files": "enabled"}}}"#;
    let (s, m) = parse(text);
    assert!(m.is_empty(), "{m:?}");
    assert_eq!((s.port, s.tmux_socket.as_str(), s.tmux_history, s.restore), (47901, "tm-test", 20000, false));
    assert_eq!(s.themes.hosts.get("devbox").map(String::as_str), Some("work"));
    assert_eq!(s.themes.sessions.get("notes").map(String::as_str), Some("paper"));
    assert_eq!(s.page, PageSettings { text_size: 15, markdown_size: 16, widgets: false, folds: false, color_scheme: Some("Rosé Pine".into()), follow_agent: true });
    assert_eq!(s.hosts["devbox"]["files"], "enabled");
    let (s, m) = parse(r#"{"page": {"followAgent": false}}"#);
    assert!(m.is_empty() && !s.page.follow_agent, "{m:?}");
    let (s, m) = parse(r#"{"page": {"followAgent": "no"}}"#);
    assert!(s.page.follow_agent && m.iter().any(|t| t.contains("page.followAgent")), "a wrong value keeps the default and is said: {m:?}");
}

#[test]
fn the_ac19_table_messages_word_for_word() {
    let cases = [
        (r#"{"size": "manual"}"#, "settings.json: size, page.lines and page.screenMode are no longer used: the active page sizes the pane"),
        (r#"{"page": {"textSize": 300}}"#, "settings.json: page.textSize must be 8–40"),
        ("{not json", "settings.json: line 1: expected a value"),
    ];
    for (text, msg) in cases {
        let (s, m) = parse(text);
        assert_eq!(m, vec![msg.to_string()], "{text}");
        assert_eq!(s.page.text_size, 13, "the default is kept");
        assert_eq!(s.page_json()["messages"], json!([msg]));
    }
}

#[test]
fn last_lines_are_300_by_default_0_turns_them_off_and_a_wrong_number_is_said() {
    assert_eq!(parse("{}").0.last_lines, 300);
    assert_eq!(parse(r#"{"lastLines": 0}"#).0.last_lines, 0);
    assert_eq!(parse(r#"{"lastLines": 2000}"#).0.page_json()["lastLines"], json!(2000));
    assert_eq!(parse(r#"{"lastLines": 500, "restore": false}"#).0.page_json()["lastLines"], json!(0), "restore off: no lines kept");
    for bad in ["2001", "-1", "\"300\"", "1.5"] {
        let (s, m) = parse(&format!(r#"{{"lastLines": {bad}}}"#));
        assert_eq!((s.last_lines, m), (300, vec!["settings.json: lastLines must be 0–2000".to_string()]), "{bad}");
    }
}

#[test]
fn a_wrong_value_keeps_only_that_default() {
    let (s, m) = parse(r#"{"page": {"textSize": 15, "lines": "grid", "screenMode": "auto", "widgets": "yes"}, "port": 0, "restore": 1}"#);
    assert_eq!(s.page.text_size, 15);
    assert_eq!((s.page.widgets, s.port, s.restore), (true, 48080, true));
    assert_eq!(m.iter().filter(|x| x.contains("no longer used")).count(), 1, "said once for both: {m:?}");
    for want in ["page.widgets must be true or false", "port must be 1–65535", "restore must be true or false"] {
        assert!(m.contains(&format!("settings.json: {want}")), "{want}: {m:?}");
    }
    let (s, m) = parse(r#"{"page": {"colorScheme": "bad\u0007name"}}"#);
    assert_eq!((s.page.color_scheme, m), (None, vec!["settings.json: page.colorScheme must be a color scheme's name".to_string()]));
    let (_, m) = parse(r#"{"page": {"textSize": 12.5}, "tmux": {"socket": "../x", "history": 5}, "themes": {"hosts": {"a": "../evil"}, "default": 3}}"#);
    assert_eq!(m.len(), 5, "{m:?}");
    assert!(m.contains(&"settings.json: themes.hosts.a must be a theme name".to_string()));
}

#[test]
fn unknown_keys_are_reported_once_each_at_their_place() {
    let (s, m) = parse(r#"{"foo": 1, "page": {"bar": 2, "textSize": 20}, "tmux": {"baz": 3}, "themes": {"qux": 1}}"#);
    assert_eq!(s.page.text_size, 20);
    assert_eq!(m, vec![
        "settings.json: unknown key foo".to_string(),
        "settings.json: unknown key page.bar".into(),
        "settings.json: unknown key themes.qux".into(),
        "settings.json: unknown key tmux.baz".into(),
    ]);
    let (_, m) = parse(r#"{"page": 5, "themes": {"sessions": []}}"#);
    assert_eq!(m, vec!["settings.json: page must be an object".to_string(), "settings.json: themes.sessions must be an object".into()]);
}

#[test]
fn host_rules_merge_over_the_remote_default() {
    let (s, m) = parse(r#"{"themes": {"hosts": {"devbox": "work"}}}"#);
    assert!(m.is_empty());
    assert_eq!(s.themes.hosts.len(), 2, "* stays remote unless named");
    let (s, _) = parse(r#"{"themes": {"hosts": {"*": "local"}}}"#);
    assert_eq!(s.themes.hosts["*"], "local");
}

#[test]
fn syntax_errors_name_their_line_and_other_shapes_are_refused() {
    let (_, m) = parse("{\n  \"size\": \"latest\",\n  \"page\": {\"textSize\": 14,}\n}");
    assert_eq!(m, vec!["settings.json: line 3: trailing comma".to_string()]);
    let (_, m) = parse("{\"size\": ");
    assert_eq!(m, vec!["settings.json: line 1: the file ends too early".to_string()]);
    let (_, m) = parse("[1, 2]");
    assert_eq!(m, vec!["settings.json: expected an object".to_string()]);
    let (_, m) = parse(r#"{"hosts": [1]}"#);
    assert_eq!(m, vec!["settings.json: hosts must be an object".to_string()]);
}

#[test]
fn a_file_over_64_kb_is_refused_with_a_message() {
    let big = format!("{{\"page\": {{\"textSize\": 20}}, \"x\": \"{}\"}}", "a".repeat(MAX_BYTES));
    let (s, m) = parse(&big);
    assert_eq!((s.page.text_size, m), (13, vec!["settings.json: larger than 64 KB, not read".to_string()]));
    let p = tmp("big");
    fs::write(&p, &big).unwrap();
    assert_eq!(load(&p).1, vec!["settings.json: larger than 64 KB, not read".to_string()]);
    fs::write(&p, r#"{"restore": false}"#).unwrap();
    assert!(!load(&p).0.restore);
}

#[test]
fn save_hosts_rewrites_only_the_hosts_value() {
    let p = tmp("hosts");
    let text = "{\n  // not json\n}";
    fs::write(&p, text).unwrap();
    let err = save_hosts(&p, |h| {
        h.insert("x".into(), json!({}));
    })
    .unwrap_err();
    assert!(err.starts_with("settings.json: line 2:") && err.ends_with("fix it first"), "{err}");
    assert_eq!(fs::read_to_string(&p).unwrap(), text, "a broken file is left alone");

    let text = "{\n  \"restore\":   false,\n  \"hosts\": {\"old\": {\"ssh\": [\"old.example\"]}},\n  \"page\": {\"textSize\": 15}\n}\n";
    fs::write(&p, text).unwrap();
    save_hosts(&p, |h| {
        h.remove("old");
        h.insert("devbox".into(), json!({"ssh": ["alex@devbox.example"], "files": null}));
    })
    .unwrap();
    let out = fs::read_to_string(&p).unwrap();
    assert!(out.starts_with("{\n  \"restore\":   false,\n  \"hosts\": {\n"), "{out}");
    assert!(out.ends_with("},\n  \"page\": {\"textSize\": 15}\n}\n"), "{out}");
    let (s, m) = parse(&out);
    assert!(m.is_empty(), "{m:?}");
    assert_eq!(s.page.text_size, 15);
    assert_eq!(s.hosts.keys().collect::<Vec<_>>(), vec!["devbox"]);
}

#[test]
fn save_hosts_adds_the_key_or_the_file() {
    let p = tmp("add");
    save_hosts(&p, |h| {
        h.insert("devbox".into(), json!({"ssh": ["devbox.example"]}));
    })
    .unwrap();
    let (s, m) = load(&p);
    assert!(m.is_empty());
    assert_eq!(s.hosts["devbox"]["ssh"], json!(["devbox.example"]));

    fs::write(&p, "{\"restore\": false}  \n").unwrap();
    save_hosts(&p, |h| {
        h.insert("b".into(), json!({}));
    })
    .unwrap();
    let out = fs::read_to_string(&p).unwrap();
    assert!(out.starts_with("{\"restore\": false,\n  \"hosts\": {"), "{out}");
    let (s, m) = parse(&out);
    assert!(m.is_empty());
    assert_eq!(s.hosts.len(), 1);
    fs::write(&p, "{}").unwrap();
    save_hosts(&p, |h| {
        h.insert("c".into(), json!({}));
    })
    .unwrap();
    assert!(parse(&fs::read_to_string(&p).unwrap()).1.is_empty());
    assert!(fs::read_dir(p.parent().unwrap()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains(".tmp-")));
}

#[test]
fn the_locator_finds_values_and_their_lines() {
    let text = "{\n \"a\": {\"b\": [1, \"x,]}\", {\"c\": true}]},\n \"d\":\n   \"#12\"\n}";
    assert_eq!(locate(text, &[Seg::Key("d")]).map(|s| (s.line, &text[s.start..s.end])), Some((4, "\"#12\"")));
    let c = locate(text, &[Seg::Key("a"), Seg::Key("b"), Seg::Index(2), Seg::Key("c")]).unwrap();
    assert_eq!((c.line, &text[c.start..c.end]), (2, "true"));
    let x = locate(text, &[Seg::Key("a"), Seg::Key("b"), Seg::Index(1)]).unwrap();
    assert_eq!(&text[x.start..x.end], "\"x,]}\"");
    assert_eq!(locate(text, &[Seg::Key("zz")]), None);
    assert_eq!(locate(text, &[Seg::Key("a"), Seg::Index(0)]), None);
    assert_eq!(line_of(text, &[Seg::Key("a"), Seg::Key("b"), Seg::Index(9)]), 1);
    assert!(is_name("work") && is_name("tm-test.2") && !is_name(".hidden") && !is_name("a/b") && !is_name(""));
}

/// AC-89: sessions of the user's own tmux, by socket; terminum's own socket is refused.
#[test]
fn attach_names_sessions_by_socket() {
    let (s, m) = parse(r#"{"attach": {"default": ["work", "notes"], "x y": ["a"], "other": ["a:b"]}}"#);
    assert_eq!(s.attach, BTreeMap::from([("default".to_string(), vec!["work".to_string(), "notes".to_string()])]));
    assert_eq!(m, vec!["settings.json: attach.other must be a list of tmux session names, under a socket name".to_string(),
        "settings.json: attach.x y must be a list of tmux session names, under a socket name".to_string()]);
    let (s, m) = parse(r#"{"attach": {"terminum": ["main"]}}"#);
    assert!(s.attach.is_empty());
    assert_eq!(m, vec!["settings.json: attach.terminum is terminum's own tmux server: its sessions are listed anyway".to_string()]);
    let p = tmp("attach");
    std::fs::write(&p, "{\n  \"port\": 47901\n}\n").unwrap();
    save_object(&p, "attach", |a| { a.insert("default".into(), json!(["work"])); }).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.starts_with("{\n  \"port\": 47901,\n  \"attach\": {"), "{text}");
    assert_eq!(load(&p).0.attach["default"], vec!["work".to_string()]);
}

/// AC-89: a host's attached sessions are kept in its record; a bad list is said and dropped.
#[test]
fn a_hosts_attached_sessions_are_checked() {
    let (s, m) = parse(r#"{"hosts": {"devbox": {"ssh": ["devbox"], "attach": ["work", "api"]}, "b": {"ssh": ["b"], "attach": ["terminum"]}, "c": {"ssh": ["c"], "attach": "x"}}}"#);
    assert_eq!(crate::attach::names_of(&s.hosts["devbox"]), vec!["work".to_string(), "api".to_string()]);
    assert!(s.hosts["b"].get("attach").is_none() && s.hosts["c"].get("attach").is_none() && s.hosts["c"]["ssh"] == json!(["c"]));
    assert_eq!(m, vec!["settings.json: hosts.b.attach must be a list of tmux session names (not terminum)".to_string(),
        "settings.json: hosts.c.attach must be a list of tmux session names (not terminum)".to_string()]);
}

#[test]
fn ac153_agent_exit_days_off_by_default_under_one_off_a_wrong_value_said() {
    assert_eq!(parse("").0.agent_exit_days, 0, "off by default");
    let (s, m) = parse(r#"{"agentExitDays": 3}"#);
    assert!(m.is_empty() && s.agent_exit_days == 3 && s.page_json()["agentExitDays"] == 3, "{m:?}");
    for off in ["0", "-2"] {
        let (s, m) = parse(&format!(r#"{{"agentExitDays": {off}}}"#));
        assert!(m.is_empty() && s.agent_exit_days == 0, "{off}: less than a day is off: {m:?}");
    }
    for bad in ["1.5", "\"3\"", "true", "3651"] {
        let (s, m) = parse(&format!(r#"{{"agentExitDays": {bad}}}"#));
        assert!(s.agent_exit_days == 0 && m.iter().any(|t| t.contains("agentExitDays must be")), "{bad}: the default kept, said: {m:?}");
    }
}

#[test]
fn ac153_the_menus_days_are_saved_as_one_top_level_key_the_rest_kept() {
    let p = tmp("agentexit");
    std::fs::write(&p, "{\n  \"port\": 47901\n}\n").unwrap();
    save_value(&p, "agentExitDays", json!(7)).unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\n  \"port\": 47901,\n  \"agentExitDays\": 7\n}\n");
    save_value(&p, "agentExitDays", json!(0)).unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\n  \"port\": 47901,\n  \"agentExitDays\": 0\n}\n", "replaced in place");
    assert_eq!(load(&p).0.agent_exit_days, 0);
}
