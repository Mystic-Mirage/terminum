// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The CLI's way in (AC-08, AC-09): `~/.terminum/state/terminumd.sock`, a Unix socket in the
//! private state folder, never a sign-in over TCP. `terminum status`, `open`, `password`,
//! `signout` and `hosts` talk to it; another program cannot.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app::Shared;
use crate::http::err;

/// Unix socket paths end at 104 bytes on macOS, the terminating NUL included.
const SUN_PATH_MAX: usize = 103;

pub fn path(dir: &Path) -> PathBuf {
    dir.join("terminumd.sock")
}

fn fail(msg: String) -> ! {
    eprintln!("error: {msg}");
    tracing::error!(event = "start.failed", msg);
    std::process::exit(2);
}

/// The CLI socket; refuses to start when another terminumd answers on it, replaces a
/// socket left by one that is gone.
pub fn bind(dir: &Path) -> tokio::net::UnixListener {
    let p = path(dir);
    if p.as_os_str().len() > SUN_PATH_MAX {
        fail(format!("{} is too long for a socket (set TM_APP_DIR to a shorter folder)", p.display()));
    }
    if std::os::unix::net::UnixStream::connect(&p).is_ok() {
        fail(format!("another terminumd uses {}", dir.display()));
    }
    let _ = std::fs::remove_file(&p);
    let l = tokio::net::UnixListener::bind(&p).unwrap_or_else(|e| fail(format!("{}: {e}", p.display())));
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    l
}

