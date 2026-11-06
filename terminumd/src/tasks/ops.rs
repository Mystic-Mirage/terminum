// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a task can do, each in the caller's transaction, its history in the same one (AC-106,
//! AC-107, AC-108, AC-113). The rules are here for humans and agents alike; who may ask is the
//! caller's (`tools.rs` for agents).

use rusqlite::{OptionalExtension, Row, Transaction};
use serde::Serialize;
use serde_json::{json, Value};

use super::store::{DEFAULT_PROJECT, HUMAN};

pub const COLUMNS: [&str; 5] = ["TODO", "IN PROGRESS", "WAIT FOR HUMAN", "FAILED", "DONE"];
pub const SEVERITIES: [&str; 4] = ["P0", "P1", "P2", "P3"];
const TITLE_MOST: usize = 200;
const TEXT_MOST: usize = 1 << 20;
const COMMENT_MOST: usize = 256 << 10;
/// Open tasks by agents in one project (AC-115).
pub const AGENTS_OPEN_MOST: i64 = 200;

pub fn sql(e: rusqlite::Error) -> String {
    format!("The tasks' store: {e}")
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// A task as the board and the tools see it, without its text.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub project: String,
    pub parent: Option<String>,
    pub title: String,
    pub status: String,
    pub severity: String,
    pub owner: String,
    pub owner_name: String,
    pub owner_kind: String,
    pub by_agent: bool,
    pub session: Option<String>,
    /// the pane of the agent that made it: an agent's reach (tools.rs)
    pub made_in: Option<String>,
    pub files: i64,
    pub subtasks_done: i64,
    pub subtasks: i64,
    pub updated_at: i64,
}

pub(super) const TASK: &str = "SELECT t.number, t.project, (SELECT 'T-' || p.number FROM tasks p WHERE p.id = t.parent), t.title, t.status, t.severity,
    t.owner, u.name, u.kind, t.by_agent, t.session_key,
    (SELECT count(*) FROM files f WHERE f.task = t.id),
    (SELECT count(*) FROM tasks s WHERE s.parent = t.id AND s.status = 'DONE'),
    (SELECT count(*) FROM tasks s WHERE s.parent = t.id), t.updated_at, t.made_in
    FROM tasks t JOIN users u ON u.id = t.owner";

pub(super) fn task_of(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: format!("T-{}", r.get::<_, i64>(0)?),
        project: r.get(1)?,
        parent: r.get(2)?,
        title: r.get(3)?,
        status: r.get(4)?,
        severity: r.get(5)?,
        owner: r.get(6)?,
        owner_name: r.get(7)?,
        owner_kind: r.get(8)?,
        by_agent: r.get::<_, i64>(9)? != 0,
        session: r.get(10)?,
        files: r.get(11)?,
        subtasks_done: r.get(12)?,
        subtasks: r.get(13)?,
        updated_at: r.get(14)?,
        made_in: r.get(15)?,
    })
}

/// "T-12" to its number.
pub fn number_of(id: &str) -> Result<i64, String> {
    id.strip_prefix("T-").and_then(|n| n.parse().ok()).filter(|n| *n > 0).ok_or_else(|| format!("No task {id}"))
}

/// The row's key of task `id`.
pub(super) fn key_of(tx: &Transaction, id: &str) -> Result<String, String> {
    let n = number_of(id)?;
    tx.query_row("SELECT id FROM tasks WHERE number = ?1", [n], |r| r.get(0)).optional().map_err(sql)?.ok_or_else(|| format!("No task {id}"))
}

pub fn get(tx: &Transaction, id: &str) -> Result<Task, String> {
    let n = number_of(id)?;
    tx.query_row(&format!("{TASK} WHERE t.number = ?1"), [n], task_of).optional().map_err(sql)?.ok_or_else(|| format!("No task {id}"))
}

pub fn text_of(tx: &Transaction, id: &str) -> Result<String, String> {
    let n = number_of(id)?;
    tx.query_row("SELECT body FROM tasks WHERE number = ?1", [n], |r| r.get(0)).optional().map_err(sql)?.ok_or_else(|| format!("No task {id}"))
}

fn user_name(tx: &Transaction, user: &str) -> Result<String, String> {
    tx.query_row("SELECT name FROM users WHERE id = ?1", [user], |r| r.get(0)).optional().map_err(sql)?.ok_or_else(|| format!("No user {user}"))
}

