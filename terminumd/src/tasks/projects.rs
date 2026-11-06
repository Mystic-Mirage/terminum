// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The session list's projects in the store (AC-110, AC-119): read at the start, each change
//! written whole in a transaction. Default stays out of the list: it is the board's alone.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::Transaction;
use serde_json::{json, Value};

use super::store::{Store, DEFAULT_PROJECT, HUMAN};
use crate::page::projects::Project;

/// What the session list's projects hold.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Saved {
    pub list: Vec<Project>,
    pub servers: BTreeMap<String, String>,
    pub order: Vec<String>,
}

fn sql(e: rusqlite::Error) -> String {
    format!("The tasks' store: {e}")
}

fn read(tx: &Transaction) -> Result<Saved, String> {
    let mut members = tx.prepare("SELECT pane_key FROM project_members WHERE project = ?1 ORDER BY place").map_err(sql)?;
    let mut projects = tx.prepare("SELECT id, name, color, host, dir FROM projects WHERE is_default = 0 ORDER BY place").map_err(sql)?;
    let rows = projects.query_map([], |r| Ok(Project { id: r.get(0)?, name: r.get(1)?, color: r.get::<_, i64>(2)? as usize, host: r.get(3)?, dir: r.get(4)?, members: Vec::new() })).map_err(sql)?;
    let mut list = Vec::new();
    for p in rows {
        let mut p = p.map_err(sql)?;
        p.members = members.query_map([&p.id], |r| r.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
        list.push(p);
    }
    let servers = tx.prepare("SELECT start, terminum_id FROM servers").map_err(sql)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    let order = tx.prepare("SELECT part FROM list_order ORDER BY place").map_err(sql)?
        .query_map([], |r| r.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    Ok(Saved { list, servers, order })
}

/// Writes them whole. A project gone from `list` that still holds tasks is refused by the foreign
/// key: `move_tasks_to_default` comes first.
pub fn write(tx: &Transaction, saved: &Saved) -> Result<(), String> {
    tx.execute("DELETE FROM project_members", []).map_err(sql)?;
    let kept: Vec<&str> = saved.list.iter().map(|p| p.id.as_str()).collect();
    let on_disk: Vec<String> = tx.prepare("SELECT id FROM projects WHERE is_default = 0").map_err(sql)?
        .query_map([], |r| r.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    for gone in on_disk.iter().filter(|id| !kept.contains(&id.as_str())) {
        tx.execute("DELETE FROM projects WHERE id = ?1", [gone]).map_err(sql)?;
    }
    for (place, p) in saved.list.iter().enumerate() {
        tx.execute("INSERT INTO projects (id, name, color, host, dir, place) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    ON CONFLICT (id) DO UPDATE SET name = ?2, color = ?3, host = ?4, dir = ?5, place = ?6",
                   (&p.id, &p.name, p.color as i64, &p.host, &p.dir, place as i64)).map_err(sql)?;
        for (at, key) in p.members.iter().enumerate() {
            tx.execute("INSERT INTO project_members (project, pane_key, place) VALUES (?1, ?2, ?3)", (&p.id, key, at as i64)).map_err(sql)?;
        }
    }
    tx.execute("DELETE FROM servers", []).map_err(sql)?;
    for (start, id) in &saved.servers {
        tx.execute("INSERT INTO servers (start, terminum_id) VALUES (?1, ?2)", (start, id)).map_err(sql)?;
    }
    tx.execute("DELETE FROM list_order", []).map_err(sql)?;
    for (place, part) in saved.order.iter().enumerate() {
        tx.execute("INSERT INTO list_order (place, part) VALUES (?1, ?2)", (place as i64, part)).map_err(sql)?;
    }
    Ok(())
}

/// A deleted project's tasks stand in Default, each one's history saying so (AC-110). How many.
pub fn move_tasks_to_default(tx: &Transaction, project: &str, name: &str, now: i64) -> Result<usize, String> {
    let tasks: Vec<String> = tx.prepare("SELECT id FROM tasks WHERE project = ?1").map_err(sql)?
        .query_map([project], |r| r.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    for task in &tasks {
        tx.execute("INSERT INTO events (id, task, at, author, kind, text) VALUES (?1, ?2, ?3, ?4, 'project', ?5)",
                   (crate::auth::crypto::random_hex::<16>(), task, now, HUMAN, format!("Project {name} was deleted: moved to Default"))).map_err(sql)?;
    }
    tx.execute("UPDATE tasks SET project = ?1, updated_at = ?2 WHERE project = ?3", (DEFAULT_PROJECT, now, project)).map_err(sql)?;
    Ok(tasks.len())
}

/// What projects.json held, read as an older terminum wrote it; one that cannot be read is set
/// aside as before (`projects.json.bak`).
fn legacy(path: &Path) -> Saved {
    crate::auth::store::read_or_set_aside(path, |v: Value| {
        if !v.is_object() {
            return Err("expected a projects object".into());
        }
        let list = serde_json::from_value(v.get("projects").cloned().unwrap_or_else(|| json!([]))).map_err(|e| e.to_string())?;
        let servers = serde_json::from_value(v.get("servers").cloned().unwrap_or_else(|| json!({}))).map_err(|e| e.to_string())?;
        let order = serde_json::from_value(v.get("order").cloned().unwrap_or_else(|| json!([]))).map_err(|e| e.to_string())?;
        Ok(Saved { list, servers, order })
    }).unwrap_or_default()
}

/// The store's projects; on its first start with this store, those of `projects_json` copied in
/// once, and the file left in place for an older terminum (AC-110).
pub fn load(store: &Store, projects_json: &Path) -> Result<Saved, String> {
    store.with(|tx| {
        let imported: Option<String> = match tx.query_row("SELECT value FROM meta WHERE key = 'projects_imported'", [], |r| r.get(0)) {
            Ok(v) => Some(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(sql(e)),
        };
        if imported.is_none() {
            write(tx, &legacy(projects_json))?;
            tx.execute("INSERT INTO meta (key, value) VALUES ('projects_imported', '1')", []).map_err(sql)?;
        }
        read(tx)
    })
}