/// The loopback port when launchd does not hold it (activation.rs); a previous terminumd
/// still shutting down is waited for (3 s). A port another program holds ends terminumd with
/// the message the log and `terminum status` show (AC-08).
pub async fn bind_port(dir: &Path, port: u16) -> tokio::net::TcpListener {
    for attempt in 0.. {
        match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => return l,
            Err(e) => {
                let ours = std::os::unix::net::UnixStream::connect(path(dir)).is_ok();
                if attempt == 15 {
                    if ours {
                        fail(format!("port {port}: another terminumd of this install runs ({e})"));
                    }
                    no_port(format!("port {port} is in use (set `port` in ~/.terminum/settings.json)"));
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
    unreachable!()
}

/// launchd's socket is unbound (another program held the port at bootstrap): says why once and
/// waits for `terminum restart`; exiting would only have launchd start it again on the same
/// unbound socket every 10 s.
pub fn unbound(msg: String) -> ! {
    eprintln!("error: {msg}");
    tracing::error!(event = "start.failed", msg);
    loop {
        std::thread::park();
    }
}

/// No port to serve on: says why in the log (`terminum status` reads it there), waits so
/// launchd, which starts terminumd again at once, tries every 10 s (AC-08), and exits.
pub fn no_port(msg: String) -> ! {
    eprintln!("error: {msg}");
    tracing::error!(event = "start.failed", msg);
    let wait = std::env::var("TM_PORT_WAIT_S").ok().and_then(|s| s.parse().ok()).unwrap_or(9);
    std::thread::sleep(std::time::Duration::from_secs(wait));
    std::process::exit(2);
}

pub fn routes(app: Shared) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/password", post(password))
        .route("/signout", post(signout))
        .route("/hosts", get(hosts))
        .route("/hosts/{action}", post(host_action))
        .route("/tmux", get(tmux_list))
        .route("/tmux/{action}", post(tmux_action))
        .route("/restore", post(restore))
        .route("/hook", post(hook))
        .route("/agent", post(crate::agentapi::agent))
        .route("/folders", post(folders))
        .with_state(app)
}

/// A coding agent's hook event (`terminum hook`, AC-39), for a pane of a tmux terminumd knows:
/// it changes what the list shows (and keeps a prompt, AC-15 sent history), nothing else.
async fn hook(State(app): State<Shared>, Json(ev): Json<Value>) -> Response {
    let server = crate::links::resolve(&app, crate::links::Via::Local, &ev).await;
    hook_on(app, server, ev).await
}

/// A hook event for the server it was resolved to (None: no server of the event's).
pub async fn hook_on(app: Shared, server: Option<std::sync::Arc<crate::tmux::server::Server>>, ev: Value) -> Response {
    let pane = ev["pane"].as_str().unwrap_or("");
    let Some(server) = server else { return StatusCode::NO_CONTENT.into_response() };
    if server.session_of(pane).is_none() {
        return StatusCode::NO_CONTENT.into_response();  // a pane it does not list: nothing is made up
    }
    if ev["event"] == "Tokens" {
        app.dispatcher.heard_tokens(&server.key(pane), &ev);   // terminum-dispatcher.md AC-129: asked for below
        return StatusCode::NO_CONTENT.into_response();
    }
    let _dispatch = app.hooks.dispatch.lock();
    let key = server.key(pane);
    let Some(changed) = app.hooks.apply_event(&key, &ev, std::time::Instant::now()) else {
        return StatusCode::NO_CONTENT.into_response();
    };
    if changed {
        app.dirty.notify_one();
    }
    // a turn ended (its last background subagent too): its reminders come up with the list
    // read this notice brings, also when it was too quick for a layout to show it working,
    // once its screen shows no work in the background (AC-91, page/turns.rs); also a Stop
    // that changed nothing the list shows
    if app.hooks.take_ended(&key) {
        crate::manager::hook_turn_ended(&app, &key, ev["event"].as_str().unwrap_or(""));
        crate::tasks::tell::turn_ended(&app, &key);
    }
    if ev["event"] == "UserPromptSubmit" && let (Some(prompt), Some(cwd)) = (ev["prompt"].as_str(), ev["cwd"].as_str()) {
        let (prompt, cwd, info) = (prompt.to_string(), cwd.to_string(), json!({"agent": ev["agent"], "via": "hook"}));
        let (app, key, host) = (app.clone(), key.clone(), server.name.clone());
        tokio::spawn(async move { crate::page::sent::keep(&app, &host, &cwd, &key, &prompt, info).await });
    }
    crate::tasks::tell::session_ended(&app, &key, &ev);
    crate::dispatcher::heard(&app, &key, &ev);
    let remind = crate::plans::hook_event(&app, &key, &ev);
    // what waits for it rides the same event
    let context = if crate::plans::carries_context(&ev) { crate::tasks::tell::with_lines(&app, &key, remind) } else { remind };
    // a task's session: its hook sums the tokens, after this turn's end has gone (AC-129)
    let tokens = matches!(ev["event"].as_str(), Some("Stop" | "SessionEnd")) && (app.dispatcher.wants_tokens(&key) || app.task_marks.has_session(&key));
    match (context, tokens) {
        (None, false) => StatusCode::NO_CONTENT.into_response(),
        (text, _) => {
            let mut answer = json!({"tokens": tokens});
            if let Some(text) = text {
                answer["remind"] = json!(text);
            }
            Json(answer).into_response()
        }
    }
}

#[cfg(test)]
mod hook_tests {
    use super::*;

    #[test]
    fn hook_dispatch_waits_before_accepting_identity_or_mutating_plans() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let _entered = rt.enter();
        let (root, app) = crate::testutil::app("hook-dispatch-order");
        let server = app.tmux.local.clone();
        server.set_panes(std::collections::HashMap::from([("%1".to_string(), "main".to_string())]));
        let key = server.key("%1");
        let serial = app.hooks.dispatch.lock();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let a = app.clone();
        let worker = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            started_tx.send(()).unwrap();
            let response = rt.block_on(hook_on(a, Some(server), json!({"agent": "claude", "event": "UserPromptSubmit",
                "pane": "%1", "session": "new", "plan": {"op": "all", "items": [{"text": "current"}]}})));
            done_tx.send(response.status()).unwrap();
        });
        started_rx.recv().unwrap();
        let blocked = done_rx.recv_timeout(std::time::Duration::from_millis(50)).is_err();
        let untouched = app.plans.details(&key, false, 2).is_none() && !app.hooks.working(&key);
        drop(serial);
        worker.join().unwrap();
        assert!(blocked && untouched, "hook acceptance and side effects bypassed dispatch serialization");
        assert_eq!(app.plans.details(&key, false, 2).unwrap().items[0].text, "current");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rejected_session_events_have_no_plan_or_turn_side_effects() {
        let (root, app) = crate::testutil::app("stale-hook-side-effects");
        let server = app.tmux.local.clone();
        server.set_panes(std::collections::HashMap::from([("%1".to_string(), "main".to_string())]));
        let key = server.key("%1");
        app.hooks.apply_event(&key, &json!({"agent": "claude", "event": "UserPromptSubmit", "session": "new"}), std::time::Instant::now());
        app.plans.hook(&key, "claude", Some("new"), &json!({"op": "all", "items": [{"text": "current"}]}), 1);
        for event in ["PostToolUse", "SessionEnd"] {
            let response = hook_on(app.clone(), Some(server.clone()), json!({"agent": "claude", "event": event,
                "pane": "%1", "session": "old", "plan": {"op": "all", "items": [{"text": "stale"}]}})).await;
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            assert_eq!(app.plans.details(&key, false, 2).unwrap().items[0].text, "current");
            assert!(app.hooks.working(&key));
            assert!(!app.hooks.take_ended(&key));
        }
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn rss_mb() -> f64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: getrusage writes into a valid rusage
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    // ru_maxrss is in bytes on macOS, in KB on Linux
    let bytes = if cfg!(target_os = "macos") { usage.ru_maxrss as f64 } else { usage.ru_maxrss as f64 * 1024.0 };
    (bytes / 1048576.0 * 10.0).round() / 10.0
}

/// The program that started terminum's tmux server, by which macOS judges its terminals'
/// folders (AC-08): None when no server runs or it did not say (a version before, by hand: "").
async fn started_by(app: &Shared) -> Option<String> {
    let lines = app.tmux.local.run("show-options -gqv @terminum-started-by").await.ok()?;
    Some(lines.into_iter().next().unwrap_or_default().trim().to_string())
}

#[derive(Deserialize, Default)]
struct Wait {
    #[serde(default)]
    wait: f64,
}

/// The wait asked for, held to 0..120 s; None when it is no number of seconds (NaN, infinite).
fn wait_of(s: f64) -> Option<std::time::Duration> {
    s.is_finite().then(|| std::time::Duration::from_secs_f64(s.clamp(0.0, 120.0)))
}

/// `terminum install` and `terminum folders`: each protected folder opened by terminumd itself,
/// so macOS asks for it while the user is at the Mac (AC-08); said as it stands after `wait` s.
async fn folders(State(app): State<Shared>, axum::extract::Query(q): axum::extract::Query<Wait>) -> Response {
    let Some(wait) = wait_of(q.wait) else { return err(StatusCode::BAD_REQUEST, "bad_wait", "wait: a number of seconds") };
    let mut v = if app.cfg.service {
        crate::folders::ask(&crate::config::home(), wait).await
    } else {
        json!({"done": true, "folders": []})
    };
    v["service"] = json!(app.cfg.service);
    v["tmux_started_by"] = json!(started_by(&app).await);
    Json(v).into_response()
}

/// A group's rows that are as last read from a server that answers: one cut off keeps its rows, dimmed (AC-81).
fn answering(items: &Value) -> usize {
    items.as_array().map_or(0, |a| a.iter().filter(|it| it["offline"] != true).count())
}

/// `terminum status`: what runs (AC-08).
async fn health(State(app): State<Shared>) -> Json<Value> {
    let servers: Vec<Value> = app.tmux.all().iter().map(|s| {
        let groups = app.layout.read().as_array().cloned().unwrap_or_default();
        let mine: Vec<&Value> = groups.iter().filter(|g| g["server"] == s.id().as_str()).collect();
        let name = if s.is_users() { s.label() } else { s.name.clone() };
        json!({"name": name, "socket": s.socket, "state": *s.status.lock(), "connected": s.connected(),
               "sessions": mine.iter().filter_map(|g| g["wid"].as_str()).collect::<Vec<_>>(),
               "panes": mine.iter().map(|g| answering(&g["items"])).sum::<usize>()})
    }).collect();
    let problem = app.tmux_problem.lock().clone();
    Json(json!({
        "build": crate::config::build_id(), "pid": std::process::id(), "uptime_s": app.started.elapsed().as_secs(),
        "port": app.cfg.port, "password": crate::webaccess::status(&app), "servers": servers,
        "hosts": crate::hosts::list(&app), "pages": app.pages.count(),
        "tmux": {"ok": problem.is_none(), "message": problem, "started_by": started_by(&app).await},
        "service": app.cfg.service,
        "signins": app.signins.count(), "workspaces": app.store.len(),
        "denied_requests": app.denied.load(Ordering::Relaxed), "rss_mb": rss_mb(),
        "hooks": app.hooks.last.lock().iter().map(|(a, t)| (a.to_string(), json!(t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()))))
            .collect::<serde_json::Map<_, _>>(),
    }))
}

