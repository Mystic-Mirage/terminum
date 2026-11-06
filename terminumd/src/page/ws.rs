// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! `GET /ws`: the page's WebSocket (spec §9). The guard checked the sign-in and the Origin
//! before the upgrade; the page's secret came as the subprotocol pair ["tm", secret], so the
//! answer names "tm". The first message is the build (a page of another build reloads,
//! AC-16), then the list, its bookmarks and settings. Messages are handled in order; reads
//! that wait on tmux or ssh run beside the loop (`reads`). A closed page ends the reader, the
//! writer, the stream and its reads at once (`Page::closed`).

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};

use super::{client, Page};
use crate::app::Shared;
use crate::http::err;

/// A page message larger than this is refused (a paste goes in 1 MB parts at most).
const MAX_MESSAGE: usize = 2 << 20;
/// A page that takes longer than this to take a message is dropped.
const SEND_WAIT: Duration = Duration::from_secs(10);
const PING: Duration = Duration::from_secs(15);

/// Whether this socket is not the Mac's own browser's: it came through a proxy (the guard marks it).
#[derive(Clone, Copy)]
pub struct FromWeb(pub bool);

pub async fn socket(State(app): State<Shared>, headers: HeaderMap, from: Option<axum::Extension<FromWeb>>, ws: WebSocketUpgrade) -> Response {
    let web = from.is_some_and(|f| f.0.0);
    let Some((page, rx)) = app.pages.add(web, app.recorder.clone()) else {
        return err(StatusCode::SERVICE_UNAVAILABLE, "too_many_pages", "Too many pages");
    };
    let offered = headers.get("sec-websocket-protocol").and_then(|v| v.to_str().ok()).unwrap_or("");
    let ws = if offered.split(',').any(|p| p.trim() == "tm") { ws.protocols(["tm"]) } else { ws };
    ws.max_message_size(MAX_MESSAGE).on_upgrade(move |s| serve(app, page, rx, s)).into_response()
}

async fn serve(app: Shared, page: Arc<Page>, mut rx: tokio::sync::mpsc::Receiver<String>, ws: WebSocket) {
    tracing::info!(event = "page.open", page = page.id, web = page.web, pages = app.pages.count());
    let (mut tx, mut incoming) = ws.split();
    let writer = {
        let page = page.clone();
        tokio::spawn(async move {
            let mut ping = tokio::time::interval(PING);
            ping.tick().await;
            let closed = page.closed();
            tokio::pin!(closed);
            loop {
                let msg = tokio::select! {
                    m = rx.recv() => match m { Some(text) => Message::Text(text.into()), None => break },
                    _ = ping.tick() => Message::Ping(Default::default()),
                    _ = &mut closed => break,
                };
                if page.is_closed() {
                    break;
                }
                let (began, kind, bytes) = match &msg {
                    Message::Text(t) => (std::time::Instant::now(), kind_of(t), t.len()),
                    _ => (std::time::Instant::now(), String::new(), 0),
                };
                match tokio::time::timeout(SEND_WAIT, tx.send(msg)).await {
                    Ok(Ok(())) => {}
                    _ => break,
                }
                if !kind.is_empty() {
                    page.tracer.lock().sent(&kind, bytes, began);
                }
            }
            page.close();
            // a peer that stopped reading never takes the Close: it is not waited for
            let _ = tokio::time::timeout(SEND_WAIT, tx.send(Message::Close(None))).await;
        })
    };
    // `mac`: this is the Mac's own browser (not through a proxy), which alone may use Finder (AC-36)
    page.send_json(&json!({"t": "build", "id": crate::config::build_id(), "date": crate::config::BUILD_DATE, "mac": !page.web}));
    hello(&app, &page);
    tokio::spawn(super::timing::follow(page.clone()));
    // the writer closes the page when the peer stops reading: the loop ends then even though
    // the peer never sends again, so the shown pane is released (`left` below)
    let closed = page.closed();
    tokio::pin!(closed);
    loop {
        let m = tokio::select! {
            m = incoming.next() => match m { Some(Ok(m)) => m, _ => break },
            _ = &mut closed => break,
        };
        let text = match m {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(msg) = serde_json::from_str::<Value>(&text) else {
            client::error(&page, "A message from the page could not be read.", None);
            continue;
        };
        let (began, kind) = (std::time::Instant::now(), msg["t"].as_str().unwrap_or("").to_string());
        // never cut part-way (a pane switch takes `shown` before handing the old size back): each
        // tmux command it waits on gives up on its own (ANSWER_WAIT)
        client::handle(&app, &page, msg).await;
        page.tracer.lock().handled(&kind, began);
    }
    page.close();
    writer.abort();
    let shown = page.state.lock().shown.take();
    if let Some(old) = shown {
        // handing the size back waits on tmux: bounded, and the pane is released either way
        let _ = tokio::time::timeout(SEND_WAIT, super::sizing::left(&app, &page, &old)).await;
    }
    app.pages.remove(page.id);
    tracing::info!(event = "page.close", page = page.id, pages = app.pages.count());
}

/// A message's kind (`"t"`), read from its head without parsing it.
fn kind_of(text: &str) -> String {
    let i = text.find("\"t\":\"").map(|i| i + 5);
    i.and_then(|i| text[i..].find('"').map(|j| text[i..i + j].to_string())).filter(|k| k.len() < 20).unwrap_or_default()
}

/// What a page learns when it connects: the list, the stall line, bookmarks, settings and
/// the last restore report.
fn hello(app: &Shared, page: &Page) {
    page.send_json(&json!({"t": "layout", "groups": app.layout.read().clone()}));
    crate::page::projects::hello(app, page);   // before the bookmarks: a project's ones never count as shown nowhere
    crate::page::marks::hello(app, page);
    crate::looks::hello(app, page);
    crate::restore::hello(app, page);
    crate::release::hello(page);
    if let Some(m) = app.tmux_problem.lock().clone() {
        page.send_json(&json!({"t": "error", "msg": m}));
    }
}