/// The user an agent writes as ("claude · api"), made the first time.
pub fn agent_user(tx: &Transaction, agent: &str, session: &str) -> Result<String, String> {
    let name = format!("{agent} · {session}");
    let id = format!("agent:{name}");
    tx.execute("INSERT OR IGNORE INTO users (id, kind, name, role) VALUES (?1, 'agent', ?2, 'agent')", (&id, &name)).map_err(sql)?;
    Ok(id)
}

/// One line of a task's history; a comment is one too. Its id.
pub fn log(tx: &Transaction, task_key: &str, author: &str, kind: &str, text: &str, data: Option<&Value>, at: i64) -> Result<String, String> {
    let id = crate::auth::crypto::random_hex::<16>();
    tx.execute("INSERT INTO events (id, task, at, author, kind, text, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
               (&id, task_key, at, author, kind, text, data.map(Value::to_string))).map_err(sql)?;
    Ok(id)
}

fn touch(tx: &Transaction, key: &str, at: i64) -> Result<(), String> {
    tx.execute("UPDATE tasks SET updated_at = ?1 WHERE id = ?2", (at, key)).map(|_| ()).map_err(sql)
}

/// The rank that stands above every card of a column's severity: new tasks and moves go on top.
fn top_rank(tx: &Transaction, status: &str, severity: &str) -> Result<f64, String> {
    let min: Option<f64> = tx.query_row("SELECT min(rank) FROM tasks WHERE status = ?1 AND severity = ?2 AND archived_at IS NULL", (status, severity), |r| r.get(0)).map_err(sql)?;
    Ok(min.map_or(0.0, |m| m - 1.0))
}

fn check_title(title: &str) -> Result<String, String> {
    let t = title.trim();
    if t.is_empty() || t.chars().count() > TITLE_MOST {
        return Err(format!("Title: 1–{TITLE_MOST} characters"));
    }
    Ok(t.to_string())
}

fn check_text(text: &str) -> Result<(), String> {
    if text.len() > TEXT_MOST { Err("Text: at most 1 MiB".into()) } else { Ok(()) }
}

fn check_severity(s: &str) -> Result<(), String> {
    if SEVERITIES.contains(&s) { Ok(()) } else { Err(format!("Severity: one of {}", SEVERITIES.join(", "))) }
}

fn check_project(tx: &Transaction, project: &str) -> Result<(), String> {
    let found: Option<i64> = tx.query_row("SELECT 1 FROM projects WHERE id = ?1", [project], |r| r.get(0)).optional().map_err(sql)?;
    found.map(|_| ()).ok_or_else(|| "That project is gone.".to_string())
}

/// What makes a task.
#[derive(Default, Clone)]
pub struct NewTask {
    pub title: String,
    pub text: String,
    pub project: Option<String>,
    pub parent: Option<String>,
    pub severity: Option<String>,
    pub owner: Option<String>,
    pub session: Option<String>,
    pub by_agent: bool,
    pub made_in: Option<String>,
}

pub fn create(tx: &Transaction, author: &str, new: NewTask, at: i64) -> Result<Task, String> {
    let title = check_title(&new.title)?;
    check_text(&new.text)?;
    let severity = new.severity.unwrap_or_else(|| "P2".into());
    check_severity(&severity)?;
    let parent = match &new.parent {
        Some(p) => {
            let up = get(tx, p)?;
            if up.parent.is_some() {
                return Err("A subtask cannot have subtasks".into());
            }
            Some((key_of(tx, p)?, up.project))
        }
        None => None,
    };
    // a subtask stands in its parent's project, whatever was asked
    let project = parent.as_ref().map(|(_, p)| p.clone()).or(new.project).unwrap_or_else(|| DEFAULT_PROJECT.to_string());
    check_project(tx, &project)?;
    if new.by_agent {
        let open: i64 = tx.query_row("SELECT count(*) FROM tasks WHERE project = ?1 AND by_agent = 1 AND archived_at IS NULL AND status <> 'DONE'", [&project], |r| r.get(0)).map_err(sql)?;
        if open >= AGENTS_OPEN_MOST {
            return Err(format!("This project holds {AGENTS_OPEN_MOST} open tasks by agents. Ask the user to close some."));
        }
    }
    let owner = new.owner.unwrap_or_else(|| HUMAN.to_string());
    let rank = top_rank(tx, "TODO", &severity)?;
    let number: i64 = tx.query_row("SELECT coalesce(max(number), 0) + 1 FROM tasks", [], |r| r.get(0)).map_err(sql)?;
    let key = crate::auth::crypto::random_hex::<16>();
    tx.execute("INSERT INTO tasks (id, number, project, parent, title, body, status, severity, rank, owner, by_agent, session_key, made_in, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'TODO', ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
               rusqlite::params![key, number, project, parent.as_ref().map(|(k, _)| k), title, new.text, severity, rank, owner, new.by_agent as i64, new.session, new.made_in, at]).map_err(sql)?;
    log(tx, &key, author, "made", &format!("{} made it", user_name(tx, author)?), None, at)?;
    get(tx, &format!("T-{number}"))
}

