// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The board's routes (`terminum-kanban.md` §9), behind the page's sign-in and write checks. Store
//! work runs off the async workers; every change tells every page (`taskchange`).

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query as Params, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use rusqlite::Transaction;
use serde::Serialize;
use serde_json::{json, Value};

use super::files;
use crate::dispatcher::runs;
use super::ops::{self, Edit, NewTask, ProjectMove, Task};
use super::query::{self, Query};
use super::store::{DEFAULT_PROJECT, HUMAN};
use crate::app::Shared;
use crate::http::{off, refuse};

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/tasks", get(board).post(make))
        .route("/api/tasks/meta", get(meta))
        .route("/api/tasks/start-empty", post(start_empty))
        .route("/api/task-projects", post(new_project))
        .route("/api/task-projects/{id}", patch(edit_project))
        .merge(super::agents::routes())
        .merge(crate::dispatcher::api::routes())
        .route("/api/tasks/{id}", get(window).patch(change))
        .route("/api/tasks/{id}/comments", post(add_comment))
        .route("/api/tasks/{id}/comments/{event}", patch(edit_comment).delete(drop_comment))
        .route("/api/tasks/{id}/purge", post(purge))
        .route("/api/tasks/{id}/archive", post(archive))
        .route("/api/task-files", post(upload).layer(DefaultBodyLimit::max(files::MOST_BYTES + 1024)))
        .route("/api/task-files/{id}", get(download).delete(remove_file))
}

/// Runs `f` in one transaction off the async workers.
pub(crate) async fn store<T: Send + 'static>(app: &Shared, f: impl FnOnce(&Transaction) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let a = app.clone();
    off(move || a.tasks.with(f)).await
}

pub(crate) fn answer<T: Serialize>(done: Result<T, String>) -> Response {
    match done {
        Ok(v) => Json(v).into_response(),
        Err(e) => refuse(StatusCode::BAD_REQUEST, e),
    }
}

/// `done`'s answer; every page hears when it changed a project or its agents.
pub(super) fn told_projects<T: Serialize>(app: &Shared, done: Result<T, String>) -> Response {
    if done.is_ok() {
        crate::page::projects::tell(app);
    }
    answer(done)
}

/// `done`'s answer; every page hears when it changed task `id`.
fn told_answer<T: Serialize>(app: &Shared, id: &str, done: Result<T, String>) -> Response {
    if done.is_ok() {
        told(app, id);
    }
    answer(done)
}

/// Every open page hears a task changed ("": several); its window reads the history anew.
pub fn change_heard(app: &Shared, task: &str) {
    app.pages.tell(&json!({"t": "taskchange", "id": task}).to_string());
}

/// A task's change: every page hears of it, and the session list of what it shows of tasks.
pub fn told(app: &Shared, task: &str) {
    change_heard(app, task);
    let a = app.clone();
    tokio::spawn(async move {
        let refreshed = tokio::task::spawn_blocking({
            let a = a.clone();
            move || a.task_marks.refresh(&a.tasks)
        }).await;
        if matches!(refreshed, Ok(true)) {
            super::marks::tell(&a);
        }
    });
}

/// A link only to a session that is there: a page's picker can be older than the list (AC-133).
fn session_gone(app: &Shared, body: &Value) -> bool {
    let layout = app.layout.read();
    body["session"].as_str().is_some_and(|key| !crate::page::marks::layout_items(layout.as_array().map_or(&[], |a| a.as_slice())).any(|it| it["id"] == key))
}

/// A task with its session: `row` its row in the list, its live state, agent and why it failed or
/// waits; none: closed.
fn with_session(t: &Task, row: Option<&Value>) -> Value {
    let mut v = serde_json::to_value(t).unwrap_or_default();
    if let Some(key) = &t.session {
        v["session"] = match row {
            Some(it) => json!({"key": key, "state": it["state"], "agent": it["agent"], "why": it["why"], "closed": false}),
            None => json!({"key": key, "closed": true}),
        };
    }
    v
}

fn card(app: &Shared, t: &Task) -> Value {
    let row = t.session.as_deref().and_then(|key| crate::page::marks::find_item(&app.layout.read(), key));
    with_session(t, row.as_ref())
}

/// What a text is compared by: a save says which it began from (AC-133).
fn text_rev(text: &str) -> String {
    crate::auth::crypto::hex(&crate::auth::crypto::sha256(text.as_bytes()))
}

fn csv(q: &HashMap<String, String>, key: &str) -> Vec<String> {
    q.get(key).map(|v| v.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect()).unwrap_or_default()
}

