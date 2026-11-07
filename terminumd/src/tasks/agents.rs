// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A project's agents (`terminum-dispatcher.md` AC-137): what RUN can start. Only a signed-in page
//! changes them, never `terminum mcp`; a command is the human's own words, kept as written.

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{patch, post};
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
}

/// What a page keeps, trimmed.
struct Given {
    name: String,
    kind: String,
    host: Option<String>,
    dir: String,
    command: String,
}

impl Given {
    fn of(body: &Value) -> Given {
        let text = |k: &str| body[k].as_str().unwrap_or_default().trim().to_string();
        Given { name: text("name"), kind: text("kind"), host: Some(text("host")).filter(|h| !h.is_empty()), dir: text("dir"), command: text("command") }
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
        if !self.dir.is_empty() && !(self.dir.starts_with('/') || self.dir.starts_with('~')) {
            return Err("A folder starts with / or ~.".into());
        }
        if self.dir.chars().count() > MOST_LINE || has_control(&self.dir) {
            return Err(format!("A folder is one line of {MOST_LINE} characters at most, with no control or invisible characters."));
        }
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

/// Every project's, as they were added.
pub fn list(tx: &Transaction) -> Result<Vec<Agent>, String> {
    rows(tx, "SELECT id, project, name, kind, host, dir, command FROM project_agents ORDER BY rowid", [], |r| {
        Ok(Agent { id: r.get(0)?, project: r.get(1)?, name: r.get(2)?, kind: r.get(3)?, host: r.get(4)?, dir: r.get(5)?, command: r.get(6)? })
    })
}

/// Keeps a new one. Its id.
fn add(tx: &Transaction, project: &str, given: &Given) -> Result<String, String> {
    check_project(tx, project)?;
    given.fits(tx, project, "")?;
    let id = crate::auth::crypto::random_hex::<8>();
    tx.execute("INSERT INTO project_agents (id, project, name, kind, host, dir, command) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
               (&id, project, &given.name, &given.kind, &given.host, &given.dir, &given.command)).map_err(sql)?;
    Ok(id)
}

fn change(tx: &Transaction, id: &str, given: &Given) -> Result<(), String> {
    let project: String = tx.query_row("SELECT project FROM project_agents WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(sql)?.ok_or_else(|| "That agent is gone.".to_string())?;
    given.fits(tx, &project, id)?;
    tx.execute("UPDATE project_agents SET name = ?2, kind = ?3, host = ?4, dir = ?5, command = ?6 WHERE id = ?1",
               (id, &given.name, &given.kind, &given.host, &given.dir, &given.command)).map(|_| ()).map_err(sql)
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

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
