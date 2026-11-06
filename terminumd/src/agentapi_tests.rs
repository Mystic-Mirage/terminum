use super::*;

fn caller(project: Option<&str>) -> Caller {
    let key = "tmux:local:t:%1".to_string();
    Caller { place: Place { key: key.clone(), project: project.map(String::from), ..Place::default() }, agent: "claude".into(), session: Some("s1".into()), key, hooks: true, other: false }
}

fn go(plans: &Plans, notes: &Mutex<Store>, c: &Caller, tool: &str, args: Value) -> (String, bool, Changed) {
    run(plans, notes, c, tool, &args, false, 1_000_000)
}

#[test]
fn update_and_get_plan() {
    let (p, n, c) = (Plans::default(), Mutex::new(Store::memory()), caller(None));
    assert_eq!(go(&p, &n, &c, "get_session_plan", json!({})).0, "No plan yet.");
    let items = json!([{"text": "read", "status": "completed"}, {"text": "write", "status": "in_progress"},
                       {"text": "test", "status": "pending"}, {"text": "ship", "status": "blocked"}]);
    let (t, err, ch) = go(&p, &n, &c, "update_session_plan", json!({"items": items, "doing": "writing", "eta_minutes": 30, "title": "R3"}));
    assert!(!err && t.starts_with("Plan stored: 1 of 4 completed (25%), ETA ~"), "{t}");
    assert_eq!(ch, Changed::Plan);
    let (t, err, ch) = go(&p, &n, &c, "get_session_plan", json!({}));
    assert!(!err && ch == Changed::Nothing);
    assert!(t.starts_with("R3\n[x] read\n[>] write\n[ ] test\n[!] ship\nDoing: writing\nETA: ~"), "{t}");
    assert!(t.ends_with("1 of 4 completed."), "{t}");
}

#[test]
fn plan_items_not_taken_when_the_agents_tool_feeds_it() {
    let (p, n, c) = (Plans::default(), Mutex::new(Store::memory()), caller(None));
    p.hook(&c.key, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "own", "status": "pending"}]}), 1);
    let (t, err, _) = go(&p, &n, &c, "update_session_plan", json!({"items": [{"text": "x", "status": "completed"}], "doing": "d"}));
    assert!(!err && t.contains("not taken") && t.contains("0 of 1 completed"), "{t}");
    let d = p.details(&c.key, false, 2).unwrap();
    assert_eq!((d.items[0].text.as_str(), d.doing.as_deref()), ("own", Some("d")));
}

#[test]
fn plan_validation() {
    let (p, n, c) = (Plans::default(), Mutex::new(Store::memory()), caller(None));
    let bad = [
        (json!({}), "items is required"),
        (json!({"items": []}), "whole plan"),
        (json!({"items": (0..51).map(|_| json!({"text": "x", "status": "pending"})).collect::<Vec<_>>()}), "1–50"),
        (json!({"items": [{"text": "x", "status": "done"}]}), "pending, in_progress, completed, blocked"),
        (json!({"items": [{"text": "x".repeat(201), "status": "pending"}]}), "over 200"),
        (json!({"items": [{"status": "pending"}]}), "text is required"),
        (json!({"items": [{"text": "x", "status": "pending"}], "doing": "d".repeat(121)}), "over 120"),
        (json!({"items": [{"text": "x", "status": "pending"}], "title": "t".repeat(81)}), "over 80"),
        (json!({"items": [{"text": "x", "status": "pending"}], "eta_minutes": 0}), "1 to 1440"),
        (json!({"items": [{"text": "x", "status": "pending"}], "eta_minutes": 1441}), "1 to 1440"),
    ];
    for (args, want) in bad {
        let (t, err, ch) = go(&p, &n, &c, "update_session_plan", args.clone());
        assert!(err && t.contains(want) && ch == Changed::Nothing, "{args}: {t}");
    }
    assert_eq!(go(&p, &n, &c, "get_session_plan", json!({})).0, "No plan yet.");
}

