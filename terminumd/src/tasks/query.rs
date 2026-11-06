// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The board's cards under its filters (AC-105, AC-111).

use rusqlite::Transaction;
use serde_json::{json, Value};

use super::ops::{sql, task_of, Task, COLUMNS, TASK};

/// A page of the board's cards.
#[derive(Default, Clone)]
pub struct Query {
    pub projects: Vec<String>,
    pub severities: Vec<String>,
    pub statuses: Vec<String>,
    pub owner: Option<String>,
    pub session: Option<String>,
    pub text: Option<String>,
    pub with_subtasks: bool,
    pub after: i64,
    pub limit: i64,
}

/// `WHERE …` of what `q` asks, over `tasks t JOIN users u`, and its arguments.
fn filter_of(q: &Query) -> (String, Vec<String>) {
    let mut text = String::from(" WHERE t.archived_at IS NULL");
    let mut args: Vec<String> = Vec::new();
    for (column, values) in [("t.project", &q.projects), ("t.severity", &q.severities), ("t.status", &q.statuses)] {
        if !values.is_empty() {
            let marks = values.iter().map(|v| { args.push(v.clone()); format!("?{}", args.len()) }).collect::<Vec<_>>().join(", ");
            text.push_str(&format!(" AND {column} IN ({marks})"));
        }
    }
    if let Some(s) = &q.session {
        args.push(s.clone());
        text.push_str(&format!(" AND t.session_key = ?{}", args.len()));
    }
    match q.owner.as_deref() {
        Some("agents") => text.push_str(" AND u.kind = 'agent'"),
        Some("humans") => text.push_str(" AND u.kind = 'human'"),
        Some(o) => {
            args.push(o.to_string());
            text.push_str(&format!(" AND t.owner = ?{}", args.len()));
        }
        None => {}
    }
    if let Some(found) = q.text.as_deref().filter(|t| !t.is_empty()) {
        args.push(format!("%{}%", found.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
        let n = args.len();
        text.push_str(&format!(" AND (t.title LIKE ?{n} ESCAPE '\\' OR t.body LIKE ?{n} ESCAPE '\\' OR 'T-' || t.number LIKE ?{n} ESCAPE '\\')"));
    }
    if !q.with_subtasks {
        text.push_str(" AND t.parent IS NULL");
    }
    (text, args)
}

/// In severity order, then by hand. `limit` at most 200.
pub fn list(tx: &Transaction, q: &Query) -> Result<Vec<Task>, String> {
    let (filter, args) = filter_of(q);
    let limit = if q.limit <= 0 { 200 } else { q.limit.min(200) };
    let text = format!("{TASK}{filter} ORDER BY t.severity, t.rank LIMIT {limit} OFFSET {}", q.after.max(0));
    let mut st = tx.prepare(&text).map_err(sql)?;
    let rows = st.query_map(rusqlite::params_from_iter(args.iter()), task_of).map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

/// The count of each column under the filter (`q`'s `statuses` ignored), so a head says it.
pub fn counts(tx: &Transaction, q: &Query) -> Result<Value, String> {
    let mut out = serde_json::Map::new();
    for column in COLUMNS {
        let only = Query { statuses: vec![column.to_string()], ..q.clone() };
        let (filter, args) = filter_of(&only);
        let n: i64 = tx.query_row(&format!("SELECT count(*) FROM tasks t JOIN users u ON u.id = t.owner{filter}"), rusqlite::params_from_iter(args.iter()), |r| r.get(0)).map_err(sql)?;
        out.insert(column.to_string(), json!(n));
    }
    Ok(Value::Object(out))
}
