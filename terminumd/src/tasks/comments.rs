// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A task's comments and its history (AC-107, AC-108, AC-149), each change in the caller's transaction.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, Transaction};
use serde_json::{json, Value};

use super::files::File;
use super::ops::{check_comment, key_of, log, rows, sql, touch, user_name};

/// Whether a human's last comment on it says just this: a press tried again keeps it once.
pub fn last_comment_is(tx: &Transaction, id: &str, text: &str) -> Result<bool, String> {
    let key = key_of(tx, id)?;
    let last: Option<String> = tx.query_row("SELECT text FROM events WHERE task = ?1 AND kind = 'comment' ORDER BY at DESC, rowid DESC LIMIT 1", [&key], |r| r.get(0)).optional().map_err(sql)?;
    Ok(last.as_deref() == Some(text))
}

/// A comment (AC-108): the event's id.
pub fn comment(tx: &Transaction, author: &str, id: &str, text: &str, at: i64) -> Result<String, String> {
    check_comment(text)?;
    let key = key_of(tx, id)?;
    touch(tx, &key, at)?;
    log(tx, &key, author, "comment", text, None, at)
}

/// A human's own comment changed or deleted: a new event holds it, and the old text leaves the
/// shown history (AC-108). Those of agents and Dispatcher stay as written.
pub fn change_comment(tx: &Transaction, author: &str, id: &str, event: &str, text: Option<&str>, at: i64) -> Result<(), String> {
    let key = key_of(tx, id)?;
    let (kind, by): (String, String) = tx.query_row("SELECT kind, author FROM events WHERE id = ?1 AND task = ?2", (event, &key), |r| Ok((r.get(0)?, r.get(1)?)))
        .optional().map_err(sql)?.ok_or("That comment is gone.")?;
    if kind != "comment" || by != author {
        return Err("Only its human author can change a comment.".into());
    }
    check_comment(text.unwrap_or(""))?;
    let who = user_name(tx, author)?;
    match text {
        Some(t) => {
            tx.execute("UPDATE events SET text = ?1, edited_at = ?2 WHERE id = ?3", (t, at, event)).map_err(sql)?;
            log(tx, &key, author, "edit", &format!("{who} changed a comment"), None, at)?;
        }
        None => {
            tx.execute("UPDATE events SET kind = 'deleted', text = '' WHERE id = ?1", [event]).map_err(sql)?;
            log(tx, &key, author, "edit", &format!("{who} deleted a comment"), None, at)?;
        }
    }
    touch(tx, &key, at)
}

/// A task's comments and history, oldest first, with its files by event.
pub fn history(tx: &Transaction, id: &str) -> Result<Value, String> {
    let key = key_of(tx, id)?;
    let mut files: HashMap<String, Vec<File>> = HashMap::new();
    for (event, file) in rows(tx, "SELECT event, id, name, bytes, mime FROM files WHERE task = ?1 AND event IS NOT NULL", [&key], |r| Ok((r.get(0)?, File::of_row(r, 1)?)))? {
        files.entry(event).or_default().push(file);
    }
    let out = rows(tx, "SELECT e.id, e.at, e.author, u.name, u.kind, e.kind, e.text, e.edited_at FROM events e JOIN users u ON u.id = e.author WHERE e.task = ?1 ORDER BY e.at, e.rowid", [&key], |r| {
        let event: String = r.get(0)?;
        let attached = files.remove(&event).unwrap_or_default();
        Ok(json!({"id": event, "at": r.get::<_, i64>(1)?, "author": r.get::<_, String>(2)?,
            "author_name": r.get::<_, String>(3)?, "author_kind": r.get::<_, String>(4)?,
            "kind": r.get::<_, String>(5)?, "text": r.get::<_, Option<String>>(6)?, "edited_at": r.get::<_, Option<i64>>(7)?, "files": attached}))
    })?;
    Ok(Value::Array(out))
}
