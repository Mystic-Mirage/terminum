// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a task can do, each in the caller's transaction, its history in the same one (AC-106,
//! AC-107, AC-108, AC-113). The rules are here for humans and agents alike; who may ask is the
//! caller's (`tools.rs` for agents).

use rusqlite::{OptionalExtension, Params, Row, Transaction};
use serde::Serialize;
use serde_json::{json, Value};

use super::store::{DEFAULT_PROJECT, HUMAN};

pub const COLUMNS: [&str; 5] = ["TODO", "IN PROGRESS", "WAIT FOR HUMAN", "FAILED", "DONE"];
pub const SEVERITIES: [&str; 4] = ["P0", "P1", "P2", "P3"];
pub(crate) const TITLE_MOST: usize = 200;
pub const TEXT_MOST: usize = 1 << 20;
pub const COMMENT_MOST: usize = 256 << 10;
/// Open tasks by agents in one project (AC-115).
pub const AGENTS_OPEN_MOST: i64 = 200;

pub fn sql(e: rusqlite::Error) -> String {
    super::store::message(&e)
}

/// Every row `query` finds.
pub(crate) fn rows<T>(tx: &Transaction, query: &str, params: impl Params, f: impl FnMut(&Row) -> rusqlite::Result<T>) -> Result<Vec<T>, String> {
    tx.prepare_cached(query).map_err(sql)?.query_map(params, f).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)
}

pub use super::users::{agent_user, pool_user, pool_user_id};
pub use super::comments::{change_comment, comment, history, last_comment_is};
pub(crate) use super::users::user_name;

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
    pub files: i64,
    pub subtasks_done: i64,
    pub subtasks: i64,
    /// the time a RUN's agent has for it (terminum-dispatcher.md AC-126)
    pub budget_seconds: Option<i64>,
    /// off the board since then (AC-118)
    pub archived_at: Option<i64>,
    /// its key's start: what names it whatever its ID becomes (AC-148)
    pub uid: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// the row's key and place in its column: the rules', never shown
    #[serde(skip)]
    pub(crate) key: String,
    #[serde(skip)]
    rank: f64,
}

pub(super) const TASK: &str = "SELECT t.slug, t.project, (SELECT p.slug FROM tasks p WHERE p.id = t.parent), t.title, t.status, t.severity,
    t.owner, u.name, u.kind, t.by_agent, t.session_key,
    (SELECT count(*) FROM files f WHERE f.task = t.id),
    (SELECT count(*) FROM tasks s WHERE s.parent = t.id AND s.status = 'DONE' AND s.archived_at IS NULL),
    (SELECT count(*) FROM tasks s WHERE s.parent = t.id AND s.archived_at IS NULL), t.id, t.rank, t.budget_seconds, t.archived_at, t.created_at, t.updated_at
    FROM tasks t JOIN users u ON u.id = t.owner";

pub(super) fn task_of(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
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
        budget_seconds: r.get(16)?, archived_at: r.get(17)?,
        uid: r.get::<_, String>(14)?.chars().take(super::slugs::UID_LEN).collect(),
        created_at: r.get(18)?,
        updated_at: r.get(19)?,
        key: r.get(14)?,
        rank: r.get(15)?,
    })
}

/// A task's number, the store's order, from its ID, one it had or its key's start (AC-148).
pub fn number_of(tx: &Transaction, id: &str) -> Result<i64, String> {
    super::slugs::resolve(tx, id)?.ok_or_else(|| format!("No task {id}"))
}

/// What `query` reads of task `id`.
fn of_task<T>(tx: &Transaction, id: &str, query: &str, f: impl FnOnce(&Row) -> rusqlite::Result<T>) -> Result<T, String> {
    tx.query_row(query, [number_of(tx, id)?], f).optional().map_err(sql)?.ok_or_else(|| format!("No task {id}"))
}

/// The row's key of task `id`.
pub(crate) fn key_of(tx: &Transaction, id: &str) -> Result<String, String> {
    of_task(tx, id, "SELECT id FROM tasks WHERE number = ?1", |r| r.get(0))
}

pub fn get(tx: &Transaction, id: &str) -> Result<Task, String> {
    of_task(tx, id, &format!("{TASK} WHERE t.number = ?1"), task_of)
}

pub fn text_of(tx: &Transaction, id: &str) -> Result<String, String> {
    of_task(tx, id, "SELECT body FROM tasks WHERE number = ?1", |r| r.get(0))
}

