// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Projects (AC-92, AC-110): named groups of sessions in the list, the same on every page, kept
//! in the tasks' store (tasks/projects.rs). Each has a name ("Group 1" …), a color of its own (the lowest of COLORS no
//! other project has: the page's palette, far apart), the host and folder of the session it
//! was made from (its [+] opens a window there) and its sessions by pane key, in the list's
//! order. A pane that closed leaves its project when its server's list is read (one added in
//! the last seconds is spared: a read on its way may not have it yet); a project with no
//! session left stays. A tmux server started anew numbers its panes anew: restored, its
//! sessions keep their projects (restore says which pane is which); else its panes leave
//! them, never naming other panes. A bookmark made in a project's session is the project's
//! (AC-33): orphaned once the project is deleted, or its session closes (bookmarks_scope.rs).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::tasks::projects::{self as stored, Saved};
use crate::tasks::store::Store;

/// The page's palette of project colors.
pub const COLORS: usize = 10;
/// Projects at most; sessions at most in one.
const MOST: usize = 50;
const MOST_MEMBERS: usize = 200;
/// The most parts an order names, and an id's most bytes: terminumd keeps what a page sends.
const ORDER_MOST: usize = 1000;
const ORDER_ID: usize = 400;

/// A session added this recently is not taken for closed by a list read.
const FRESH: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub color: usize,
    pub host: Option<String>,
    pub dir: String,
    pub members: Vec<String>,
}

/// Which panes of a server whose list was read closed: none (just restored: adopted as they
/// are), all (started anew: its pane ids name other panes now), those not in the list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Closed {
    None,
    All,
    NotLive,
}

#[derive(Default)]
pub struct Projects {
    /// where they are kept; in tests of the rules alone, none
    store: Option<Arc<Store>>,
    /// projects.json, read once by the first load that succeeds
    legacy: PathBuf,
    /// the store's were read: until then nothing here may be written over them
    loaded: bool,
    list: Vec<Project>,
    /// each server's identity (its `@terminum-id`) by its keys' start, as its members were read
    servers: BTreeMap<String, String>,
    /// members added lately, when (in memory)
    fresh: HashMap<String, Instant>,
    /// when a list read is due for a member kept only for being fresh (`fresh_left`)
    recheck: Option<Instant>,
    /// a list read changed them and the pages are not told yet (`tell_read`): beside the lock, which
    /// writers hold over the store, each publish reads it
    pub untold: Arc<AtomicBool>,
    /// the session list's order, projects and tmux groups together ("p:<id>", "g:<group>"): every page's
    order: Vec<String>,
}

/// A project's name as typed (trimmed): a short name (settings::is_short_name), and not Default's.
fn name_ok(name: &str) -> Result<String, String> {
    let n = name.trim();
    if !crate::settings::is_short_name(n) {
        return Err("A project's name has 1–40 characters".into());
    }
    if n.eq_ignore_ascii_case("default") {
        return Err("The name Default is taken".into());
    }
    Ok(n.to_string())
}

impl Projects {
    /// The store's; a store that cannot be used leaves none, its message logged and on the board.
    pub fn load(store: Arc<Store>, projects_json: &Path) -> Projects {
        let mut ps = Projects { store: Some(store), legacy: projects_json.to_path_buf(), ..Projects::default() };
        if let Err(e) = ps.reload() {
            tracing::warn!(event = "projects.unread", error = %e);
        }
        ps
    }

    /// Reads them from the store, over what memory holds.
    pub fn reload(&mut self) -> Result<(), String> {
        let Some(store) = &self.store else { return Ok(()) };
        let saved = stored::load(store, &self.legacy)?;
        (self.list, self.servers, self.order) = (saved.list, saved.servers, saved.order);
        self.loaded = true;
        self.untold.store(true, Ordering::Release);
        Ok(())
    }

    /// Before any change or check of them: a store that could not be read before is read now, so
    /// nothing applies to the empty memory of the failed read, which a write would make the store's.
    pub fn ready(&mut self) -> Result<(), String> {
        if self.unread() { self.reload() } else { Ok(()) }
    }

    fn unread(&self) -> bool {
        self.store.is_some() && !self.loaded
    }

    /// `change` holds only if the store took it; refused, nothing changed (AC-119).
    fn kept<T>(&mut self, change: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        self.ready()?;
        let before = self.saved();
        let done = change(self).and_then(|v| self.save().map(|()| v));
        if done.is_err() {
            (self.list, self.servers, self.order) = (before.list, before.servers, before.order);
        }
        done
    }

    pub fn list(&self) -> &[Project] {
        &self.list
    }

    fn saved(&self) -> Saved {
        Saved { list: self.list.clone(), servers: self.servers.clone(), order: self.order.clone() }
    }

    /// Writes them to the store (each change does; after `seen` or `restarted`, their caller).
    pub fn save(&mut self) -> Result<(), String> {
        let Some(store) = self.store.clone() else { return Ok(()) };
        if !self.loaded {
            return Err("The tasks' store could not be read: try again.".into());   // never written over it
        }
        store.with(|tx| stored::write(tx, &self.saved()))
    }

