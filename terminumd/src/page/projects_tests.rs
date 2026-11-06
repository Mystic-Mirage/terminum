// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Projects (AC-92): names, colors, members, order and closed panes, in memory.

use super::*;

const A: &str = "tmux:local:terminum:%1";
const B: &str = "tmux:local:terminum:%2";
const C: &str = "tmux:devbox:default:%1";

fn store() -> Projects {
    Projects::default()
}

#[test]
fn invalid_project_schemas_are_backed_up_before_a_later_save() {
    let dir = crate::testutil::fresh_dir("projects-schema");
    let path = dir.join("projects.json");
    assert!(Projects::load(path.clone()).list().is_empty(), "missing state starts empty");
    for malformed in [
        json!({"projects": [{"name": "recoverable", "members": []}], "servers": {}}),
        json!({"projects": {}, "servers": {}}),
        json!({"projects": [], "servers": []}),
        json!([]),
    ] {
        let original = malformed.to_string();
        std::fs::write(&path, &original).unwrap();
        let projects = Projects::load(path.clone());
        let backup = path.with_extension("json.bak");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert!(!path.exists(), "invalid schema must be set aside before writing");
        projects.save().unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), original);
    }
    std::fs::write(&path, "{}").unwrap();
    assert!(Projects::load(path.clone()).list().is_empty(), "missing keys keep empty defaults");
    let valid = json!({"projects": [{"id": "p1", "name": "work", "color": 2, "host": null,
        "dir": "/sandbox", "members": [A]}], "servers": {"tmux:local:terminum:": "server1"}, "order": ["g:local/main", "p:p1"]});
    std::fs::write(&path, valid.to_string()).unwrap();
    Projects::load(path.clone()).save().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap(), valid);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_new_project_is_named_and_colored_apart_from_the_others() {
    let mut ps = store();
    let p1 = ps.add(A, None, None, None, "/Users/alex/api".into(), None).unwrap();
    let p2 = ps.add(B, None, None, None, "/Users/alex/web".into(), None).unwrap();
    let names: Vec<_> = ps.list().iter().map(|p| (p.name.clone(), p.color, p.dir.clone())).collect();
    assert_eq!(names, [("Group 1".to_string(), 0, "/Users/alex/api".to_string()), ("Group 2".into(), 1, "/Users/alex/web".into())]);
    ps.delete(&p1).unwrap();
    let p3 = ps.add(A, None, None, None, "/tmp".into(), None).unwrap();
    let p = ps.list().iter().find(|p| p.id == p3).unwrap();
    assert_eq!((p.name.as_str(), p.color), ("Group 1", 0), "the lowest free name and color again");
    let p4 = ps.add(C, None, None, None, "/c".into(), Some("  API  ")).unwrap();
    assert_eq!(ps.list().iter().find(|p| p.id == p4).unwrap().name, "API", "named as it is made");
    assert!(ps.add(B, None, None, None, "/c".into(), Some("a\u{7}")).is_err());
    assert_ne!(p2, p3);
}

#[test]
fn a_pane_is_in_one_project_and_an_empty_one_goes() {
    let mut ps = store();
    let p1 = ps.add(A, None, None, None, "/a".into(), None).unwrap();
    ps.add(B, Some(&p1), None, None, "/a".into(), None).unwrap();
    let p2 = ps.add(B, None, None, None, "/b".into(), None).unwrap();
    assert_eq!(ps.of(B).unwrap().id, p2, "moved, not copied");
    assert_eq!(ps.of(A).unwrap().members, [A]);
    ps.remove(A).unwrap();
    assert!(ps.list().iter().all(|p| p.id != p1), "its last session left: the project goes");
    assert!(ps.add(A, Some("nope"), None, None, "/".into(), None).is_err());
}

#[test]
fn order_members_rename_and_after() {
    let mut ps = store();
    let p1 = ps.add(A, None, None, None, "/a".into(), None).unwrap();
    ps.add(B, Some(&p1), None, None, "/a".into(), None).unwrap();
    ps.add(C, Some(&p1), Some(A), None, "/a".into(), None).unwrap();
    assert_eq!(ps.of(A).unwrap().members, [A, C, B], "a clone goes right after its session");
    ps.members(&p1, &[B.into(), A.into()]).unwrap();
    assert_eq!(ps.of(A).unwrap().members, [B, A, C]);
    ps.rename(&p1, "  API  ").unwrap();
    assert_eq!(ps.of(A).unwrap().name, "API");
    assert!(ps.rename(&p1, " ").is_err() && ps.rename(&p1, "a\u{7}").is_err());
    let p2 = ps.add("tmux:local:terminum:%9", None, None, None, "/z".into(), None).unwrap();
    ps.order(&[format!("p:{p2}"), "g:local/main".to_string(), format!("p:{p1}")]).unwrap();
    assert_eq!(ps.list_order(), [format!("p:{p2}"), "g:local/main".to_string(), format!("p:{p1}")], "the projects and groups together");
    assert!(ps.order(&vec!["g:x".to_string(); ORDER_MOST + 1]).is_err() && ps.order(&["g:".to_string() + &"x".repeat(ORDER_ID)]).is_err(), "what a page sends is bounded");
    assert_eq!(ps.list().iter().map(|p| p.id.clone()).collect::<Vec<_>>(), [p2, p1]);
}

