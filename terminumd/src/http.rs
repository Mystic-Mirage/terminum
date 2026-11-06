// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Response helpers shared by the handlers, and the embedded files: the page (`web/static`,
//! at `/`) and the Files panel (`ui/dist`, at `/fb/`), with the CSP that keeps terminal
//! output and file content inert (AC-09).

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::ops::OpError;

/// Handlers return `Err(response)` for early exits, so `?` replaces match boilerplate.
pub type ApiResult = Result<Response, Response>;

pub fn err(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (status, Json(json!({"error": code, "message": message.into()}))).into_response()
}

pub fn op_err(e: OpError) -> Response {
    let status = match &e {
        OpError::BadName(_) => StatusCode::BAD_REQUEST,
        OpError::Denied(_) => StatusCode::FORBIDDEN,
        OpError::Missing(_) => StatusCode::NOT_FOUND,
        OpError::Exists(_) | OpError::Conflict { .. } => StatusCode::CONFLICT,
        OpError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let mut body = json!({"error": e.code(), "message": e.message()});
    if let OpError::Conflict { etag, .. } = &e {
        body["etag"] = json!(etag);
    }
    (status, Json(body)).into_response()
}

/// File-system work runs off the async workers; a panic in it answers 500.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, Response> {
    tokio::task::spawn_blocking(f).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))
}

/// An absolute path without `..` components.
pub fn abs(path: &str) -> Result<PathBuf, Response> {
    let p = PathBuf::from(path);
    if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(err(StatusCode::BAD_REQUEST, "bad_path", "Path must be absolute without '..'"));
    }
    Ok(p)
}

#[derive(rust_embed::Embed)]
#[folder = "../web/static"]
struct PageFiles;

#[derive(rust_embed::Embed)]
#[folder = "../ui/dist"]
struct PanelFiles;

/// The CSP of the page and the panel: no script but their own, no content from elsewhere,
/// framed only by the page itself. The socket by name: WebKit's 'self' may not cover ws:.
/// `blob:`: files are fetched with the sign-in's header and shown from blob addresses (a PDF in a frame).
fn csp(host: &str) -> String {
    format!(
        "default-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; style-src 'self' 'unsafe-inline'; font-src 'self' data:; script-src 'self'; \
         connect-src 'self' ws://{host} wss://{host}; frame-src 'self' blob:; frame-ancestors 'self'; base-uri 'self'; form-action 'self'"
    )
}

fn file_response(path: &str, data: Vec<u8>, host: &str, immutable: bool) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut res = Response::new(Body::from(data));
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(if immutable { "public, max-age=31536000, immutable" } else { "no-cache" }));
    if path.ends_with(".html") {
        h.insert("content-security-policy", HeaderValue::from_str(&csp(host)).unwrap_or(HeaderValue::from_static("default-src 'self'")));
    }
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    res
}

fn host_of(headers: &HeaderMap) -> String {
    headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("").to_string()
}

/// The page's index with its build.
pub fn page_index(headers: &HeaderMap) -> Response {
    page_named("index.html", headers)
}

/// A page's HTML with its build.
pub fn page_named(name: &str, headers: &HeaderMap) -> Response {
    let Some(f) = PageFiles::get(name) else {
        return err(StatusCode::NOT_FOUND, "not_found", "the page is not built into this terminumd");
    };
    let html = String::from_utf8_lossy(&f.data).replace("__BUILD__", crate::config::build_id());
    file_response(name, html.into_bytes(), &host_of(headers), false)
}

/// A file of the page (`/app.js`, `/style.css`…), or None.
pub fn page_file(path: &str, headers: &HeaderMap) -> Option<Response> {
    let path = path.trim_start_matches('/');
    if path.is_empty() || path == "index.html" {
        return Some(page_index(headers));
    }
    PageFiles::get(path).map(|f| file_response(path, f.data.into_owned(), &host_of(headers), false))
}

/// A file of the Files panel under `/fb/` (`path` without the prefix).
pub fn panel_file(path: &str, headers: &HeaderMap) -> Response {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match PanelFiles::get(path) {
        Some(f) => {
            let data = if path == "index.html" { String::from_utf8_lossy(&f.data).replace("__BUILD__", crate::config::build_id()).into_bytes() } else { f.data.into_owned() };
            file_response(path, data, &host_of(headers), path.starts_with("chunks/"))
        }
        None => err(StatusCode::NOT_FOUND, "not_found", format!("{path} not found")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_carries_its_build_and_a_strict_csp() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_static("127.0.0.1:48080"));
        let res = page_index(&h);
        assert_eq!(res.status(), StatusCode::OK);
        let csp = res.headers()["content-security-policy"].to_str().unwrap().to_string();
        assert!(csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'self'") && csp.contains("ws://127.0.0.1:48080"));
    }

    #[test]
    fn hashed_panel_chunks_are_kept_and_the_rest_revalidated() {
        let h = HeaderMap::new();
        if let Some(chunk) = PanelFiles::iter().find(|p| p.starts_with("chunks/")) {
            assert_eq!(panel_file(&chunk, &h).headers()[header::CACHE_CONTROL], "public, max-age=31536000, immutable");
        }
        assert_eq!(panel_file("main.js", &h).headers()[header::CACHE_CONTROL], "no-cache");
        assert_eq!(panel_file("nope.js", &h).status(), StatusCode::NOT_FOUND);
    }
}