    /// A name no other project has (AC-110).
    fn name_free(&self, name: &str, but: Option<&str>) -> Result<(), String> {
        if self.list.iter().any(|p| p.name == name && Some(p.id.as_str()) != but) {
            return Err(format!("A project is already named {name}"));
        }
        Ok(())
    }

    pub fn by_id(&self, id: &str) -> Option<&Project> {
        self.list.iter().find(|p| p.id == id)
    }

    fn find(&mut self, id: &str) -> Result<&mut Project, String> {
        self.list.iter_mut().find(|p| p.id == id).ok_or_else(|| "That project is gone.".to_string())
    }

    /// Puts pane `key` into project `to`, after `after` when that is a member, else last; out of
    /// any other.
    pub fn add_to(&mut self, key: &str, to: &str, after: Option<&str>) -> Result<(), String> {
        self.kept(|ps| ps.place(key, to, after))?;
        self.fresh.insert(key.to_string(), Instant::now());
        Ok(())
    }

    /// Puts pane `key` into a new project, named `name`, else "Group N", made from the pane's host
    /// and folder; out of any other. The project's id.
    pub fn add_new(&mut self, key: &str, name: Option<&str>, host: Option<String>, dir: String) -> Result<String, String> {
        let id = self.kept(|ps| {
            let id = ps.make(name, host, dir)?;
            ps.place(key, &id, None)?;
            Ok(id)
        })?;
        self.fresh.insert(key.to_string(), Instant::now());
        Ok(id)
    }

    /// Pane `key` in project `id` alone; the caller saves.
    fn place(&mut self, key: &str, id: &str, after: Option<&str>) -> Result<(), String> {
        let p = self.find(id)?;
        if p.members.len() >= MOST_MEMBERS {
            return Err(format!("At most {MOST_MEMBERS} sessions in a project."));
        }
        p.members.retain(|m| m != key);
        let at = after.and_then(|a| p.members.iter().position(|m| m == a)).map_or(p.members.len(), |i| i + 1);
        p.members.insert(at, key.to_string());
        for p in self.list.iter_mut().filter(|p| p.id != id) {
            p.members.retain(|m| m != key);
        }
        Ok(())
    }

    /// A new project with no session (AC-110): named `name`, else "Group N"; the first color no
    /// other has. Its id; the caller saves.
    fn make(&mut self, name: Option<&str>, host: Option<String>, dir: String) -> Result<String, String> {
        let named = name.map(name_ok).transpose()?;
        if let Some(n) = &named {
            self.name_free(n, None)?;
        }
        if self.list.len() >= MOST {
            return Err(format!("{MOST} projects at most: delete one first"));
        }
        let taken = |n: usize| self.list.iter().any(|p| p.name == format!("Group {n}"));
        let n = (1..).find(|n| !taken(*n)).unwrap_or(1);
        let color = (0..COLORS).find(|c| !self.list.iter().any(|p| p.color == *c)).unwrap_or(self.list.len() % COLORS);
        let id = crate::auth::crypto::random_hex::<6>();
        self.list.push(Project { id: id.clone(), name: named.unwrap_or_else(|| format!("Group {n}")), color, host, dir, members: Vec::new() });
        Ok(id)
    }

    /// A project made on the board, with no session (AC-110). Its id.
    pub fn create(&mut self, name: &str, host: Option<String>, dir: String) -> Result<String, String> {
        crate::tasks::projects::check_folder(&dir)?;
        self.kept(|ps| ps.make(Some(name), host, dir))
    }

    /// A project's window (terminum-kanban.md AC-139): its name and folder together; what is not said stays.
    pub fn edit(&mut self, id: &str, name: Option<&str>, dir: Option<String>) -> Result<(), String> {
        let name = name.map(name_ok).transpose()?;
        self.ready()?;                                   // the names to compare with are the store's
        if let Some(name) = &name {
            self.name_free(name, Some(id))?;
        }
        if let Some(dir) = &dir {
            crate::tasks::projects::check_folder(dir)?;
        }
        self.kept(|ps| {
            let p = ps.find(id)?;
            if let Some(name) = name {
                p.name = name;
            }
            if let Some(dir) = dir {
                p.dir = dir;
            }
            Ok(())
        })
    }

    /// The folder a project asks for at its first [+] (AC-110).
    pub fn set_dir(&mut self, id: &str, dir: String) -> Result<(), String> {
        crate::tasks::projects::check_folder(&dir)?;
        self.kept(|ps| {
            ps.find(id)?.dir = dir;
            Ok(())
        })
    }

    /// Takes pane `key` out of its project, which stays.
    pub fn remove(&mut self, key: &str) -> Result<(), String> {
        self.kept(|ps| {
            ps.prune(|m| m != key);
            Ok(())
        })
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), String> {
        self.edit(id, Some(name), None)
    }

