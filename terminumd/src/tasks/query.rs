// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The board's cards under its filters (AC-105, AC-111).

use rusqlite::Transaction;
use serde_json::{json, Value};

use super::ops::{rows, sql, task_of, Task, COLUMNS, TASK};

/// A page of the board's cards.
#[derive(Default, Clone)]
pub struct Query {
    pub projects: Vec<String>,
    pub severities: Vec<String>,
    pub statuses: Vec<String>,
    pub owner: Option<String>,
    /// one task: whether the board's filters show it
    pub number: Option<i64>,
    /// an agent's reach: tasks of this pane (linked or made), or of this project
    pub reach: Option<(String, Option<String>)>,
    pub text: Option<String>,
    /// the tasks made in this pane: by its agent, or of its records (AC-150)
    pub made_in: Option<String>,
    pub with_subtasks: bool,
    /// the archived ones instead of the board's (AC-118)
    pub archived: bool,
    pub after: i64,
    pub limit: i64,
}

/// `text` for a LIKE with `ESCAPE '\\'`: found as it is.
pub(super) fn like_escaped(text: &str) -> String {
    text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// An agent's reach, over `tasks t`: tasks of this pane, linked or made there, and of its project;
/// never by a session's title, which two sessions can share (AC-110, AC-115).
fn reach_sql(pane: &str, project: Option<&str>, args: &mut Vec<String>) -> String {
    let n = bind(args, pane);
    let mut any = format!("t.session_key = {n} OR t.made_in = {n}");
    if let Some(p) = project {
        any.push_str(&format!(" OR t.project = {}", bind(args, p)));
    }
    any
}

/// Whether task number `number` is within that reach.
pub fn within_reach(tx: &Transaction, number: i64, pane: &str, project: Option<&str>) -> Result<bool, String> {
    let mut args = vec![number.to_string()];
    let reach = reach_sql(pane, project, &mut args);
    tx.query_row(&format!("SELECT count(*) FROM tasks t WHERE t.number = ?1 AND ({reach})"), rusqlite::params_from_iter(args.iter()), |r| r.get::<_, i64>(0)).map(|n| n > 0).map_err(sql)
}

/// `v` as the next argument: its mark in the statement.
fn bind(args: &mut Vec<String>, v: impl Into<String>) -> String {
    args.push(v.into());
    format!("?{}", args.len())
}

/// `WHERE …` of what `q` asks, over `tasks t JOIN users u`, and its arguments.
fn filter_of(q: &Query) -> (String, Vec<String>) {
    let mut text = String::from(if q.archived { " WHERE t.archived_at IS NOT NULL" } else { " WHERE t.archived_at IS NULL" });
    let mut args: Vec<String> = Vec::new();
    for (column, values) in [("t.project", &q.projects), ("t.severity", &q.severities), ("t.status", &q.statuses)] {
        if !values.is_empty() {
            let marks = values.iter().map(|v| bind(&mut args, v)).collect::<Vec<_>>().join(", ");
            text.push_str(&format!(" AND {column} IN ({marks})"));
        }
    }
    if let Some(n) = q.number {
        text.push_str(&format!(" AND t.number = {}", bind(&mut args, n.to_string())));
    }
    if let Some((pane, project)) = &q.reach {
        let any = reach_sql(pane, project.as_deref(), &mut args);
        text.push_str(&format!(" AND ({any})"));
    }
    match q.owner.as_deref() {
        Some("agents") => text.push_str(" AND u.kind = 'agent'"),
        Some("humans") => text.push_str(" AND u.kind = 'human'"),
        Some(o) => text.push_str(&format!(" AND t.owner = {}", bind(&mut args, o))),
        None => {}
    }
    if let Some(found) = q.text.as_deref().filter(|t| !t.is_empty()) {
        let n = bind(&mut args, format!("%{}%", like_escaped(found)));
        text.push_str(&format!(" AND (t.title LIKE {n} ESCAPE '\\' OR t.body LIKE {n} ESCAPE '\\' OR t.slug LIKE {n} ESCAPE '\\')"));
    }
    if let Some(pane) = &q.made_in {
        text.push_str(&format!(" AND t.made_in = {}", bind(&mut args, pane.as_str())));
    }
    if !q.with_subtasks {
        text.push_str(" AND t.parent IS NULL");
    }
    (text, args)
}

/// The most a list holds.
pub(crate) const LIST_MOST: i64 = 200;

/// In severity order, then by hand. `limit` at most LIST_MOST.
pub fn list(tx: &Transaction, q: &Query) -> Result<Vec<Task>, String> {
    let (filter, args) = filter_of(q);
    let limit = if q.limit <= 0 { LIST_MOST } else { q.limit.min(LIST_MOST) };
    let order = if q.archived { "t.archived_at DESC" } else { "t.severity, t.rank" };   // Archived: newest first (AC-118)
    rows(tx, &format!("{TASK}{filter} ORDER BY {order} LIMIT {limit} OFFSET {}", q.after.max(0)), rusqlite::params_from_iter(args.iter()), task_of)
}

/// Those of a parent, in the order they were made (AC-112).
pub fn subtasks(tx: &Transaction, parent: &str) -> Result<Vec<Task>, String> {
    rows(tx, &format!("{TASK} WHERE t.parent = ?1 ORDER BY t.number"), [parent], task_of)
}

/// The count of each column under the filter (`q`'s `statuses` ignored), so a head says it.
pub fn counts(tx: &Transaction, q: &Query) -> Result<Value, String> {
    let (filter, args) = filter_of(&Query { statuses: Vec::new(), ..q.clone() });
    let found: Vec<(String, i64)> = rows(tx, &format!("SELECT t.status, count(*) FROM tasks t JOIN users u ON u.id = t.owner{filter} GROUP BY t.status"), rusqlite::params_from_iter(args.iter()), |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out: serde_json::Map<String, Value> = COLUMNS.iter().map(|c| (c.to_string(), json!(0))).collect();
    out.extend(found.into_iter().map(|(status, n)| (status, json!(n))));
    Ok(Value::Object(out))
}

/// A title equal to an open task's in `project`, case, runs of spaces and end punctuation aside
/// (AC-115): that task.
pub fn repeat_of(tx: &Transaction, project: &str, title: &str) -> Result<Option<String>, String> {
    let wanted = plain(title);
    let open: Vec<(String, String)> = rows(tx, "SELECT slug, title FROM tasks WHERE project = ?1 AND status <> 'DONE' AND archived_at IS NULL", [project], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(open.into_iter().find(|(_, title)| plain(title) == wanted).map(|(number, _)| number))
}

fn plain(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['.', '!', '?', ':', ';', ',']).to_lowercase()
}
