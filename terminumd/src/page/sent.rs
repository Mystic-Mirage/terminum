// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Sent history from the page (AC-90): `sent {text, agent, line}` keeps what stood at the
//! cursor of the shown pane when the user pressed Enter, and the history line it began on (a
//! coding agent whose hooks spoke gives its prompt itself, sock.rs: the page's only leaves its
//! line for it); `sentget {n}` answers `sentone {sid, n, total, item, host, path}`, the n-th
//! newest of the shown pane's project; `sentmarks` answers `sentmarks {sid, items: [{n, text}],
//! ends: [{n, text}]}`, where the newest MARKS of the shown pane's own messages and of its
//! agent's answers' ends (answers.rs) stand now, found by their text near their line.
//! `askno` answers `askno {sid, n}`, the shown pane's next question number (AC-37 Ask). Every
//! page hears `sentnew {host, path, key}` when a project gained one (in pane `key`).

use std::sync::Arc;

use serde_json::{json, Value};

use super::Page;
use crate::app::Shared;

/// Keeps `text` sent in pane `key` of project (`host`, `path`) and tells the pages; a failure
/// is logged (the user's command went to the terminal all the same).
pub async fn keep(app: &Shared, host: &str, path: &str, key: &str, text: &str, mut info: Value) {
    if info["title"].is_null() {
        info["title"] = json!(title_of(app, key));
    }
    if info["line"].is_null() && let Some(line) = app.sent.take_hint(key, text) {
        info["line"] = json!(line);                     // a hook's prompt: where the page saw it
    }
    let (a, host, path, key, text) = (app.clone(), host.to_string(), path.to_string(), key.to_string(), text.to_string());
    let k = key.clone();
    let (h, p) = (host.clone(), path.clone());
    match tokio::task::spawn_blocking(move || a.sent.add(&h, &p, &key, &text, info)).await.map_err(|e| e.to_string()).and_then(|r| r) {
        Ok(true) => app.pages.tell(&json!({"t": "sentnew", "host": host, "path": path, "key": k}).to_string()),
        Ok(false) => {}
        Err(e) => tracing::warn!(event = "sent.unsaved", error = %e),
    }
}

fn title_of(app: &Shared, key: &str) -> String {
    super::marks::find_item(&app.layout.read(), key).and_then(|it| it["title"].as_str().map(str::to_string)).unwrap_or_default()
}

/// `at`: the pane shown when the page sent it: what was typed there stays that pane's (AC-132).
pub async fn handle(app: &Shared, page: &Arc<Page>, msg: &Value, at: super::client::At) {
    let t = msg["t"].as_str().unwrap_or("");
    let agent = msg["agent"].as_str().filter(|a| !a.is_empty());
    let shown_key = at.as_ref().map(|(k, ..)| k.clone());
    let hooked = shown_key.as_ref().is_some_and(|k| app.hooks.has(k));
    let line = msg["line"].as_i64();
    if t == "sent" && agent.is_some() && hooked {
        // its hooks give the prompt exactly; where it stood is the page's
        if let (Some(key), Some(line), Some(text)) = (shown_key, line, msg["text"].as_str()) {
            app.sent.hint(&key, text, line);
        }
        return;
    }
    if t == "sentmarks" {
        let (app, page) = (app.clone(), page.clone());
        tokio::spawn(async move { marks(&app, &page).await });
        return;
    }
    // the project: the pane's host ("local" for every server of this Mac) and folder
    let (key, host, path) = match &at {
        None => return { page.send_json(&json!({"t": "sentone", "error": "No pane is shown."})); },
        Some((key, server, pane)) => match super::client::cwd_of(server, pane).await {
            Ok(path) => (key.clone(), server.name.clone(), path),
            Err(e) => return { page.send_json(&json!({"t": "sentone", "error": e})); },
        },
    };
    if t == "askno" {
        let (a, h, p, k) = (app.clone(), host.clone(), path.clone(), key.clone());
        match tokio::task::spawn_blocking(move || a.sent.next_question(&h, &p, &k)).await.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok(n) => page.send_json(&json!({"t": "askno", "sid": key, "n": n})),
            Err(e) => page.send_json(&json!({"t": "askno", "sid": key, "error": format!("The quote's number could not be kept: {e}")})),
        };
        return;
    }
    if t == "asked" || t == "askedmarks" {
        return asked(app, page, msg, &key, &host, &path).await;
    }
    if t == "sent" {
        let text = msg["text"].as_str().unwrap_or("");
        return keep(app, &host, &path, &key, text, json!({"agent": agent, "via": "page", "line": line})).await;
    }
    let n = msg["n"].as_u64().unwrap_or(0) as usize;
    let a = app.clone();
    let (h, p) = (host.clone(), path.clone());
    let (total, item) = tokio::task::spawn_blocking(move || a.sent.get(&h, &p, n)).await.unwrap_or((0, None));
    page.send_json(&json!({"t": "sentone", "sid": key, "n": n, "total": total, "item": item, "host": host, "path": path}));
}

