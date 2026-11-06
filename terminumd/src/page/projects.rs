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
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::Page;
use crate::app::Shared;
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
    /// open tasks by project, for the dialog of Delete Project
    tasks: HashMap<String, i64>,
    /// open tasks by session, for their rows' marks (AC-113)
    marks: BTreeMap<String, Vec<String>>,
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
        let saved = stored::load(&store, projects_json).unwrap_or_else(|e| {
            tracing::warn!(event = "projects.unread", error = %e);
            Saved::default()
        });
        let mut ps = Projects { store: Some(store), list: saved.list, servers: saved.servers, fresh: HashMap::new(), recheck: None, untold: false, order: saved.order, tasks: HashMap::new(), marks: BTreeMap::new() };
        ps.refresh_counts();
        ps
    }

    /// Reads the tasks by project again: whether they changed. Off the async workers.
    pub fn refresh_counts(&mut self) -> bool {
        let Some(store) = &self.store else { return false };
        use crate::tasks::ops::sql;
        let (counts, marks) = store.with(|tx| {
            let mut st = tx.prepare("SELECT project, count(*) FROM tasks WHERE archived_at IS NULL GROUP BY project").map_err(sql)?;
            let counts = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(sql)?.collect::<Result<HashMap<_, _>, _>>().map_err(sql)?;
            let mut st = tx.prepare("SELECT session_key, 'T-' || number FROM tasks WHERE session_key IS NOT NULL AND archived_at IS NULL AND status <> 'DONE' ORDER BY number").map_err(sql)?;
            let mut marks: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(sql)? {
                let (key, id) = row.map_err(sql)?;
                marks.entry(key).or_default().push(id);
            }
            Ok((counts, marks))
        }).unwrap_or_default();
        let changed = counts != self.tasks || marks != self.marks;
        self.tasks = counts;
        self.marks = marks;
        changed
    }

    /// The projects as a page gets them.
    fn told(&self) -> Vec<Value> {
        self.list.iter().map(|p| {
            let mut v = serde_json::to_value(p).unwrap_or_default();
            v["tasks"] = json!(self.tasks.get(&p.id).copied().unwrap_or(0));
            v
        }).collect()
    }

    pub fn list(&self) -> &[Project] {
        &self.list
    }

    fn saved(&self) -> Saved {
        Saved { list: self.list.clone(), servers: self.servers.clone(), order: self.order.clone() }
    }

    /// Writes them to the store (each change does; after `seen` or `restarted`, their caller).
    pub fn save(&self) -> Result<(), String> {
        match &self.store {
            Some(store) => store.with(|tx| stored::write(tx, &self.saved())),
            None => Ok(()),
        }
    }

    /// A name no other project has (AC-110).
    fn name_free(&self, name: &str, but: Option<&str>) -> Result<(), String> {
        if self.list.iter().any(|p| p.name == name && Some(p.id.as_str()) != but) {
            return Err(format!("A project is already named {name}"));
        }
        Ok(())
    }

    fn find(&mut self, id: &str) -> Result<&mut Project, String> {
        self.list.iter_mut().find(|p| p.id == id).ok_or_else(|| "That project is gone.".to_string())
    }

    /// Puts pane `key` into project `to` (a new one when none: named `name`, else "Group N",
    /// made from the pane's host and folder), after `after` when that is a member, else last;
    /// out of any other. The project's id.
    pub fn add(&mut self, key: &str, to: Option<&str>, after: Option<&str>, host: Option<String>, dir: String, name: Option<&str>) -> Result<String, String> {
        let id = match to {
            Some(id) => self.find(id)?.id.clone(),
            None => self.make(name, host, dir)?,
        };
        if self.find(&id)?.members.len() >= MOST_MEMBERS {
            return Err(format!("At most {MOST_MEMBERS} sessions in a project."));
        }
        let p = self.find(&id)?;
        p.members.retain(|m| m != key);
        let at = after.and_then(|a| p.members.iter().position(|m| m == a)).map_or(p.members.len(), |i| i + 1);
        p.members.insert(at, key.to_string());
        for p in self.list.iter_mut().filter(|p| p.id != id) {
            p.members.retain(|m| m != key);
        }
        self.fresh.insert(key.to_string(), Instant::now());
        self.save()?;
        Ok(id)
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
        let id = self.make(Some(name), host, dir)?;
        self.save()?;
        Ok(id)
    }

    /// The folder a project asks for at its first [+] (AC-110).
    pub fn set_dir(&mut self, id: &str, dir: String) -> Result<(), String> {
        self.find(id)?.dir = dir;
        self.save()
    }

    /// Takes pane `key` out of its project, which stays.
    pub fn remove(&mut self, key: &str) -> Result<(), String> {
        self.prune(|m| m != key);
        self.save()
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = name_ok(name)?;
        self.name_free(&name, Some(id))?;
        self.find(id)?.name = name;
        self.save()
    }

    /// The project goes in one transaction: its sessions are back in their tmux groups, its tasks
    /// in Default (AC-110). Refused by the store, nothing changes.
    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let name = self.find(id)?.name.clone();
        let mut after = self.saved();
        after.list.retain(|p| p.id != id);
        if let Some(store) = &self.store {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
            store.with(|tx| {
                stored::move_tasks_to_default(tx, id, &name, now)?;
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

fn message(app: &Shared) -> String {
    let projects = app.projects.lock();
    json!({"t": "projects", "items": projects.told(), "order": projects.list_order(), "taskmarks": projects.marks}).to_string()
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
    if closed == Closed::All {
        links_changed(app, {
            let prefix = prefix.clone();
            move |tx, now| stored::links_dropped(tx, &prefix, now)
        }).await;
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
    links_changed(app, {
        let (prefix, panes) = (prefix.to_string(), panes.to_vec());
        move |tx, now| stored::links_restored(tx, &prefix, &panes, now)
    }).await;
    let (a, prefix, panes) = (app.clone(), prefix.to_string(), panes.to_vec());
    told(app, tokio::task::spawn_blocking(move || a.bookmarks.lock().restarted(&prefix, &panes)).await);
}

/// Tasks' links changed with a server (AC-113): the board and the rows' marks hear of it.
async fn links_changed(app: &Shared, f: impl FnOnce(&rusqlite::Transaction, i64) -> Result<usize, String> + Send + 'static) {
    let a = app.clone();
    let done = tokio::task::spawn_blocking(move || a.tasks.with(|tx| f(tx, crate::tasks::ops::now()))).await;
    match done {
        Ok(Ok(0)) => {}
        Ok(Ok(_)) => crate::tasks::api::told(app, ""),
        Ok(Err(e)) => tracing::warn!(event = "tasks.links", error = %e),
        Err(e) => tracing::warn!(event = "tasks.links", error = %e),
    }
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

/// Written to the store off the async workers.
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
