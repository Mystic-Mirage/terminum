// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Projects (AC-92): named groups of sessions in the list, the same on every page, kept in
//! `projects.json`. Each has a name ("Group 1" …), a color of its own (the lowest of COLORS no
//! other project has: the page's palette, far apart), the host and folder of the session it
//! was made from (its [+] opens a window there) and its sessions by pane key, in the list's
//! order. A pane that closed leaves its project when its server's list is read (one added in
//! the last seconds is spared: a read on its way may not have it yet); a project with no
//! session left goes. A tmux server started anew numbers its panes anew: restored, its
//! sessions keep their projects (restore says which pane is which); else its panes leave
//! them, never naming other panes. A bookmark made in a project's session is the project's
//! (AC-33): orphaned once the project goes, or its session closes (bookmarks_scope.rs).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::Page;
use crate::app::Shared;

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
    path: PathBuf,
    list: Vec<Project>,
    /// each server's identity (its `@terminum-id`) by its keys' start, as its members were read
    servers: BTreeMap<String, String>,
    /// members added lately, when (in memory)
    fresh: HashMap<String, Instant>,
    /// when a list read is due for a member kept only for being fresh (`fresh_left`)
    recheck: Option<Instant>,
    /// a list read changed them and the pages are not told yet (`tell_read`)
    untold: bool,
    /// the session list's order, projects and tmux groups together ("p:<id>", "g:<group>"): every page's
    order: Vec<String>,
}

/// A project's name as typed (trimmed): a short name (settings::is_short_name).
fn name_ok(name: &str) -> Result<String, String> {
    let n = name.trim();
    if crate::settings::is_short_name(n) { Ok(n.to_string()) } else { Err("A project's name has 1–40 characters".into()) }
}

impl Projects {
    pub fn load(path: PathBuf) -> Projects {
        // one that cannot be read is set aside (projects.json.bak), never written over
        let (list, servers, order) = crate::auth::store::read_or_set_aside(&path, |v| {
            if !v.is_object() {
                return Err("expected a projects object".into());
            }
            let list = serde_json::from_value(v.get("projects").cloned().unwrap_or_else(|| json!([]))).map_err(|e| e.to_string())?;
            let servers = serde_json::from_value(v.get("servers").cloned().unwrap_or_else(|| json!({}))).map_err(|e| e.to_string())?;
            let order = serde_json::from_value(v.get("order").cloned().unwrap_or_else(|| json!([]))).map_err(|e| e.to_string())?;
            Ok((list, servers, order))
        }).unwrap_or_default();
        Projects { path, list, servers, fresh: HashMap::new(), recheck: None, untold: false, order }
    }

    pub fn list(&self) -> &[Project] {
        &self.list
    }

    /// Writes projects.json (each change does; after `seen` or `restarted`, their caller).
    pub fn save(&self) -> Result<(), String> {
        if self.path.as_os_str().is_empty() {
            return Ok(());                                  // in tests: kept in memory
        }
        crate::config::write_private(&self.path, json!({"projects": self.list, "servers": self.servers, "order": self.order}).to_string().as_bytes()).map_err(|e| format!("projects.json: {e}"))
    }

    fn find(&mut self, id: &str) -> Result<&mut Project, String> {
        self.list.iter_mut().find(|p| p.id == id).ok_or_else(|| "That project is gone.".to_string())
    }

    /// Puts pane `key` into project `to` (a new one when none: named `name`, else "Group N",
    /// made from the pane's host and folder), after `after` when that is a member, else last;
    /// out of any other. The project's id.
    pub fn add(&mut self, key: &str, to: Option<&str>, after: Option<&str>, host: Option<String>, dir: String, name: Option<&str>) -> Result<String, String> {
        let named = name.map(name_ok).transpose()?;
        let id = match to {
            Some(id) => self.find(id)?.id.clone(),
            None => {
                if self.list.len() >= MOST {
                    return Err(format!("At most {MOST} projects."));
                }
                let taken = |n: usize| self.list.iter().any(|p| p.name == format!("Group {n}"));
                let n = (1..).find(|n| !taken(*n)).unwrap_or(1);
                let color = (0..COLORS).find(|c| !self.list.iter().any(|p| p.color == *c)).unwrap_or(self.list.len() % COLORS);
                let id = crate::auth::crypto::random_hex::<6>();
                self.list.push(Project { id: id.clone(), name: named.unwrap_or_else(|| format!("Group {n}")), color, host, dir, members: Vec::new() });
                id
            }
        };
        if self.find(&id)?.members.len() >= MOST_MEMBERS {
            self.list.retain(|p| !p.members.is_empty());   // a project made just now goes again
            return Err(format!("At most {MOST_MEMBERS} sessions in a project."));
        }
        let p = self.find(&id)?;
        p.members.retain(|m| m != key);
        let at = after.and_then(|a| p.members.iter().position(|m| m == a)).map_or(p.members.len(), |i| i + 1);
        p.members.insert(at, key.to_string());
        for p in self.list.iter_mut().filter(|p| p.id != id) {
            p.members.retain(|m| m != key);
        }
        self.list.retain(|p| !p.members.is_empty());
        self.fresh.insert(key.to_string(), Instant::now());
        self.save()?;
        Ok(id)
    }

