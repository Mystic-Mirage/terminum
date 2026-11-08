// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A project's routes from the board (AC-110, AC-139): made, renamed, its folder, the letters of its
//! tasks' IDs (AC-148) and its template for agents' tasks (AC-145).

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{patch, post};
use axum::{Json, Router};
use rusqlite::Transaction;
use serde_json::{json, Value};

use super::api::{store, text, told, told_projects};
use super::ops::rows;
use super::store::DEFAULT_PROJECT;
use crate::app::Shared;
use crate::http::{off, refuse};

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/task-projects", post(new_project))
        .route("/api/task-projects/{id}", patch(edit_project))
}

/// Each project's letters and its own template (none: the default), by its id.
pub fn settings(tx: &Transaction) -> Result<HashMap<String, Value>, String> {
    let all = rows(tx, "SELECT id, prefix, task_template FROM projects", [], |r| Ok((r.get::<_, String>(0)?, json!({"prefix": r.get::<_, Option<String>>(1)?, "template": r.get::<_, Option<String>>(2)?}))))?;
    Ok(all.into_iter().collect())
}

/// New project on the board: no session yet (AC-110).
async fn new_project(State(app): State<Shared>, Json(body): Json<Value>) -> Response {
    let (name, dir, host) = (text(&body, "name").unwrap_or_default(), text(&body, "dir").unwrap_or_default(), text(&body, "host").filter(|h| !h.is_empty()));
    let a = app.clone();
    let done = off(move || a.projects.lock().create(&name, host, dir)).await;
    told_projects(&app, done.map(|id| json!({"id": id})))
}

/// A project's name and folder from its window (AC-139); its server stays: its sessions are there.
async fn edit_project(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Value>) -> Response {
    let (prefix, template) = (text(&body, "prefix"), text(&body, "template"));
    if prefix.is_some() || template.is_some() {
        let p = id.clone();
        let done = store(&app, move |tx| {
            super::ops::check_project(tx, &p)?;
            if let Some(t) = &template {
                super::template::set(tx, &p, t)?;
            }
            if let Some(x) = &prefix {
                super::slugs::set_prefix(tx, &p, x)?;
            }
            Ok(())
        }).await;
        if let Err(e) = done {
            return refuse(StatusCode::BAD_REQUEST, e);
        }
        told(&app, "");                                // every card's ID may have changed
        if text(&body, "name").is_none() && text(&body, "dir").is_none() {
            return told_projects(&app, Ok(json!({"ok": true})));
        }
    }
    if id == DEFAULT_PROJECT {
        return refuse(StatusCode::BAD_REQUEST, "Default is no real project: it has no name or folder to change.");
    }
    let (name, dir) = (text(&body, "name"), text(&body, "dir"));
    let a = app.clone();
    let done = off(move || a.projects.lock().edit(&id, name.as_deref(), dir)).await;
    told_projects(&app, done.map(|()| json!({"ok": true})))
}

