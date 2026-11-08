// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A task's ID as people see it (terminum-kanban.md AC-148): its project's prefix and the project's own
//! count, TERM-7. The row's key stays the task's for good; every ID a task had stays found in
//! `task_slugs` ("T-12" of before, one of a project it left, one of an older prefix), and the key's
//! first 8 hex find it too.

use rusqlite::{OptionalExtension, Transaction};

use super::ops::{rows, sql};
use super::store::DEFAULT_PROJECT;

pub const PREFIX_MOST: usize = 8;
/// The first characters of the key that name a task, as shown.
pub const UID_LEN: usize = 8;

/// A prefix as kept: 1–8 of A–Z and 0–9, a letter first; lower case taken as upper.
pub fn check_prefix(prefix: &str) -> Result<String, String> {
    let p = prefix.trim().to_ascii_uppercase();
    let ok = (1..=PREFIX_MOST).contains(&p.len()) && p.starts_with(|c: char| c.is_ascii_uppercase()) && p.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if ok { Ok(p) } else { Err(format!("ID letters: 1–{PREFIX_MOST} of A–Z and 0–9, a letter first")) }
}

/// The first 4 letters of the folder's name, else of the project's name, upper case; PROJ with none.
pub fn default_prefix(dir: &str, name: &str) -> String {
    let letters = |s: &str| s.chars().filter(char::is_ascii_alphabetic).take(4).collect::<String>().to_ascii_uppercase();
    let folder = dir.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    [letters(folder), letters(name)].into_iter().find(|l| !l.is_empty()).unwrap_or_else(|| "PROJ".into())
}

fn taken(tx: &Transaction, prefix: &str, but: &str) -> Result<bool, String> {
    tx.query_row("SELECT count(*) FROM projects WHERE prefix = ?1 AND id <> ?2", (prefix, but), |r| r.get::<_, i64>(0)).map(|n| n > 0).map_err(sql)
}

/// `base` if no other project has it, else its last letter changed (A–Z, then 2–9), else a number added.
fn free_prefix(tx: &Transaction, base: &str, project: &str) -> Result<String, String> {
    let stem = if base.len() >= 4 { &base[..base.len() - 1] } else { base };
    let tries = std::iter::once(base.to_string())
        .chain(('A'..='Z').chain('2'..='9').map(|c| format!("{stem}{c}")))
        .chain((2..1000).map(|n| { let n = n.to_string(); format!("{}{n}", &base[..base.len().min(PREFIX_MOST - n.len())]) }));
    for p in tries {
        if !taken(tx, &p, project)? {
            return Ok(p);
        }
    }
    Err("No free ID letters: set them in the project's window.".into())
}

/// The count a project's next task takes: above every ID found with its prefix, so an ID a task left
/// behind never names another.
fn bump_above_found(tx: &Transaction, project: &str, prefix: &str) -> Result<(), String> {
    let like = format!("{}-%", super::query::like_escaped(prefix));
    let top: Option<i64> = tx.query_row("SELECT max(CAST(substr(slug, ?2) AS INTEGER)) FROM task_slugs WHERE slug LIKE ?1 ESCAPE '\\'",
                                        (&like, prefix.len() as i64 + 2), |r| r.get(0)).map_err(sql)?;
    if let Some(top) = top {
        tx.execute("UPDATE projects SET next_seq = max(next_seq, ?1) WHERE id = ?2", (top + 1, project)).map_err(sql)?;
    }
    Ok(())
}

