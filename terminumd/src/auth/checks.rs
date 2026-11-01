// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Who may ask what over TCP (AC-09): the HTTP guard's decisions as pure functions, so the
//! table of refused requests is tested without a server. The one listener is 127.0.0.1:<port>;
//! other devices reach it through a proxy on this Mac (`tailscale serve`). The guard computes
//! `signed_in` (from `signin_credential`) and `forwarded` (from the header names) and answers a
//! `Denial` as `{"error": code, "message": message}` with its status.
//!
//! Order, first refusal wins: `/internal/*` is 404 (served only on the CLI socket); a path
//! that could be judged as one thing and routed as another (dot segments; `%` in an API path)
//! is 400; Tailscale Funnel (the public internet); the Host (DNS rebinding); what only the
//! Mac's own browser may do (`not_here`); the sign-in; the Origin; the Content-Type of a write.
//!
//! Host and session actions (`addhost`, `hostaction`, `password`, `signout`) have no TCP route
//! at all: they are the CLI socket's.

use serde_json::{json, Value};

/// The listener, with what its Host check needs.
#[derive(Clone, Copy, Debug)]
pub struct Listener<'a> {
    /// 127.0.0.1:<port>
    pub port: u16,
    /// web.json's `allow_hosts`: names another proxy forwards
    pub allow: &'a [String],
}

/// A refused request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Denial {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl Denial {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Denial {
        Denial { status, code, message: message.into() }
    }

    pub fn body(&self) -> Value {
        json!({"error": self.code, "message": self.message})
    }
}

impl axum::response::IntoResponse for Denial {
    fn into_response(self) -> axum::response::Response {
        let status = axum::http::StatusCode::from_u16(self.status).unwrap_or(axum::http::StatusCode::FORBIDDEN);
        (status, axum::Json(self.body())).into_response()
    }
}

/// The name in a Host header: without its port and an IPv6 address's brackets, lowercase.
pub fn host_name(host: &str) -> String {
    let host = host.trim();
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else if host.matches(':').count() == 1 {
        host.split(':').next().unwrap_or("")
    } else {
        host // a name, or a bare IPv6 address
    };
    name.to_ascii_lowercase()
}

/// Which kind of name a request asked for, or None when it is not served (DNS rebinding: a
/// page of another site whose DNS name points at this Mac sends that name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostKind {
    /// `127.0.0.1:<port>`, exactly: the Mac's own browser (plain HTTP)
    Loopback,
    /// `*.ts.net`: `tailscale serve`, always HTTPS
    Tailnet,
    /// web.json's `allow_hosts`: another proxy, HTTP or HTTPS
    Allowed,
}

pub fn host_kind(host: &str, listener: &Listener) -> Option<HostKind> {
    if host == format!("127.0.0.1:{}", listener.port) {
        return Some(HostKind::Loopback);
    }
    let name = host_name(host);
    if name.is_empty() || matches!(name.as_str(), "127.0.0.1" | "localhost" | "::1") {
        return None;
    }
    if name.strip_suffix(".ts.net").is_some_and(|n| !n.is_empty() && !n.ends_with('.')) {
        return Some(HostKind::Tailnet);
    }
    listener.allow.iter().any(|n| n.eq_ignore_ascii_case(&name)).then_some(HostKind::Allowed)
}

/// The page's own origins for a request to `host`: the scheme its kind of name is served with.
pub fn expected_origins(kind: HostKind, host: &str) -> Vec<String> {
    match kind {
        HostKind::Loopback => vec![format!("http://{host}")],
        HostKind::Tailnet => vec![format!("https://{host}")],
        HostKind::Allowed => vec![format!("http://{host}"), format!("https://{host}")],
    }
}

/// An Origin is one of the page's own, exactly (browsers send it serialized: scheme, host, port).
pub fn origin_ok(origin: &str, expected: &[String]) -> bool {
    expected.iter().any(|e| e == origin)
}

/// A request a proxy forwarded: it names a forwarding header. `tailscale serve` passes the
/// browser's Host through as it came, so the Host never says where a request came from; it
/// always adds X-Forwarded-Host, and the Tailscale-* headers for a tailnet user (and removes
/// those a client sent). Fail closed: any of them is a forwarded request.
pub fn is_forwarded<'a>(mut names: impl Iterator<Item = &'a str>) -> bool {
    names.any(|n| {
        let n = n.to_ascii_lowercase();
        n == "forwarded" || n.starts_with("x-forwarded-") || n.starts_with("tailscale-")
    })
}

/// `path` without repeated slashes, so `//fb/api/os/x` is judged as `/fb/api/os/x`.
fn tidy(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if !(c == '/' && out.ends_with('/')) {
            out.push(c);
        }
    }
    out
}