/// A task of `session` standing in IN PROGRESS other than `but` (AC-113).
fn in_progress_of(tx: &Transaction, session: &str, but: &str) -> Result<Option<String>, String> {
    tx.query_row("SELECT 'T-' || number FROM tasks WHERE session_key = ?1 AND status = 'IN PROGRESS' AND id <> ?2 AND archived_at IS NULL", (session, but), |r| r.get(0))
        .optional().map_err(sql)
}

/// Moves a task to a column, on top of its severity's cards; its history says who did it.
pub fn move_to(tx: &Transaction, author: &str, id: &str, status: &str, at: i64) -> Result<Task, String> {
    if !COLUMNS.contains(&status) {
        return Err(format!("Status: one of {}", COLUMNS.join(", ")));
    }
    let task = get(tx, id)?;
    if task.status == status {
        return Ok(task);
    }
    let key = key_of(tx, id)?;
    if status == "IN PROGRESS"
        && let Some(session) = &task.session
            && let Some(other) = in_progress_of(tx, session, &key)? {
                return Err(format!("{} already works on {other}. Finish or move {other} first.", session_name(session)));
            }
    let rank = top_rank(tx, status, &task.severity)?;
    tx.execute("UPDATE tasks SET status = ?1, rank = ?2, updated_at = ?3 WHERE id = ?4", (status, rank, at, &key)).map_err(sql)?;
    let who = user_name(tx, author)?;
    log(tx, &key, author, "move", &format!("{who} moved it from {} to {status}", task.status), Some(&json!({"from": task.status, "to": status})), at)?;
    get(tx, id)
}

/// A session's name in a refusal: the pane's key names no more; the page says its title elsewhere.
fn session_name(key: &str) -> String {
    format!("Session {}", key.rsplit(':').next().unwrap_or(key))
}

/// A card dragged within its column: above `before`, last with none. A card dropped among another
/// severity's stays its severity, on top of its own (AC-105).
pub fn reorder(tx: &Transaction, id: &str, before: Option<&str>, at: i64) -> Result<Task, String> {
    let task = get(tx, id)?;
    let key = key_of(tx, id)?;
    let neighbour = before.map(|b| get(tx, b)).transpose()?;
    let rank = match neighbour {
        Some(n) if n.severity == task.severity && n.status == task.status => {
            let (n_key, n_rank): (String, f64) = tx.query_row("SELECT id, rank FROM tasks WHERE number = ?1", [number_of(&n.id)?], |r| Ok((r.get(0)?, r.get(1)?))).map_err(sql)?;
            let above: Option<f64> = tx.query_row("SELECT max(rank) FROM tasks WHERE status = ?1 AND severity = ?2 AND archived_at IS NULL AND rank < ?3 AND id <> ?4 AND id <> ?5",
                                                  (&task.status, &task.severity, n_rank, &n_key, &key), |r| r.get(0)).map_err(sql)?;
            above.map_or(n_rank - 1.0, |a| (a + n_rank) / 2.0)
        }
        Some(_) => top_rank(tx, &task.status, &task.severity)?,
        None => {
            let max: Option<f64> = tx.query_row("SELECT max(rank) FROM tasks WHERE status = ?1 AND severity = ?2 AND archived_at IS NULL AND id <> ?3", (&task.status, &task.severity, &key), |r| r.get(0)).map_err(sql)?;
            max.map_or(0.0, |m| m + 1.0)
        }
    };
    tx.execute("UPDATE tasks SET rank = ?1, updated_at = ?2 WHERE id = ?3", (rank, at, &key)).map_err(sql)?;
    get(tx, id)
}

