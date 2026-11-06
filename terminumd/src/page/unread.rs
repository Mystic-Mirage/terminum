// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Unread (AC-93): a pane whose coding agent ended a turn while no page in sight showed it
//! is unread until a page shows it, and the list marks its row. Kept in state/unread.json with
//! the identity of each tmux server the marks were made in, so a restart of terminumd (an
//! upgrade) keeps them while a tmux server started anew (a reboot, a killed tmux: its pane ids
//! start again at %0) sets its marks aside until its restore hands them to the panes it made
//! (AC-15); those of panes not made again go. A change marks the list as last sent and tells
//! the pages (no new read of the servers).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::{json, Value};

use crate::app::Shared;

const FILE: &str = "unread.json";

/// The unread panes' keys, and each server's identity (`@terminum-id`) by its keys' start.
#[derive(Default)]
pub struct Marks {
    keys: HashSet<String>,
    servers: HashMap<String, String>,
    /// the marks of a server started anew, by its keys' start and the identity they were made
    /// in, until its restore hands them on (not kept on disk: a restore follows within seconds)
    aside: HashMap<String, (String, HashSet<String>)>,
    /// the marks a restore handed on, and when: a whole list read before it shows their panes
    /// (a host's, read in the same round) keeps them
    handed: HashMap<String, std::time::Instant>,
}

/// How long a mark a restore handed on stays though no list shows its pane.
const HANDED_FOR: std::time::Duration = std::time::Duration::from_secs(30);

impl Marks {
    /// The marks a terminumd before this one kept; none when there is no file (one that cannot
    /// be read is set aside).
    pub fn load(dir: &Path) -> Marks {
        crate::auth::store::read_or_set_aside(&dir.join(FILE), |v| {
            let keys = serde_json::from_value(v["unread"].clone()).map_err(|e| e.to_string())?;
            let servers = serde_json::from_value(v["servers"].clone()).map_err(|e| e.to_string())?;
            Ok(Marks { keys, servers, ..Marks::default() })
        }).unwrap_or_default()
    }

    /// A server's list was read (`prefix` its keys' start): a server with another identity than
    /// the marks were made in started anew, and its marks are set aside for its restore. True
    /// when anything changed.
    fn seen(&mut self, prefix: &str, identity: &str) -> bool {
        let was = self.servers.insert(prefix.to_string(), identity.to_string());
        if was.as_deref() == Some(identity) {
            return false;
        }
        if let Some(was) = was {
            let (gone, kept) = std::mem::take(&mut self.keys).into_iter().partition(|k| k.starts_with(prefix));
            self.keys = kept;
            // started anew twice before a restore (it failed): the first marks wait still
            if !gone.is_empty() || !self.aside.contains_key(prefix) {
                self.aside.insert(prefix.to_string(), (was, gone));
            }
        }
        true
    }

    /// A restore made the panes `pairs` (old id -> new) again on the server `prefix` names,
    /// from its checkpoint of identity `old`, now `new`: their marks go to the new panes. The
    /// marks are where they are: still current (the list not read since) or set aside.
    fn restarted(&mut self, prefix: &str, old: &str, new: &str, pairs: &[(String, String)]) -> bool {
        let mut from = HashSet::new();
        if self.servers.get(prefix).map(String::as_str) == Some(old) {
            let (mine, kept) = std::mem::take(&mut self.keys).into_iter().partition(|k: &String| k.starts_with(prefix));
            self.keys = kept;
            from = mine;
        } else if let Some((was, keys)) = self.aside.remove(prefix) && was == old {
            from = keys;
        }
        self.servers.insert(prefix.to_string(), new.to_string());
        let before = self.keys.len();
        let now = std::time::Instant::now();
        for (_, n) in pairs.iter().filter(|(o, _)| from.contains(&format!("{prefix}{o}"))) {
            self.keys.insert(format!("{prefix}{n}"));
            self.handed.insert(format!("{prefix}{n}"), now);
        }
        self.keys.len() != before || !from.is_empty()
    }

    fn json(&self) -> Value {
        let mut keys: Vec<&String> = self.keys.iter().collect();
        keys.sort();
        json!({"unread": keys, "servers": self.servers})
    }
}

