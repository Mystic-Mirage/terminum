use super::*;

fn caller() -> Caller {
    let key = "tmux:local:t:%1".to_string();
    Caller { agent: "claude".into(), session: Some("s1".into()), key, hooks: true, other: false }
}

fn go(plans: &Plans, c: &Caller, tool: &str, args: Value) -> (String, bool, bool) {
    run(plans, c, tool, &args, false, 1_000_000)
}

#[test]
fn update_and_get_plan() {
    let (p, c) = (Plans::default(), caller());
    assert_eq!(go(&p, &c, "get_session_plan", json!({})).0, "No plan yet.");
    let items = json!([{"text": "read", "status": "completed"}, {"text": "write", "status": "in_progress"},
                       {"text": "test", "status": "pending"}, {"text": "ship", "status": "blocked"}]);
    let (t, err, ch) = go(&p, &c, "update_session_plan", json!({"items": items, "doing": "writing", "eta_minutes": 30, "title": "R3"}));
    assert!(!err && t.starts_with("Plan stored: 1 of 4 completed (25%), ETA ~"), "{t}");
    assert!(ch);
    let (t, err, ch) = go(&p, &c, "get_session_plan", json!({}));
    assert!(!err && !ch);
    assert!(t.starts_with("R3\n[x] read\n[>] write\n[ ] test\n[!] ship\nDoing: writing\nETA: ~"), "{t}");
    assert!(t.ends_with("1 of 4 completed."), "{t}");
}

#[test]
fn plan_items_not_taken_when_the_agents_tool_feeds_it() {
    let (p, c) = (Plans::default(), caller());
    p.hook(&c.key, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "own", "status": "pending"}]}), 1);
    let (t, err, _) = go(&p, &c, "update_session_plan", json!({"items": [{"text": "x", "status": "completed"}], "doing": "d"}));
    assert!(!err && t.contains("not taken") && t.contains("0 of 1 completed"), "{t}");
    let d = p.details(&c.key, false, 2).unwrap();
    assert_eq!((d.items[0].text.as_str(), d.doing.as_deref()), ("own", Some("d")));
}

#[test]
fn plan_validation() {
    let (p, c) = (Plans::default(), caller());
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
        let (t, err, ch) = go(&p, &c, "update_session_plan", args.clone());
        assert!(err && t.contains(want) && !ch, "{args}: {t}");
    }
    assert_eq!(go(&p, &c, "get_session_plan", json!({})).0, "No plan yet.");
}

#[test]
fn ac98_an_agent_holding_the_old_list_is_told_the_tools_are_gone() {
    for tool in ["add_notes_for_user", "get_notes_for_user", "update_notes_for_user", "delete_notes_for_user"] {
        let (t, err, ch) = go(&Plans::default(), &caller(), tool, json!({"notes": [{"title": "t", "text": "a"}]}));
        assert_eq!((t.as_str(), err, ch), ("This tool is gone. Use create_task for what must not get lost.", true, false), "{tool}");
    }
}

#[test]
fn unknown_tool() {
    let (t, err, ch) = go(&Plans::default(), &caller(), "rm_rf", json!({}));
    assert!(err && t.starts_with("unknown tool rm_rf") && !ch, "{t}");
}

#[test]
fn a_call_of_an_unknown_agent_after_a_restart_keeps_the_plan() {
    let p = Plans::default();
    let c = caller();
    p.hook(&c.key, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "a"}]}), 999_000);
    let restarted = Caller { agent: crate::plans::UNKNOWN.into(), session: None, ..caller() };
    let (t, err, _) = go(&p, &restarted, "update_session_plan", json!({"items": [{"text": "b", "status": "pending"}], "doing": "d"}));
    assert!(!err && t.contains("not taken"), "{t}");
    assert_eq!(p.archived(), 0);
    assert!(go(&p, &c, "update_session_plan", json!({"items": [{"text": "b", "status": "pending"}], "eta_minutes": 5.0})).1, "a float ETA is refused");
}

#[test]
fn without_hook_events_the_plan_answers_say_hooks_are_off() {
    let p = Plans::default();
    let c = Caller { hooks: false, agent: crate::plans::UNKNOWN.into(), ..caller() };
    let items = json!([{"text": "a", "status": "pending"}]);
    let (t, err, _) = go(&p, &c, "update_session_plan", json!({"items": items}));
    assert!(!err && t.ends_with(&format!("\n{HOOKS_OFF}")), "{t}");
    let (t, _, _) = go(&p, &c, "get_session_plan", json!({}));
    assert!(t.starts_with("[ ] a") && t.ends_with(HOOKS_OFF), "{t}");
    let (t, _, _) = go(&p, &caller(), "get_session_plan", json!({}));
    assert!(!t.contains(HOOKS_OFF), "hooked: no line");
}

#[test]
fn another_sessions_plan_update_is_not_applied() {
    let (p, c) = (Plans::default(), caller());
    go(&p, &c, "update_session_plan", json!({"items": [{"text": "main", "status": "in_progress"}], "doing": "main step"}));
    let sub = Caller { other: true, ..caller() };
    let (t, err, ch) = go(&p, &sub, "update_session_plan", json!({"items": [{"text": "sub", "status": "pending"}], "doing": "sub step", "eta_minutes": 5}));
    assert!(err && t == OTHER_SESSION && !ch, "{t}");
    let (t, _, _) = go(&p, &c, "get_session_plan", json!({}));
    assert!(t.starts_with("[>] main\nDoing: main step") && !t.contains("ETA"), "{t}");
    let (t, err, _) = go(&p, &sub, "get_session_plan", json!({}));
    assert!(!err && t.starts_with("[>] main"), "a subagent may read it: {t}");
}