    /// Takes pane `key` out of its project (an empty project goes).
    pub fn remove(&mut self, key: &str) -> Result<(), String> {
        self.prune(|m| m != key);
        self.save()
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = name_ok(name)?;
        self.find(id)?.name = name;
        self.save()
    }

    /// The project goes; its sessions are back in their tmux groups.
    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        self.find(id)?;
        self.list.retain(|p| p.id != id);
        self.save()
    }

    /// The session list's order `ids`, projects and tmux groups together; the projects follow it
    /// (those not named keep their order, after).
    pub fn order(&mut self, ids: &[String]) -> Result<(), String> {
        if ids.len() > ORDER_MOST || ids.iter().any(|i| i.len() > ORDER_ID) {
            return Err("That order is too long".into());
        }
        let at = |p: &Project| ids.iter().position(|i| i.strip_prefix("p:") == Some(p.id.as_str())).unwrap_or(usize::MAX);
        self.list.sort_by_key(|p| at(p));
        self.order = ids.to_vec();
        self.save()
    }

    /// The session list's order as `order` got it.
    pub fn list_order(&self) -> &[String] {
        &self.order
    }

    /// Project `id`'s sessions in the order of `keys` (those not named keep their order, after).
    pub fn members(&mut self, id: &str, keys: &[String]) -> Result<(), String> {
        let p = self.find(id)?;
        let at = |m: &String| keys.iter().position(|k| k == m).unwrap_or(usize::MAX);
        p.members.sort_by_key(|m| at(m));
        self.save()
    }

    /// The panes of a server whose list was just read (`prefix` its keys' start, `identity` its
    /// `@terminum-id`): its members that are not `live` closed, but those added lately; a server
    /// started anew without a restore (another identity) leaves no member of its old panes; one
    /// not seen before (just restored) keeps them all once. True when something changed (then
    /// `save`), and which of its panes closed.
    pub fn seen(&mut self, prefix: &str, identity: Option<&str>, live: &HashSet<&str>) -> (bool, Closed) {
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

    /// Members `keep` refuses go, and projects left empty with them; how many members went.
    fn prune(&mut self, keep: impl Fn(&str) -> bool) -> usize {
        let before: usize = self.list.iter().map(|p| p.members.len()).sum();
        for p in self.list.iter_mut() {
            p.members.retain(|m| keep(m));
        }
        self.list.retain(|p| !p.members.is_empty());
        before - self.list.iter().map(|p| p.members.len()).sum::<usize>()
    }

    /// The project pane `key` is in.
    pub fn of(&self, key: &str) -> Option<&Project> {
        self.list.iter().find(|p| p.members.iter().any(|m| m == key))
    }
}

fn message(app: &Shared) -> String {
    let projects = app.projects.lock();
    json!({"t": "projects", "items": projects.list(), "order": projects.list_order()}).to_string()
}

/// A page that connects learns the projects.
pub fn hello(app: &Shared, page: &Page) {
    page.send(message(app));
}

/// Every page learns the projects (after a change).
pub fn tell(app: &Shared) {
    app.pages.tell(&message(app));
}

/// The servers' lists were read and told: the projects those reads changed are told now, after the
/// list without the closed panes (their rows do not stand in their tmux groups meanwhile). Kept until
/// told: a read cut short tells them with the next.
pub fn tell_read(app: &Shared) {
    let untold = std::mem::take(&mut app.projects.lock().untold);
    if untold {
        tell(app);
    }
}