/// What a human may change in a task's window; absent keeps.
#[derive(Default)]
pub struct Edit {
    pub title: Option<String>,
    pub text: Option<String>,
    pub severity: Option<String>,
    pub owner: Option<String>,
}

pub fn edit(tx: &Transaction, author: &str, id: &str, change: Edit, at: i64) -> Result<Task, String> {
    let key = key_of(tx, id)?;
    let who = user_name(tx, author)?;
    if let Some(t) = change.title {
        let t = check_title(&t)?;
        tx.execute("UPDATE tasks SET title = ?1 WHERE id = ?2", (&t, &key)).map_err(sql)?;
        log(tx, &key, author, "edit", &format!("{who} renamed it"), None, at)?;
    }
    if let Some(text) = change.text {
        check_text(&text)?;
        tx.execute("UPDATE tasks SET body = ?1 WHERE id = ?2", (&text, &key)).map_err(sql)?;
        log(tx, &key, author, "edit", &format!("{who} changed its text"), None, at)?;
    }
    if let Some(s) = change.severity {
        check_severity(&s)?;
        let task = get(tx, id)?;
        if task.severity != s {
            let rank = top_rank(tx, &task.status, &s)?;
            tx.execute("UPDATE tasks SET severity = ?1, rank = ?2 WHERE id = ?3", (&s, rank, &key)).map_err(sql)?;
            log(tx, &key, author, "edit", &format!("{who} set severity {s}"), None, at)?;
        }
    }
    if let Some(o) = change.owner {
        let name = user_name(tx, &o)?;
        tx.execute("UPDATE tasks SET owner = ?1 WHERE id = ?2", (&o, &key)).map_err(sql)?;
        log(tx, &key, author, "edit", &format!("{who} gave it to {name}"), None, at)?;
    }
    touch(tx, &key, at)?;
    get(tx, id)
}

/// Links a session (AC-113), or unlinks with none.
pub fn link(tx: &Transaction, author: &str, id: &str, session: Option<&str>, at: i64) -> Result<Task, String> {
    let key = key_of(tx, id)?;
    let task = get(tx, id)?;
    if let (Some(s), "IN PROGRESS") = (session, task.status.as_str())
        && let Some(other) = in_progress_of(tx, s, &key)? {
            return Err(format!("{} already works on {other}. Finish or move {other} first.", session_name(s)));
        }
    tx.execute("UPDATE tasks SET session_key = ?1, updated_at = ?2 WHERE id = ?3", (session, at, &key)).map_err(sql)?;
    let who = user_name(tx, author)?;
    let said = if session.is_some() { format!("{who} linked a session") } else { format!("{who} unlinked its session") };
    log(tx, &key, author, "session", &said, None, at)?;
    get(tx, id)
}

