// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use serde_json::json;

use super::*;

const K: &str = "tmux:local:terminum:%3";
const T0: u64 = 1_800_000_000;

fn items(p: &Plans) -> Vec<(String, String)> {
    p.details(K, true, T0).map(|d| d.items.into_iter().map(|i| (i.text, i.status)).collect()).unwrap_or_default()
}

fn pair(t: &str, s: &str) -> (String, String) {
    (t.to_string(), s.to_string())
}

#[test]
fn deleting_a_task_with_a_subject_never_renames_the_next_task() {
    let p = Plans::default();
    p.hook(K, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "one"}, {"text": "two"}]}), T0);
    p.hook(K, "claude", Some("s1"), &json!({"op": "set", "id": "1", "status": "deleted", "text": "renamed"}), T0 + 1);
    assert_eq!(items(&p), [pair("two", "pending")]);
}

#[test]
fn task_ops_build_the_plan_and_its_percent() {
    let p = Plans::default();
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "1", "text": "alpha"}), T0);
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "2", "text": "beta"}), T0);
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "3", "text": "gamma"}), T0);
    p.hook(K, "claude", Some("s1"), &json!({"op": "set", "id": "1", "status": "completed"}), T0 + 5);
    p.hook(K, "claude", Some("s1"), &json!({"op": "set", "id": "2", "status": "in_progress", "text": "beta two"}), T0 + 5);
    assert_eq!(items(&p), vec![pair("alpha", "completed"), pair("beta two", "in_progress"), pair("gamma", "pending")]);
    assert_eq!(p.row(K, true, T0 + 5), Some(Row { done: 1, total: 3, pct: 33, eta: None, updated: T0 + 5, stale: false, partial: false, doing: None }));
    p.hook(K, "claude", Some("s1"), &json!({"op": "set", "id": "3", "status": "deleted"}), T0 + 6);
    assert_eq!(p.row(K, true, T0 + 6).unwrap().pct, 50, "deleted: removed, not counted");
    assert_eq!((percent(2, 3), percent(0, 0), percent(1, 8)), (67, 0, 13));
}

#[test]
fn an_unknown_id_marks_partial_until_a_whole_list_heals_it() {
    let p = Plans::default();
    p.hook(K, "claude", Some("s1"), &json!({"op": "set", "id": "7", "status": "completed"}), T0);
    assert!(items(&p).is_empty(), "never invents an item");
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "8", "text": "next"}), T0);
    assert!(p.row(K, true, T0).unwrap().partial);
    p.hook(K, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "a", "status": "completed"}, {"text": "b", "status": "weird"}]}), T0);
    assert_eq!(items(&p), vec![pair("a", "completed"), pair("b", "pending")], "an unknown status is pending");
    assert!(!p.row(K, true, T0).unwrap().partial);
    p.hook(K, "claude", Some("s1"), &json!({"op": "nonsense"}), T0 + 9);
    assert_eq!(p.row(K, true, T0 + 9).unwrap().updated, T0, "an unknown op changes nothing");
}

