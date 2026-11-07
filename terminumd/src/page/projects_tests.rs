// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Projects (AC-92): names, colors, members, order and closed panes, in memory.

use super::*;

const A: &str = "tmux:local:terminum:%1";
const B: &str = "tmux:local:terminum:%2";
const C: &str = "tmux:devbox:default:%1";

fn store() -> Projects {
    Projects::default()
}

/// Projects kept in a store of their own, with projects.json beside it.
fn stored(name: &str) -> (Arc<Store>, std::path::PathBuf) {
    let dir = crate::testutil::fresh_dir(name);
    (Arc::new(Store::open(dir.join("kanban.sqlite"))), dir.join("projects.json"))
}

fn reloaded(store: &Arc<Store>, json: &std::path::Path) -> Projects {
    Projects::load(store.clone(), json)
}

#[test]
fn ac110_projects_json_is_copied_once_and_left_in_place() {
    let (store, json) = stored("projects-import");
    let older = json!({"projects": [{"id": "p1", "name": "work", "color": 2, "host": null, "dir": "/sandbox", "members": [A]}],
        "servers": {"tmux:local:terminum:": "server1"}, "order": ["g:local/main", "p:p1"]});
    std::fs::write(&json, older.to_string()).unwrap();
    let mut ps = reloaded(&store, &json);
    assert_eq!(ps.list(), [Project { id: "p1".into(), name: "work".into(), color: 2, host: None, dir: "/sandbox".into(), members: vec![A.into()] }]);
    assert_eq!(ps.list_order(), ["g:local/main", "p:p1"], "tmux groups keep their places");
    assert_eq!(ps.servers.get("tmux:local:terminum:").map(String::as_str), Some("server1"), "and the servers' identities");
    assert_eq!(std::fs::read_to_string(&json).unwrap(), older.to_string(), "left for an older terminum");
    ps.rename("p1", "api").unwrap();
    std::fs::write(&json, json!({"projects": []}).to_string()).unwrap();
    assert_eq!(reloaded(&store, &json).list()[0].name, "api", "read once: the store is the owner since");
}

#[test]
fn ac110_a_key_an_older_projects_json_lacks_reads_as_empty() {
    let (store, json) = stored("projects-partial");
    std::fs::write(&json, json!({"order": ["g:local/main"], "newer": 1}).to_string()).unwrap();
    let ps = reloaded(&store, &json);
    assert!(ps.list().is_empty() && ps.servers.is_empty());
    assert_eq!(ps.list_order(), ["g:local/main"]);
    assert!(!json.with_extension("json.bak").exists(), "nothing wrong with it: not set aside");
}

#[test]
fn ac110_an_unreadable_projects_json_is_set_aside_and_the_store_starts_empty() {
    for malformed in [json!({"projects": {}}), json!({"projects": [], "servers": []}), json!([])] {
        let (store, json) = stored("projects-schema");
        std::fs::write(&json, malformed.to_string()).unwrap();
        assert!(reloaded(&store, &json).list().is_empty());
        assert_eq!(std::fs::read_to_string(json.with_extension("json.bak")).unwrap(), malformed.to_string());
    }
}

#[test]
fn ac110_every_change_reaches_the_store() {
    let (store, json) = stored("projects-saved");
    let mut ps = reloaded(&store, &json);
    let p1 = ps.add_new(A, Some("API"), Some("devbox.example".into()), "/a".into()).unwrap();
    ps.add_to(B, &p1, None).unwrap();
    let p2 = ps.create("Billing", None, String::new()).unwrap();
    ps.order(&[format!("p:{p2}"), "g:local/main".into(), format!("p:{p1}")]).unwrap();
    ps.members(&p1, &[B.into(), A.into()]).unwrap();
    ps.seen("tmux:local:terminum:", Some("m1"), &[A, B].into_iter().collect());
    ps.save().unwrap();
    let back = reloaded(&store, &json);
    assert_eq!(back.list(), ps.list(), "names, colors, hosts, folders, members and their order");
    assert_eq!(back.list_order(), ps.list_order());
    assert_eq!(back.servers, ps.servers);
    assert_eq!(back.list()[0].name, "Billing", "in the list's order");
}

