// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A project's agents (`terminum-dispatcher.md` AC-137): what RUN can start. Only a signed-in page
//! changes them, never `terminum mcp`; a command is the human's own words, kept as written.

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{patch, post, put};
use axum::{Json, Router};
use rusqlite::{OptionalExtension, Transaction};
use serde::Serialize;
use serde_json::{json, Value};

use super::api::{answer, store, told_projects};
use super::ops::{check_project, rows, sql};
use crate::app::Shared;
use crate::termtext::{has_control, stray};
use crate::tmux::hookstate::AGENTS;

const MOST_NAME: usize = 40;
const MOST_LINE: usize = 1000;

#[derive(Clone, Serialize)]
pub struct Agent {
    pub id: String,
    pub project: String,
    pub name: String,
    pub kind: String,
    pub host: Option<String>,
    pub dir: String,
    pub command: String,
    pub effort: String,
    /// its place in the palette (listparts.js): none of a project's two share one while another is free
    pub color: i64,
    /// what starts it: its command with its effort
    pub line: String,
}

/// The effort a kind takes, as its command line names it; empty goes by the agent's own setting.
pub const EFFORTS: [(&str, &[&str]); 2] = [("claude", &["low", "medium", "high", "xhigh", "max"]), ("codex", &["low", "medium", "high", "xhigh"])];

fn command_line(kind: &str, command: &str, effort: &str) -> String {
    match (kind, effort) {
        (_, "") => command.to_string(),
        ("codex", e) => format!("{command} -c model_reasoning_effort={e}"),
        (_, e) => format!("{command} --effort {e}"),
    }
}

/// What a page keeps, trimmed.
struct Given {
    name: String,
    kind: String,
    host: Option<String>,
    dir: String,
    command: String,
    effort: String,
}

impl Given {
    fn of(body: &Value) -> Given {
        let text = |k: &str| body[k].as_str().unwrap_or_default().trim().to_string();
        Given { name: text("name"), kind: text("kind"), host: Some(text("host")).filter(|h| !h.is_empty()), dir: text("dir"), command: text("command"), effort: text("effort") }
    }

    /// What needs no store: refused with the words the window shows.
    fn check(&self, hosts: &[String]) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("An agent needs a name.".into());
        }
        if self.name.chars().count() > MOST_NAME {
            return Err(format!("An agent's name is {MOST_NAME} characters at most."));
        }
        if self.name.chars().any(stray) {
            return Err("An agent's name is one line, with no control or invisible characters.".into());
        }
        if !AGENTS.contains(&self.kind.as_str()) {
            return Err("An agent is Claude Code or Codex.".into());
        }
        if self.command.is_empty() {
            return Err("An agent needs a command.".into());
        }
        if self.command.chars().count() > MOST_LINE {
            return Err(format!("An agent's command is {MOST_LINE} characters at most."));
        }
        if has_control(&self.command) {
            return Err("An agent's command is one line, with no control or invisible characters.".into());
        }
        if !self.effort.is_empty() {
            if !EFFORTS.iter().any(|(kind, levels)| *kind == self.kind && levels.contains(&self.effort.as_str())) {
                return Err(format!("{} has no effort {}.", self.kind, self.effort));
            }
            // the flag goes after the command: after a `;` it would go to another program
            if self.command.contains([';', '|', '&', '#']) {
                return Err("An effort goes on one program's command: this one has ; | & or #. Write it in the command.".into());
            }
        }
        check_folder(&self.dir)?;
        if let Some(host) = self.host.as_ref().filter(|h| !hosts.contains(h)) {
            return Err(format!("The server {host} is not one of this terminum's hosts."));
        }
        Ok(())
    }

    /// What its project decides: a name once in it; "the project's folder" is on the project's server.
    fn fits(&self, tx: &Transaction, project: &str, but: &str) -> Result<(), String> {
        let (name, host): (String, Option<String>) = tx.query_row("SELECT name, host FROM projects WHERE id = ?1", [project], |r| Ok((r.get(0)?, r.get(1)?))).map_err(sql)?;
        let taken: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM project_agents WHERE project = ?1 AND name = ?2 AND id != ?3)", (project, &self.name, but), |r| r.get(0)).map_err(sql)?;
        if taken {
            return Err(format!("{name} already has an agent named {}.", self.name));
        }
        if self.dir.is_empty() && self.host != host {
            return Err(format!("{name}'s folder is on {}: an agent on another server needs its own folder.", host.as_deref().unwrap_or("This Mac")));
        }
        Ok(())
    }
}