    /// The project goes in one transaction: its sessions are back in their tmux groups, its tasks
    /// in Default (AC-110). Refused by the store, nothing changes.
    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        self.ready()?;
        let name = self.find(id)?.name.clone();
        let mut after = self.saved();
        after.list.retain(|p| p.id != id);
        if let Some(store) = &self.store {
            store.with(|tx| {
                stored::move_tasks_to_default(tx, id, &name, crate::tasks::ops::now())?;
                stored::write(tx, &after)
            })?;
        }
        self.list = after.list;
        Ok(())
    }

    /// The session list's order `ids`, projects and tmux groups together; the projects follow it
    /// (those not named keep their order, after).
    pub fn order(&mut self, ids: &[String]) -> Result<(), String> {
        if ids.len() > ORDER_MOST || ids.iter().any(|i| i.len() > ORDER_ID) {
            return Err("That order is too long".into());
        }
        let at = |p: &Project| ids.iter().position(|i| i.strip_prefix("p:") == Some(p.id.as_str())).unwrap_or(usize::MAX);
        self.kept(|ps| {
            ps.list.sort_by_key(|p| at(p));
            ps.order = ids.to_vec();
            Ok(())
        })
    }

    /// The session list's order as `order` got it.
    pub fn list_order(&self) -> &[String] {
        &self.order
    }

    /// Project `id`'s sessions in the order of `keys` (those not named keep their order, after).
    pub fn members(&mut self, id: &str, keys: &[String]) -> Result<(), String> {
        let at = |m: &String| keys.iter().position(|k| k == m).unwrap_or(usize::MAX);
        self.kept(|ps| {
            ps.find(id)?.members.sort_by_key(|m| at(m));
            Ok(())
        })
    }

    /// The panes of a server whose list was just read (`prefix` its keys' start, `identity` its
    /// `@terminum-id`): its members that are not `live` closed, but those added lately; a server
    /// started anew without a restore (another identity) leaves no member of its old panes; one
    /// not seen before (just restored) keeps them all once. True when something changed (then
    /// `save`), and which of its panes closed.
    pub fn seen(&mut self, prefix: &str, identity: Option<&str>, live: &HashSet<&str>) -> (bool, Closed) {
        if self.unread() {
            return (false, Closed::None);                // never judged against the failed read's empty memory
        }
        self.fresh.retain(|_, at| at.elapsed() < FRESH);
        let was = identity.and_then(|now| self.servers.insert(prefix.to_string(), now.to_string()));
        let adopted = identity.is_some() && was.is_none();
        let anew = matches!((&was, identity), (Some(old), Some(now)) if old != now);
        let fresh: HashSet<String> = self.fresh.keys().cloned().collect();
        let gone = if adopted { 0 } else { self.prune(|m| !m.starts_with(prefix) || (!anew && (live.contains(m) || fresh.contains(m)))) };
        (adopted || anew || gone > 0, if adopted { Closed::None } else if anew { Closed::All } else { Closed::NotLive })
    }

    /// How long until a member of `prefix` the list does not show stops being fresh, when one
    /// was kept for that alone (its pane may have closed at once): the list is read again then,
    /// so it leaves its project without waiting for another change (AC-92). None when one such
    /// read is due already.
    pub fn fresh_left(&mut self, prefix: &str, live: &HashSet<&str>) -> Option<Duration> {
        let due = self.fresh.iter().filter(|(k, _)| k.starts_with(prefix) && !live.contains(k.as_str())).map(|(_, at)| *at + FRESH).min()?;
        let now = Instant::now();
        if self.recheck.is_some_and(|r| r > now && r <= due) {
            return None;
        }
        self.recheck = Some(due);
        Some(due.saturating_duration_since(now) + Duration::from_millis(50))
    }

    /// The server of `prefix` was made again from its checkpoint (AC-15): its members take the
    /// new panes' keys (`panes`: old id, new id), the others go, and its next read adopts it.
    pub fn restarted(&mut self, prefix: &str, panes: &[(String, String)]) -> bool {
        if self.unread() {
            return false;                                // `ready` failed: nothing to map
        }
        let new_key = |m: &str| m.strip_prefix(prefix).and_then(|old| panes.iter().find(|(o, _)| o == old)).map(|(_, n)| format!("{prefix}{n}"));
        let mut changed = self.servers.remove(prefix).is_some();
        for p in self.list.iter_mut() {
            for m in p.members.iter_mut().filter(|m| m.starts_with(prefix)) {
                let new = new_key(m).unwrap_or_default();     // not made again: "" goes below
                changed |= *m != new;
                *m = new;
            }
        }
        self.prune(|m| !m.is_empty());
        changed
    }

    /// Members `keep` refuses go, their projects staying; how many went.
    fn prune(&mut self, keep: impl Fn(&str) -> bool) -> usize {
        let before: usize = self.list.iter().map(|p| p.members.len()).sum();
        for p in self.list.iter_mut() {
            p.members.retain(|m| keep(m));
        }
        before - self.list.iter().map(|p| p.members.len()).sum::<usize>()
    }

    /// The project pane `key` is in.
    pub fn of(&self, key: &str) -> Option<&Project> {
        self.list.iter().find(|p| p.members.iter().any(|m| m == key))
    }
}


#[path = "projects_app.rs"]
mod app;
pub use app::{handle, hello, restarted, seen, tell, tell_read};

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