/// `asked {n, text, line}` keeps a quote the shown pane was asked about (AC-37) and tells
/// every page `askednew {key}`; both it and `askedmarks` answer `askedmarks {sid, items: [{n,
/// text, line}]}`, the pane's quotes, which the page underlines where they stand.
async fn asked(app: &Shared, page: &Arc<Page>, msg: &Value, key: &str, host: &str, path: &str) {
    let (a, h, p, k) = (app.clone(), host.to_string(), path.to_string(), key.to_string());
    let keep = msg["t"] == "asked";
    let (n, text, line) = (msg["n"].as_u64(), msg["text"].as_str().unwrap_or("").to_string(), msg["line"].as_i64());
    let (kept, items) = tokio::task::spawn_blocking(move || {
        let kept = match (keep, n, line) {
            (false, ..) => Ok(false),
            (true, Some(n), Some(line)) if !text.trim().is_empty() => a.sent.keep_asked(&h, &p, &k, n, &text, line).map(|_| true),
            (true, ..) => Err("a quote needs its number, text and line".to_string()),
        };
        (kept, a.sent.asked_of(&h, &p, &k))
    }).await.unwrap_or_else(|e| (Err(e.to_string()), Vec::new()));
    match kept {
        Ok(true) => app.pages.tell(&json!({"t": "askednew", "key": key}).to_string()),
        Ok(false) => {}
        Err(e) => tracing::warn!(event = "asked.unsaved", error = %e),
    }
    page.send_json(&json!({"t": "askedmarks", "sid": key, "items": items}));
}

/// The newest messages of the shown pane's own placed on the sent history's strip (AC-90).
const MARKS: usize = 200;

/// Where the shown pane's newest MARKS messages stand now: each found by its text nearest the
/// line it was sent on (one not found is left out); read beside typing.
async fn marks(app: &Shared, page: &Arc<Page>) {
    let Some((key, server, pane, ctl)) = super::find::shown(page) else { return };
    let Ok((_, s, path)) = super::client::shown_cwd(page).await else { return };
    let (a, host, k) = (app.clone(), s.name.clone(), key.clone());
    let (sent, ends) = tokio::task::spawn_blocking(move || (a.sent.of_pane(&host, &path, &k, MARKS), a.sent.ends_of_pane(&host, &path, &k, MARKS))).await.unwrap_or_default();
    let (items, ends): (Vec<Value>, Vec<Value>) = if sent.is_empty() && ends.is_empty() { Default::default() } else {
        let rows = match super::find::rows_of(&server, &pane, &ctl, app.cfg.history, None).await {
            Ok(rows) => rows,
            Err(e) => return tracing::debug!(event = "sentmarks.unread", error = %e),
        };
        tokio::task::spawn_blocking(move || {
            let rows: Vec<(i64, String)> = rows.into_iter().map(|(n, t, _)| (n, t)).collect();
            // each as its transcript shows it (a folded paste left out: crate::sent::findable)
            // an agent's prompt only on its prompt row (find_each), not where it is quoted
            let placed = |list: Vec<(String, i64, bool)>| -> Vec<Value> {
                let texts: Vec<super::bookmarks_find::Wanted> = list.into_iter()
                    .map(|(t, l, prompt)| super::bookmarks_find::Wanted { text: crate::sent::findable(&t), hint: Some(l), prompt })
                    .filter(|w| !w.text.is_empty()).collect();
                super::bookmarks_find::find_each(&rows, &texts).into_iter().zip(texts)
                    .filter_map(|(n, w)| Some(json!({"n": n?, "prompt": w.prompt, "text": w.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(120).collect::<String>()})))
                    .collect()
            };
            (placed(sent), placed(ends))
        }).await.unwrap_or_default()
    };
    page.send_json(&json!({"t": "sentmarks", "sid": key, "items": items, "ends": ends}));
}
