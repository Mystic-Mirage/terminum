// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! macOS's protected folders, asked for while the user is at the Mac (AC-08): terminumd opens
//! each one itself, so macOS asks for terminumd (the program that starts terminum's tmux, by
//! which it judges every program in its terminals) instead of when an agent first reads one,
//! maybe while the user is away. Only opened: nothing is listed, read or sent. An open macOS
//! has not decided waits for the answer on the Mac's screen, so the opens run on a thread of
//! their own, never joined: a prompt nobody answers holds neither terminumd's exit nor the
//! blocking pool. One run at a time; a request during one joins it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

/// The protected folders under `home` that exist (and those that do not, said as absent):
/// Desktop, Documents, Downloads and each cloud folder (a File Provider domain each: Dropbox,
/// Google Drive, OneDrive…). Not iCloud Drive (~/Library/Mobile Documents): macOS's sandbox
/// refuses it to every program without Full Disk Access and never asks (USAGE). None but on
/// macOS.
pub fn protected(home: &Path) -> Vec<(String, PathBuf)> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let mut out: Vec<(String, PathBuf)> = ["Desktop", "Documents", "Downloads"].iter().map(|n| (n.to_string(), home.join(n))).collect();
    let mut clouds: Vec<(String, PathBuf)> = std::fs::read_dir(home.join("Library/CloudStorage")).into_iter().flatten().flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, _)| !n.starts_with('.'))
        .collect();
    clouds.sort();
    out.extend(clouds);
    out
}

/// What opening `path` says: allowed, refused (macOS: EPERM), absent, or an error.
pub fn open(path: &Path) -> (&'static str, Option<String>) {
    match std::fs::read_dir(path) {
        Ok(_) => ("allowed", None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ("absent", None),
        Err(e) if e.raw_os_error() == Some(libc::EPERM) => ("refused", None),
        Err(e) => ("error", Some(e.to_string())),
    }
}

struct Run {
    rows: Mutex<Vec<Value>>,
    done: AtomicBool,
    changed: tokio::sync::Notify,
}

static RUNNING: Mutex<Option<Arc<Run>>> = Mutex::new(None);

/// Opens `folders` one after the other on a thread of its own, each row's state as it goes
/// (pending, asking while its open waits, then what it said).
fn start(folders: Vec<(String, PathBuf)>) -> Arc<Run> {
    let rows = folders.iter().map(|(n, p)| json!({"name": n, "path": p, "state": "pending"})).collect();
    let run = Arc::new(Run { rows: Mutex::new(rows), done: AtomicBool::new(false), changed: tokio::sync::Notify::new() });
    let r = run.clone();
    std::thread::spawn(move || {
        for (i, (_, path)) in folders.iter().enumerate() {
            r.rows.lock()[i]["state"] = json!("asking");
            let (state, error) = open(path);
            let mut rows = r.rows.lock();
            rows[i]["state"] = json!(state);
            if let Some(e) = error {
                rows[i]["error"] = json!(e);
            }
            drop(rows);
            r.changed.notify_waiters();
        }
        r.done.store(true, Ordering::Release);
        r.changed.notify_waiters();
    });
    run
}

/// The folders as a run says them within `wait`: a new run, or the one still going.
pub async fn ask(home: &Path, wait: Duration) -> Value {
    let run = {
        let mut running = RUNNING.lock();
        match running.as_ref().filter(|r| !r.done.load(Ordering::Acquire)) {
            Some(r) => r.clone(),
            None => running.insert(start(protected(home))).clone(),
        }
    };
    let _ = tokio::time::timeout(wait, async {
        loop {
            let changed = run.changed.notified();
            if run.done.load(Ordering::Acquire) {
                return;
            }
            changed.await;
        }
    }).await;
    json!({"done": run.done.load(Ordering::Acquire), "folders": *run.rows.lock()})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(tag: &str) -> PathBuf {
        let h = crate::testutil::temp_root().join(format!("tm-folders-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&h);
        std::fs::create_dir_all(&h).unwrap();
        h
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_three_home_folders_and_each_cloud_folder_not_icloud() {
        let h = home("list");
        assert_eq!(protected(&h).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Desktop", "Documents", "Downloads"], "absent ones are said too");
        std::fs::create_dir_all(h.join("Library/Mobile Documents/com~apple~CloudDocs")).unwrap();
        for d in ["OneDrive-Personal", "Dropbox", ".hidden"] {
            std::fs::create_dir_all(h.join("Library/CloudStorage").join(d)).unwrap();
        }
        std::fs::write(h.join("Library/CloudStorage/a-file"), "").unwrap();
        let names: Vec<String> = protected(&h).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["Desktop", "Documents", "Downloads", "Dropbox", "OneDrive-Personal"], "iCloud Drive is never asked for");
        std::fs::remove_dir_all(&h).unwrap();
    }

    #[test]
    fn an_open_says_allowed_absent_or_why_not() {
        use std::os::unix::fs::PermissionsExt;
        let h = home("open");
        let shut = h.join("shut");
        std::fs::create_dir(&shut).unwrap();
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(open(&h), ("allowed", None));
        assert_eq!(open(&h.join("none")), ("absent", None));
        let (state, why) = open(&shut);
        assert_eq!(state, "error", "a folder the file modes shut is not macOS refusing");
        assert!(why.is_some_and(|w| w.contains("ermission denied")));
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir_all(&h).unwrap();
    }

    #[tokio::test]
    async fn a_run_answers_each_folder_and_a_request_during_one_joins_it() {
        let h = home("run");
        std::fs::create_dir(h.join("Desktop")).unwrap();
        let v = ask(&h, Duration::from_secs(5)).await;
        assert_eq!(v["done"], true);
        if cfg!(target_os = "macos") {
            let states: Vec<&str> = v["folders"].as_array().unwrap().iter().map(|r| r["state"].as_str().unwrap()).collect();
            assert_eq!(states, ["allowed", "absent", "absent"]);
        }
        // one still waiting on the Mac's screen: a request joins it and says where it is
        let waiting = Arc::new(Run { rows: Mutex::new(vec![json!({"name": "Dropbox", "state": "asking"})]), done: AtomicBool::new(false), changed: tokio::sync::Notify::new() });
        *RUNNING.lock() = Some(waiting.clone());
        let v = ask(&h, Duration::from_millis(50)).await;
        assert_eq!(v, json!({"done": false, "folders": [{"name": "Dropbox", "state": "asking"}]}));
        waiting.done.store(true, Ordering::Release);
        std::fs::remove_dir_all(&h).unwrap();
    }
}