#[test]
fn mcp_items_only_while_no_tool_plan() {
    let p = Plans::default();
    let up = |list: &[(&str, &str)]| Update { items: Some(list.iter().map(|(t, s)| pair(t, s)).collect()), ..Default::default() };
    assert!(p.mcp(K, "codex", Some("x1"), Update { eta_minutes: Some(20), doing: Some("reading".into()), ..up(&[("read", "in_progress"), ("fix", "blocked")]) }, T0));
    let d = p.details(K, true, T0).unwrap();
    assert_eq!((d.source, d.doing.as_deref(), d.row.eta, d.row.total), (Source::Mcp, Some("reading"), Some(T0 + 1200), 2));
    assert_eq!(items(&p)[1], pair("fix", "blocked"));
    // a built-in plan replaces MCP items; doing stays
    p.hook(K, "codex", Some("x1"), &json!({"op": "all", "items": [{"text": "one", "status": "pending"}]}), T0 + 60);
    let d = p.details(K, true, T0 + 60).unwrap();
    assert_eq!((d.source, d.doing.as_deref(), d.row.eta), (Source::Tool, Some("reading"), None), "items changed without an eta: eta dropped");
    assert!(!p.mcp(K, "codex", Some("x1"), Update { title: Some("Port".into()), eta_minutes: Some(5), ..up(&[("other", "pending")]) }, T0 + 90));
    let d = p.details(K, true, T0 + 90).unwrap();
    assert_eq!((d.items.len(), d.title.as_deref(), d.row.eta, d.eta_given), (1, Some("Port"), Some(T0 + 390), Some(T0 + 90)), "title and eta kept on a tool plan");
    // Claude Code names no session to an MCP server: the plan's own
    assert!(p.mcp(K, "codex", None, Update { doing: Some(String::new()), ..Default::default() }, T0 + 99));
    assert_eq!(p.details(K, true, T0 + 99).unwrap().doing, None, "an empty doing clears it");
    assert_eq!(p.archived(), 0);
}

#[test]
fn a_new_session_or_program_archives() {
    let p = Plans::default();
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "1", "text": "a"}), T0);
    p.session(K, Some("s1"));
    assert_eq!((items(&p).len(), p.archived()), (1, 0), "a resumed session keeps its plan");
    p.hook(K, "claude", Some("s2"), &json!({"op": "add", "id": "1", "text": "b"}), T0 + 1);
    assert_eq!((items(&p), p.archived()), (vec![pair("b", "pending")], 1), "another session id");
    p.mcp(K, "codex", None, Update::default(), T0 + 2);
    assert_eq!((items(&p).len(), p.archived()), (0, 2), "another agent");
    p.hook(K, "codex", Some("x"), &json!({"op": "add", "id": "1", "text": "c"}), T0 + 3);
    p.session(K, None);
    assert_eq!((p.row(K, true, T0), p.archived()), (None, 3), "the program changed");
    for n in 0..60 {
        p.hook(K, "claude", Some(&format!("s{n}")), &json!({"op": "add", "id": "1", "text": "x"}), T0);
    }
    assert_eq!(p.archived(), ARCHIVED);
    p.retain("tmux:local:terminum:", &[]);
    assert_eq!(p.details(K, true, T0), None, "a pane gone is dropped");
}

#[test]
fn the_reminder_is_adaptive_and_never_twice_in_a_window() {
    let min = 60;
    assert_eq!(threshold(Some(T0), T0 + 30 * min), 10 * min);
    assert_eq!(threshold(Some(T0), T0 + 120 * min), 20 * min);
    assert_eq!(threshold(Some(T0), T0 + 600 * min), 30 * min);
    assert_eq!(threshold(None, T0), 10 * min);
    let r = reminder(Some(T0), true, Some(T0), T0 + 12 * min, None).unwrap();
    assert!(r.contains("was last updated 12 min ago") && r.contains("update_session_plan"), "{r}");
    assert_eq!(reminder(Some(T0), true, Some(T0), T0 + 9 * min, None), None, "fresh");
    assert_eq!(reminder(Some(T0), false, Some(T0), T0 + 60 * min, None), None, "never for an idle pane");
    assert_eq!(reminder(Some(T0), true, Some(T0), T0 + 15 * min, Some(T0 + 12 * min)), None, "reminded within the window");
    assert!(reminder(Some(T0), true, Some(T0), T0 + 22 * min, Some(T0 + 12 * min)).is_some());
    // no plan: once a turn, after 10 min of it
    assert_eq!(reminder(None, true, Some(T0), T0 + 9 * min, None), None);
    assert!(reminder(None, true, Some(T0), T0 + 10 * min, None).unwrap().contains("has no plan yet"));
    assert_eq!(reminder(None, true, Some(T0), T0 + 50 * min, Some(T0 + 10 * min)), None, "once per turn");
    assert!(reminder(None, true, Some(T0 + 60 * min), T0 + 70 * min, Some(T0 + 10 * min)).is_some(), "a new turn");
    assert_eq!(reminder(None, true, None, T0 + 70 * min, None), None, "no turn seen");
}