/// One write of unread.json at a time.
static WRITING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// unread.json with the marks as they are when it is written.
fn write(app: &Shared) {
    let _one = WRITING.lock();
    let v = app.unread.lock().json();
    if let Err(e) = crate::auth::store::write_private(&app.cfg.dir.join(FILE), &v) {
        tracing::warn!(event = "unread.unsaved", error = %e);
    }
}

/// unread.json written off the async workers, one write at a time: the last one holds the last
/// change (and `flush` the marks a stop came with).
fn save(app: &Shared) {
    if app.cfg.agent {
        return;
    }
    let a = app.clone();
    match tokio::runtime::Handle::try_current() {
        Ok(rt) => drop(rt.spawn_blocking(move || write(&a))),
        Err(_) => write(&a),
    }
}

/// Before terminumd stops: the marks as they are now on disk, whether or not a write
/// `save` started has run yet (the process ends before it would).
pub fn flush(app: &Shared) {
    if !app.cfg.agent {
        write(app);
    }
}

/// A server's list was read: its marks stay only while it is the server they were made in.
pub async fn seen(app: &Shared, server: &crate::tmux::Server) {
    let Some(identity) = server.identity().await else { return };
    if app.unread.lock().seen(&server.key(""), &identity) {
        save(app);
    }
}

/// A restore made panes again (AC-15): the marks of the old panes go to the new ones.
pub fn restarted(app: &Shared, prefix: &str, old: Option<&str>, new: Option<&str>, pairs: &[(String, String)]) {
    let (Some(old), Some(new)) = (old.filter(|i| !i.is_empty()), new.filter(|i| !i.is_empty())) else { return };
    if old != new && app.unread.lock().restarted(prefix, old, new, pairs) {
        save(app);
        relist(app);
    }
}

/// The panes whose agent ended a turn: those no page in sight shows are unread now.
pub fn ended(app: &Shared, keys: &[String]) {
    let looking: Vec<String> = app.pages.all().iter().filter_map(|p| {
        let st = p.state.lock();
        st.shown.as_ref().filter(|_| st.active()).map(|s| s.key.clone())
    }).collect();
    let mut unread = app.unread.lock();
    let more: Vec<&String> = keys.iter().filter(|k| !looking.contains(k) && unread.keys.insert((*k).clone())).collect();
    drop(unread);
    if !more.is_empty() {
        save(app);
        relist(app);
    }
}

/// The list as last sent takes the marks as they are now, and every page gets it.
fn relist(app: &Shared) {
    let told = {
        let mut layout = app.layout.write();
        if let Some(groups) = layout.as_array_mut() {
            mark(app, groups, false);
        }
        json!({"t": "layout", "groups": *layout}).to_string()
    };
    app.pages.tell(&told);
}

/// The user's Mark as Unread (`on`) or Mark as Read in a row's menu, for a pane the list has.
pub fn set(app: &Shared, key: &str, on: bool) {
    if app.tmux.resolve(key).is_none() {
        return;
    }
    let changed = if on { app.unread.lock().keys.insert(key.to_string()) } else { app.unread.lock().keys.remove(key) };
    if changed {
        save(app);
        relist(app);
    }
}

/// A page in sight shows pane `key`: it is read.
pub fn read(app: &Shared, key: &str) {
    if app.unread.lock().keys.remove(key) {
        save(app);
        relist(app);
    }
}