fn task_in(store: &Store, project: &str, number: i64) {
    store.with(|tx| {
        tx.execute("INSERT INTO tasks (id, number, project, title, body, status, severity, rank, owner, updated_at)
                    VALUES (?1, ?2, ?3, 't', '', 'TODO', 'P2', 0, 'human', 0)", (format!("t{number}"), number, project)).map_err(|e| e.to_string())
    }).unwrap();
}

fn project_of_task(store: &Store, number: i64) -> String {
    store.with(|tx| tx.query_row("SELECT project FROM tasks WHERE number = ?1", [number], |r| r.get(0)).map_err(|e| e.to_string())).unwrap()
}

#[test]
fn ac110_deleting_a_project_moves_its_tasks_to_default_and_says_so() {
    let (store, json) = stored("projects-delete");
    let mut ps = reloaded(&store, &json);
    let p1 = ps.add_new(A, Some("Billing"), None, "/a".into()).unwrap();
    let p2 = ps.add_new(B, None, None, "/b".into()).unwrap();
    task_in(&store, &p1, 1);
    task_in(&store, &p2, 2);
    ps.delete(&p1).unwrap();
    assert!(ps.of(A).is_none(), "its session is back in its tmux group");
    assert_eq!(project_of_task(&store, 1), "default");
    assert_eq!(project_of_task(&store, 2), p2, "another project's stays");
    let said: String = store.with(|tx| tx.query_row("SELECT text FROM events WHERE task = 't1'", [], |r| r.get(0)).map_err(|e| e.to_string())).unwrap();
    assert_eq!(said, "Project Billing was deleted: moved to Default");
    assert!(reloaded(&store, &json).list().iter().all(|p| p.id != p1), "gone from the store");
}

#[test]
fn ac110_a_project_is_not_lost_when_the_store_refuses_its_deletion() {
    let (store, json) = stored("projects-refused");
    let mut ps = reloaded(&store, &json);
    let p1 = ps.add_new(A, None, None, "/a".into()).unwrap();
    store.with(|tx| tx.execute_batch("CREATE TRIGGER no BEFORE DELETE ON projects BEGIN SELECT RAISE(ABORT, 'refused'); END;").map_err(|e| e.to_string())).unwrap();
    assert!(ps.delete(&p1).is_err());
    assert!(ps.of(A).is_some(), "nothing changed");
}

#[test]
fn ac119_projects_are_never_written_over_a_store_that_could_not_be_read() {
    use std::os::unix::fs::PermissionsExt;
    let (store, json) = stored("projects-failed");
    let mut ps = reloaded(&store, &json);
    ps.add_new(A, Some("Keep me"), None, "/a".into()).unwrap();
    ps.seen("tmux:local:terminum:", Some("m1"), &[A].into_iter().collect());
    ps.save().unwrap();
    let path = json.with_file_name("kanban.sqlite");
    drop(ps);
    drop(store);
    let mode = |m| std::fs::set_permissions(&path, std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o000);
    let store = Arc::new(Store::open(path.clone()));
    let mut failed = reloaded(&store, &json);
    assert!(failed.list().is_empty() && store.problem().is_some(), "it could not be read");
    mode(0o600);
    failed.seen("tmux:local:terminum:", Some("m1"), &HashSet::new());
    assert!(failed.save().is_err(), "the first save after it comes back writes nothing, and reads them");
    assert_eq!(failed.list()[0].name, "Keep me", "memory holds the store's now");
    assert_eq!(reloaded(&store, &json).list()[0].name, "Keep me", "and the store still does");
    failed.rename(&failed.list()[0].id.clone(), "Kept").unwrap();
    assert_eq!(reloaded(&store, &json).list()[0].name, "Kept", "changes reach it again");
}

#[test]
fn ac119_a_change_the_store_refuses_stays_out_of_memory() {
    let (store, json) = stored("projects-refused-change");
    let mut ps = reloaded(&store, &json);
    let p = ps.add_new(A, Some("One"), None, "/a".into()).unwrap();
    store.with(|tx| tx.execute_batch("CREATE TRIGGER no BEFORE UPDATE ON projects BEGIN SELECT RAISE(ABORT, 'refused'); END;").map_err(|e| e.to_string())).unwrap();
    assert!(ps.rename(&p, "Two").is_err());
    assert_eq!(ps.list()[0].name, "One");
}

#[test]
fn ac119_start_empty_does_not_bring_an_old_projects_json_back() {
    let (_, json) = stored("projects-empty");
    let path = json.with_file_name("kanban.sqlite");
    std::fs::write(&json, json!({"projects": [{"id": "p1", "name": "old", "color": 2, "host": null, "dir": "/x", "members": []}]}).to_string()).unwrap();
    std::fs::write(&path, vec![0x5a; 8192]).unwrap();
    let store = Arc::new(Store::open(path));
    store.start_empty().unwrap();
    let mut ps = reloaded(&store, &json);
    assert!(ps.list().is_empty(), "the user asked for empty");
    ps.create("Billing", None, String::new()).unwrap();
    assert_eq!(reloaded(&store, &json).list()[0].name, "Billing", "and what they make stays");
}

#[test]
fn ac110_a_project_made_with_no_session_stands_and_takes_its_folder_later() {
    let mut ps = store();
    let p = ps.create("Billing", None, String::new()).unwrap();
    assert_eq!(ps.list()[0].members.len(), 0);
    ps.set_dir(&p, "/Users/alex/billing".into()).unwrap();
    assert_eq!(ps.list()[0].dir, "/Users/alex/billing");
    assert_eq!(ps.create("Billing", None, String::new()).unwrap_err(), "A project is already named Billing");
    assert_eq!(ps.create(" default ", None, String::new()).unwrap_err(), "The name Default is taken");
    assert_eq!(ps.create("", None, String::new()).unwrap_err(), "A project's name has 1–40 characters");
    let p2 = ps.create("Web", None, String::new()).unwrap();
    assert_eq!(ps.rename(&p2, "Billing").unwrap_err(), "A project is already named Billing");
    assert!(ps.rename(&p2, "Web").is_ok(), "its own name is free");
    for n in 0..48 {
        ps.create(&format!("P{n}"), None, String::new()).unwrap();
    }
    assert_eq!(ps.create("One more", None, String::new()).unwrap_err(), "50 projects at most: delete one first");
}

#[test]
fn a_new_project_is_named_and_colored_apart_from_the_others() {
    let mut ps = store();
    let p1 = ps.add_new(A, None, None, "/Users/alex/api".into()).unwrap();
    let p2 = ps.add_new(B, None, None, "/Users/alex/web".into()).unwrap();
    let names: Vec<_> = ps.list().iter().map(|p| (p.name.clone(), p.color, p.dir.clone())).collect();
    assert_eq!(names, [("Group 1".to_string(), 0, "/Users/alex/api".to_string()), ("Group 2".into(), 1, "/Users/alex/web".into())]);
    ps.delete(&p1).unwrap();
    let p3 = ps.add_new(A, None, None, "/tmp".into()).unwrap();
    let p = ps.list().iter().find(|p| p.id == p3).unwrap();
    assert_eq!((p.name.as_str(), p.color), ("Group 1", 0), "the lowest free name and color again");
    let p4 = ps.add_new(C, Some("  API  "), None, "/c".into()).unwrap();
    assert_eq!(ps.list().iter().find(|p| p.id == p4).unwrap().name, "API", "named as it is made");
    assert!(ps.add_new(B, Some("a\u{7}"), None, "/c".into()).is_err());
    assert_ne!(p2, p3);
}

#[test]
fn ac110_a_pane_is_in_one_project_and_an_empty_one_stays() {
    let mut ps = store();
    let p1 = ps.add_new(A, None, None, "/a".into()).unwrap();
    ps.add_to(B, &p1, None).unwrap();
    let p2 = ps.add_new(B, None, None, "/b".into()).unwrap();
    assert_eq!(ps.of(B).unwrap().id, p2, "moved, not copied");
    assert_eq!(ps.of(A).unwrap().members, [A]);
    ps.remove(A).unwrap();
    let empty = ps.list().iter().find(|p| p.id == p1).unwrap();
    assert!(empty.members.is_empty(), "its last session left: the project stays");
    assert!(ps.add_to(A, "nope", None).is_err());
}

#[test]
fn order_members_rename_and_after() {
    let mut ps = store();
    let p1 = ps.add_new(A, None, None, "/a".into()).unwrap();
    ps.add_to(B, &p1, None).unwrap();
    ps.add_to(C, &p1, Some(A)).unwrap();
    assert_eq!(ps.of(A).unwrap().members, [A, C, B], "a clone goes right after its session");
    ps.members(&p1, &[B.into(), A.into()]).unwrap();
    assert_eq!(ps.of(A).unwrap().members, [B, A, C]);
    ps.rename(&p1, "  API  ").unwrap();
    assert_eq!(ps.of(A).unwrap().name, "API");
    assert!(ps.rename(&p1, " ").is_err() && ps.rename(&p1, "a\u{7}").is_err());
    let p2 = ps.add_new("tmux:local:terminum:%9", None, None, "/z".into()).unwrap();
    ps.order(&[format!("p:{p2}"), "g:local/main".to_string(), format!("p:{p1}")]).unwrap();
    assert_eq!(ps.list_order(), [format!("p:{p2}"), "g:local/main".to_string(), format!("p:{p1}")], "the projects and groups together");
    assert!(ps.order(&vec!["g:x".to_string(); ORDER_MOST + 1]).is_err() && ps.order(&["g:".to_string() + &"x".repeat(ORDER_ID)]).is_err(), "what a page sends is bounded");
    assert_eq!(ps.list().iter().map(|p| p.id.clone()).collect::<Vec<_>>(), [p2, p1]);
}

#[test]
fn closed_panes_leave_when_their_server_is_read() {
    let mut ps = store();
    let p1 = ps.add_new(A, None, None, "/a".into()).unwrap();
    ps.add_to(C, &p1, None).unwrap();
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
    assert!(ps.list()[0].members.is_empty(), "the project stays, with no session");
}

/// A tmux server started anew numbers its panes anew: its old members never name new panes.
#[test]
fn a_server_started_anew_keeps_projects_only_through_its_restore() {
    let mut ps = store();
    let p1 = ps.add_new(A, None, None, "/a".into()).unwrap();
    ps.add_to(B, &p1, None).unwrap();
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
    assert!(ps.list()[0].members.is_empty(), "another server's pane %7 is not the project's");
}

/// A restore again passes over the sessions still running: paired with themselves, they keep their projects.
#[test]
fn a_restore_again_keeps_the_running_sessions() {
    let mut ps = store();
    let p1 = ps.add_new(A, None, None, "/a".into()).unwrap();
    ps.add_to(B, &p1, None).unwrap();
    ps.restarted("tmux:local:terminum:", &[("%9".into(), "%12".into()), ("%1".into(), "%1".into()), ("%2".into(), "%2".into())]);
    assert_eq!(ps.of(A).unwrap().members, [A, B]);
}

/// AC-92: a member kept for being fresh while the list no longer shows it asks for one list
/// read when it stops being fresh, so its closed pane leaves the project without another change.
#[test]
fn a_fresh_member_the_list_lost_is_read_again_when_it_stops_being_fresh() {
    let mut ps = store();
    ps.add_new(A, None, None, "/a".into()).unwrap();
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