/// Every project gets a prefix and every task an ID, once: after a migration and when projects are written.
/// A task of before keeps its number under its project's letters, T-12 becoming TERM-12, so an old mention
/// and the new ID say the same number; the project's next counts on from there.
pub fn fill(tx: &Transaction) -> Result<(), String> {
    for (id, dir, name) in rows(tx, "SELECT id, dir, name FROM projects WHERE prefix IS NULL ORDER BY place", [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
        fill_project(tx, &id, &dir, &name)?;
    }
    for (key, number, prefix) in rows(tx, "SELECT t.id, t.number, p.prefix FROM tasks t JOIN projects p ON p.id = t.project WHERE t.slug IS NULL", [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))? {
        let slug = format!("{prefix}-{number}");
        tx.execute("UPDATE tasks SET slug = ?1 WHERE id = ?2", (&slug, &key)).map_err(sql)?;
        tx.execute("INSERT OR IGNORE INTO task_slugs (slug, task) VALUES (?1, ?2)", (&slug, &key)).map_err(sql)?;
        tx.execute("UPDATE projects SET next_seq = max(next_seq, ?1) WHERE prefix = ?2", (number + 1, &prefix)).map_err(sql)?;
    }
    Ok(())
}

fn fill_project(tx: &Transaction, project: &str, dir: &str, name: &str) -> Result<(), String> {
    let prefix = free_prefix(tx, &default_prefix(dir, name), project)?;
    tx.execute("UPDATE projects SET prefix = ?1 WHERE id = ?2", (&prefix, project)).map_err(sql)?;
    bump_above_found(tx, project, &prefix)
}

/// Task `key` takes the next ID of `project`; the one it had stays found.
pub fn assign(tx: &Transaction, key: &str, project: &str) -> Result<String, String> {
    let (prefix, seq, dir, name): (Option<String>, i64, String, String) = tx.query_row("SELECT prefix, next_seq, dir, name FROM projects WHERE id = ?1", [project],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(sql)?;
    let Some(prefix) = prefix else {
        fill_project(tx, project, &dir, &name)?;       // a project written since the last fill
        return assign(tx, key, project);
    };
    let slug = format!("{prefix}-{seq}");
    tx.execute("UPDATE projects SET next_seq = ?1 WHERE id = ?2", (seq + 1, project)).map_err(sql)?;
    tx.execute("UPDATE tasks SET slug = ?1 WHERE id = ?2", (&slug, key)).map_err(sql)?;
    tx.execute("INSERT INTO task_slugs (slug, task) VALUES (?1, ?2)", (&slug, key)).map_err(sql)?;
    Ok(slug)
}

/// A project's ID letters changed: its tasks keep their numbers under the new ones, the old IDs stay found.
pub fn set_prefix(tx: &Transaction, project: &str, prefix: &str) -> Result<(), String> {
    if project == DEFAULT_PROJECT {
        return Err("Default's tasks keep TASK.".into());
    }
    let prefix = check_prefix(prefix)?;
    if taken(tx, &prefix, project)? {
        return Err(format!("{prefix} names another project's tasks."));
    }
    let old: Option<String> = tx.query_row("SELECT prefix FROM projects WHERE id = ?1", [project], |r| r.get(0)).optional().map_err(sql)?.ok_or("That project is gone.")?;
    if old.as_deref() == Some(prefix.as_str()) {
        return Ok(());
    }
    let tasks = rows(tx, "SELECT id, slug FROM tasks WHERE project = ?1 AND slug IS NOT NULL", [project], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let renamed: Vec<(String, String)> = tasks.into_iter().map(|(key, slug)| {
        let seq = slug.rsplit('-').next().unwrap_or("0").to_string();
        (key, format!("{prefix}-{seq}"))
    }).collect();
    // an ID another task had or has keeps finding that one: such letters are refused, never taken over
    for (key, new) in &renamed {
        let other: Option<String> = tx.query_row("SELECT task FROM task_slugs WHERE slug = ?1 AND task <> ?2", (new, key), |r| r.get(0)).optional().map_err(sql)?;
        if other.is_some() {
            return Err(format!("{new} already names another task: choose other letters."));
        }
    }
    tx.execute("UPDATE projects SET prefix = ?1 WHERE id = ?2", (&prefix, project)).map_err(sql)?;
    for (key, new) in &renamed {
        tx.execute("UPDATE tasks SET slug = ?1 WHERE id = ?2", (new, key)).map_err(sql)?;
        tx.execute("INSERT OR IGNORE INTO task_slugs (slug, task) VALUES (?1, ?2)", (new, key)).map_err(sql)?;
    }
    bump_above_found(tx, project, &prefix)
}

/// A task's number (the store's order) from what a person or an agent calls it: its ID now, one it had,
/// or its key's first 8 hex characters or the whole key. None when nothing, or more than one, answers.
pub fn resolve(tx: &Transaction, id: &str) -> Result<Option<i64>, String> {
    let said = id.trim();
    let upper = said.to_ascii_uppercase();
    let now: Option<i64> = tx.query_row("SELECT number FROM tasks WHERE slug = ?1", [&upper], |r| r.get(0)).optional().map_err(sql)?;
    if now.is_some() {
        return Ok(now);
    }
    let was: Option<i64> = tx.query_row("SELECT t.number FROM task_slugs s JOIN tasks t ON t.id = s.task WHERE s.slug = ?1", [&upper], |r| r.get(0)).optional().map_err(sql)?;
    if was.is_some() {
        return Ok(was);
    }
    let lower = said.to_ascii_lowercase();
    // the shown start of its key, or the whole key
    if (lower.len() == UID_LEN || lower.len() == 32) && lower.chars().all(|c| c.is_ascii_hexdigit()) {
        let found = rows(tx, "SELECT number FROM tasks WHERE id LIKE ?1 LIMIT 2", [format!("{lower}%")], |r| r.get::<_, i64>(0))?;
        return Ok(if found.len() == 1 { Some(found[0]) } else { None });
    }
    Ok(None)
}

#[cfg(test)]
#[path = "slugs_tests.rs"]
mod tests;