/// The list's rows of unread panes say so, but not while busy (working, waiting, its subagents
/// running: its work goes on, and a list sent before a hook's Stop may still say so). When the
/// list is whole, panes gone are forgotten.
pub fn mark(app: &Shared, groups: &mut [Value], whole: bool) {
    let mut unread = app.unread.lock();
    let mut listed = HashSet::new();
    for it in groups.iter_mut().flat_map(|g| g["items"].as_array_mut().into_iter().flatten()) {
        let Some(id) = it["id"].as_str() else { continue };
        let on = unread.keys.contains(id) && !crate::page::turns::busy(it);
        listed.insert(id.to_string());
        match it.as_object_mut() {
            Some(o) if on => { o.insert("unread".into(), json!(true)); }
            Some(o) => { o.remove("unread"); }
            None => {}
        }
    }
    let before = unread.keys.len();
    if whole {
        let Marks { keys, handed, .. } = &mut *unread;
        handed.retain(|_, at| at.elapsed() < HANDED_FOR);
        keys.retain(|k| listed.contains(k) || handed.contains_key(k));
    }
    if unread.keys.len() != before {
        drop(unread);
        save(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "tmux:local:terminum:";

    #[test]
    fn marks_come_back_from_the_file_and_a_broken_one_is_set_aside() {
        let dir = crate::testutil::temp_root().join(format!("tm-unread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(Marks::load(&dir).keys.is_empty(), "no file: none");
        let mut m = Marks::default();
        m.keys.insert(format!("{A}%3"));
        m.servers.insert(A.into(), "id1".into());
        crate::auth::store::write_private(&dir.as_path().join(FILE), &m.json()).unwrap();
        let back = Marks::load(dir.as_path());
        assert_eq!(back.keys, m.keys);
        assert_eq!(back.servers, m.servers);
        std::fs::write(dir.as_path().join(FILE), "{not json").unwrap();
        assert!(Marks::load(dir.as_path()).keys.is_empty(), "broken: starts empty");
        assert!(dir.join("unread.json.bak").is_file(), "and the broken file is kept aside");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_server_started_anew_takes_its_marks_the_same_one_keeps_them() {
        let mut m = Marks::default();
        m.keys.extend([format!("{A}%3"), "tmux:devbox.example:default:%3".to_string()]);
        assert!(m.seen(A, "id1"), "first sight: its identity is kept");
        assert_eq!(m.keys.len(), 2, "and nothing goes");
        assert!(!m.seen(A, "id1"), "the same server (terminumd restarted): nothing changes");
        assert_eq!(m.keys.len(), 2);
        assert!(m.seen(A, "id2"), "started anew: %3 is another pane now");
        assert_eq!(m.keys.iter().collect::<Vec<_>>(), vec!["tmux:devbox.example:default:%3"], "only that server's marks go");
    }

    fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
        p.iter().map(|(o, n)| (o.to_string(), n.to_string())).collect()
    }

    #[test]
    fn a_restore_hands_the_marks_to_the_panes_made_again_whether_the_list_was_read_or_not() {
        // the list read after the restart first: the marks wait aside
        let mut m = Marks::default();
        m.keys.extend([format!("{A}%3"), format!("{A}%7"), "tmux:devbox.example:default:%3".to_string()]);
        m.seen(A, "id1");
        m.seen(A, "id2");
        assert!(m.restarted(A, "id1", "id2", &pairs(&[("%3", "%0"), ("%5", "%1")])));
        let mut keys: Vec<&String> = m.keys.iter().collect();
        keys.sort();
        assert_eq!(keys, vec!["tmux:devbox.example:default:%3", "tmux:local:terminum:%0"], "%3's mark on %0; %7 not made again goes");
        assert!(!m.seen(A, "id2"), "the new identity is the marks' now");
        // the restore first: the marks are still current
        let mut m = Marks::default();
        m.keys.extend([format!("{A}%0"), format!("{A}%1")]);
        m.seen(A, "id1");
        m.restarted(A, "id1", "id2", &pairs(&[("%1", "%0"), ("%0", "%1")]));
        let mut keys: Vec<&String> = m.keys.iter().collect();
        keys.sort();
        assert_eq!(keys, vec![&format!("{A}%0"), &format!("{A}%1")], "ids that swap keep each mark on its own pane");
        assert!(!m.seen(A, "id2"), "and the list read later keeps them");
        assert_eq!(m.keys.len(), 2);
        // a checkpoint of another identity hands on nothing
        let mut m = Marks::default();
        m.keys.insert(format!("{A}%3"));
        m.seen(A, "id1");
        m.seen(A, "id2");
        m.restarted(A, "id0", "id2", &pairs(&[("%3", "%0")]));
        assert!(m.keys.is_empty(), "the marks were made in another server than the checkpoint's");
        // started anew twice before its restore: the first server's marks still wait
        let mut m = Marks::default();
        m.keys.insert(format!("{A}%3"));
        m.seen(A, "id1");
        m.seen(A, "id2");
        m.seen(A, "id3");
        m.restarted(A, "id1", "id3", &pairs(&[("%3", "%0")]));
        assert_eq!(m.keys.iter().collect::<Vec<_>>(), vec![&format!("{A}%0")]);
    }
}
