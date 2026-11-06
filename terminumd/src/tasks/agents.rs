// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A project's agents (`terminum-dispatcher.md` AC-137): what RUN can start. Only a signed-in page
//! changes them, never `terminum mcp`; a command is the human's own words, kept as written.

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{patch, post};
use axum::Router;
use axum::Json;
use rusqlite::{OptionalExtension, Transaction};
use serde::Serialize;
use serde_json::{json, Value};

use super::api::{answer, store};
use super::ops::{rows, sql};
use crate::app::Shared;

const MOST_NAME: usize = 40;
const MOST_LINE: usize = 1000;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Agent {
    pub id: String,
    pub project: String,
    pub name: String,
    pub kind: String,
    pub host: Option<String>,
    pub dir: String,
    pub command: String,
}

/// What a page keeps, checked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Given {
    pub name: String,
    pub kind: String,
    pub host: Option<String>,
    pub dir: String,
    pub command: String,
}

impl Given {
    fn of(body: &Value) -> Given {
        let text = |k: &str| body[k].as_str().unwrap_or_default().trim().to_string();
        Given { name: text("name"), kind: text("kind"), host: Some(text("host")).filter(|h| !h.is_empty()), dir: text("dir"), command: text("command") }
    }

    fn check(&self, hosts: &[String]) -> Result<(), String> {
        let tame = |s: &str| !s.chars().any(char::is_control);
        if self.name.is_empty() {
            return Err("An agent needs a name.".into());
        }
        if self.name.chars().count() > MOST_NAME {
            return Err(format!("An agent's name is {MOST_NAME} characters at most."));
        }
        if !tame(&self.name) {
            return Err("An agent's name is one line, with no control characters.".into());
        }
        if !matches!(self.kind.as_str(), "claude" | "codex") {
            return Err("An agent is Claude Code or Codex.".into());
        }
        if self.command.is_empty() {
            return Err("An agent needs a command.".into());
        }
        if self.command.chars().count() > MOST_LINE {
            return Err(format!("An agent's command is {MOST_LINE} characters at most."));
        }
        if !tame(&self.command) {
            return Err("An agent's command is one line, with no control characters.".into());
        }
        if !self.dir.is_empty() && !(self.dir.starts_with('/') || self.dir.starts_with('~')) {
            return Err("A folder starts with / or ~.".into());
        }
        if self.dir.chars().count() > MOST_LINE || !tame(&self.dir) {
            return Err(format!("A folder is one line of {MOST_LINE} characters at most."));
        }
        if let Some(host) = self.host.as_ref().filter(|h| !hosts.contains(h)) {
            return Err(format!("The server {host} is not one of this terminum's hosts."));
        }
        Ok(())
    }
}

/// The hosts an agent can stand on, by their records.
pub fn known_hosts(app: &Shared) -> Vec<String> {
    app.looks.settings.read().hosts.keys().cloned().collect()
}

/// Every project's, in their order.
pub fn list(tx: &Transaction) -> Result<Vec<Agent>, String> {
    rows(tx, "SELECT id, project, name, kind, host, dir, command FROM project_agents ORDER BY place", [], |r| {
        Ok(Agent { id: r.get(0)?, project: r.get(1)?, name: r.get(2)?, kind: r.get(3)?, host: r.get(4)?, dir: r.get(5)?, command: r.get(6)? })
    })
}

fn project_name(tx: &Transaction, project: &str) -> Result<String, String> {
    tx.query_row("SELECT name FROM projects WHERE id = ?1", [project], |r| r.get(0)).optional().map_err(sql)?.ok_or_else(|| "That project is gone.".to_string())
}

/// The refusal when `project` holds another agent of that name.
fn check_free(tx: &Transaction, project: &str, name: &str, but: &str) -> Result<(), String> {
    let taken: Option<String> = tx.query_row("SELECT id FROM project_agents WHERE project = ?1 AND name = ?2 AND id != ?3", (project, name, but), |r| r.get(0)).optional().map_err(sql)?;
    match taken {
        Some(_) => Err(format!("{} already has an agent named {name}.", project_name(tx, project)?)),
        None => Ok(()),
    }
}

/// Keeps a new one at its project's end. Its id.
pub fn add(tx: &Transaction, project: &str, given: &Given, hosts: &[String]) -> Result<String, String> {
    given.check(hosts)?;
    project_name(tx, project)?;
    check_free(tx, project, &given.name, "")?;
    let id = crate::auth::crypto::random_hex::<8>();
    let place: i64 = tx.query_row("SELECT COALESCE(MAX(place), -1) + 1 FROM project_agents", [], |r| r.get(0)).map_err(sql)?;
    tx.execute("INSERT INTO project_agents (id, project, name, kind, host, dir, command, place) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
               (&id, project, &given.name, &given.kind, &given.host, &given.dir, &given.command, place)).map_err(sql)?;
    Ok(id)
}

pub fn change(tx: &Transaction, id: &str, given: &Given, hosts: &[String]) -> Result<(), String> {
    given.check(hosts)?;
    let project: String = tx.query_row("SELECT project FROM project_agents WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(sql)?.ok_or_else(|| "That agent is gone.".to_string())?;
    check_free(tx, &project, &given.name, id)?;
    tx.execute("UPDATE project_agents SET name = ?2, kind = ?3, host = ?4, dir = ?5, command = ?6 WHERE id = ?1",
               (id, &given.name, &given.kind, &given.host, &given.dir, &given.command)).map(|_| ()).map_err(sql)
}

pub fn remove(tx: &Transaction, id: &str) -> Result<(), String> {
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

/// Its answer; every page hears, as of a project's change.
fn told<T: Serialize>(app: &Shared, done: Result<T, String>) -> Response {
    if done.is_ok() {
        crate::page::projects::tell(app);
    }
    answer(done)
}

async fn new_agent(State(app): State<Shared>, Path(project): Path<String>, Json(body): Json<Value>) -> Response {
    let (given, hosts) = (Given::of(&body), known_hosts(&app));
    let done = store(&app, move |tx| add(tx, &project, &given, &hosts).map(|id| json!({"id": id}))).await;
    told(&app, done)
}

async fn change_agent(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Value>) -> Response {
    let (given, hosts) = (Given::of(&body), known_hosts(&app));
    let done = store(&app, move |tx| change(tx, &id, &given, &hosts).map(|()| json!({"ok": true}))).await;
    told(&app, done)
}

async fn drop_agent(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let done = store(&app, move |tx| remove(tx, &id).map(|()| json!({"ok": true}))).await;
    told(&app, done)
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
