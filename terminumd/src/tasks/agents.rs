// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The pool of agents (`terminum-dispatcher.md` AC-137): what RUN can start, none a project's. Where it
//! starts is the project's (AC-140). Only a signed-in page changes them, never `terminum mcp`; a command is
//! the human's own words, kept as written.

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{patch, post};
use axum::{Json, Router};
use rusqlite::Transaction;
use serde::Serialize;
use serde_json::{json, Value};

use super::api::{answer, store, told_projects};
use super::ops::{rows, sql};
use crate::app::Shared;
use crate::termtext::{has_control, stray};
use crate::tmux::hookstate::AGENTS;

const MOST_NAME: usize = 40;
/// The most of one agent, or of a project's agents, at once (terminum-dispatcher.md AC-141).
pub const MOST_AT_ONCE: i64 = 100;
const MOST_LINE: usize = 1000;

#[derive(Clone, Serialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub command: String,
    pub effort: String,
    /// its place in the palette (listparts.js): none of two share one while another is free
    pub color: i64,
    /// how many of it may run at once (terminum-dispatcher.md AC-141)
    pub at_once: i64,
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
    command: String,
    effort: String,
    at_once: i64,
}

impl Given {
    fn of(body: &Value) -> Given {
        let text = |k: &str| body[k].as_str().unwrap_or_default().trim().to_string();
        Given { name: text("name"), kind: text("kind"), command: text("command"), effort: text("effort"), at_once: if body["at_once"].is_null() { 1 } else { body["at_once"].as_i64().unwrap_or(-1) } }
    }

    /// What needs no store: refused with the words the window shows.
    fn check(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("An agent needs a name.".into());
        }
        if !(1..=MOST_AT_ONCE).contains(&self.at_once) {
            return Err("At once is 1 to 100.".into());
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
        Ok(())
    }

    /// A name stands once in the pool.
    fn fits(&self, tx: &Transaction, but: &str) -> Result<(), String> {
        let taken: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM agents WHERE name = ?1 AND id != ?2)", (&self.name, but), |r| r.get(0)).map_err(sql)?;
        if taken {
            return Err(format!("The pool already has an agent named {}.", self.name));
        }
        Ok(())
    }
}

/// The pool, as they were added.
pub fn list(tx: &Transaction) -> Result<Vec<Agent>, String> {
    rows(tx, "SELECT id, name, kind, command, effort, color, at_once FROM agents ORDER BY rowid", [], |r| {
        let (kind, command, effort): (String, String, String) = (r.get(2)?, r.get(3)?, r.get(4)?);
        Ok(Agent { id: r.get(0)?, name: r.get(1)?, line: command_line(&kind, &command, &effort), kind, command, effort, color: r.get(5)?, at_once: r.get(6)? })
    })
}

/// One of the pool, if it is still there.
pub fn by_id(tx: &Transaction, id: &str) -> Result<Option<Agent>, String> {
    Ok(list(tx)?.into_iter().find(|a| a.id == id))
}

/// Keeps a new one. Its id.
fn add(tx: &Transaction, given: &Given) -> Result<String, String> {
    given.fits(tx, "")?;
    let id = crate::auth::crypto::random_hex::<8>();
    // the next color after the last: agents far apart, and no two alike for a palette's length
    let color: i64 = tx.query_row("SELECT COALESCE(MAX(color) + 1, 0) FROM agents", [], |r| r.get(0)).map_err(sql)?;
    tx.execute("INSERT INTO agents (id, name, kind, command, effort, color, at_once) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
               (&id, &given.name, &given.kind, &given.command, &given.effort, color, given.at_once)).map_err(sql)?;
    Ok(id)
}

fn change(tx: &Transaction, id: &str, given: &Given) -> Result<(), String> {
    given.fits(tx, id)?;
    // what it wrote stays under its name, the new one (AC-137)
    tx.execute("UPDATE users SET name = ?2 WHERE id = ?1", (super::ops::pool_user_id(id), &given.name)).map_err(sql)?;
    match tx.execute("UPDATE agents SET name = ?2, kind = ?3, command = ?4, effort = ?5, at_once = ?6 WHERE id = ?1", (id, &given.name, &given.kind, &given.command, &given.effort, given.at_once)).map_err(sql)? {
        0 => Err("That agent is gone.".into()),
        _ => Ok(()),
    }
}

fn remove(tx: &Transaction, id: &str) -> Result<(), String> {
    if let Some(run) = crate::dispatcher::runs::holding_agent(tx, id)? {
        return Err(format!("{} has a session open: {}. Close it, or Give up, first.", run.agent_name, run.names()));
    }
    match tx.execute("DELETE FROM agents WHERE id = ?1", [id]).map_err(sql)? {
        0 => Err("That agent is gone.".into()),
        _ => Ok(()),
    }
}

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/task-agents", post(new_agent))
        .route("/api/task-agents/{id}", patch(change_agent).delete(drop_agent))
}

/// The body as kept, or what is wrong with it.
fn checked(body: &Value) -> Result<Given, String> {
    let given = Given::of(body);
    given.check()?;
    Ok(given)
}

async fn new_agent(State(app): State<Shared>, Json(body): Json<Value>) -> Response {
    let given = match checked(&body) {
        Ok(g) => g,
        Err(e) => return answer::<()>(Err(e)),
    };
    let done = store(&app, move |tx| add(tx, &given).map(|id| json!({"id": id}))).await;
    told_projects(&app, done)
}

async fn change_agent(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Value>) -> Response {
    let given = match checked(&body) {
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
