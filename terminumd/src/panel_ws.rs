// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The Files panel's event socket (`/fb/api/ws`, AC-61): file changes (`fs-change`), workspace
//! writes, rescans after a host's stream reconnected. The guard checked the sign-in and the
//! Origin before the upgrade; the first frame is the state (the build: AC-16). A sign-in revoked
//! later (Sign out, `terminum signout`, a new password) ends the socket at once, one expired at
//! the next ping.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::broadcast::{error::RecvError, Receiver};
use tokio::sync::watch;

use crate::app::Shared;
use crate::events::Event;

/// Keeps the socket busy; a dead peer on loopback is reset by the kernel, so no pong is awaited.
const PING: Duration = Duration::from_secs(15);
/// A panel that stops reading without closing ends its socket.
const SEND: Duration = Duration::from_secs(10);
/// The panel sends nothing but control frames.
const MAX_MESSAGE: usize = 4096;

pub async fn socket(State(app): State<Shared>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    let rx = app.bus.subscribe(); // before the first state, so nothing falls between them
    let offered = headers.get("sec-websocket-protocol").and_then(|v| v.to_str().ok()).unwrap_or("");
    let ws = if offered.split(',').any(|p| p.trim() == "tm") { ws.protocols(["tm"]) } else { ws };
    let credential = crate::auth::checks::credential_of(&headers).unwrap_or_default();
    let revoked = app.signins.revocations(); // before the check in `serve`: no revocation falls between
    ws.max_message_size(MAX_MESSAGE).on_upgrade(move |s| serve(app, credential, rx, revoked, s))
}

async fn serve(app: Shared, credential: String, mut rx: Receiver<Event>, mut revoked: watch::Receiver<u64>, ws: WebSocket) {
    // an agent's socket comes with its token (agent_guard), not a sign-in
    let signed_in = |app: &Shared| app.cfg.agent || app.signins.check(&credential, false);
    let (mut tx, mut incoming) = ws.split();
    if !signed_in(&app) {
        send(&mut tx, Message::Close(None)).await;
        return;
    }
    let state = Event::State(json!({"version": 1, "bridge": true, "build": crate::config::build_id()}));
    if !send(&mut tx, frame(&state)).await {
        return;
    }
    let mut ping = tokio::time::interval(PING);
    ping.tick().await;
    loop {
        tokio::select! {
            e = next(&mut rx) => {
                let Some(e) = e else {
                    // behind: the panel opens it again and starts from the current state
                    send(&mut tx, Message::Close(None)).await;
                    return;
                };
                if !send(&mut tx, frame(&e)).await { return }
            }
            // a revocation at once; an expiry at the next ping
            r = revoked.changed() => if r.is_err() || !signed_in(&app) {
                send(&mut tx, Message::Close(None)).await;
                return;
            },
            _ = ping.tick() => {
                if !signed_in(&app) {
                    send(&mut tx, Message::Close(None)).await;
                    return;
                }
                if !send(&mut tx, Message::Ping(Default::default())).await { return }
            }
            m = incoming.next() => if !matches!(m, Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Text(_) | Message::Binary(_)))) { return },
        }
    }
}

async fn send(tx: &mut SplitSink<WebSocket, Message>, m: Message) -> bool {
    matches!(tokio::time::timeout(SEND, tx.send(m)).await, Ok(Ok(())))
}

/// The next event, or `None` when this socket fell behind the bus (events were dropped:
/// the panel must not go on showing stale state) or the bus is gone.
async fn next(rx: &mut Receiver<Event>) -> Option<Event> {
    match rx.recv().await {
        Ok(e) => Some(e),
        Err(RecvError::Lagged(n)) => {
            tracing::warn!(event = "ws.lagged", missed = n);
            None
        }
        Err(RecvError::Closed) => None,
    }
}

/// `{"event": name, "data": …}`, with the same payload as the SSE event.
fn frame(e: &Event) -> Message {
    Message::Text(format!(r#"{{"event":"{}","data":{}}}"#, e.name(), e.data()).into())
}

/// launchd gives a process 256 open files; up to 100 pages and panels hold a socket each.
pub fn raise_open_files() {
    let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: getrlimit writes into a valid rlimit
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) } != 0 {
        return;
    }
    let want = r.rlim_max.min(8192);
    if r.rlim_cur < want {
        let raised = libc::rlimit { rlim_cur: want, rlim_max: r.rlim_max };
        // SAFETY: setrlimit reads a valid rlimit
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raised) } != 0 {
            tracing::warn!(event = "nofile.unchanged", limit = r.rlim_cur);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[tokio::test]
    async fn a_socket_behind_the_bus_is_closed() {
        let bus = crate::events::Bus::new();
        let mut rx = bus.subscribe();
        bus.send(Event::Rescan { host: None });
        assert!(next(&mut rx).await.is_some());
        for _ in 0..300 {
            bus.send(Event::Rescan { host: None });
        }
        assert!(next(&mut rx).await.is_none(), "lagged: close, the panel reconnects with the current state");
    }

    #[test]
    fn frames_carry_the_sse_payload() {
        let e = Event::Workspace { key: "k".into(), rev: 3, by: None };
        let Message::Text(t) = frame(&e) else { panic!("text frame") };
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["event"], "workspace");
        assert_eq!(v["data"], serde_json::from_str::<Value>(&e.data()).unwrap());
    }
}