#[test]
fn closed_panes_leave_when_their_server_is_read() {
    let mut ps = store();
    let p1 = ps.add(A, None, None, None, "/a".into(), None).unwrap();
    ps.add(C, Some(&p1), None, None, "/a".into(), None).unwrap();
    let live: HashSet<&str> = [B].into_iter().collect();
    assert!(ps.seen("tmux:local:terminum:", Some("m1"), &live).0, "the first read adopts the server");
    assert_eq!(ps.of(A).unwrap().members, [A, C], "added just now: spared (a read on its way may not have it)");
    ps.fresh.clear();
    assert!(ps.seen("tmux:local:terminum:", Some("m1"), &live).0);
    assert_eq!(ps.of(C).unwrap().members, [C], "another server's pane stays until that server is read");
    assert!(!ps.seen("tmux:local:terminum:", Some("m1"), &live).0, "nothing more to change");
    ps.seen("tmux:devbox:default:", Some("d1"), &HashSet::new());
    ps.fresh.clear();
    assert!(ps.seen("tmux:devbox:default:", Some("d1"), &HashSet::new()).0);
    assert!(ps.list().is_empty());
}

/// A tmux server started anew numbers its panes anew: its old members never name new panes.
#[test]
fn a_server_started_anew_keeps_projects_only_through_its_restore() {
    let mut ps = store();
    let p1 = ps.add(A, None, None, None, "/a".into(), None).unwrap();
    ps.add(B, Some(&p1), None, None, "/a".into(), None).unwrap();
    ps.fresh.clear();
    let both: HashSet<&str> = [A, B].into_iter().collect();
    ps.seen("tmux:local:terminum:", Some("m1"), &both);
    // restored: %1 -> %7, %2 not made again; the next read (with the old keys still) adopts it
    assert!(ps.restarted("tmux:local:terminum:", &[("%1".into(), "%7".into())]));
    assert_eq!(ps.of("tmux:local:terminum:%7").unwrap().members, ["tmux:local:terminum:%7"]);
    assert_eq!(ps.seen("tmux:local:terminum:", Some("m2"), &HashSet::new()).1, Closed::None, "bookmarks: none closed");
    assert!(ps.of("tmux:local:terminum:%7").is_some(), "a read right after the restore takes nothing");
    assert_eq!(ps.seen("tmux:local:terminum:", Some("m2"), &HashSet::new()).1, Closed::NotLive, "then those not listed");
    // started anew without a restore: its %7 is another pane now
    let seven: HashSet<&str> = ["tmux:local:terminum:%7"].into_iter().collect();
    assert_eq!(ps.seen("tmux:local:terminum:", Some("m3"), &seven), (true, Closed::All), "bookmarks: all its panes' closed");
    assert!(ps.list().is_empty(), "another server's pane %7 is not the project's");
}

/// A restore again passes over the sessions still running: paired with themselves, they keep their projects.
#[test]
fn a_restore_again_keeps_the_running_sessions() {
    let mut ps = store();
    let p1 = ps.add(A, None, None, None, "/a".into(), None).unwrap();
    ps.add(B, Some(&p1), None, None, "/a".into(), None).unwrap();
    ps.restarted("tmux:local:terminum:", &[("%9".into(), "%12".into()), ("%1".into(), "%1".into()), ("%2".into(), "%2".into())]);
    assert_eq!(ps.of(A).unwrap().members, [A, B]);
}

/// AC-92: a member kept for being fresh while the list no longer shows it asks for one list
/// read when it stops being fresh, so its closed pane leaves the project without another change.
#[test]
fn a_fresh_member_the_list_lost_is_read_again_when_it_stops_being_fresh() {
    let mut ps = store();
    ps.add(A, None, None, None, "/a".into(), None).unwrap();
    let none: HashSet<&str> = HashSet::new();
    ps.seen("tmux:local:terminum:", Some("m1"), &none);
    assert!(ps.of(A).is_some(), "kept: added a moment ago");
    let left = ps.fresh_left("tmux:local:terminum:", &none).expect("a read is due");
    assert!(left > Duration::from_secs(4) && left <= FRESH + Duration::from_millis(50), "{left:?}");
    assert_eq!(ps.fresh_left("tmux:local:terminum:", &none), None, "one read is due already");
    let live: HashSet<&str> = [A].into_iter().collect();
    ps.recheck = None;
    assert_eq!(ps.fresh_left("tmux:local:terminum:", &live), None, "listed: nothing to read again for");
}