/// A page of cards under the board's filters; with `counts=1`, each column's count too.
async fn board(State(app): State<Shared>, Params(q): Params<HashMap<String, String>>) -> Response {
    let filter = Query {
        projects: csv(&q, "project"),
        severities: csv(&q, "severity"),
        statuses: csv(&q, "status"),
        owner: q.get("owner").filter(|o| !o.is_empty()).cloned(),
        number: q.get("id").and_then(|id| ops::number_of(id).ok()),
        reach: None,
        text: q.get("q").cloned(),
        with_subtasks: q.get("subtasks").is_some_and(|c| c == "1"),
        archived: q.get("archived").is_some_and(|c| c == "1"),
        after: q.get("after").and_then(|a| a.parse().ok()).unwrap_or(0),
        limit: q.get("limit").and_then(|l| l.parse().ok()).unwrap_or(0),
    };
    let with_counts = q.get("counts").is_some_and(|c| c == "1");
    let done = store(&app, move |tx| {
        let tasks = query::list(tx, &filter)?;
        let keys: Vec<&str> = tasks.iter().map(|t| t.key.as_str()).collect();
        let (run_marks, on_agents) = (runs::card_marks(tx, &keys)?, runs::card_agents(tx, &keys)?);
        let counts = if with_counts { Some((query::counts(tx, &filter)?, runs::busy_agents(&runs::live(tx)?))) } else { None };
        Ok((tasks, counts, run_marks, on_agents))
    }).await;
    match done {
        Ok((tasks, counts, run_marks, on_agents)) => {
            let layout = app.layout.read();
            let rows: HashMap<&str, &Value> = crate::page::marks::layout_items(layout.as_array().map_or(&[], |a| a.as_slice())).filter_map(|it| Some((it["id"].as_str()?, it))).collect();
            let shown: Vec<Value> = tasks.iter().map(|t| {
                let mut v = with_session(t, t.session.as_deref().and_then(|k| rows.get(k).copied()));
                if let Some(state) = run_marks.get(&t.key) {
                    v["run"] = json!(state);
                }
                if let Some(agent) = on_agents.get(&t.key) {
                    v["agent"] = agent.clone();
                }
                v
            }).collect();
            let (counts, busy) = counts.unzip();
            Json(json!({"tasks": shown, "counts": counts, "busy": busy})).into_response()
        }
        Err(e) => refuse(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

/// What the board's pickers offer, and the store's trouble if any.
async fn meta(State(app): State<Shared>) -> Response {
    let projects: Vec<Value> = std::iter::once(json!({"id": DEFAULT_PROJECT, "name": "Default"}))
        .chain(app.projects.lock().list().iter().map(|p| json!({"id": p.id, "name": p.name, "color": p.color, "dir": p.dir, "host": p.host}))).collect();
    let sessions: Vec<Value> = crate::page::marks::layout_items(app.layout.read().as_array().map_or(&[], |a| a.as_slice()))
        .map(|it| json!({"key": it["id"], "title": it["title"]})).collect();
    // the store's lock waits out a running query: off the async workers, its state too
    let a = app.clone();
    let (users, agents, limits, last_agents, problem, set_aside) = tokio::task::spawn_blocking(move || {
        let (users, agents, limits, last_agents) = a.tasks.with(|tx| Ok((
            ops::rows(tx, "SELECT id, name FROM users WHERE kind = 'human' ORDER BY name", [], |r| Ok(json!({"id": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?})))?,
            super::agents::list(tx)?,
            super::agents::limits(tx)?,
            runs::last_agents(tx)?,
        ))).unwrap_or_default();
        (users, agents, limits, last_agents, a.tasks.problem(), a.tasks.is_set_aside())
    }).await.unwrap_or_default();
    Json(json!({"projects": projects, "agents": agents, "last_agents": last_agents, "efforts": super::agents::EFFORTS.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(), "limits": limits.iter().map(|(project, warn, end)| json!({"project": project, "warn": warn, "end": end})).collect::<Vec<_>>(), "hosts": crate::hosts::names(&app), "sessions": sessions, "users": users, "problem": problem, "set_aside": set_aside, "most_bytes": files::MOST_BYTES, "theme": app.looks.theme(None, "").1, "scheme": app.looks.settings.read().page.color_scheme})).into_response()
}

async fn start_empty(State(app): State<Shared>) -> Response {
    let a = app.clone();
    let done = off(move || {
        a.tasks.start_empty()?;
        a.task_marks.refresh(&a.tasks);
        a.projects.lock().reload()
    }).await;
    if done.is_ok() {
        crate::page::projects::tell(&app);
        super::marks::tell(&app);
    }
    answer(done.map(|()| json!({"ok": true})))
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
    if id == DEFAULT_PROJECT {
        return refuse(StatusCode::BAD_REQUEST, "Default is no real project: it has no name or folder to change.");
    }
    let (name, dir) = (text(&body, "name"), text(&body, "dir"));
    let a = app.clone();
    let done = off(move || a.projects.lock().edit(&id, name.as_deref(), dir)).await;
    told_projects(&app, done.map(|()| json!({"ok": true})))
}

fn text(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::to_string)
}

fn ids(v: &Value, key: &str) -> Vec<String> {
    v[key].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

async fn make(State(app): State<Shared>, Json(body): Json<Value>) -> Response {
    if session_gone(&app, &body) {
        return refuse(StatusCode::BAD_REQUEST, "That session is gone.");
    }
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
        if let Some(up) = &t.parent {
            told(&app, up);                            // its card counts it, its window lists it (AC-112)
        }
    }
    answer(done.map(|t| card(&app, &t)))
}

/// A task's window: its fields, text, comments and history, files.
async fn window(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let wanted = id.clone();
    let done = store(&app, move |tx| {
        let t = ops::get(tx, &wanted)?;
        let held = runs::holding(tx, &t.key)?.map(|r| r.names());
        let agent = runs::card_agents(tx, &[&t.key])?.remove(&t.key);
        let kids = query::subtasks(tx, &t.key)?;
        Ok((ops::text_of(tx, &wanted)?, ops::history(tx, &wanted)?, files::of_task(tx, &wanted)?, held, agent, kids, t))
    }).await;
    answer(done.map(|(body, history, attached, held, agent, kids, t)| {
        let mut v = card(&app, &t);
        v["children"] = json!(kids);
        v["text_rev"] = json!(text_rev(&body));
        v["text"] = json!(body);
        v["history"] = history;
        v["files"] = json!(attached);
        v["run_holds"] = json!(held);
        if let Some(agent) = agent {
            v["agent"] = agent;
        }
        v
    }))
}

/// What a human changes on a card.
async fn change(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Value>) -> Response {
    if session_gone(&app, &body) {
        return refuse(StatusCode::BAD_REQUEST, "That session is gone.");
    }
    let began_from = text(&body, "text_rev");
    // a task moves when it runs and when its run ends: a human runs it (terminum-dispatcher.md AC-138)
    let to_status = text(&body, "status");
    if to_status.is_some() && !crate::config::test_store() {   // the live tests place cards by it
        return refuse(StatusCode::BAD_REQUEST, "A task moves by running it: drop it on an agent, or press Run.");
    }
    let wanted = id.clone();
    let to_project = text(&body, "project");
    let choice = text(&body, "session_choice");
    let edit = Edit { title: text(&body, "title"), text: text(&body, "text"), severity: text(&body, "severity"), owner: text(&body, "owner") };
    let dropped = ids(&body, "files");
    let before = body.get("before").filter(|b| !b.is_null());
    let reorder = before.is_some() || body.get("order_end").is_some();
    let before = before.and_then(Value::as_str).map(str::to_string);
    let session = body.get("session").cloned();
    // only null takes it off: another value is refused, never read as none (AC-126)
    let budget = match body.get("budget_minutes") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => match v.as_i64() {
            Some(m) => Some(Some(m)),
            None => return refuse(StatusCode::BAD_REQUEST, "Budget: 5 minutes to 7 days"),
        },
    };
    let done = store(&app, move |tx| {
        let at = ops::now();
        let was = ops::get(tx, &wanted)?;
        // the text is no longer the one the human began from: the page asks (AC-133)
        if began_from.is_some_and(|rev| rev != text_rev(&ops::text_of(tx, &wanted).unwrap_or_default())) {
            return Ok(Err((StatusCode::PRECONDITION_FAILED, "The text changed meanwhile.")));
        }
        // its run holds it and its session until it ends, or the human gives up (AC-138)
        if (to_project.is_some() || session.is_some()) && runs::holding(tx, &was.key)?.is_some() {
            return Ok(Err((StatusCode::BAD_REQUEST, "Its run holds it: give up on the run first.")));
        }
        let mut follow = None;
        if let Some(project) = &to_project {
            match ops::change_project(tx, HUMAN, &wanted, project, choice.as_deref(), at)? {
                // it has a session: the page's dialog asks what becomes of it, and sends `session_choice`
                ProjectMove::Ask => return Ok(Err((StatusCode::CONFLICT, "session_choice"))),
                ProjectMove::SessionFollows(s) => follow = Some((s, project.clone())),
                ProjectMove::Done => {}
            }
        }
        if let Some(s) = &session {
            ops::link(tx, HUMAN, &wanted, s.as_str(), at)?;
        }
        ops::edit(tx, HUMAN, &wanted, edit, at)?;
        files::attach(tx, &dropped, &wanted, None)?;   // what was dropped in its text as it was edited (AC-109)
        if let Some(minutes) = budget {
            ops::set_budget(tx, HUMAN, &wanted, minutes, at)?;
        }
        if let Some(status) = &to_status {
            ops::move_to(tx, HUMAN, &wanted, status, at)?;
        }
        if reorder {
            ops::reorder(tx, &wanted, before.as_deref(), at)?;
        }
        Ok(Ok((ops::get(tx, &wanted)?, follow)))
    }).await;
    let (task, follow) = match done {
        Ok(Ok(d)) => d,
        Ok(Err((status, why))) => return refuse(status, why),
        Err(e) => return refuse(StatusCode::BAD_REQUEST, e),
    };
    let note = match follow {
        Some((session, project)) => follow_session(&app, session, project).await.err(),
        None => None,
    };
    if budget.is_some() {
        crate::dispatcher::api::budget_changed(&app, &task);
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
    let joined = off(move || {
        let mut ps = a.projects.lock();
        if project == DEFAULT_PROJECT { ps.remove(&session) } else { ps.add(&session, Some(&project), None, host, String::new(), None).map(|_| ()) }
    }).await;
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
    told_answer(&app, &id, done)
}

async fn edit_comment(State(app): State<Shared>, Path((id, event)): Path<(String, String)>, Json(body): Json<Value>) -> Response {
    let (wanted, said) = (id.clone(), text(&body, "text").unwrap_or_default());
    let done = store(&app, move |tx| ops::change_comment(tx, HUMAN, &wanted, &event, Some(&said), ops::now()).map(|()| json!({"ok": true}))).await;
    told_answer(&app, &id, done)
}

async fn drop_comment(State(app): State<Shared>, Path((id, event)): Path<(String, String)>) -> Response {
    let wanted = id.clone();
    let done = store(&app, move |tx| ops::change_comment(tx, HUMAN, &wanted, &event, None, ops::now()).map(|()| json!({"ok": true}))).await;
    if done.is_ok() {
        scrub(&app).await;
    }
    told_answer(&app, &id, done)
}

/// A purge's text also leaves the store's write-ahead log (AC-109).
async fn scrub(app: &Shared) {
    let a = app.clone();
    let _ = tokio::task::spawn_blocking(move || a.tasks.checkpoint()).await;
}

/// Off the board, or back (AC-118); a run that holds it ends first.
async fn archive(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Value>) -> Response {
    let (wanted, on) = (id.clone(), body["on"].as_bool().unwrap_or(true));
    let done = store(&app, move |tx| {
        let key = ops::key_of(tx, &wanted)?;
        if runs::holding(tx, &key)?.is_some() {
            return Err("Its run holds it: give up on the run first.".to_string());
        }
        ops::archive(tx, HUMAN, &wanted, on, ops::now())
    }).await;
    told_answer(&app, &id, done.map(|()| json!({"ok": true})))
}

async fn purge(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let wanted = id.clone();
    let done = store(&app, move |tx| ops::purge_text(tx, HUMAN, &wanted, ops::now()).map(|()| json!({"ok": true}))).await;
    if done.is_ok() {
        scrub(&app).await;
    }
    told_answer(&app, &id, done)
}

async fn upload(State(app): State<Shared>, Params(q): Params<HashMap<String, String>>, data: Bytes) -> Response {
    let name = q.get("name").cloned().unwrap_or_default();
    let a = app.clone();
    answer(off(move || files::keep(&a.tasks, &a.cfg.dir, &name, &data, ops::now())).await)
}

/// Bytes with the header sign-in only: a raster picture shows, all else downloads (AC-109).
async fn download(State(app): State<Shared>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let a = app.clone();
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).map(str::to_string);
    let done = off(move || {
        let (file, bytes, inline) = files::open(&a.tasks, &a.cfg.dir, &id)?;
        let len = bytes.metadata().map_err(|e| format!("{}: {e}", file.name))?.len();
        Ok((file, inline, crate::raw_body::response(bytes, len, range.as_deref())))
    }).await;
    let (file, inline, mut res) = match done {
        Ok(d) => d,
        Err(e) => return refuse(StatusCode::NOT_FOUND, e),
    };
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
    let done = off(move || files::delete(&a.tasks, &a.cfg.dir, &id, HUMAN, ops::now())).await;
    if let Ok(Some(task)) = &done {
        told(&app, task);
    }
    answer(done.map(|_| json!({"ok": true})))
}