/// A server's list was read with these pane keys, `every` its panes shown or not: those no longer
/// shown leave their projects; the bookmarks of those that closed are orphaned (a session detached
/// keeps its own, and shows them when attached again); the pages are told by `tell_read`.
pub async fn seen(app: &Shared, server: &crate::tmux::Server, keys: &[String], every: &[String]) {
    let identity = server.identity().await;
    let live: HashSet<&str> = keys.iter().map(String::as_str).collect();
    let prefix = server.key("");
    let (changed, closed, left) = {
        let mut p = app.projects.lock();
        let (changed, closed) = p.seen(&prefix, identity.as_deref(), &live);
        p.untold |= changed;
        (changed, closed, p.fresh_left(&prefix, &live))
    };
    if let Some(wait) = left {
        let dirty = app.dirty.clone();
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            dirty.notify_one();                         // the list read again: a closed fresh member goes
        });
    }
    if changed {
        save(app).await;
    }
    let live: HashSet<String> = every.iter().cloned().collect();
    orphan(app, move |k| k.starts_with(&prefix) && match closed {
        Closed::None => false,
        Closed::All => true,
        Closed::NotLive => !live.contains(k),
    }).await;
}

/// The server of `prefix` was restored (AC-15; `panes`: old id, new id, its kept panes paired
/// with themselves): its sessions keep their projects and bookmarks, before its lists are read.
pub async fn restarted(app: &Shared, prefix: &str, panes: &[(String, String)]) {
    if app.projects.lock().restarted(prefix, panes) {
        save(app).await;
        tell(app);
    }
    let (a, prefix, panes) = (app.clone(), prefix.to_string(), panes.to_vec());
    told(app, tokio::task::spawn_blocking(move || a.bookmarks.lock().restarted(&prefix, &panes)).await);
}

/// The bookmarks whose session `closed` says closed are orphaned (AC-33): saved off the async
/// workers when one is (named again there, under the lock), every
/// page told.
async fn orphan(app: &Shared, closed: impl Fn(&str) -> bool + Send + 'static) {
    if super::bookmarks::lost(app.bookmarks.lock().all(), &closed).is_empty() {
        return;
    }
    let a = app.clone();
    told(app, tokio::task::spawn_blocking(move || {
        let mut store = a.bookmarks.lock();
        let ids = super::bookmarks::lost(store.all(), &closed);
        store.orphan(&ids)
    }).await);
}

/// Every page learns of the bookmarks a change made; one not saved is logged.
fn told(app: &Shared, done: Result<Result<Vec<super::bookmarks::Item>, super::bookmarks::MarkError>, tokio::task::JoinError>) {
    match done {
        Ok(Ok(items)) => items.iter().for_each(|i| super::marks::tell_mark(app, i)),
        Ok(Err(e)) => tracing::warn!(event = "bookmarks.unsaved", error = %e),
        Err(e) => tracing::warn!(event = "bookmarks.unsaved", error = %e),
    }
}

/// projects.json written off the async workers.
async fn save(app: &Shared) {
    let a = app.clone();
    if let Ok(Err(e)) = tokio::task::spawn_blocking(move || a.projects.lock().save()).await {
        tracing::warn!(event = "projects.unsaved", error = %e);
    }
}

/// `project {action, …}` from the page's session list (AC-92).
pub async fn handle(app: &Shared, page: &Page, msg: &Value) {
    let s = |k: &str| msg[k].as_str().map(str::to_string);
    let ids = |k: &str| msg[k].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
    let action = s("action").unwrap_or_default();
    let (key, project, given) = (s("id").unwrap_or_default(), s("project"), s("name").filter(|n| !n.trim().is_empty()));
    let name = given.clone().unwrap_or_default();
    let (order, keys) = (ids("ids"), ids("keys"));
    // the pane's host and folder, for a new project's [+]
    let (host, dir) = if action == "add" { crate::page::marks::find_item(&app.layout.read(), &key).map(|it| crate::page::marks::project_of(&it)).unwrap_or_default() } else { Default::default() };
    let a = app.clone();
    let done = tokio::task::spawn_blocking(move || {
        let mut ps = a.projects.lock();
        match action.as_str() {
            "add" if crate::tmux::parse_key(&key).is_some() => ps.add(&key, project.as_deref(), None, host, dir.unwrap_or_default(), given.as_deref()).map(|_| ()),
            "remove" => ps.remove(&key),
            "rename" => ps.rename(project.as_deref().unwrap_or(""), &name),
            "delete" => ps.delete(project.as_deref().unwrap_or("")),
            "order" => ps.order(&order),
            "members" => ps.members(project.as_deref().unwrap_or(""), &keys),
            _ => Err(format!("Not a project action: {action}")),
        }
    }).await.map_err(|e| e.to_string()).and_then(|r| r);
    match done {
        Ok(()) => tell(app),
        Err(e) => super::client::error(page, e, None),
    }
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
