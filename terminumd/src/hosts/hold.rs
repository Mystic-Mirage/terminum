// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A host's control client kept up (AC-81): connects, waits while it lives, reconnects with
//! back-off, and stops when ssh works and its tmux gives no answer (`silent`).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use super::{learn_timeout, silent, sync, tunnel, ATTACH_WAIT, BACKOFF_MAX};
use crate::app::Shared;
use crate::tmux::control::SSH_NO_ANSWER;
use crate::tmux::server::{Link, Server, Status};

/// Holds a host's control client: connects, waits while it lives, reconnects with back-off.
pub(super) async fn hold(app: Shared, server: Arc<Server>, retry: Arc<Notify>) {
    if let Link::Ssh { args, .. } = &server.link {
        learn_timeout(args).await;
    }
    let (mut backoff, mut silent_tries) = (1u64, 0u32);
    loop {
        *server.status.lock() = Status::Connecting;
        app.dirty.notify_one();
        let t0 = std::time::Instant::now();
        let attached = match server.sync_clients(&[]) {
            Ok(()) => wait_attached(&server).await,
            Err(e) => Err(e),
        };
        match attached {
            Ok(()) => {
                tracing::info!(event = "host.attached", host = %server.name, ms = t0.elapsed().as_millis() as u64);
                *server.status.lock() = Status::Attached;
                (backoff, silent_tries) = (1, 0);
                app.dirty.notify_one();
                tunnel::reconcile(&app, &server.name);
                // judged by its `terminum` client alone: the user's session clients (AC-89)
                // end and come back with it
                while server.attached() {
                    tokio::select! {
                        _ = server.ended.notified() => {}
                        _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                    }
                }
                server.disconnect();
                let why = "connection lost".to_string();
                tracing::warn!(event = "host.lost", host = %server.name, retry_s = backoff);
                *server.status.lock() = Status::Reconnecting { why, retry_s: backoff };
                if server.held.lock().is_some() {
                    // it kept this link for a newer record: that record now, not this one again
                    let app = app.clone();
                    tokio::spawn(async move { sync(&app) });
                }
            }
            Err(why) => {
                server.disconnect();
                // ssh works and tmux stays silent: a try costs the host a client, and helps nothing
                let found = if why == SSH_NO_ANSWER || why == NO_ATTACH { silent::look(&server).await } else { None };
                silent_tries = if found.is_some() { silent_tries + 1 } else { 0 };
                match found {
                    Some(found) if silent_tries >= silent::TRIES => {
                        silent::sweep(&server, &found).await;
                        tracing::warn!(event = "host.stopped", host = %server.name, why = found.why);
                        *server.status.lock() = Status::Stopped { why: found.why };
                    }
                    other => {
                        let why = other.map_or(why, |f| f.why);
                        tracing::warn!(event = "host.failed", host = %server.name, why, retry_s = backoff);
                        *server.status.lock() = Status::Failed { why };
                    }
                }
            }
        }
        app.dirty.notify_one();
        if silent_tries >= silent::TRIES {
            retry.notified().await;                  // one try for each Reconnect: the count stays
            backoff = 1;
            continue;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
            _ = retry.notified() => {}
        }
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// No answer from tmux while ssh stayed up.
const NO_ATTACH: &str = "no answer from tmux on the host within 20 s";

/// Until tmux's first answer (attached) or the end of ssh (its last words).
async fn wait_attached(server: &Server) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + ATTACH_WAIT;
    loop {
        if server.attached() {
            return Ok(());
        }
        if !server.connected() {
            return Err(server.last_words().unwrap_or_else(|| "ssh ended".into()));
        }
        if tokio::time::Instant::now() > deadline {
            server.disconnect();
            return Err(NO_ATTACH.into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
