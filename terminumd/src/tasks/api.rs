// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The board's routes (`terminum-kanban.md` §9), behind the page's sign-in and write checks. Store
//! work runs off the async workers; every change tells every page (`taskchange`).

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query as Params, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use rusqlite::Transaction;
use serde::Serialize;
use serde_json::{json, Value};

use super::files;
use super::ops::{self, Edit, NewTask, Task};
use super::query::{self, Query};
use super::store::{DEFAULT_PROJECT, HUMAN};
use crate::app::Shared;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/tasks", get(board).post(make))
        .route("/api/tasks/meta", get(meta))
        .route("/api/tasks/start-empty", post(start_empty))
        .route("/api/task-projects", post(new_project))
        .route("/api/tasks/{id}", get(window).patch(change))
        .route("/api/tasks/{id}/comments", post(add_comment))
        .route("/api/tasks/{id}/comments/{event}", patch(edit_comment).delete(drop_comment))
        .route("/api/tasks/{id}/purge", post(purge))
        .route("/api/task-files", post(upload).layer(DefaultBodyLimit::max(files::MOST_BYTES + 1024)))
        .route("/api/task-files/{id}", get(download).delete(remove_file))
}

fn refuse(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error": message.into()}))).into_response()
}

/// Runs `f` in one transaction off the async workers.
async fn store<T: Send + 'static>(app: &Shared, f: impl FnOnce(&Transaction) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let a = app.clone();
    tokio::task::spawn_blocking(move || a.tasks.with(f)).await.map_err(|e| e.to_string())?
}

fn answer<T: Serialize>(done: Result<T, String>) -> Response {
    match done {
        Ok(v) => Json(v).into_response(),
        Err(e) => refuse(StatusCode::BAD_REQUEST, e),
    }
}

/// Every page hears of a task's change, and the session list of its project's count.
pub fn told(app: &Shared, task: &str) {
    app.pages.tell(&json!({"t": "taskchange", "id": task}).to_string());
    let a = app.clone();
    tokio::spawn(async move {
        let refreshed = tokio::task::spawn_blocking({
            let a = a.clone();
            move || a.projects.lock().refresh_counts()
        }).await;
        if matches!(refreshed, Ok(true)) {
            crate::page::projects::tell(&a);
        }
    });
}

/// A session's title and live state as the list shows them, or closed.
fn session_of(app: &Shared, key: &str) -> Value {
    match crate::page::marks::find_item(&app.layout.read(), key) {
        Some(it) => json!({"key": key, "title": it["title"], "state": it["state"], "agent": it["agent"], "closed": false}),
        None => json!({"key": key, "closed": true}),
    }
}

fn card(app: &Shared, t: &Task) -> Value {
    let mut v = serde_json::to_value(t).unwrap_or_default();
    if let Some(key) = &t.session {
        v["session"] = session_of(app, key);
    }
    v
}

fn csv(q: &HashMap<String, String>, key: &str) -> Vec<String> {
    q.get(key).map(|v| v.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect()).unwrap_or_default()
}