#[test]
fn post_tool_reminds_and_records_it() {
    let p = Plans::default();
    assert_eq!(p.post_tool(K, true, T0), None, "a pane never seen");
    p.turn(K, "claude", Some("s1"), T0);
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "1", "text": "a"}), T0);
    assert_eq!(p.post_tool(K, true, T0 + 300), None);
    assert!(p.row(K, true, T0 + 700).unwrap().stale);
    assert!(p.post_tool(K, true, T0 + 700).is_some());
    assert_eq!(p.post_tool(K, true, T0 + 800), None, "reminded_at kept");
    assert_eq!(p.post_tool(K, false, T0 + 5000), None);
}

#[test]
fn a_finished_plan_or_one_updated_this_turn_is_not_reminded() {
    let p = Plans::default();
    p.turn(K, "claude", Some("s1"), T0 + 3600);                  // finished in its turn (the next request archives it)
    p.hook(K, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "a", "status": "completed"}]}), T0 + 3600);
    assert_eq!(p.post_tool(K, true, T0 + 3600 + 5000), None, "every item completed: nothing to update");
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "9", "text": "b"}), T0 + 3600 + 5000);
    p.turn(K, "claude", Some("s1"), T0 + 99_000);
    assert_eq!(p.post_tool(K, true, T0 + 99_010), None, "an earlier turn's plan: staleness counts from this turn");
    assert!(p.post_tool(K, true, T0 + 99_000 + 600).is_some(), "the turn's own threshold still reminds");
}

#[test]
fn the_row_is_stale_as_the_reminder_judges_it() {
    let p = Plans::default();
    p.turn(K, "claude", Some("s1"), T0 + 3600);                  // finished in its turn (the next request archives it)
    p.hook(K, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "a", "status": "completed"}]}), T0 + 3600);
    assert!(!p.row(K, true, T0 + 3600 + 5000).unwrap().stale, "every item completed: never stale");
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "9", "text": "b"}), T0 + 3600 + 5000);
    p.turn(K, "claude", Some("s1"), T0 + 99_000);
    assert!(!p.row(K, true, T0 + 99_010).unwrap().stale, "an earlier turn's plan: its age counts from this turn");
    assert!(p.row(K, true, T0 + 99_000 + 600).unwrap().stale);
    assert!(!p.row(K, false, T0 + 99_000 + 600).unwrap().stale, "an idle pane is never stale");
}

#[test]
fn a_malformed_op_leaves_an_mcp_plan_alone() {
    let p = Plans::default();
    let u = Update { items: Some(vec![pair("m", "pending")]), ..Default::default() };
    assert!(p.mcp(K, "claude", Some("s1"), u, T0));
    for op in [json!({"op": "add", "id": "1"}), json!({"op": "add", "text": "x"}), json!({"op": "set"}), json!({"op": "zap"})] {
        p.hook(K, "claude", Some("s1"), &op, T0 + 1);
        assert_eq!(items(&p), vec![pair("m", "pending")], "{op}");
    }
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "1", "text": "t"}), T0 + 2);
    assert_eq!(items(&p), vec![pair("t", "pending")], "a well-formed op of the tool takes over");
}