#[test]
fn records_add_and_update_in_scope() {
    let (p, n) = (Plans::default(), Mutex::new(Store::memory()));
    let c = caller(Some("p1"));
    let (t, err, ch) = go(&p, &n, &c, "add_notes_for_user", json!({"notes": [{"title": "Report", "text": "a"}, {"title": "Decide", "text": "b", "remind": true, "kind": "question"}]}));
    let ids: Vec<String> = n.lock().all().iter().map(|i| i.id.clone()).collect();
    assert!(!err && matches!(&ch, Changed::Marks(t) if t.len() == 2) && t.contains(&format!("bookmark {} \"Report\"", ids[0])), "{t}");
    assert_eq!(n.lock().all()[1].project.as_deref(), Some("p1"));
    let (t, err, _) = go(&p, &n, &c, "update_notes_for_user", json!({"id": ids[1], "status": "done"}));
    assert!(!err && t == format!("Reminder {} updated (resolved).", ids[1]), "{t}");
    let other = Caller { place: Place { key: "tmux:local:t:%2".into(), project: Some("p1".into()), ..Place::default() }, key: "tmux:local:t:%2".into(), ..caller(None) };
    let (t, err, _) = go(&p, &n, &other, "update_notes_for_user", json!({"id": ids[0], "status": "done"}));
    assert!(err && t.contains("No record"), "{t}");
    assert!(go(&p, &n, &c, "update_notes_for_user", json!({"id": ids[0]})).1);
    assert!(go(&p, &n, &c, "update_notes_for_user", json!({"status": "done"})).1);
    assert!(go(&p, &n, &c, "add_notes_for_user", json!({"notes": [{"title": "t"}]})).1);
    assert!(go(&p, &n, &c, "add_notes_for_user", json!({})).1);
}

#[test]
fn unknown_tool() {
    let (t, err, ch) = go(&Plans::default(), &Mutex::new(Store::memory()), &caller(None), "rm_rf", json!({}));
    assert!(err && t.starts_with("unknown tool rm_rf") && ch == Changed::Nothing, "{t}");
}

#[test]
fn a_call_of_an_unknown_agent_after_a_restart_keeps_the_plan() {
    let (p, n) = (Plans::default(), Mutex::new(Store::memory()));
    let c = caller(None);
    p.hook(&c.key, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "a"}]}), 999_000);
    let restarted = Caller { agent: crate::plans::UNKNOWN.into(), session: None, ..caller(None) };
    let (t, err, _) = go(&p, &n, &restarted, "update_session_plan", json!({"items": [{"text": "b", "status": "pending"}], "doing": "d"}));
    assert!(!err && t.contains("not taken"), "{t}");
    assert_eq!(p.archived(), 0);
    assert!(go(&p, &n, &c, "update_session_plan", json!({"items": [{"text": "b", "status": "pending"}], "eta_minutes": 5.0})).1, "a float ETA is refused");
}

#[test]
fn without_hook_events_the_plan_answers_say_hooks_are_off() {
    let (p, n) = (Plans::default(), Mutex::new(Store::memory()));
    let c = Caller { hooks: false, agent: crate::plans::UNKNOWN.into(), ..caller(None) };
    let items = json!([{"text": "a", "status": "pending"}]);
    let (t, err, _) = go(&p, &n, &c, "update_session_plan", json!({"items": items}));
    assert!(!err && t.ends_with(&format!("\n{HOOKS_OFF}")), "{t}");
    let (t, _, _) = go(&p, &n, &c, "get_session_plan", json!({}));
    assert!(t.starts_with("[ ] a") && t.ends_with(HOOKS_OFF), "{t}");
    let (t, _, _) = go(&p, &n, &c, "add_notes_for_user", json!({"notes": [{"kind": "bug", "text": "x"}]}));
    assert!(!t.contains(HOOKS_OFF), "notes answers stay as they are");
    let (t, _, _) = go(&p, &n, &caller(None), "get_session_plan", json!({}));
    assert!(!t.contains(HOOKS_OFF), "hooked: no line");
}

#[test]
fn another_sessions_plan_update_is_not_applied() {
    let (p, n, c) = (Plans::default(), Mutex::new(Store::memory()), caller(None));
    go(&p, &n, &c, "update_session_plan", json!({"items": [{"text": "main", "status": "in_progress"}], "doing": "main step"}));
    let sub = Caller { other: true, ..caller(None) };
    let (t, err, ch) = go(&p, &n, &sub, "update_session_plan", json!({"items": [{"text": "sub", "status": "pending"}], "doing": "sub step", "eta_minutes": 5}));
    assert!(err && t == OTHER_SESSION && ch == Changed::Nothing, "{t}");
    let (t, _, _) = go(&p, &n, &c, "get_session_plan", json!({}));
    assert!(t.starts_with("[>] main\nDoing: main step") && !t.contains("ETA"), "{t}");
    let (t, err, _) = go(&p, &n, &sub, "get_session_plan", json!({}));
    assert!(!err && t.starts_with("[>] main"), "a subagent may read it: {t}");
}
