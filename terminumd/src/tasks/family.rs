// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A task and its subtasks, one level (AC-112): a subtask is a full task with a parent; a task goes
//! under another or out of it, its history and its parent's saying so.

use std::collections::HashMap;

use rusqlite::Transaction;
use serde_json::{json, Value};

use super::ops::{get, log, rows, sql, touch, user_name};

/// Task `id` goes under `parent`, or out of its parent with none.
pub fn set_parent(tx: &Transaction, author: &str, id: &str, parent: Option<&str>, at: i64) -> Result<(), String> {
    let task = get(tx, id)?;
    let who = user_name(tx, author)?;
    let Some(parent) = parent else {
        let Some(was) = task.parent else { return Ok(()) };
        let up = get(tx, &was)?;
        tx.execute("UPDATE tasks SET parent = NULL, updated_at = ?1 WHERE id = ?2", (at, &task.key)).map_err(sql)?;
        log(tx, &task.key, author, "parent", &format!("{who} took it out of {}", up.id), None, at)?;
        log(tx, &up.key, author, "parent", &format!("{who} took {} out of its subtasks", task.id), None, at)?;
        return touch(tx, &up.key, at);
    };
    let up = get(tx, parent)?;
    if up.key == task.key {
        return Err("A task cannot be its own subtask.".into());
    }
    if task.parent.as_deref() == Some(up.id.as_str()) {
        return Ok(());
    }
    if up.parent.is_some() {
        return Err("A subtask cannot have subtasks".into());
    }
    if has_any(tx, &task.key)? {
        return Err(format!("{} has subtasks of its own: it cannot become a subtask.", task.id));
    }
    if up.project != task.project {
        return Err(format!("{} is in another project: move {} there first.", up.id, task.id));
    }
    let left = task.parent.as_deref().map(|p| get(tx, p)).transpose()?;
    tx.execute("UPDATE tasks SET parent = ?1, updated_at = ?2 WHERE id = ?3", (&up.key, at, &task.key)).map_err(sql)?;
    log(tx, &task.key, author, "parent", &format!("{who} made it a subtask of {}", up.id), None, at)?;
    log(tx, &up.key, author, "parent", &format!("{who} made {} its subtask", task.id), None, at)?;
    touch(tx, &up.key, at)?;
    if let Some(left) = left {
        log(tx, &left.key, author, "parent", &format!("{who} moved {} to {}", task.id, up.id), None, at)?;
        touch(tx, &left.key, at)?;
    }
    Ok(())
}

/// Archived ones too: a subtask taken off the board still makes its parent a parent.
fn has_any(tx: &Transaction, key: &str) -> Result<bool, String> {
    tx.query_row("SELECT count(*) FROM tasks WHERE parent = ?1", [key], |r| r.get::<_, i64>(0)).map(|n| n > 0).map_err(sql)
}

/// The subtasks each card shows as boxes (AC-112): ID, status and title, in the order they were made,
/// those taken off the board left out; by the parent's key.
pub fn boxes(tx: &Transaction, parents: &[&str]) -> Result<HashMap<String, Vec<Value>>, String> {
    let mut out: HashMap<String, Vec<Value>> = HashMap::new();
    if parents.is_empty() {
        return Ok(out);
    }
    let marks = (1..=parents.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    let found = rows(tx, &format!("SELECT parent, slug, status, title FROM tasks WHERE parent IN ({marks}) AND archived_at IS NULL ORDER BY number"),
                     rusqlite::params_from_iter(parents.iter()), |r| Ok((r.get::<_, String>(0)?, json!({"id": r.get::<_, String>(1)?, "status": r.get::<_, String>(2)?, "title": r.get::<_, String>(3)?}))))?;
    for (parent, kid) in found {
        out.entry(parent).or_default().push(kid);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "family_tests.rs"]
mod tests;