/// A page of cards under the board's filters, with each column's count.
async fn board(State(app): State<Shared>, Params(q): Params<HashMap<String, String>>) -> Response {
    let filter = Query {
        projects: csv(&q, "project"),
        severities: csv(&q, "severity"),
        statuses: csv(&q, "status"),
        owner: q.get("owner").filter(|o| !o.is_empty()).cloned(),
        session: q.get("session").filter(|s| !s.is_empty()).cloned(),
        reach: None,
        text: q.get("q").cloned(),
        with_subtasks: q.get("subtasks").is_some_and(|s| s == "1"),
        after: q.get("after").and_then(|a| a.parse().ok()).unwrap_or(0),
        limit: q.get("limit").and_then(|l| l.parse().ok()).unwrap_or(0),
    };
    let done = store(&app, move |tx| Ok((query::list(tx, &filter)?, query::counts(tx, &filter)?))).await;
    match done {
        Ok((tasks, counts)) => Json(json!({"tasks": tasks.iter().map(|t| card(&app, t)).collect::<Vec<_>>(), "counts": counts})).into_response(),
        Err(e) => refuse(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

/// What the board's pickers offer, and the store's trouble if any.
async fn meta(State(app): State<Shared>) -> Response {
    // what is told of each lock is copied out: nothing here waits under another
    let (projects, member_of): (Vec<Value>, HashMap<String, String>) = {
        let ps = app.projects.lock();
        (std::iter::once(json!({"id": DEFAULT_PROJECT, "name": "Default", "default": true})).chain(ps.list().iter().map(|p| json!({"id": p.id, "name": p.name, "color": p.color, "host": p.host, "dir": p.dir}))).collect(),
         ps.list().iter().flat_map(|p| p.members.iter().map(|m| (m.clone(), p.id.clone()))).collect())
    };
    let sessions: Vec<Value> = crate::page::marks::layout_items(app.layout.read().as_array().map_or(&[], |a| a.as_slice()))
        .map(|it| json!({"key": it["id"], "title": it["title"], "agent": it["agent"], "state": it["state"], "project": member_of.get(it["id"].as_str().unwrap_or(""))})).collect();
    let users = store(&app, |tx| {
        let mut st = tx.prepare("SELECT id, kind, name FROM users WHERE kind = 'human' ORDER BY name").map_err(ops::sql)?;
        let rows = st.query_map([], |r| Ok(json!({"id": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "name": r.get::<_, String>(2)?}))).map_err(ops::sql)?;
        rows.collect::<Result<Vec<Value>, _>>().map_err(ops::sql)
    }).await.unwrap_or_default();
    Json(json!({"projects": projects, "sessions": sessions, "users": users, "problem": app.tasks.problem(), "set_aside": app.tasks.is_set_aside(), "theme": app.looks.theme(None, "").1})).into_response()
}

async fn start_empty(State(app): State<Shared>) -> Response {
    let a = app.clone();
    let done = tokio::task::spawn_blocking(move || {
        a.tasks.start_empty()?;
        a.projects.lock().reload()
    }).await.map_err(|e| e.to_string()).and_then(|r| r);
    if done.is_ok() {
        crate::page::projects::tell(&app);
    }
    answer(done.map(|()| json!({"ok": true})))
}

/// New project on the board: no session yet (AC-110).
async fn new_project(State(app): State<Shared>, Json(body): Json<Value>) -> Response {
    let (name, dir, host) = (text(&body, "name").unwrap_or_default(), text(&body, "dir").unwrap_or_default(), text(&body, "host").filter(|h| !h.is_empty()));
    let a = app.clone();
    let done = tokio::task::spawn_blocking(move || a.projects.lock().create(&name, host, dir)).await.map_err(|e| e.to_string()).and_then(|r| r);
    if done.is_ok() {
        crate::page::projects::tell(&app);
    }
    answer(done.map(|id| json!({"id": id})))
}

fn text(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::to_string)
}

fn ids(v: &Value, key: &str) -> Vec<String> {
    v[key].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

async fn make(State(app): State<Shared>, Json(body): Json<Value>) -> Response {
    let new = NewTask {
        title: text(&body, "title").unwrap_or_default(),
        text: text(&body, "text").unwrap_or_default(),
        project: text(&body, "project"),
        parent: text(&body, "parent"),
        severity: text(&body, "severity"),
        owner: text(&body, "owner"),
        session: text(&body, "session"),
        by_agent: false,
        made_in: None,
    };
    let attached = ids(&body, "files");
    let done = store(&app, move |tx| {
        let task = ops::create(tx, HUMAN, new, ops::now())?;
        files::attach(tx, &attached, &task.id, None)?;
        ops::get(tx, &task.id)
    }).await;
    if let Ok(t) = &done {
        told(&app, &t.id);
    }
    answer(done.map(|t| card(&app, &t)))
}

/// A task's window: its fields, text, comments and history, files.
async fn window(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let wanted = id.clone();
    let done = store(&app, move |tx| Ok((ops::get(tx, &wanted)?, ops::text_of(tx, &wanted)?, ops::history(tx, &wanted)?, files::of_task(tx, &wanted)?))).await;
    answer(done.map(|(t, body, history, attached)| {
        let mut v = card(&app, &t);
        v["text"] = json!(body);
        v["history"] = history;
        v["files"] = json!(attached);
        v
    }))
}

/// What a human changes on a card; `session_choice` answers the dialog of a move across projects.
async fn change(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Value>) -> Response {
    let wanted = id.clone();
    let moved_to = text(&body, "status");
    let to_status = moved_to.clone();
    let to_project = text(&body, "project");
    let choice = text(&body, "session_choice");
    let edit = Edit { title: text(&body, "title"), text: text(&body, "text"), severity: text(&body, "severity"), owner: text(&body, "owner") };
    let before = body.get("before").filter(|b| !b.is_null());
    let reorder = before.is_some() || body.get("order_end").is_some();
    let before = before.and_then(Value::as_str).map(str::to_string);
    let session = body.get("session").cloned();
    let done = store(&app, move |tx| {
        let at = ops::now();
        let was = ops::get(tx, &wanted)?;
        let mut follow = None;
        if let Some(project) = &to_project
            && project != &was.project {
                match (&was.session, choice.as_deref()) {
                    (Some(_), None) => return Err("session_choice".into()),
                    (Some(s), Some("move")) => {
                        if let Some(other) = ops::other_open_of_session(tx, s, &wanted)? {
                            return Err(format!("This session also holds {other}: unlink it, or move {other} first."));
                        }
                        follow = Some((s.clone(), project.clone()));
                        ops::set_project(tx, HUMAN, &wanted, project, false, at)?;
                    }
                    _ => { ops::set_project(tx, HUMAN, &wanted, project, was.session.is_some(), at)?; }
                }
            }
        if let Some(s) = &session {
            ops::link(tx, HUMAN, &wanted, s.as_str(), at)?;
        }
        ops::edit(tx, HUMAN, &wanted, edit, at)?;
        if let Some(status) = &to_status {
            ops::move_to(tx, HUMAN, &wanted, status, at)?;
        }
        if reorder {
            ops::reorder(tx, &wanted, before.as_deref(), at)?;
        }
        Ok((ops::get(tx, &wanted)?, was, follow))
    }).await;
    let (task, was, follow) = match done {
        Ok(d) => d,
        Err(e) if e == "session_choice" => return refuse(StatusCode::CONFLICT, "session_choice"),
        Err(e) => return refuse(StatusCode::BAD_REQUEST, e),
    };
    let note = match follow {
        Some((session, project)) => follow_session(&app, session, project).await.err(),
        None => None,
    };
    if moved_to.is_some() && was.status != task.status {
        super::tell::human_moved(&app, &task).await;
    }
    told(&app, &task.id);
    let mut answered = card(&app, &task);
    if let Some(n) = note {
        answered["note"] = json!(format!("The task moved, its session did not: {n}"));
    }
    Json(answered).into_response()
}

/// A session goes where its task went, Default meaning no project (AC-113).
async fn follow_session(app: &Shared, session: String, project: String) -> Result<(), String> {
    let a = app.clone();
    let host = crate::tmux::parse_key(&session).and_then(|(h, _, _)| (h != "local").then(|| h.to_string()));
    let joined = tokio::task::spawn_blocking(move || {
        let mut ps = a.projects.lock();
        if project == DEFAULT_PROJECT { ps.remove(&session) } else { ps.add(&session, Some(&project), None, host, String::new(), None).map(|_| ()) }
    }).await.map_err(|e| e.to_string()).and_then(|r| r);
    crate::page::projects::tell(app);
    joined
}

async fn add_comment(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Value>) -> Response {
    let (wanted, said, attached) = (id.clone(), text(&body, "text").unwrap_or_default(), ids(&body, "files"));
    let done = store(&app, move |tx| {
        let event = ops::comment(tx, HUMAN, &wanted, &said, ops::now())?;
        files::attach(tx, &attached, &wanted, Some(&event))?;
        Ok(json!({"event": event}))
    }).await;
    if done.is_ok() {
        told(&app, &id);
    }
    answer(done)
}

async fn edit_comment(State(app): State<Shared>, Path((id, event)): Path<(String, String)>, Json(body): Json<Value>) -> Response {
    let (wanted, said) = (id.clone(), text(&body, "text").unwrap_or_default());
    let done = store(&app, move |tx| ops::change_comment(tx, HUMAN, &wanted, &event, Some(&said), ops::now()).map(|()| json!({"ok": true}))).await;
    if done.is_ok() {
        told(&app, &id);
    }
    answer(done)
}

async fn drop_comment(State(app): State<Shared>, Path((id, event)): Path<(String, String)>) -> Response {
    let wanted = id.clone();
    let done = store(&app, move |tx| ops::change_comment(tx, HUMAN, &wanted, &event, None, ops::now()).map(|()| json!({"ok": true}))).await;
    if done.is_ok() {
        scrub(&app).await;
        told(&app, &id);
    }
    answer(done)
}

/// A purge's text also leaves the store's write-ahead log (AC-109).
async fn scrub(app: &Shared) {
    let a = app.clone();
    let _ = tokio::task::spawn_blocking(move || a.tasks.checkpoint()).await;
}

async fn purge(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let wanted = id.clone();
    let done = store(&app, move |tx| ops::purge_text(tx, HUMAN, &wanted, ops::now()).map(|()| json!({"ok": true}))).await;
    if done.is_ok() {
        scrub(&app).await;
        told(&app, &id);
    }
    answer(done)
}

async fn upload(State(app): State<Shared>, Params(q): Params<HashMap<String, String>>, data: Bytes) -> Response {
    let name = q.get("name").cloned().unwrap_or_default();
    let a = app.clone();
    let done = tokio::task::spawn_blocking(move || files::keep(&a.tasks, &a.cfg.dir, &name, HUMAN, &data, ops::now())).await.map_err(|e| e.to_string()).and_then(|r| r);
    answer(done)
}

/// Bytes with the header sign-in only: a raster picture shows, all else downloads (AC-109).
async fn download(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let a = app.clone();
    let done = tokio::task::spawn_blocking(move || files::read(&a.tasks, &a.cfg.dir, &id)).await.map_err(|e| e.to_string()).and_then(|r| r);
    let (file, data) = match done {
        Ok(d) => d,
        Err(e) => return refuse(StatusCode::NOT_FOUND, e),
    };
    let inline = files::inline_mime(&data);
    let mut res = data.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(inline.unwrap_or("application/octet-stream")));
    let safe: String = file.name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect();
    let how = if inline.is_some() { "inline" } else { "attachment" };
    if let Ok(v) = HeaderValue::from_str(&format!("{how}; filename=\"{safe}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    res
}

async fn remove_file(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let a = app.clone();
    let done = tokio::task::spawn_blocking(move || files::delete(&a.tasks, &a.cfg.dir, &id, HUMAN, ops::now())).await.map_err(|e| e.to_string()).and_then(|r| r);
    if let Ok(Some(task)) = &done {
        told(&app, task);
    }
    answer(done.map(|_| json!({"ok": true})))
}