/// `path` is `prefix` or below it.
fn under(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Requests that read or change terminals, sessions or files: everything but the page's own
/// files, `/api/hello` and the sign-in (`/auth/*`).
pub fn needs_signin(path: &str) -> bool {
    let p = tidy(path);
    p == "/ws" || p == "/paste" || p == "/upload" || p == "/custom.css" || (under(&p, "/api") && p != "/api/hello") || under(&p, "/fb/api")
}

/// What only the Mac's own browser may do: Finder and default apps, typing a path into a
/// terminal (the spec's table names them as the Files API has them, `/fb/api/…`, and as fbd
/// had them, `/api/…`).
fn mac_only(path: &str) -> bool {
    let p = tidy(path);
    ["/fb/api/os", "/fb/api/terminal", "/api/os", "/api/terminal"].iter().any(|prefix| under(&p, prefix))
}

/// A path whose meaning depends on who decodes it: `.` or `..` segments anywhere, `%` in an
/// API path (a router that decodes `/fb/api/%6fs/x` would serve `/fb/api/os/x`).
fn ambiguous(path: &str) -> bool {
    path.split('/').any(|seg| seg == "." || seg == "..")
        || (path.contains('%') && (under(path, "/api") || under(path, "/fb/api") || path.starts_with("/fb/api%")))
}

fn is_websocket(path: &str, header: &dyn Fn(&str) -> Option<String>) -> bool {
    let p = tidy(path);
    p == "/ws" || p == "/fb/api/ws" || header("upgrade").is_some_and(|u| u.to_ascii_lowercase().contains("websocket"))
}

/// The media type of a Content-Type header is `application/json` (parameters such as a charset
/// allowed).
fn is_json(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|c| c.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"))
}

/// Allows or refuses a request before any handler runs. `header(name)` gives a request header
/// by its lowercase name; `forwarded` says a proxy forwarded it (`is_forwarded`); `signed_in`
/// whether it carries a valid sign-in. Ok: whether it is the Mac's own browser's (not
/// forwarded, at a loopback name), which alone may use Finder and the like.
pub fn decide(listener: &Listener, method: &str, path: &str, header: impl Fn(&str) -> Option<String>,
              forwarded: bool, signed_in: bool) -> Result<bool, Denial> {
    let p = tidy(path);
    if under(&p, "/internal") {
        return Err(Denial::new(404, "not_found", "Not found"));
    }
    if ambiguous(&p) {
        return Err(Denial::new(400, "bad_path", "The path has dot segments or escapes"));
    }
    if header("tailscale-funnel-request").is_some() {
        return Err(Denial::new(403, "funnel", "terminum is not served on the public internet (tailscale funnel): use tailscale serve"));
    }
    let host = header("host").unwrap_or_default();
    let Some(kind) = host_kind(&host, listener) else {
        return Err(Denial::new(403, "bad_host", format!("Host {} is not allowed", clip(&host))));
    };
    let local = kind == HostKind::Loopback && !forwarded;
    if !local && mac_only(&p) {
        return Err(Denial::new(403, "not_here", "Only the Mac's own browser can do this"));
    }
    if needs_signin(&p) && !signed_in {
        return Err(Denial::new(401, "bad_session", "Missing or expired sign-in"));
    }
    let write = !matches!(method, "GET" | "HEAD");
    let upload = write && (p == "/paste" || p == "/upload" || p == "/api/midi" || p == "/fb/api/fs/upload");   // bytes, not JSON
    // A browser names the page on every write and WebSocket; only these may lack an Origin:
    // a write from no page at all, on this Mac and not forwarded.
    let origin_needed = is_websocket(&p, &header) || upload || (write && !local);
    match header("origin") {
        Some(origin) if !origin_ok(&origin, &expected_origins(kind, &host)) => {
            return Err(Denial::new(403, "bad_origin", format!("Origin {} is not allowed", clip(&origin))));
        }
        None if origin_needed => return Err(Denial::new(403, "bad_origin", "Origin is required")),
        _ => {}
    }
    if write && !upload && !is_json(header("content-type").as_deref()) {
        return Err(Denial::new(415, "bad_content_type", "Content-Type must be application/json"));
    }
    Ok(local)
}

/// A header value as quoted in a message, at most 200 characters.
fn clip(s: &str) -> String {
    s.chars().take(200).collect()
}

/// The sign-in a request presents (a page's per-boot credential, a script's secret): the
/// `X-TM-Session` header; on a WebSocket the second of the subprotocols `tm, <credential>` (the
/// answer selects `tm`). Never the query: an address is copied and kept (an image's, in
/// history), so files are fetched with the header.
pub fn signin_credential(header: impl Fn(&str) -> Option<String>) -> Option<String> {
    if let Some(s) = header("x-tm-session").filter(|s| !s.is_empty()) {
        return Some(s);
    }
    if let Some(protocols) = header("sec-websocket-protocol") {
        let mut it = protocols.split(',').map(str::trim);
        if it.next() == Some("tm")
            && let Some(s) = it.next().filter(|s| !s.is_empty())
        {
            return Some(s.to_string());
        }
    }
    None
}

#[cfg(test)]
#[path = "checks_tests.rs"]
mod tests;