#[derive(Deserialize, Default)]
struct Password {
    #[serde(default)]
    password: String,
}

/// `terminum password` (AC-09).
async fn password(State(app): State<Shared>, body: Option<Json<Password>>) -> Response {
    match crate::webaccess::set_password(&app, &body.map(|b| b.0.password).unwrap_or_default()).await {
        Ok(v) => Json(json!({"ok": true, "password": v})).into_response(),
        Err(e) => err(StatusCode::CONFLICT, "password", e),
    }
}

/// `terminum signout`: every browser signs in again (AC-09).
async fn signout(State(app): State<Shared>) -> Response {
    match crate::webaccess::sign_out_all(&app).await {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, "not_saved", e),
    }
}

async fn hosts(State(app): State<Shared>) -> Json<Value> {
    Json(json!({"hosts": crate::hosts::list(&app)}))
}

#[derive(Deserialize, Default)]
struct HostBody {
    #[serde(default)]
    name: String,
    #[serde(default)]
    recipe: String,
    #[serde(default)]
    files: bool,
}

/// `terminum hosts add|remove|enable|files-remove|off|on|retry` (AC-87), `hosts agents` (AC-99).
async fn host_action(State(app): State<Shared>, UrlPath(action): UrlPath<String>, Json(b): Json<HostBody>) -> Response {
    let mut message = None;
    let result = match action.as_str() {
        "add" => crate::hosts::add(&app, &b.name, &b.recipe, b.files).await,
        // AC-99: the per-agent result lines are the CLI's message
        "agents-on" | "agents-off" => crate::hosts::agents::set_agents(&app, &b.name, action == "agents-on").await.map(|lines| {
            message = Some(lines.join("\n"));
            b.name
        }),
        _ => crate::hosts::action(&app, &b.name, &action).await.map(|()| b.name),
    };
    match result {
        Ok(name) => {
            crate::lists::request_all(&app);
            Json(json!({"ok": true, "name": name, "message": message, "hosts": crate::hosts::list(&app)})).into_response()
        }
        Err(e) => err(StatusCode::CONFLICT, "host", e),
    }
}

