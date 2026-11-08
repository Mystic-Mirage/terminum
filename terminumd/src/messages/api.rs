// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The Message history's and the Dashboard's routes (terminum.md AC-151, AC-152), behind the
//! page's sign-in; reads only, off the async workers.
//!
//! `GET /api/messages?q=&sort=best|new&from=&to=&project=a,b&sender=&kind=&pane=&cursor=`: a page
//! (search::page), each item with `live` (its pane is in the session list), the first page with
//! the day strip's `days`; `GET /api/messages/meta`: the filters' projects, first day, hosts, the
//! page's theme;
//! `GET /api/messages/{id}/context?before=&after=`; `GET /api/dashboard?days=&project=`.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use serde_json::Value;

use super::search::{self, Filter};
use crate::app::Shared;
use crate::tasks::api::{answer, store};

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/messages", get(list))
        .route("/api/messages/meta", get(meta))
        .route("/api/messages/{id}/context", get(context))
        .route("/api/dashboard", get(dashboard))
}

type Params = Query<HashMap<String, String>>;

/// The filter a query asks for; unknown values are no filter.
pub fn filter(q: &HashMap<String, String>) -> Filter {
    let some = |k: &str| q.get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    Filter {
        q: q.get("q").cloned().unwrap_or_default().chars().take(500).collect(),
        from: some("from"),
        to: some("to"),
        // "-": no project (the page's word for the store's "")
        projects: q.get("project").filter(|p| !p.is_empty()).map(|p| p.split(',').map(|p| if p == "-" { String::new() } else { p.to_string() }).collect()).unwrap_or_default(),
        sender: some("sender").filter(|s| s == crate::tasks::store::HUMAN || s == crate::tasks::store::DISPATCHER),
        kind: some("kind").filter(|k| k == "agent" || k == "shell"),
        pane: some("pane"),
    }
}

/// Marks each item whose pane the session list still shows.
fn mark_live(app: &Shared, items: &mut [Value]) {
    let layout = app.layout.read();
    for it in items {
        let live = it["key"].as_str().is_some_and(|k| crate::page::marks::find_item(&layout, k).is_some());
        it["live"] = Value::Bool(live);
    }
}

async fn list(State(app): State<Shared>, Query(q): Params) -> Response {
    let f = filter(&q);
    let best = q.get("sort").map(String::as_str) != Some("new");
    let cursor = q.get("cursor").cloned();
    let done = store(&app, move |tx| {
        let mut page = search::page(tx, &f, best, cursor.as_deref())?;
        if cursor.is_none() {
            page["days"] = serde_json::json!(search::days(tx, &f)?);
        }
        Ok(page)
    }).await;
    answer(done.map(|mut page| {
        if let Some(items) = page["items"].as_array_mut() {
            mark_live(&app, items);
        }
        page
    }))
}

/// With the page's theme and color scheme, as the board's meta (AC-12).
async fn meta(State(app): State<Shared>) -> Response {
    let colors = colors(&app);
    answer(store(&app, search::meta).await.map(|mut m| {
        m["theme"] = serde_json::json!(app.looks.theme(None, "").1);
        m["scheme"] = serde_json::json!(app.looks.settings.read().page.color_scheme);
        if let Some(ps) = m["projects"].as_array_mut() {
            for p in ps {
                p["color"] = serde_json::json!(p["id"].as_str().and_then(|id| colors.get(id)));
            }
        }
        m
    }))
}

async fn context(State(app): State<Shared>, Path(id): Path<i64>, Query(q): Params) -> Response {
    let n = |k: &str| q.get(k).and_then(|v| v.parse::<usize>().ok()).unwrap_or(3).min(50);
    let (before, after) = (n("before"), n("after"));
    let done = store(&app, move |tx| search::context(tx, id, before, after)?.ok_or_else(|| "That message is no longer kept.".to_string())).await;
    answer(done.map(|mut c| {
        for side in ["before", "after"] {
            if let Some(items) = c[side].as_array_mut() {
                mark_live(&app, items);
            }
        }
        let mut one = [c["item"].take()];
        mark_live(&app, &mut one);
        let [it] = one;
        c["item"] = it;
        c
    }))
}

/// The session list's color of each project.
fn colors(app: &Shared) -> HashMap<String, usize> {
    app.projects.lock().list().iter().map(|p| (p.id.clone(), p.color)).collect()
}

async fn dashboard(State(app): State<Shared>, Query(q): Params) -> Response {
    let days = q.get("days").and_then(|d| d.parse::<i64>().ok()).unwrap_or(30);
    let project = q.get("project").cloned();
    let colors = colors(&app);
    answer(store(&app, move |tx| super::stats::dashboard(tx, crate::inputs::now_ms(), days, project.as_deref(), &colors)).await)
}
