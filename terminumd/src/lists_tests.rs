// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a server's list keeps when it cannot be read (AC-81, AC-132).

use super::*;

fn working(id: &str) -> Value {
    json!({"id": id, "title": "t", "state": "working", "tool": "Bash", "why": "x", "subagents": 2, "turnOver": true, "background": true, "agent": "claude"})
}

fn group(wid: &str, items: Vec<Value>) -> Value {
    json!({"wid": wid, "items": items, "state": "attached"})
}

fn read(groups: Vec<Value>) -> Got {
    Got::Read(Told { groups, keys: vec!["k".into()], ok: true, ..Default::default() })
}

#[test]
fn a_down_host_keeps_its_rows_where_they_were_with_no_agent_state() {
    let list = ServerList::default();
    assert!(list.store(read(vec![group("devbox/terminum", vec![working("a")])]), Instant::now()), "a good read is fresh");
    let down = json!({"wid": "devbox/terminum", "items": [], "state": "reconnecting", "why": "reconnecting (1 s)"});
    assert!(!list.store(Got::Down(vec![down]), Instant::now()), "a down host's rows are not its own fresh list");
    let t = list.told().unwrap();
    let row = &t.groups[0]["items"][0];
    assert_eq!(t.groups[0]["state"], "reconnecting", "the group says so");
    assert_eq!(row["id"], "a");
    assert_eq!(row["offline"], true);
    assert_eq!(row["agent"], "claude", "what runs there is still known");
    for k in ["tool", "why", "subagents", "turnOver", "background"] {
        assert!(row.get(k).is_none(), "{k} is gone: nothing says it still works");
    }
    assert!(row["state"].is_null(), "AC-99: no \"working\" kept forever");
    assert!(!t.ok, "a turn's end and the inventory do not count it as read");
}

#[test]
fn a_host_turned_off_keeps_nothing() {
    let list = ServerList::default();
    list.store(read(vec![group("devbox/terminum", vec![working("a")])]), Instant::now());
    list.store(Got::Gone(vec![json!({"wid": "devbox/terminum", "items": [], "state": "off"})]), Instant::now());
    assert_eq!(list.told().unwrap().groups[0]["items"], json!([]));
    list.store(Got::Down(vec![json!({"wid": "devbox/terminum", "items": [], "state": "reconnecting"})]), Instant::now());
    assert_eq!(list.told().unwrap().groups[0]["items"], json!([]), "turned on again: the old rows do not come back");
}

#[test]
fn a_group_absent_from_the_last_read_stays_as_it_says() {
    let list = ServerList::default();
    list.store(read(vec![group("devbox/terminum", vec![working("a")])]), Instant::now());
    list.store(Got::Down(vec![json!({"wid": "devbox/other", "items": [], "state": "absent"})]), Instant::now());
    let t = list.told().unwrap();
    assert_eq!(t.groups.len(), 1, "no group twice");
    assert_eq!(t.groups[0]["items"], json!([]));
}

#[test]
fn an_unanswered_list_keeps_the_rows_and_dims_them_only_after_a_while() {
    let list = ServerList::default();
    let t0 = Instant::now();
    list.store(read(vec![group("devbox/terminum", vec![working("a")])]), t0);
    assert!(!list.store(Got::Unread, t0 + Duration::from_secs(5)), "not fresh");
    let t = list.told().unwrap();
    assert_eq!(t.groups[0]["items"][0]["state"], "working", "a slow link: as last read");
    assert!(t.groups[0]["items"][0].get("offline").is_none());
    assert!(!t.ok);
    list.store(Got::Unread, t0 + STALE_AFTER);
    let row = list.told().unwrap().groups[0]["items"][0].clone();
    assert_eq!(row["offline"], true, "not read for long: not said to work");
    assert!(row["state"].is_null());
    list.store(read(vec![group("devbox/terminum", vec![working("a")])]), t0 + STALE_AFTER);
    assert_eq!(list.told().unwrap().groups[0]["items"][0]["state"], "working", "read again: as it is");
}

#[test]
fn a_server_with_no_way_to_read_keeps_nothing_it_says() {
    let list = ServerList::default();
    list.store(read(vec![group("local/main", vec![working("a")])]), Instant::now());
    assert!(!list.store(Got::Gone(Vec::new()), Instant::now()));
    assert!(list.told().unwrap().groups.is_empty(), "AC-01: tmux gone, and nothing else on the list");
}

#[test]
fn a_read_with_a_failure_in_it_is_shown_but_not_kept_or_fresh() {
    let list = ServerList::default();
    list.store(read(vec![group("local/main", vec![working("a")])]), Instant::now());
    let partial = Got::Read(Told { groups: vec![group("local/main", Vec::new())], ..Default::default() });
    assert!(!list.store(partial, Instant::now()));
    assert_eq!(list.told().unwrap().groups[0]["items"], json!([]));
    list.store(Got::Down(vec![json!({"wid": "local/main", "items": [], "state": "reconnecting"})]), Instant::now());
    assert_eq!(list.told().unwrap().groups[0]["items"][0]["id"], "a", "the last good one's rows");
}

#[tokio::test]
async fn ac132_news_of_one_pane_or_host_asks_only_its_server() {
    let (root, app) = crate::testutil::app("one-server-news");
    let link = crate::tmux::server::Link::Ssh { ssh: "unused".into(), args: Vec::new() };
    let host = Server::new("devbox", "default", link, app.changed.clone());
    app.tmux.add_host(host.clone());
    let asked = |s: &Server| s.list.changed.swap(false, Ordering::Relaxed);
    let all_woken = || async { tokio::time::timeout(Duration::from_millis(20), app.dirty.notified()).await.is_ok() };

    pane_changed(&app, &host.key("%1"));
    assert!(asked(&host) && !asked(&app.tmux.local), "a pane's news: its server");
    assert!(!all_woken().await, "not all of them");
    host_changed(&app, "devbox");
    assert!(asked(&host) && !asked(&app.tmux.local), "a host's news: that host");
    assert!(!all_woken().await);
    host_changed(&app, "gone.example");
    assert!(all_woken().await, "a host that is gone from the list: read all");
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