#[test]
fn an_unknown_agent_or_session_matches_the_stored_plan() {
    let p = Plans::default();
    p.hook(K, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "a"}]}), T0);
    let u = || Update { doing: Some("d".into()), ..Default::default() };
    assert!(p.mcp(K, UNKNOWN, None, u(), T0 + 1), "terminumd restarted: the hooks know no agent yet");
    assert!(p.mcp(K, UNKNOWN, Some("mcp-meta-id"), u(), T0 + 2));
    assert_eq!((items(&p), p.archived()), (vec![pair("a", "pending")], 0));
    p.hook(K, "claude", Some("s1"), &json!({"op": "set", "id": "1", "status": "completed"}), T0 + 3);
    assert_eq!(p.archived(), 0, "the hooks' agent comes back to its own plan");
    p.mcp(K, UNKNOWN, None, Update { items: Some(vec![pair("x", "pending")]), ..Default::default() }, T0 + 4);
    let q = Plans::default();
    q.mcp(K, UNKNOWN, None, Update { items: Some(vec![pair("x", "pending")]), ..Default::default() }, T0);
    q.turn(K, "codex", Some("c1"), T0 + 1);
    assert_eq!((q.archived(), q.details(K, true, T0 + 1).map(|d| (d.agent, d.items.len()))), (0, Some(("codex".into(), 1))),
               "a plan of an unknown agent takes the first known one");
    q.turn(K, "codex", Some("c2"), T0 + 2);
    assert_eq!(q.archived(), 1, "a known other session archives");
    q.turn(K, "claude", Some("c2"), T0 + 3);
    q.mcp(K, "claude", None, Update { items: Some(vec![pair("y", "pending")]), ..Default::default() }, T0 + 4);
    q.turn(K, "codex", Some("c2"), T0 + 5);
    assert_eq!(q.archived(), 2, "a known other agent archives");
}

#[test]
fn the_last_archived_plan_shows_until_a_new_one() {
    let p = Plans::default();
    assert!(p.last_archived(K, T0).is_none(), "nothing archived");
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "1", "text": "a"}), T0);
    assert!(p.last_archived(K, T0).is_none(), "a live plan is no finished one");
    p.session(K, None);
    let d = p.last_archived(K, T0 + 5).expect("the exited agent's plan");
    assert_eq!((d.items.len(), d.row.total, d.row.stale), (1, 1, false));
    assert!(p.last_archived("tmux:local:x:%9", T0).is_none(), "another pane's");
    p.hook(K, "claude", Some("s2"), &json!({"op": "add", "id": "1", "text": "b"}), T0 + 9);
    assert!(p.last_archived(K, T0 + 9).is_none(), "a new session's plan replaces it");
}

#[test]
fn a_finished_plan_leaves_with_the_next_request_and_its_row_after_its_turn() {
    let p = Plans::default();
    p.turn(K, "claude", Some("s1"), T0);
    p.hook(K, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "a", "status": "completed"}, {"text": "b", "status": "completed"}]}), T0 + 10);
    assert_eq!(p.row(K, true, T0 + 5000).map(|r| r.pct), Some(100), "working: shown at 100%");
    assert_eq!(p.row(K, false, T0 + 10 + FINISHED_SHOWN - 1).map(|r| r.pct), Some(100), "just finished: shown a while");
    assert_eq!(p.row(K, false, T0 + 10 + FINISHED_SHOWN), None, "its turn over: gone from the row");
    assert_eq!(p.details(K, false, T0 + 10 + FINISHED_SHOWN).map(|d| d.row.pct), Some(100), "the Plan pane still has it");
    p.turn(K, "claude", Some("s1"), T0 + 9000);
    assert_eq!((p.row(K, true, T0 + 9000), p.archived()), (None, 1), "the next request starts without it, archived");
    assert!(p.last_archived(K, T0 + 9000).is_none(), "not shown as an exited session's");
    // one not finished stays across requests
    p.hook(K, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "c", "status": "completed"}, {"text": "d", "status": "pending"}]}), T0 + 9010);
    p.turn(K, "claude", Some("s1"), T0 + 20000);
    assert_eq!((p.row(K, false, T0 + 20000).map(|r| r.pct), p.archived()), (Some(50), 1));
}
