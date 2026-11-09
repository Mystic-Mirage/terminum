// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The message history (terminum.md AC-147, terminum-terminal.md AC-90): each message sent in a pane,
//! a shell's command at Enter or a coding agent's prompt, a row of the tasks' store's `messages`
//! table beside the input log, its words in `messages_fts` (FTS5) for the Message history's
//! search (AC-151) and its counts for the Dashboard (AC-152). What is kept out and how a text is
//! redacted: `crate::sent`. Called off the async workers, in the store's transactions.

pub mod api;
pub mod search;
pub mod stats;
#[cfg(test)]
mod tests;

use rusqlite::{OptionalExtension, Transaction};
use serde_json::{json, Value};

/// A message to keep.
#[derive(Clone, Debug, Default)]
pub struct Message {
    /// unix ms
    pub at: i64,
    /// `users.id`: `human`, `dispatcher`
    pub sender: String,
    /// `hook` (the agent's own prompt) or `page` (what stood at the cursor)
    pub via: String,
    pub agent: Option<String>,
    /// the pane's state key
    pub pane: String,
    /// the history line its text began on, a hint (AC-90)
    pub line: Option<i64>,
    /// the pane's server ("local" for every server of this Mac) and folder: the old per-folder history's project
    pub host: String,
    pub cwd: String,
    /// the session list's title of the pane, and its project there
    pub title: Option<String>,
    pub project: Option<String>,
    pub project_name: Option<String>,
    pub text: String,
}

pub(crate) fn sql(e: rusqlite::Error) -> String {
    format!("The message history: {e}")
}

/// The columns `item` reads, in its order, of the messages as `m` (`m.` or ""); the last, the local day it
/// was sent (as the day strip counts).
pub fn columns(m: &str) -> String {
    let cols = ["id", "at", "sender", "via", "agent", "pane", "line", "host", "cwd", "title", "project", "project_name", "text"];
    format!("{}, date({m}at / 1000, 'unixepoch', 'localtime')", cols.map(|c| format!("{m}{c}")).join(", "))
}
/// How many `columns` are.
pub const N_COLUMNS: usize = 14;

/// A row of `columns` as the page reads a message: `at` in unix seconds (AC-90's shape), `key` its pane.
pub fn item(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let at: i64 = r.get(1)?;
    let mut v = json!({"id": r.get::<_, i64>(0)?, "at": at / 1000, "ms": at, "sender": r.get::<_, String>(2)?, "via": r.get::<_, String>(3)?,
                       "key": r.get::<_, String>(5)?, "host": r.get::<_, String>(7)?, "path": r.get::<_, String>(8)?, "text": r.get::<_, String>(12)?,
                       "day": r.get::<_, String>(13)?});
    for (i, k) in [(4, "agent"), (9, "title"), (10, "project"), (11, "project_name")] {
        if let Some(s) = r.get::<_, Option<String>>(i)?.filter(|s| !s.is_empty()) {
            v[k] = json!(s);
        }
    }
    if let Some(line) = r.get::<_, Option<i64>>(6)? {
        v["line"] = json!(line);
    }
    Ok(v)
}

/// Keeps `m`; its id.
pub fn insert(tx: &Transaction, m: &Message) -> Result<i64, String> {
    tx.prepare_cached("INSERT INTO messages (at, sender, via, agent, pane, line, host, cwd, title, project, project_name, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)")
        .and_then(|mut st| st.execute(rusqlite::params![m.at, m.sender, m.via, m.agent, m.pane, m.line, m.host, m.cwd, m.title, m.project, m.project_name, m.text]))
        .map_err(sql)?;
    Ok(tx.last_insert_rowid())
}

/// The `n`-th newest message of folder (`host`, `cwd`) (0: the last) and how many it has.
pub fn nth_of_folder(tx: &Transaction, host: &str, cwd: &str, n: usize) -> Result<(usize, Option<Value>), String> {
    let total: i64 = tx.query_row("SELECT count(*) FROM messages WHERE host = ?1 AND cwd = ?2", (host, cwd), |r| r.get(0)).map_err(sql)?;
    let n = i64::try_from(n).unwrap_or(i64::MAX);
    let item = tx.query_row(&format!("SELECT {} FROM messages WHERE host = ?1 AND cwd = ?2 ORDER BY at DESC, id DESC LIMIT 1 OFFSET ?3", columns("")), (host, cwd, n), item)
        .optional().map_err(sql)?;
    Ok((total as usize, item))
}

/// The message `id`, if kept.
pub fn by_id(tx: &Transaction, id: i64) -> Result<Option<Value>, String> {
    tx.query_row(&format!("SELECT {} FROM messages WHERE id = ?1", columns("")), [id], item).optional().map_err(sql)
}

/// The `most` newest messages sent in pane `pane` with a line: (text, line, an agent's prompt), newest first.
pub fn of_pane(tx: &Transaction, pane: &str, most: usize) -> Result<Vec<(String, i64, bool)>, String> {
    let mut st = tx.prepare_cached("SELECT text, line, agent IS NOT NULL AND agent != '' FROM messages WHERE pane = ?1 AND line IS NOT NULL ORDER BY at DESC, id DESC LIMIT ?2").map_err(sql)?;
    let rows = st.query_map((pane, most as i64), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

/// The highest N of a "Quote #N" pane `pane` sent (AC-37), 0 when none.
pub fn highest_quote(tx: &Transaction, pane: &str) -> Result<u64, String> {
    let mut st = tx.prepare_cached("SELECT text FROM messages WHERE pane = ?1 AND text LIKE '%Quote #%'").map_err(sql)?;
    let texts = st.query_map([pane], |r| r.get::<_, String>(0)).map_err(sql)?;
    let mut most = 0;
    for t in texts {
        most = most.max(crate::sent::quote_numbers(&t.map_err(sql)?).into_iter().max().unwrap_or(0));
    }
    Ok(most)
}

/// Messages older than the input log's days, then the oldest beyond `rows`, go (its writer's hourly
/// trim). Newest is by time, never by id: rows kept before this store had them came in later.
pub fn trim(tx: &Transaction, now_ms: i64, days: i64, rows: i64) -> Result<(), String> {
    tx.execute("DELETE FROM messages WHERE at < ?1", [now_ms - days * 86_400_000]).map_err(sql)?;
    tx.execute("DELETE FROM messages WHERE id IN (SELECT id FROM messages ORDER BY at DESC, id DESC LIMIT -1 OFFSET ?1)", [rows]).map_err(sql)?;
    Ok(())
}