/// One line of a task's history; a comment is one too. Its id.
pub fn log(tx: &Transaction, task_key: &str, author: &str, kind: &str, text: &str, data: Option<&Value>, at: i64) -> Result<String, String> {
    let id = crate::auth::crypto::random_hex::<16>();
    tx.execute("INSERT INTO events (id, task, at, author, kind, text, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
               (&id, task_key, at, author, kind, text, data.map(Value::to_string))).map_err(sql)?;
    Ok(id)
}

pub(super) fn touch(tx: &Transaction, key: &str, at: i64) -> Result<(), String> {
    tx.execute("UPDATE tasks SET updated_at = ?1 WHERE id = ?2", (at, key)).map(|_| ()).map_err(sql)
}

/// The rank that stands above every card of a column's severity: new tasks and moves go on top.
fn top_rank(tx: &Transaction, status: &str, severity: &str) -> Result<f64, String> {
    let min: Option<f64> = tx.query_row("SELECT min(rank) FROM tasks WHERE status = ?1 AND severity = ?2 AND archived_at IS NULL", (status, severity), |r| r.get(0)).map_err(sql)?;
    Ok(min.map_or(0.0, |m| m - 1.0))
}

/// A title as kept: nothing that hides or reorders it, whoever writes it.
fn check_title(title: &str) -> Result<String, String> {
    let cleaned = crate::termtext::clean(title, false);
    let t = cleaned.trim();
    if t.is_empty() || t.chars().count() > TITLE_MOST {
        return Err(format!("Title: 1–{TITLE_MOST} characters"));
    }
    Ok(t.to_string())
}

fn check_text(text: &str) -> Result<(), String> {
    if text.len() > TEXT_MOST { Err("Text: at most 1 MiB".into()) } else { Ok(()) }
}

pub(super) fn check_comment(text: &str) -> Result<(), String> {
    if text.len() > COMMENT_MOST { Err("Comment: at most 256 KiB".into()) } else { Ok(()) }
}

fn check_severity(s: &str) -> Result<(), String> {
    if SEVERITIES.contains(&s) { Ok(()) } else { Err(format!("Severity: one of {}", SEVERITIES.join(", "))) }
}

pub(super) fn check_project(tx: &Transaction, project: &str) -> Result<(), String> {
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
            Some((up.key, up.project))
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
    tx.execute("INSERT INTO tasks (id, number, project, parent, title, body, status, severity, rank, owner, by_agent, session_key, made_in, updated_at, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'TODO', ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
               rusqlite::params![key, number, project, parent.as_ref().map(|(k, _)| k), title, new.text, severity, rank, owner, new.by_agent as i64, new.session, new.made_in, at]).map_err(sql)?;
    let slug = super::slugs::assign(tx, &key, &project)?;
    log(tx, &key, author, "made", &format!("{} made it", user_name(tx, author)?), None, at)?;
    get(tx, &slug)
}

