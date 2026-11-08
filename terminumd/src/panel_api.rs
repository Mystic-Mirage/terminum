// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The Files panel's own endpoints beside the file operations (terminum-files.md §9): its
//! state (build, a host's files offer), events (SSE for an agent's relay), workspaces and
//! prefs (AC-55, AC-71), typing a path or a `cd` into the shown pane (AC-60), an agent's
//! watch list and health.

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::extract::{ConnectInfo, Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::app::Shared;
use crate::http::{err, ApiResult};
use crate::termtext;
use crate::workspace::Pane;

#[derive(Deserialize)]
pub struct StateQuery {
    host: Option<String>,
}

/// What the panel needs besides files: the build (another one means an upgrade: it reloads
/// keeping unsaved edits, AC-16) and, for a host's pane, its files offer (AC-83).
pub async fn get_state(State(app): State<Shared>, Query(q): Query<StateQuery>) -> Json<Value> {
    let mut s = json!({"version": 1, "bridge": true, "build": crate::config::build_id()});
    if let Some(h) = q.host.filter(|h| !h.is_empty()) {
        s["host"] = json!(h);
        s["remote"] = crate::hosts::offer(&app, &h);
    }
    Json(s)
}

/// An agent's event stream (the Mac's terminumd relays its fs-change events, AC-82).
pub async fn events(State(app): State<Shared>) -> Sse<impl Stream<Item = Result<SseEvent, std::io::Error>>> {
    let first = SseEvent::default().event("state").data(json!({"agent": true}).to_string());
    let rx = BroadcastStream::new(app.bus.subscribe()).map(|e| {
        e.map(|e| SseEvent::default().event(e.name()).data(e.data())).map_err(|e| std::io::Error::other(format!("event stream lost changes: {e}")))
    });
    Sse::new(tokio_stream::once(Ok(first)).chain(rx)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[derive(Deserialize)]
pub struct KeyQuery {
    key: String,
    /// the pane's folder now: the tree re-roots there when it moved (AC-51, OQ-51)
    cwd: Option<String>,
}

pub async fn get_workspace(State(app): State<Shared>, Query(q): Query<KeyQuery>) -> Json<Pane> {
    let moved = q.cwd.as_deref().filter(|c| c.starts_with('/')).is_some_and(|cwd| app.store.set_root(&q.key, cwd));
    if app.web_panes.touch(&q.key) || moved {
        app.refresh_watch();
    }
    Json(app.store.get(&q.key, None))
}

/// `X-TM-Client` names the panel that wrote: it ignores the echo of its own writes.
fn client(headers: &HeaderMap) -> Option<String> {
    headers.get("x-tm-client").and_then(|v| v.to_str().ok()).map(str::to_string)
}

pub async fn put_workspace(State(app): State<Shared>, Query(q): Query<KeyQuery>, headers: HeaderMap, Json(pane): Json<Pane>) -> Response {
    let fresh = app.web_panes.touch(&q.key);
    match app.store.put(&q.key, pane, client(&headers)) {
        Ok((rev, watched)) => {
            // a scroll or a selection changes nothing watched (the panel saves those often)
            if fresh || watched {
                app.refresh_watch();
            }
            Json(json!({"rev": rev})).into_response()
        }
        Err(cur) => (StatusCode::CONFLICT, Json(json!({"error": "stale_rev", "message": "Workspace changed; reload", "current": cur}))).into_response(),
    }
}

pub async fn get_prefs(State(app): State<Shared>) -> Json<Value> {
    Json(app.store.prefs())
}

pub async fn put_prefs(State(app): State<Shared>, Json(v): Json<Value>) -> StatusCode {
    app.store.set_prefs(v);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
pub struct TermBody {
    /// insert: paths typed (quoted) without Enter; cd: the folder
    #[serde(default)]
    paths: Vec<String>,
    path: Option<String>,
    /// the pane the panel shows (`web:[host, key]` or the key itself)
    key: String,
}

/// The pane key inside a panel's workspace key `web:["host","tmux:…"]`.
fn pane_key(key: &str) -> String {
    key.strip_prefix("web:").and_then(|k| serde_json::from_str::<[String; 2]>(k).ok()).map_or_else(|| key.to_string(), |[_, k]| k)
}

/// `POST /fb/api/terminal/{insert,cd}` (the Mac's page only): quoted for the pane's shell,
/// never Enter, `cd` only into an idle shell (AC-60).
pub async fn terminal(State(app): State<Shared>, UrlPath(action): UrlPath<String>, ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>, headers: HeaderMap, Json(b): Json<TermBody>) -> ApiResult {
    let key = pane_key(&b.key);
    let Some((server, pane)) = app.tmux.resolve(&key) else {
        return Err(err(StatusCode::CONFLICT, "pane_gone", "The pane has closed — command not sent"));
    };
    let job = server.run(format!("display -p -t {pane} '#{{pane_current_command}}'")).await
        .map_err(|e| err(StatusCode::CONFLICT, "pane_gone", format!("The pane did not answer — command not sent ({e})")))?
        .into_iter().next().unwrap_or_default();
    let shell = crate::tmux::titles::SHELLS.contains(&job.trim_start_matches('-'));
    if action == "cd" && !shell {
        return Err(err(StatusCode::CONFLICT, "busy", format!("Terminal is busy ({job}) — command not sent")));
    }
    let text = termtext::line(&job, &action, &b.paths, b.path.as_deref()).map_err(|m| err(StatusCode::BAD_REQUEST, "not_typed", m))?;
    let line = format!("send-keys -t {pane} -l -- {}", crate::tmux::args::quote(&text));   // a "-name" is text
    let took = server.run_all_on(&pane, vec![line]).await.into_iter().find_map(Result::err);
    let who = crate::inputs::Who::user("panel", None, &crate::webaccess::client(&headers, peer, false));
    crate::inputs::wrote(&app, who, "path", &server, &pane, &text, took.clone(), None);
    if let Some(e) = took {
        return Err(err(StatusCode::BAD_GATEWAY, "tmux", format!("tmux did not take the text: {e}")));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct WatchBody {
    root: String,
    #[serde(default)]
    expanded: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
}

/// An agent watches what the Mac shows of its host (AC-61): root, expanded folders, open files.
pub async fn watch(State(app): State<Shared>, Json(b): Json<WatchBody>) -> ApiResult {
    if !app.cfg.agent {
        return Err(err(StatusCode::NOT_FOUND, "not_found", "only an agent takes a watch list"));
    }
    if let Some(w) = &app.watcher {
        w.set(crate::watcher::wanted(&b.root, &b.expanded, b.files.into_iter()));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct HostBody {
    key: String,
}

/// The panel's answer to a host's files offer: enable, not now, remove (AC-83).
pub async fn remote_action(State(app): State<Shared>, UrlPath(action): UrlPath<String>, Json(b): Json<HostBody>) -> ApiResult {
    let action = match action.as_str() {
        "enable" => "enable",
        "dismiss" => "decline",
        "remove" => "files-remove",
        _ => return Err(err(StatusCode::NOT_FOUND, "not_found", "unknown action")),
    };
    crate::hosts::action(&app, &b.key, action).await.map_err(|e| err(StatusCode::BAD_GATEWAY, "host", e))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// An agent's health (the Mac's checks it through the tunnel).
pub async fn health(State(app): State<Shared>) -> Json<Value> {
    let (cache_bytes, cache_dirs) = app.cache.stats();
    Json(json!({
        "uptime_s": app.started.elapsed().as_secs(), "build": crate::config::build_id(), "agent_id": crate::agent::AGENT_ID,
        "cache_bytes": cache_bytes, "cache_dirs": cache_dirs, "workspaces": app.store.len(),
        "watched_dirs": app.watcher.as_ref().map_or(0, |w| w.len()), "writable_roots": app.roots.list(),
        "denied_requests": app.denied.load(Ordering::Relaxed),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_names_its_pane_inside_its_workspace_key() {
        assert_eq!(pane_key(r#"web:["","tmux:local:terminum:%3"]"#), "tmux:local:terminum:%3");
        assert_eq!(pane_key(r#"web:["devbox","tmux:devbox:default:%7"]"#), "tmux:devbox:default:%7");
        assert_eq!(pane_key("tmux:local:terminum:%3"), "tmux:local:terminum:%3");
    }
}
