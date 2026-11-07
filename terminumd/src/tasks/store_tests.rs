// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The tasks' store (AC-119): made, transactions, upgrades and what is never written over.

use super::*;
use std::os::unix::fs::PermissionsExt;

fn fresh(name: &str) -> PathBuf {
    crate::testutil::fresh_dir(name).join("kanban.sqlite")
}

fn number_of_tasks(s: &Store) -> Result<i64, String> {
    s.with(|tx| tx.query_row("SELECT count(*) FROM tasks", [], |r| r.get(0)).map_err(|e| e.to_string()))
}

#[test]
fn ac119_a_new_store_is_private_and_holds_the_user_default_and_dispatcher() {
    let path = fresh("tasks-new");
    let s = Store::open(path.clone());
    assert_eq!(s.problem(), None);
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    let row = s.with(|tx| {
        let name: String = tx.query_row("SELECT name FROM projects WHERE is_default = 1", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        let kinds: String = tx.query_row("SELECT group_concat(kind) FROM (SELECT kind FROM users ORDER BY kind)", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        Ok((name, kinds))
    }).unwrap();
    assert_eq!(row, ("Default".to_string(), "human,system".to_string()));
    let human: String = s.with(|tx| tx.query_row("SELECT name FROM users WHERE id = 'human'", [], |r| r.get(0)).map_err(|e| e.to_string())).unwrap();
    assert!(!human.is_empty(), "the one human is named");
}

#[test]
fn ac119_a_change_is_one_transaction() {
    let s = Store::open(fresh("tasks-tx"));
    let failed: Result<(), String> = s.with(|tx| {
        tx.execute("INSERT INTO meta (key, value) VALUES ('a', '1')", []).map_err(|e| e.to_string())?;
        Err("refused".into())
    });
    assert_eq!(failed, Err("refused".to_string()));
    let n: i64 = s.with(|tx| tx.query_row("SELECT count(*) FROM meta WHERE key = 'a'", [], |r| r.get(0)).map_err(|e| e.to_string())).unwrap();
    assert_eq!(n, 0, "rolled back whole");
}

#[test]
fn ac119_a_store_survives_being_opened_again() {
    let path = fresh("tasks-again");
    Store::open(path.clone()).with(|tx| tx.execute("INSERT INTO meta (key, value) VALUES ('a', '1')", []).map(|_| ()).map_err(|e| e.to_string())).unwrap();
    let v: String = Store::open(path).with(|tx| tx.query_row("SELECT value FROM meta WHERE key = 'a'", [], |r| r.get(0)).map_err(|e| e.to_string())).unwrap();
    assert_eq!(v, "1");
}

#[test]
fn ac119_an_older_schema_is_copied_then_upgraded() {
    let path = fresh("tasks-older");
    let mut conn = Connection::open(&path).unwrap();
    upgrade(&mut conn, &path, &["CREATE TABLE a (x)"]).unwrap();
    conn.execute("INSERT INTO a VALUES (7)", []).unwrap();
    upgrade(&mut conn, &path, &["CREATE TABLE a (x)", "CREATE TABLE b (y)"]).unwrap();
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 2);
    assert_eq!(conn.query_row("SELECT count(*) FROM b", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    let copy = Connection::open(path.with_file_name("kanban.sqlite.bak-1")).unwrap();
    assert_eq!(copy.query_row("SELECT x FROM a", [], |r| r.get::<_, i64>(0)).unwrap(), 7, "the copy holds what was there");
    assert!(copy.query_row("SELECT count(*) FROM b", [], |r| r.get::<_, i64>(0)).is_err(), "as it was");
}

#[test]
fn ac119_a_failed_upgrade_changes_nothing() {
    let path = fresh("tasks-badup");
    let mut conn = Connection::open(&path).unwrap();
    upgrade(&mut conn, &path, &["CREATE TABLE a (x)"]).unwrap();
    assert!(upgrade(&mut conn, &path, &["CREATE TABLE a (x)", "CREATE TABLE b (y); NOT SQL"]).is_err());
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 1);
    assert!(conn.query_row("SELECT count(*) FROM b", [], |r| r.get::<_, i64>(0)).is_err(), "rolled back");
}

#[test]
fn ac119_a_newer_store_is_said_and_never_written() {
    let path = fresh("tasks-newer");
    drop(Store::open(path.clone()));
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    let before = std::fs::read(&path).unwrap();
    let s = Store::open(path.clone());
    assert_eq!(s.problem().unwrap(), "The tasks' store is newer than this terminum: upgrade terminum.");
    assert_eq!(number_of_tasks(&s).unwrap_err(), "The tasks' store is newer than this terminum: upgrade terminum.");
    assert_eq!(std::fs::read(&path).unwrap(), before, "not written");
    assert!(path.exists(), "not set aside");
}

#[test]
fn ac119_an_unreadable_file_is_set_aside_and_empty_only_when_asked() {
    let path = fresh("tasks-broken");
    let garbage = vec![0x5a; 8192];
    std::fs::write(&path, &garbage).unwrap();
    let s = Store::open(path.clone());
    let said = s.problem().unwrap();
    assert!(said.contains("set aside as"), "{said}");
    assert!(!path.exists(), "nothing is made until the user asks");
    let aside: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    let main = aside.iter().find(|n| n.starts_with("kanban.sqlite.bad-")).unwrap();
    assert_eq!(std::fs::read(path.with_file_name(main)).unwrap(), garbage, "kept whole");
    assert!(number_of_tasks(&s).is_err());
    assert!(!path.exists());
    assert!(s.is_set_aside());
    let restarted = Store::open(path.clone());
    assert!(restarted.problem().unwrap().contains("set aside as") && !path.exists(), "a restart makes no empty one unasked");
    s.start_empty().unwrap();
    assert_eq!(number_of_tasks(&s).unwrap(), 0);
    assert_eq!(s.problem(), None);
    assert!(s.start_empty().is_err(), "only after one went aside");
}

#[test]
fn ac119_setting_aside_takes_the_wal_and_shm_too() {
    let path = fresh("tasks-aside");
    for (ext, bytes) in [("", b"main".as_slice()), ("-wal", b"wal"), ("-shm", b"shm")] {
        std::fs::write(format!("{}{ext}", path.display()), bytes).unwrap();
    }
    let aside = set_aside(&path);
    assert_eq!(std::fs::read(&aside).unwrap(), b"main");
    assert_eq!(std::fs::read(format!("{aside}-wal")).unwrap(), b"wal", "a new store never meets an old WAL");
    assert_eq!(std::fs::read(format!("{aside}-shm")).unwrap(), b"shm");
    assert!(!path.exists() && !Path::new(&format!("{}-wal", path.display())).exists());
}

#[test]
fn ac119_a_store_that_cannot_be_reached_stays_and_is_tried_again() {
    let dir = crate::testutil::fresh_dir("tasks-refused");
    let path = dir.join("kanban.sqlite");
    drop(Store::open(path.clone()));
    let readonly = |mode| std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    readonly(0o000);
    let s = Store::open(path.clone());
    readonly(0o600);
    assert!(s.problem().is_some(), "not permitted is said");
    assert!(path.exists(), "and it stays");
    assert_eq!(number_of_tasks(&s).unwrap(), 0, "the next call tries again");
    assert_eq!(s.problem(), None);
}

#[test]
fn ac119_the_quick_check_passes_a_healthy_store() {
    let s = Store::open(fresh("tasks-check"));
    s.quick_check();
    assert_eq!(s.problem(), None);
}

#[test]
fn ac109_a_purged_text_leaves_the_file() {
    let path = fresh("tasks-purge");
    let s = Store::open(path.clone());
    let secret = "sk-very-secret-value-0123456789";
    s.with(|tx| tx.execute("INSERT INTO meta (key, value) VALUES ('k', ?1)", [secret]).map(|_| ()).map_err(|e| e.to_string())).unwrap();
    s.checkpoint();
    s.with(|tx| tx.execute("UPDATE meta SET value = '' WHERE key = 'k'", []).map(|_| ()).map_err(|e| e.to_string())).unwrap();
    s.checkpoint();
    let holds = |p: &Path| std::fs::read(p).is_ok_and(|b| b.windows(secret.len()).any(|w| w == secret.as_bytes()));
    assert!(!holds(&path) && !holds(&path.with_file_name("kanban.sqlite-wal")), "neither the file nor its log holds it");
}

#[test]
fn ac119_foreign_keys_are_enforced() {
    let s = Store::open(fresh("tasks-fk"));
    let bad = s.with(|tx| {
        tx.execute("INSERT INTO tasks (id, number, project, title, body, status, severity, rank, owner, updated_at)
                    VALUES ('x', 1, 'no such project', 't', '', 'TODO', 'P2', 0, 'human', 0)", []).map(|_| ()).map_err(|e| e.to_string())
    });
    assert!(bad.is_err(), "{bad:?}");
}

#[test]
fn ac137_a_store_of_the_first_schema_gains_the_agents_and_keeps_its_tasks() {
    let path = fresh("tasks-mig2");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(include_str!("schema1.sql")).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    conn.execute("INSERT INTO tasks (id, number, project, status, severity, rank, owner, updated_at, title, body) VALUES ('T-1', 1, 'default', 'TODO', 'P2', 1.0, 'human', 1, 'kept', '')", []).unwrap();
    drop(conn);
    let s = Store::open(path.clone());
    assert_eq!(s.problem(), None);
    assert_eq!(number_of_tasks(&s).unwrap(), 1, "its tasks stay");
    assert!(s.with(crate::tasks::agents::list).unwrap().is_empty(), "the table stands");
    assert!(path.with_file_name("kanban.sqlite.bak-1").exists(), "a copy before the change");
    drop(s);
    let version: i64 = Connection::open(&path).unwrap().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, MIGRATIONS.len() as i64);
}