/// `dir` as a run or an agent may have it (AC-121, AC-137).
pub fn check_folder(dir: &str) -> Result<(), String> {
    if !dir.is_empty() && !(dir.starts_with('/') || dir.starts_with('~')) {
        return Err("A folder starts with / or ~.".into());
    }
    if dir.chars().count() > MOST_LINE || has_control(dir) {
        return Err(format!("A folder is one line of {MOST_LINE} characters at most, with no control or invisible characters."));
    }
    Ok(())
}

/// Every project's, as they were added.
pub fn list(tx: &Transaction) -> Result<Vec<Agent>, String> {
    rows(tx, "SELECT id, project, name, kind, host, dir, command, effort, color FROM project_agents ORDER BY rowid", [], |r| {
        let (kind, command, effort): (String, String, String) = (r.get(3)?, r.get(6)?, r.get(7)?);
        Ok(Agent { id: r.get(0)?, project: r.get(1)?, name: r.get(2)?, line: command_line(&kind, &command, &effort), kind, host: r.get(4)?, dir: r.get(5)?, command, effort, color: r.get(8)? })
    })
}

/// One project's.
pub fn of_project(tx: &Transaction, project: &str) -> Result<Vec<Agent>, String> {
    Ok(list(tx)?.into_iter().filter(|a| a.project == project).collect())
}

/// Keeps a new one. Its id.
fn add(tx: &Transaction, project: &str, given: &Given) -> Result<String, String> {
    check_project(tx, project)?;
    given.fits(tx, project, "")?;
    let id = crate::auth::crypto::random_hex::<8>();
    // the next color after the project's last: agents far apart, and no two alike for a palette's length
    let color: i64 = tx.query_row("SELECT COALESCE(MAX(color) + 1, 0) FROM project_agents WHERE project = ?1", [project], |r| r.get(0)).map_err(sql)?;
    tx.execute("INSERT INTO project_agents (id, project, name, kind, host, dir, command, effort, color) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
               (&id, project, &given.name, &given.kind, &given.host, &given.dir, &given.command, &given.effort, color)).map_err(sql)?;
    Ok(id)
}

fn change(tx: &Transaction, id: &str, given: &Given) -> Result<(), String> {
    let project: String = tx.query_row("SELECT project FROM project_agents WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(sql)?.ok_or_else(|| "That agent is gone.".to_string())?;
    given.fits(tx, &project, id)?;
    tx.execute("UPDATE project_agents SET name = ?2, kind = ?3, host = ?4, dir = ?5, command = ?6, effort = ?7 WHERE id = ?1",
               (id, &given.name, &given.kind, &given.host, &given.dir, &given.command, &given.effort)).map(|_| ()).map_err(sql)
}

fn remove(tx: &Transaction, id: &str) -> Result<(), String> {
    match tx.execute("DELETE FROM project_agents WHERE id = ?1", [id]).map_err(sql)? {
        0 => Err("That agent is gone.".into()),
        _ => Ok(()),
    }
}

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/task-projects/{id}/agents", post(new_agent))
        .route("/api/task-agents/{id}", patch(change_agent).delete(drop_agent))
        .route("/api/task-projects/{id}/limits", put(set_limits))
}

/// The body as kept, or what is wrong with it.
fn checked(app: &Shared, body: &Value) -> Result<Given, String> {
    let given = Given::of(body);
    given.check(&crate::hosts::names(app))?;
    Ok(given)
}

async fn new_agent(State(app): State<Shared>, Path(project): Path<String>, Json(body): Json<Value>) -> Response {
    let given = match checked(&app, &body) {
        Ok(g) => g,
        Err(e) => return answer::<()>(Err(e)),
    };
    let done = store(&app, move |tx| add(tx, &project, &given).map(|id| json!({"id": id}))).await;
    told_projects(&app, done)
}

async fn change_agent(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Value>) -> Response {
    let given = match checked(&app, &body) {
        Ok(g) => g,
        Err(e) => return answer::<()>(Err(e)),
    };
    let done = store(&app, move |tx| change(tx, &id, &given).map(|()| json!({"ok": true}))).await;
    told_projects(&app, done)
}

async fn drop_agent(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let done = store(&app, move |tx| remove(tx, &id).map(|()| json!({"ok": true}))).await;
    told_projects(&app, done)
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

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
