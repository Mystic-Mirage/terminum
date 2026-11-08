// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The password (AC-09): every browser signs in with it, the Mac's own at 127.0.0.1 and other
//! devices through `tailscale serve`. Set by the CLI over the socket (`terminum password`);
//! until it is set no browser signs in and the page says how to set it. The right password
//! gets a sign-in (`auth::signin`); wrong ones meet the brakes (`auth::websignin`).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app::Shared;
use crate::auth::web::{PasswordHash, WebConfig};
use crate::auth::websignin::{self, Login, WebAuth};

/// The cookie sign-ins of 0.5.0's second listener: removed, every device signs in again.
const OLD_SESSIONS: &str = "web-sessions.json";

#[derive(Default)]
pub struct Web {
    /// The password's check; None while no usable password is set.
    auth: Mutex<Option<Arc<WebAuth>>>,
    /// Why there is no usable password (web.json cannot be read, a broken hash).
    error: Mutex<Option<String>>,
    allow: Mutex<Vec<String>>,
}

/// web.json's `allow_hosts` (the Host check, AC-09).
pub fn allow_hosts(app: &Shared) -> Vec<String> {
    app.web.allow.lock().clone()
}

/// `{"set", "error"}` for `terminum status` and the page.
pub fn status(app: &Shared) -> Value {
    json!({"set": app.web.auth.lock().is_some(), "error": app.web.error.lock().clone()})
}

/// Reads web.json: the password and the allowed names. A new check comes with it, so the
/// brakes start over (a new password, `terminum signout`).
pub fn apply(app: &Shared) {
    if let Err(e) = std::fs::remove_file(app.cfg.dir.join(OLD_SESSIONS))
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(event = "web.old_sessions_not_removed", error = %e);
    }
    let (auth, error, allow) = match WebConfig::load(&app.cfg.dir) {
        Err(e) => (None, Some(e), Vec::new()),
        Ok(cfg) => match cfg.usable_password() {
            Ok(hash) => (hash.map(|h| Arc::new(WebAuth::new(h))), None, cfg.allow_hosts),
            Err(e) => (None, Some(e), cfg.allow_hosts),
        },
    };
    if let Some(e) = &error {
        tracing::warn!(event = "web.no_password", why = %e);
    }
    *app.web.auth.lock() = auth;
    *app.web.error.lock() = error;
    *app.web.allow.lock() = allow;
}

/// `terminum password`: a new password (at least 8 characters); every sign-in goes and every
/// page closes, so each browser signs in with the new one.
pub async fn set_password(app: &Shared, password: &str) -> Result<Value, String> {
    let hash = PasswordHash::new(password)?;
    let a = app.clone();
    crate::http::blocking(move || {
        let _one = WRITING.lock();
        let mut cfg = WebConfig::load(&a.cfg.dir)?;
        cfg.password = Some(hash);
        cfg.save(&a.cfg.dir).map_err(|e| format!("web.json not saved: {e}"))
    })
    .await
    .map_err(|_| "the password could not be saved".to_string())??;
    // the new check first, then the sign-ins go: a login the old password passed meanwhile
    // either made its sign-in before (revoked here) or finds its check replaced (`login`)
    apply(app);
    end_all(app).await.map_err(|e| format!("The new password is set. {e}"))?;
    Ok(status(app))
}

/// Every sign-in ends, and with it every page and panel (`Signins::revocations`), even when
/// signins.json cannot be written: then the error says what a restart would do.
async fn end_all(app: &Shared) -> Result<(), String> {
    let a = app.clone();
    crate::http::off(move || a.signins.revoke_all().map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("Every browser is signed out, but {}", unsaved(&e, "the old sign-ins")))
}

/// What a revocation signins.json could not keep leaves: `who` back after a restart.
fn unsaved(e: &str, who: &str) -> String {
    format!("signins.json could not be written ({e}): until terminum signout succeeds on the Mac, a restart of terminumd would let {who} back.")
}

/// One writer of web.json at a time: a password set while a sign-in strengthens the old one's
/// hash is never written over.
static WRITING: Mutex<()> = Mutex::new(());

/// A hash of fewer iterations than today's, made again from the password just found right
/// (`WebConfig::strengthen`), off the async workers; the check in memory stays until web.json
/// is read again (a restart, `terminum signout`).
fn strengthen(app: &Shared, old: PasswordHash, password: String) {
    let a = app.clone();
    tokio::task::spawn_blocking(move || {
        let _one = WRITING.lock();
        let Ok(mut cfg) = WebConfig::load(&a.cfg.dir) else { return };
        if cfg.strengthen(&old, &password) {
            match cfg.save(&a.cfg.dir) {
                Ok(()) => tracing::info!(event = "web.password_strengthened", iterations = crate::auth::web::ITERATIONS),
                Err(e) => tracing::warn!(event = "web.password_not_strengthened", error = %e),
            }
        }
    });
}