/// A session works on one task at a time (AC-113), but for those one run gave it together (terminum-dispatcher.md
/// AC-138): refused while another than `but` stands in IN PROGRESS.
fn check_free(tx: &Transaction, session: &str, but: &str) -> Result<(), String> {
    let still = crate::dispatcher::runs::STILL;
    let other: Option<String> = tx.query_row(&format!("SELECT slug FROM tasks WHERE session_key = ?1 AND status = 'IN PROGRESS' AND id <> ?2 AND archived_at IS NULL
         AND NOT EXISTS (SELECT 1 FROM run_tasks a JOIN run_tasks b ON a.run = b.run JOIN runs r ON r.id = a.run WHERE a.task = tasks.id AND b.task = ?2 AND {still})"), (session, but), |r| r.get(0))
        .optional().map_err(sql)?;
    match other {
        Some(other) => Err(format!("Session {} already works on {other}. Finish or move {other} first.", pane_of(session))),
        None => Ok(()),
    }
}

/// Moves a task to a column, on top of its severity's cards; its history says who did it.
pub fn move_to(tx: &Transaction, author: &str, id: &str, status: &str, at: i64) -> Result<(), String> {
    if !COLUMNS.contains(&status) {
        return Err(format!("Status: one of {}", COLUMNS.join(", ")));
    }
    let task = get(tx, id)?;
    if task.status == status {
        return Ok(());
    }
    // Dispatcher's own move follows its run: another's task in that session is that task's matter
    if status == "IN PROGRESS" && author != super::store::DISPATCHER
        && let Some(session) = &task.session {
            check_free(tx, session, &task.key)?;
        }
    let rank = top_rank(tx, status, &task.severity)?;
    tx.execute("UPDATE tasks SET status = ?1, rank = ?2, updated_at = ?3 WHERE id = ?4", (status, rank, at, &task.key)).map_err(sql)?;
    let who = user_name(tx, author)?;
    log(tx, &task.key, author, "move", &format!("{who} moved it from {} to {status}", task.status), Some(&json!({"from": task.status, "to": status})), at).map(|_| ())
}

/// A session in a refusal: its pane; the page says its title elsewhere.
pub(super) fn pane_of(key: &str) -> &str {
    key.rsplit(':').next().unwrap_or(key)
}

/// A card dragged within its column: above `before`, last with none. A card dropped among another
/// severity's stays its severity, on top of its own (AC-105).
pub fn reorder(tx: &Transaction, id: &str, before: Option<&str>, at: i64) -> Result<(), String> {
    let task = get(tx, id)?;
    let key = &task.key;
    let neighbour = before.map(|b| get(tx, b)).transpose()?;
    let rank = match neighbour {
        Some(n) if n.severity == task.severity && n.status == task.status => {
            let above: Option<f64> = tx.query_row("SELECT max(rank) FROM tasks WHERE status = ?1 AND severity = ?2 AND archived_at IS NULL AND rank < ?3 AND id <> ?4 AND id <> ?5",
                                                  (&task.status, &task.severity, n.rank, &n.key, key), |r| r.get(0)).map_err(sql)?;
            above.map_or(n.rank - 1.0, |a| (a + n.rank) / 2.0)
        }
        Some(_) => top_rank(tx, &task.status, &task.severity)?,
        None => {
            let max: Option<f64> = tx.query_row("SELECT max(rank) FROM tasks WHERE status = ?1 AND severity = ?2 AND archived_at IS NULL AND id <> ?3", (&task.status, &task.severity, key), |r| r.get(0)).map_err(sql)?;
            max.map_or(0.0, |m| m + 1.0)
        }
    };
    tx.execute("UPDATE tasks SET rank = ?1, updated_at = ?2 WHERE id = ?3", (rank, at, key)).map(|_| ()).map_err(sql)
}

/// A task's time budget (AC-126): 5 minutes to 7 days; none takes it off.
pub fn set_budget(tx: &Transaction, author: &str, id: &str, minutes: Option<i64>, at: i64) -> Result<(), String> {
    if minutes.is_some_and(|m| !(5..=7 * 24 * 60).contains(&m)) {
        return Err("Budget: 5 minutes to 7 days".to_string());
    }
    let key = key_of(tx, id)?;
    tx.execute("UPDATE tasks SET budget_seconds = ?1, updated_at = ?2 WHERE id = ?3", (minutes.map(|m| m * 60), at, &key)).map_err(sql)?;
    let who = user_name(tx, author)?;
    let said = minutes.map_or(format!("{who} took its budget off"), |m| format!("{who} set its budget to {m} minutes"));
    log(tx, &key, author, "edit", &said, None, at).map(|_| ())
}

/// Where a project's agents start: its name, server and folder (terminum-dispatcher.md AC-140).
pub(crate) fn project_place(tx: &Transaction, project: &str) -> Result<(String, Option<String>, String), String> {
    tx.query_row("SELECT name, host, dir FROM projects WHERE id = ?1", [project], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(sql)
}

/// Its owner, with no line in its history: Dispatcher's run says who took it.
pub(crate) fn set_owner(tx: &Transaction, key: &str, owner: &str, at: i64) -> Result<(), String> {
    tx.execute("UPDATE tasks SET owner = ?1, updated_at = ?2 WHERE id = ?3", (owner, at, key)).map(|_| ()).map_err(sql)
}

/// What a human may change in a task's window; absent keeps.
#[derive(Default)]
pub struct Edit {
    pub title: Option<String>,
    pub text: Option<String>,
    pub severity: Option<String>,
    pub owner: Option<String>,
}

pub fn edit(tx: &Transaction, author: &str, id: &str, change: Edit, at: i64) -> Result<(), String> {
    if change.title.is_none() && change.text.is_none() && change.severity.is_none() && change.owner.is_none() {
        return Ok(());
    }
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
    touch(tx, &key, at)
}

/// Links a session (AC-113), or unlinks with none.
pub fn link(tx: &Transaction, author: &str, id: &str, session: Option<&str>, at: i64) -> Result<(), String> {
    let task = get(tx, id)?;
    let key = &task.key;
    if let (Some(s), "IN PROGRESS") = (session, task.status.as_str()) {
        check_free(tx, s, key)?;
    }
    tx.execute("UPDATE tasks SET session_key = ?1, updated_at = ?2 WHERE id = ?3", (session, at, key)).map_err(sql)?;
    let who = user_name(tx, author)?;
    let said = if session.is_some() { format!("{who} linked a session") } else { format!("{who} unlinked its session") };
    log(tx, key, author, "session", &said, None, at).map(|_| ())
}

/// What became of a move to another project.
#[derive(Debug, PartialEq)]
pub enum ProjectMove {
    Done,
    /// the task moved, and this session goes where it went
    SessionFollows(String),
    /// it has a session: nothing moved until the human says what becomes of that
    Ask,
}

/// Takes a task off the board, or brings it back to where it stood (AC-118).
pub fn archive(tx: &Transaction, author: &str, id: &str, on: bool, at: i64) -> Result<(), String> {
    let key = key_of(tx, id)?;
    tx.execute("UPDATE tasks SET archived_at = CASE WHEN ?1 THEN ?2 END, updated_at = ?2 WHERE id = ?3", (on, at, &key)).map_err(sql)?;
    let who = user_name(tx, author)?;
    log(tx, &key, author, "archive", &if on { format!("Archived by {who}") } else { format!("{who} brought it back") }, None, at).map(|_| ())
}

/// A human moves a task to `project` (AC-113): a session goes with it (`choice` "move") unless it
/// holds another open task, or is unlinked (any other choice); with no choice, that is asked first.
pub fn change_project(tx: &Transaction, author: &str, id: &str, project: &str, choice: Option<&str>, at: i64) -> Result<ProjectMove, String> {
    let task = get(tx, id)?;
    if task.project == project {
        return Ok(ProjectMove::Done);
    }
    let Some(session) = task.session else {
        set_project(tx, author, id, project, at)?;
        return Ok(ProjectMove::Done);
    };
    match choice {
        None => Ok(ProjectMove::Ask),
        Some("move") => {
            let other: Option<String> = tx.query_row("SELECT slug FROM tasks WHERE session_key = ?1 AND id <> ?2 AND status <> 'DONE' AND archived_at IS NULL", (&session, &task.key), |r| r.get(0))
                .optional().map_err(sql)?;
            if let Some(other) = other {
                return Err(format!("This session also holds {other}: unlink it, or move {other} first."));
            }
            set_project(tx, author, id, project, at)?;
            Ok(ProjectMove::SessionFollows(session))
        }
        Some(_) => {
            link(tx, author, id, None, at)?;
            set_project(tx, author, id, project, at)?;
            Ok(ProjectMove::Done)
        }
    }
}

/// Another project, its subtasks with it, in one transaction (AC-112).
fn set_project(tx: &Transaction, author: &str, id: &str, project: &str, at: i64) -> Result<(), String> {
    check_project(tx, project)?;
    let key = key_of(tx, id)?;
    let who = user_name(tx, author)?;
    let (name, _, _) = project_place(tx, project)?;
    let kids: Vec<String> = rows(tx, "SELECT id FROM tasks WHERE parent = ?1", [&key], |r| r.get(0))?;
    let held: Option<String> = tx.query_row("SELECT slug FROM tasks WHERE parent = ?1 AND session_key IS NOT NULL", [&key], |r| r.get(0)).optional().map_err(sql)?;
    if let Some(kid) = held {
        return Err(format!("{kid} has a session of its own: unlink it first."));
    }
    for k in std::iter::once(&key).chain(kids.iter()) {
        tx.execute("UPDATE tasks SET project = ?1, updated_at = ?2 WHERE id = ?3", (project, at, k)).map_err(sql)?;
        // it takes the project's next ID; the one it had still finds it (AC-148)
        let slug = super::slugs::assign(tx, k, project)?;
        log(tx, k, author, "project", &format!("{who} moved it to {name}, as {slug}"), None, at)?;
    }
    Ok(())
}

/// A human wipes the task's text for good, a secret pasted by mistake (AC-109).
pub fn purge_text(tx: &Transaction, author: &str, id: &str, at: i64) -> Result<(), String> {
    let key = key_of(tx, id)?;
    tx.execute("UPDATE tasks SET body = '', updated_at = ?1 WHERE id = ?2", (at, &key)).map_err(sql)?;
    let who = user_name(tx, author)?;
    log(tx, &key, author, "edit", &format!("{who} purged its text"), None, at).map(|_| ())
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
