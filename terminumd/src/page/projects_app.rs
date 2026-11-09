// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Projects' part in terminumd (AC-92, AC-110): the pages told of them, a server's list read
//! and restore applied to them, the page's project actions; the rules stay in projects.rs.

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use serde_json::{json, Value};

use super::{stored, Closed};
use crate::app::Shared;
use crate::page::{lost_marks, Page};

fn message(app: &Shared) -> String {
    let projects = app.projects.lock();
    json!({"t": "projects", "items": projects.list(), "order": projects.list_order()}).to_string()
}

/// A page that connects learns the projects.
pub fn hello(app: &Shared, page: &Page) {
    page.send(message(app));
    crate::tasks::marks::hello(app, page);
}

/// Every page learns the projects (after a change).
pub fn tell(app: &Shared) {
    app.pages.tell(&message(app));
}

/// The servers' lists were read and told: the projects those reads changed are told now, after the
/// list without the closed panes (their rows do not stand in their tmux groups meanwhile). Kept until
/// told: a read cut short tells them with the next.
pub fn tell_read(app: &Shared) {
    if app.projects_untold.swap(false, Ordering::AcqRel) {
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
    ready(app).await;
    let (changed, closed, left) = {
        let mut p = app.projects.lock();
        let (changed, closed) = p.seen(&prefix, identity.as_deref(), &live);
        if changed {
            p.untold.store(true, Ordering::Release);
        }
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
    lost_marks::orphan(app, move |k| k.starts_with(&prefix) && match closed {
        Closed::None => false,
        Closed::All => true,
        Closed::NotLive => !live.contains(k),
    }).await;
}

/// The server of `prefix` was restored (AC-15; `panes`: old id, new id, its kept panes paired
/// with themselves): its sessions keep their projects and bookmarks, before its lists are read.
pub async fn restarted(app: &Shared, prefix: &str, panes: &[(String, String)]) {
    ready(app).await;
    if app.projects.lock().restarted(prefix, panes) {
        save(app).await;
        tell(app);
    }
    links_changed(app, {
        let (prefix, panes) = (prefix.to_string(), panes.to_vec());
        move |tx, now| stored::links_restored(tx, &prefix, &panes, now)
    }).await;
    let (a, prefix, panes) = (app.clone(), prefix.to_string(), panes.to_vec());
    lost_marks::told(app, tokio::task::spawn_blocking(move || a.bookmarks.lock().restarted(&prefix, &panes)).await);
}

/// Tasks' links changed with a server (AC-113): the board and the rows' marks hear of it.
async fn links_changed(app: &Shared, f: impl FnOnce(&rusqlite::Transaction, i64) -> Result<usize, String> + Send + 'static) {
    match crate::tasks::api::store(app, move |tx| f(tx, crate::tasks::ops::now())).await {
        Ok(0) => {}
        Ok(_) => crate::tasks::api::told(app, ""),
        Err(e) => tracing::warn!(event = "tasks.links", error = %e),
    }
}

/// A store unread at start read off the async workers, before a server's panes are judged against it.
async fn ready(app: &Shared) {
    let a = app.clone();
    if a.projects.lock().unread()
        && let Ok(Err(e)) = tokio::task::spawn_blocking(move || a.projects.lock().ready()).await
    {
        tracing::warn!(event = "projects.unread", error = %e);
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
    let ids = |k: &str| crate::config::strings(&msg[k]);
    let action = s("action").unwrap_or_default();
    let (key, project, given) = (s("id").unwrap_or_default(), s("project"), s("name").filter(|n| !n.trim().is_empty()));
    let name = given.clone().unwrap_or_default();
    let (order, keys) = (ids("ids"), ids("keys"));
    // the pane's host and folder, for a new project's [+]
    let (host, dir) = if action == "add" { crate::page::marks::find_item(&app.layout.read(), &key).map(crate::page::marks::project_of).unwrap_or_default() } else { Default::default() };
    let a = app.clone();
    let done = crate::http::off(move || {
        let mut ps = a.projects.lock();
        match action.as_str() {
            "add" if crate::tmux::parse_key(&key).is_some() => match project.as_deref() {
                Some(to) => ps.add_to(&key, to, None),
                None => ps.add_new(&key, given.as_deref(), host, dir.unwrap_or_default()).map(|_| ()),
            },
            "remove" => ps.remove(&key),
            "rename" => ps.rename(project.as_deref().unwrap_or(""), &name),
            "delete" => ps.delete(project.as_deref().unwrap_or("")),
            "order" => ps.order(&order),
            "members" => ps.members(project.as_deref().unwrap_or(""), &keys),
            _ => Err(format!("Not a project action: {action}")),
        }
    }).await;
    match done {
        Ok(()) => tell(app),
        Err(e) => crate::page::client::error(page, e, None),
    }
}