/// `terminum signout`: every browser signs in again, and the brakes start over.
pub async fn sign_out_all(app: &Shared) -> Result<(), String> {
    apply(app); // first, as in `set_password`: a login in flight then signs no one in
    end_all(app).await
}

#[derive(Deserialize)]
pub struct LoginBody {
    #[serde(default)]
    password: String,
}

/// The address a sign-in counts against: the peer, or behind `tailscale serve` (a loopback
/// peer, the proxy's name as Host) X-Forwarded-For's last entry.
fn address(headers: &HeaderMap, peer: SocketAddr) -> String {
    let host = headers.get(axum::http::header::HOST).and_then(|v| v.to_str().ok()).map(crate::auth::checks::host_name).unwrap_or_default();
    let proxied = !matches!(host.as_str(), "" | "127.0.0.1" | "localhost" | "::1");
    websignin::client_address(peer.ip(), headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()), proxied)
}

/// What a request says of its browser, for the input log (AC-147): its address, its browser in a
/// few words, and the tailnet's user when `tailscale serve` forwarded it (`web`).
pub fn client(headers: &HeaderMap, peer: SocketAddr, web: bool) -> crate::inputs::Client {
    let header = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
    crate::inputs::Client {
        web,
        ip: address(headers, peer),
        browser: crate::inputs::text::browser(&header("user-agent").unwrap_or_default()),
        tailnet_user: header("tailscale-user-login").filter(|_| web),
    }
}

/// How to set the password, said where it is missing.
pub const NO_PASSWORD: &str = "No password is set yet. On the Mac, run: terminum password";

/// `POST /auth/login {"password"}` → `{"secret"}`: the page keeps it (signin.js).
pub async fn login(State(app): State<Shared>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(b): Json<LoginBody>) -> Response {
    let Some(auth) = app.web.auth.lock().clone() else {
        let message = app.web.error.lock().clone().unwrap_or_else(|| NO_PASSWORD.into());
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"reason": "no_password", "message": message}))).into_response();
    };
    let password = b.password.clone();
    match auth.login(b.password, address(&headers, peer)).await {
        Login::Right => {
            let a = app.clone();
            match crate::http::blocking(move || a.signins.create("password")).await {
                Ok(Ok(secret)) if !current(&app, &auth) => {
                    // the password changed while this one was checked: no sign-in from the old one
                    let a = app.clone();
                    let _ = crate::http::blocking(move || a.signins.revoke(&secret)).await;
                    (StatusCode::UNAUTHORIZED, Json(json!({"reason": "wrong", "retry": 0}))).into_response()
                }
                Ok(Ok(secret)) => {
                    tracing::info!(event = "signin.created", signins = app.signins.count());
                    strengthen(&app, auth.hash().clone(), password);
                    Json(json!({"secret": secret})).into_response()
                }
                Ok(Err(e)) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"reason": "failed", "message": format!("The sign-in could not be saved: {e}")}))).into_response(),
                Err(r) => r,
            }
        }
        Login::Wrong { retry } => (StatusCode::UNAUTHORIZED, Json(json!({"reason": "wrong", "retry": retry}))).into_response(),
        Login::Locked { retry } => (StatusCode::TOO_MANY_REQUESTS, Json(json!({"reason": "locked", "retry": retry}))).into_response(),
        Login::Failed(why) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"reason": "failed", "message": why}))).into_response(),
    }
}

/// `auth` is still the password's check (no new password since it was taken).
fn current(app: &Shared, auth: &Arc<WebAuth>) -> bool {
    app.web.auth.lock().as_ref().is_some_and(|now| Arc::ptr_eq(now, auth))
}

/// `POST /auth/logout`: this browser's sign-in ends (the one it presents, if any), and every
/// page and panel still open with it closes (`Signins::revocations`): another tab or device of
/// the same sign-in. A credential
/// of another boot (an upgrade under the page) is 401 bad_session, which the page proves again
/// and sends once more, so the sign-in it meant ends instead of living on unseen. One that
/// cannot be removed from the file is 503 signout_unsaved: ended, but a restart would bring it back.
pub async fn logout(State(app): State<Shared>, headers: HeaderMap) -> Response {
    if let Some(credential) = crate::auth::checks::credential_of(&headers) {
        let a = app.clone();
        match crate::http::blocking(move || a.signins.revoke(&credential)).await {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => return crate::auth::checks::Denial::new(401, "bad_session", "Missing or expired sign-in").into_response(),
            Ok(Err(e)) => {
                // gone from memory all the same (`Signins::revoke`); the page says the rest
                tracing::warn!(event = "signout.unsaved", error = %e);
                let message = format!("Signed out, but {}", unsaved(&e.to_string(), "this sign-in"));
                return crate::http::err(StatusCode::SERVICE_UNAVAILABLE, "signout_unsaved", message);
            }
            Err(r) => return r,
        }
    }
    StatusCode::NO_CONTENT.into_response()
}