/// `terminum tmux`: each attached session, here and on hosts, and whether it runs (AC-89).
async fn tmux_list(State(app): State<Shared>) -> Json<Value> {
    use crate::attach::Place;
    let groups = app.layout.read().as_array().cloned().unwrap_or_default();
    let attached: Vec<Value> = crate::attach::every(&app).into_iter().map(|(place, s)| {
        let g = groups.iter().find(|g| !g["attached"].is_null() && g["server"] == place.id().as_str() && g["label"] == s.as_str());
        let state = match g {
            Some(g) if g["state"] == "absent" => g["why"].as_str().unwrap_or("not running").to_string(),
            Some(g) => format!("attached, {} panes", answering(&g["items"])),
            None => "not read yet".into(),
        };
        let (socket, host) = match &place {
            Place::Local(socket) => (Some(socket.clone()), None),
            Place::Host(h) => (None, Some(h.clone())),
        };
        json!({"session": s, "socket": socket, "host": host, "server": place.id(), "state": state})
    }).collect();
    Json(json!({"attached": attached}))
}

#[derive(Deserialize, Default)]
struct TmuxBody {
    #[serde(default)]
    session: String,
    #[serde(default)]
    socket: String,
    #[serde(default)]
    host: String,
}

/// `terminum tmux attach|detach <session> [--socket S | --host H]` (AC-89).
async fn tmux_action(State(app): State<Shared>, UrlPath(action): UrlPath<String>, Json(b): Json<TmuxBody>) -> Response {
    let id = match (b.host.as_str(), b.socket.as_str()) {
        ("", "") => String::new(),
        ("", socket) => format!("local:{socket}"),
        (host, _) => format!("{host}:default"),
    };
    let place = crate::attach::Place::parse(&app, &id);
    let result = match (action.as_str(), &place) {
        (_, Err(e)) => Err(e.clone()),
        ("attach", Ok(p)) => crate::attach::add(&app, p, std::slice::from_ref(&b.session)),
        ("detach", Ok(p)) => crate::attach::detach(&app, p, &b.session).await,
        _ => Err(format!("unknown tmux action: {action}")),
    };
    match result {
        Ok(()) => {
            match place.ok().and_then(|p| p.server(&app)) {
                Some(server) => crate::lists::refresh_server(&app, &server).await,
                None => crate::lists::request_all(&app),
            }
            Json(json!({"ok": true})).into_response()
        }
        Err(e) => err(StatusCode::CONFLICT, "tmux", e),
    }
}

/// "Restore again" from the CLI (AC-15).
async fn restore(State(app): State<Shared>) -> Response {
    match crate::restore::again(&app).await {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(e) => err(StatusCode::CONFLICT, "restore", e),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_wait_is_a_number_of_seconds_held_to_two_minutes() {
        use std::time::Duration;
        assert_eq!(super::wait_of(5.0), Some(Duration::from_secs(5)));
        assert_eq!(super::wait_of(-1.0), Some(Duration::ZERO));
        assert_eq!(super::wait_of(1e9), Some(Duration::from_secs(120)));
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(super::wait_of(bad), None, "{bad}");
        }
    }
}
