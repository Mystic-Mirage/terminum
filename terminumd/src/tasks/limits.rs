// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A project's memory limits (`terminum-dispatcher.md` AC-128) and how many agents it runs at once (AC-141).

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::put;
use axum::{Json, Router};
use rusqlite::Transaction;
use serde_json::{json, Value};

use super::api::{store, told_projects};
use super::ops::{check_project, rows, sql};
use super::agents::MOST_AT_ONCE;
use crate::app::Shared;

/// What a project's Agents at once says when it is no number of 1 to MOST_AT_ONCE.
const AGENTS_AT_ONCE: &str = "Agents at once is 1 to 100, or empty.";

pub fn routes() -> Router<Shared> {
    Router::new().route("/api/task-projects/{id}/limits", put(set_limits)).route("/api/task-projects/{id}/agents-at-once", put(set_agent_limit))
}

/// A project's memory limits, or none to go by the global ones (terminum-dispatcher.md AC-128); 0: off.
fn keep_limits(tx: &Transaction, project: &str, limits: Option<(f64, f64)>) -> Result<(), String> {
    check_project(tx, project)?;
    match limits {
        None => tx.execute("DELETE FROM project_limits WHERE project = ?1", [project]).map_err(sql)?,
        Some((warn, end)) => {
            if warn < 0.0 || end < 0.0 {
                return Err("A limit is a number of GiB, 0 for off.".to_string());
            }
            if end > 0.0 && warn > end {
                return Err("The warning is above the end.".to_string());
            }
            tx.execute("INSERT INTO project_limits (project, warn_gib, end_gib) VALUES (?1, ?2, ?3) ON CONFLICT (project) DO UPDATE SET warn_gib = ?2, end_gib = ?3", (project, warn, end)).map_err(sql)?
        }
    };
    Ok(())
}

/// Every project's limits: (project, warn, end).
pub fn limits(tx: &Transaction) -> Result<Vec<(String, f64, f64)>, String> {
    rows(tx, "SELECT project, warn_gib, end_gib FROM project_limits", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
}

async fn set_limits(State(app): State<Shared>, Path(project): Path<String>, Json(body): Json<Value>) -> Response {
    let wanted = match (&body["warn"], &body["end"]) {
        (Value::Null, Value::Null) => Ok(None),
        (w, e) => w.as_f64().zip(e.as_f64()).map(Some).ok_or_else(|| "A limit is a number of GiB, 0 for off.".to_string()),
    };
    let done = match wanted {
        Ok(limits) => store(&app, move |tx| keep_limits(tx, &project, limits).map(|()| json!({"ok": true}))).await,
        Err(e) => Err(e),
    };
    told_projects(&app, done)
}

/// How many agents a project runs at once; none: no limit.
fn keep_agent_limit(tx: &Transaction, project: &str, limit: Option<i64>) -> Result<(), String> {
    check_project(tx, project)?;
    if limit.is_some_and(|n| !(1..=MOST_AT_ONCE).contains(&n)) {
        return Err(AGENTS_AT_ONCE.to_string());
    }
    tx.execute("UPDATE projects SET agent_limit = ?2 WHERE id = ?1", (project, limit)).map(drop).map_err(sql)
}

/// One project's: how many agents it runs at once; none: no limit.
pub fn agent_limit(tx: &Transaction, project: &str) -> Result<Option<i64>, String> {
    tx.query_row("SELECT agent_limit FROM projects WHERE id = ?1", [project], |r| r.get(0)).map_err(sql)
}

/// Every project's that has one.
pub fn agent_limits(tx: &Transaction) -> Result<std::collections::HashMap<String, i64>, String> {
    Ok(rows(tx, "SELECT id, agent_limit FROM projects WHERE agent_limit IS NOT NULL", [], |r| Ok((r.get(0)?, r.get(1)?)))?.into_iter().collect())
}

async fn set_agent_limit(State(app): State<Shared>, Path(project): Path<String>, Json(body): Json<Value>) -> Response {
    let wanted = match &body["limit"] {
        Value::Null => Ok(None),
        n => n.as_i64().map(Some).ok_or_else(|| AGENTS_AT_ONCE.to_string()),
    };
    let done = match wanted {
        Ok(limit) => store(&app, move |tx| keep_agent_limit(tx, &project, limit).map(|()| json!({"ok": true}))).await,
        Err(e) => Err(e),
    };
    told_projects(&app, done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::store::{Store, DEFAULT_PROJECT};

    #[test]
    fn ac141_a_project_s_agents_at_once_are_checked() {
        let s = Store::open(crate::testutil::fresh_dir("limits-agents").join("kanban.sqlite"));
        s.with(|tx| keep_agent_limit(tx, DEFAULT_PROJECT, Some(3))).unwrap();
        assert_eq!(s.with(agent_limits).unwrap()[DEFAULT_PROJECT], 3);
        for bad in [0, 101] {
            assert_eq!(s.with(|tx| keep_agent_limit(tx, DEFAULT_PROJECT, Some(bad))).unwrap_err(), "Agents at once is 1 to 100, or empty.");
        }
        s.with(|tx| keep_agent_limit(tx, DEFAULT_PROJECT, None)).unwrap();
        assert!(s.with(agent_limits).unwrap().is_empty(), "no limit");
        assert_eq!(s.with(|tx| keep_agent_limit(tx, "gone", None)).unwrap_err(), "That project is gone.");
    }

    #[test]
    fn ac128_a_project_s_limits_are_checked_and_go_with_it() {
        let s = Store::open(crate::testutil::fresh_dir("agents-limits").join("kanban.sqlite"));
        s.with(|tx| {
            tx.execute("INSERT INTO projects (id, name, color, dir, place) VALUES ('p1', 'api', 0, '/srv/api', 0)", []).map_err(sql)?;
            keep_limits(tx, "p1", Some((6.0, 12.0)))
        }).unwrap();
        assert_eq!(s.with(limits).unwrap()[0].2, 12.0);
        assert_eq!(s.with(|tx| keep_limits(tx, "p1", Some((9.0, 3.0)))).unwrap_err(), "The warning is above the end.");
        assert_eq!(s.with(|tx| keep_limits(tx, "p1", Some((-1.0, 3.0)))).unwrap_err(), "A limit is a number of GiB, 0 for off.");
        s.with(|tx| keep_limits(tx, "p1", Some((9.0, 0.0)))).unwrap();
        s.with(|tx| keep_limits(tx, "p1", None)).unwrap();
        assert!(s.with(limits).unwrap().is_empty(), "back to the global ones");
        assert_eq!(s.with(|tx| keep_limits(tx, "gone", None)).unwrap_err(), "That project is gone.");
    }
}