/// Another project, its subtasks with it, in one transaction (AC-112, AC-113). A linked session is
/// the caller's to move or unlink first: `unlink` drops the link here.
pub fn set_project(tx: &Transaction, author: &str, id: &str, project: &str, unlink: bool, at: i64) -> Result<Task, String> {
    check_project(tx, project)?;
    let key = key_of(tx, id)?;
    let who = user_name(tx, author)?;
    let name: String = tx.query_row("SELECT name FROM projects WHERE id = ?1", [project], |r| r.get(0)).map_err(sql)?;
    if unlink {
        link(tx, author, id, None, at)?;
    }
    let kids: Vec<String> = tx.prepare("SELECT id FROM tasks WHERE parent = ?1").map_err(sql)?.query_map([&key], |r| r.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    let held: Option<String> = tx.query_row("SELECT 'T-' || number FROM tasks WHERE parent = ?1 AND session_key IS NOT NULL", [&key], |r| r.get(0)).optional().map_err(sql)?;
    if let Some(kid) = held {
        return Err(format!("{kid} has a session of its own: unlink it first."));
    }
    for k in kids.iter().chain(std::iter::once(&key)) {
        tx.execute("UPDATE tasks SET project = ?1, updated_at = ?2 WHERE id = ?3", (project, at, k)).map_err(sql)?;
        log(tx, k, author, "project", &format!("{who} moved it to {name}"), None, at)?;
    }
    get(tx, id)
}

/// Another open task of `session` than `id`, which stops the session following it (AC-113).
pub fn other_open_of_session(tx: &Transaction, session: &str, id: &str) -> Result<Option<String>, String> {
    let key = key_of(tx, id)?;
    tx.query_row("SELECT 'T-' || number FROM tasks WHERE session_key = ?1 AND id <> ?2 AND status <> 'DONE' AND archived_at IS NULL", (session, &key), |r| r.get(0))
        .optional().map_err(sql)
}

/// A comment (AC-108): the event's id.
pub fn comment(tx: &Transaction, author: &str, id: &str, text: &str, at: i64) -> Result<String, String> {
    if text.len() > COMMENT_MOST {
        return Err("Comment: at most 256 KiB".into());
    }
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
    if text.is_some_and(|t| t.len() > COMMENT_MOST) {
        return Err("Comment: at most 256 KiB".into());
    }
    let who = user_name(tx, author)?;
    match text {
        Some(t) => {
            tx.execute("UPDATE events SET text = ?1 WHERE id = ?2", (t, event)).map_err(sql)?;
            log(tx, &key, author, "edit", &format!("{who} changed a comment"), None, at)?;
        }
        None => {
            tx.execute("UPDATE events SET kind = 'deleted', text = '' WHERE id = ?1", [event]).map_err(sql)?;
            log(tx, &key, author, "edit", &format!("{who} deleted a comment"), None, at)?;
        }
    }
    touch(tx, &key, at)
}

/// A human wipes the task's text for good, a secret pasted by mistake (AC-109).
pub fn purge_text(tx: &Transaction, author: &str, id: &str, at: i64) -> Result<(), String> {
    let key = key_of(tx, id)?;
    tx.execute("UPDATE tasks SET body = '', updated_at = ?1 WHERE id = ?2", (at, &key)).map_err(sql)?;
    let who = user_name(tx, author)?;
    log(tx, &key, author, "edit", &format!("{who} purged its text"), None, at).map(|_| ())
}

/// A task's comments and history, oldest first, with its files by event.
pub fn history(tx: &Transaction, id: &str) -> Result<Value, String> {
    let key = key_of(tx, id)?;
    let mut files = tx.prepare("SELECT id, name, bytes, mime FROM files WHERE event = ?1").map_err(sql)?;
    let mut rows = tx.prepare("SELECT e.id, e.at, e.author, u.name, u.kind, e.kind, e.text FROM events e JOIN users u ON u.id = e.author WHERE e.task = ?1 ORDER BY e.at, e.rowid").map_err(sql)?;
    let mut out = Vec::new();
    let mut got = rows.query([&key]).map_err(sql)?;
    while let Some(r) = got.next().map_err(sql)? {
        let event: String = r.get(0).map_err(sql)?;
        let attached: Vec<Value> = files.query_map([&event], |f| Ok(json!({"id": f.get::<_, String>(0)?, "name": f.get::<_, String>(1)?, "bytes": f.get::<_, i64>(2)?, "mime": f.get::<_, String>(3)?})))
            .map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
        out.push(json!({"id": event, "at": r.get::<_, i64>(1).map_err(sql)?, "author": r.get::<_, String>(2).map_err(sql)?,
            "author_name": r.get::<_, String>(3).map_err(sql)?, "author_kind": r.get::<_, String>(4).map_err(sql)?,
            "kind": r.get::<_, String>(5).map_err(sql)?, "text": r.get::<_, Option<String>>(6).map_err(sql)?, "files": attached}));
    }
    Ok(Value::Array(out))
}

/// A title equal to an open task's in `project`, case, runs of spaces and end punctuation aside
/// (AC-115): that task.
pub fn repeat_of(tx: &Transaction, project: &str, title: &str) -> Result<Option<String>, String> {
    let wanted = plain(title);
    let mut st = tx.prepare("SELECT 'T-' || number, title FROM tasks WHERE project = ?1 AND status <> 'DONE' AND archived_at IS NULL").map_err(sql)?;
    let mut rows = st.query([project]).map_err(sql)?;
    while let Some(r) = rows.next().map_err(sql)? {
        if plain(&r.get::<_, String>(1).map_err(sql)?) == wanted {
            return Ok(Some(r.get(0).map_err(sql)?));
        }
    }
    Ok(None)
}

fn plain(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['.', '!', '?', ':', ';', ',']).to_lowercase()
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
