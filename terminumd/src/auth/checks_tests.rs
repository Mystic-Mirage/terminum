// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The refused-request table of AC-09: one listener on 127.0.0.1, other devices through
//! `tailscale serve`.

use super::*;

const PORT: u16 = 48080;
const L: Listener<'static> = Listener { port: PORT, allow: &[] };
const PAGE: &str = "http://127.0.0.1:48080";
const TS: &str = "mac.tail1234.ts.net";
const TS_PAGE: &str = "https://mac.tail1234.ts.net";

/// Headers as (lowercase name, value) pairs; forwarded when one of them says so. Ok(the Mac's
/// own browser).
fn ask(l: &Listener, method: &str, path: &str, headers: &[(&str, &str)], signed_in: bool) -> Result<bool, (u16, &'static str)> {
    let h = |name: &str| headers.iter().find(|(n, _)| *n == name).map(|(_, v)| v.to_string());
    let forwarded = is_forwarded(headers.iter().map(|(n, _)| *n));
    decide(l, method, path, h, forwarded, signed_in).map_err(|d| (d.status, d.code))
}

fn local(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut h = vec![("host", "127.0.0.1:48080")];
    h.extend_from_slice(extra);
    h
}

/// A request as `tailscale serve` forwards it: the tailnet name, HTTPS, its headers.
fn tailnet(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut h = vec![("host", TS), ("x-forwarded-host", TS), ("x-forwarded-proto", "https"), ("x-forwarded-for", "100.64.0.7")];
    h.extend_from_slice(extra);
    h
}

#[test]
fn ac09_outline_requests_without_a_sign_in_or_from_elsewhere_are_refused() {
    // GET /ws (no sign-in) → 401 bad_session, with or without the page's Origin
    assert_eq!(ask(&L, "GET", "/ws", &local(&[("origin", PAGE), ("upgrade", "websocket")]), false), Err((401, "bad_session")));
    assert_eq!(ask(&L, "GET", "/ws", &local(&[]), false), Err((401, "bad_session")));
    // a project's agents (terminum-dispatcher.md AC-137): no sign-in, no write
    for (method, path) in [("POST", "/api/task-projects/p1/agents"), ("PATCH", "/api/task-agents/a1"), ("DELETE", "/api/task-agents/a1")] {
        assert_eq!(ask(&L, method, path, &local(&[("origin", PAGE), ("content-type", "application/json")]), false), Err((401, "bad_session")), "{path}");
    }
    // GET /fb/api/ls?path=/Users/alex, Host: evil.example:48080 → 403 bad_host, signed in or not
    for path in ["/api/ls", "/fb/api/ls"] {
        for signed in [true, false] {
            assert_eq!(ask(&L, "GET", path, &[("host", "evil.example:48080")], signed), Err((403, "bad_host")), "{path}");
        }
    }
    // GET /ws with a sign-in, Origin: http://evil.example → 403 bad_origin
    assert_eq!(ask(&L, "GET", "/ws", &local(&[("origin", "http://evil.example"), ("upgrade", "websocket")]), true),
               Err((403, "bad_origin")));
    // POST /fb/api/fs/mkdir with a sign-in, Content-Type: text/plain → 415 bad_content_type
    for path in ["/api/fs/mkdir", "/fb/api/fs/mkdir"] {
        assert_eq!(ask(&L, "POST", path, &local(&[("origin", PAGE), ("content-type", "text/plain")]), true),
                   Err((415, "bad_content_type")), "{path}");
    }
    // POST /fb/api/os/reveal through tailscale serve → 403 not_here, signed in or not
    for path in ["/api/os/reveal", "/api/os/dropped", "/fb/api/os/reveal", "/fb/api/terminal/insert", "/fb/api/os", "//fb/api/os/open"] {
        for signed in [true, false] {
            let h = tailnet(&[("origin", TS_PAGE), ("content-type", "application/json")]);
            assert_eq!(ask(&L, "POST", path, &h, signed), Err((403, "not_here")), "{path}");
        }
    }
    // Tailscale Funnel → 403 funnel, before anything else
    assert_eq!(ask(&L, "GET", "/", &tailnet(&[("tailscale-funnel-request", "?1")]), false), Err((403, "funnel")));
    assert_eq!(ask(&L, "GET", "/", &local(&[("tailscale-funnel-request", "?1")]), true), Err((403, "funnel")));
}

#[test]
fn only_the_macs_own_browser_may_use_finder_and_type_paths() {
    let json = local(&[("origin", PAGE), ("content-type", "application/json")]);
    assert_eq!(ask(&L, "POST", "/fb/api/os/reveal", &json, true), Ok(true));
    assert_eq!(ask(&L, "POST", "/fb/api/terminal/insert", &json, true), Ok(true));
    assert_eq!(ask(&L, "POST", "/fb/api/osx", &json, true), Ok(true), "a prefix is a whole segment");
    // tailscale serve passes the browser's Host through: a tailnet client can send the loopback
    // one, and any forwarding header makes the request forwarded all the same
    for header in ["x-forwarded-host", "x-forwarded-for", "x-forwarded-proto", "forwarded", "tailscale-user-login", "Tailscale-Headers-Info"] {
        let mut h = json.clone();
        h.push((header, "x"));
        assert_eq!(ask(&L, "POST", "/fb/api/os/reveal", &h, true), Err((403, "not_here")), "{header}");
        assert_eq!(ask(&L, "POST", "/fb/api/fs/mkdir", &h, true), Ok(false), "{header}: the rest is allowed");
    }
}

#[test]
fn forwarding_headers_are_known_by_name_in_any_case() { // AC-10
    assert!(is_forwarded(["host", "X-Forwarded-Host"].into_iter()));
    assert!(is_forwarded(["tailscale-app-capabilities"].into_iter()));
    assert!(is_forwarded(["Forwarded"].into_iter()));
    assert!(!is_forwarded(["host", "origin", "x-tm-session", "forwarded-x"].into_iter()));
}

#[test]
fn internal_paths_are_not_on_tcp_at_all() {
    for path in ["/internal/web", "/internal", "//internal/x"] {
        assert_eq!(ask(&L, "GET", path, &local(&[]), true), Err((404, "not_found")), "{path}");
        assert_eq!(ask(&L, "POST", path, &[("host", "evil.example")], false), Err((404, "not_found")), "{path}");
    }
}

#[test]
fn the_page_the_hello_and_the_sign_in_need_no_sign_in() {
    for path in ["/", "/index.html", "/app.js", "/fb/", "/fb/renderers.js", "/api/hello", "/auth/check"] {
        assert_eq!(ask(&L, "GET", path, &local(&[]), false), Ok(true), "{path}");
        assert_eq!(ask(&L, "GET", path, &tailnet(&[]), false), Ok(false), "{path}");
    }
    for path in ["/auth/login", "/auth/logout"] {
        assert_eq!(ask(&L, "POST", path, &local(&[("origin", PAGE), ("content-type", "application/json")]), false), Ok(true), "{path}");
        assert_eq!(ask(&L, "POST", path, &tailnet(&[("origin", TS_PAGE), ("content-type", "application/json")]), false), Ok(false));
        assert_eq!(ask(&L, "POST", path, &tailnet(&[("content-type", "application/json")]), false), Err((403, "bad_origin")),
                   "{path}: a browser always names the page");
        assert_eq!(ask(&L, "POST", path, &local(&[("origin", "http://evil.example"), ("content-type", "application/json")]), false),
                   Err((403, "bad_origin")));
    }
    for path in ["/ws", "/paste", "/upload", "/api/settings", "/api/hello/x", "/fb/api/state", "/fb/api/ws", "/fb/api", "//ws"] {
        assert_eq!(ask(&L, "GET", path, &local(&[("origin", PAGE)]), false), Err((401, "bad_session")), "{path}");
    }
}

#[test]
fn writes_need_json_and_an_exact_origin_which_only_the_mac_may_leave_out() {
    let ok = |h: &[(&'static str, &'static str)]| ask(&L, "POST", "/fb/api/fs/mkdir", &local(h), true);
    assert_eq!(ok(&[("content-type", "application/json")]), Ok(true), "no Origin: not from a page");
    assert_eq!(ok(&[("origin", PAGE), ("content-type", "application/json; charset=utf-8")]), Ok(true));
    assert_eq!(ok(&[("origin", PAGE), ("content-type", "Application/JSON")]), Ok(true));
    assert_eq!(ok(&[("origin", PAGE)]), Err((415, "bad_content_type")), "none");
    assert_eq!(ok(&[("origin", PAGE), ("content-type", "application/jsonp")]), Err((415, "bad_content_type")));
    assert_eq!(ok(&[("origin", PAGE), ("content-type", "multipart/form-data; boundary=x")]), Err((415, "bad_content_type")));
    for bad in ["http://evil.example", "null", "http://127.0.0.1:48081", "http://localhost:48080", "https://127.0.0.1:48080"] {
        assert_eq!(ok(&[("origin", bad), ("content-type", "application/json")]), Err((403, "bad_origin")), "{bad}");
    }
    assert_eq!(ask(&L, "PUT", "/fb/api/file", &local(&[("origin", PAGE), ("content-type", "text/plain")]), true),
               Err((415, "bad_content_type")), "every method but GET and HEAD writes");
    let fwd = tailnet(&[("content-type", "application/json")]);
    assert_eq!(ask(&L, "POST", "/fb/api/fs/mkdir", &fwd, true), Err((403, "bad_origin")), "a forwarded write names its page");
}

#[test]
fn each_kind_of_name_has_its_scheme() {
    let json = |host: &'static str, origin: &'static str| vec![("host", host), ("origin", origin), ("content-type", "application/json"), ("x-forwarded-host", host)];
    assert_eq!(ask(&L, "POST", "/fb/api/fs/mkdir", &json(TS, TS_PAGE), true), Ok(false));
    assert_eq!(ask(&L, "POST", "/fb/api/fs/mkdir", &json(TS, "http://mac.tail1234.ts.net"), true), Err((403, "bad_origin")),
               "tailscale serve is HTTPS only");
    assert_eq!(ask(&L, "POST", "/fb/api/fs/mkdir", &local(&[("origin", "https://127.0.0.1:48080"), ("content-type", "application/json")]), true),
               Err((403, "bad_origin")), "the loopback page is plain HTTP");
    let allow = vec!["Devbox.Example".to_string()];
    let a = Listener { port: PORT, allow: &allow };
    for origin in ["http://devbox.example:8080", "https://devbox.example:8080"] {
        let h = [("host", "devbox.example:8080"), ("origin", origin), ("content-type", "application/json"), ("x-forwarded-for", "192.0.2.2")];
        assert_eq!(ask(&a, "POST", "/fb/api/fs/mkdir", &h, true), Ok(false), "{origin}");
    }
}

#[test]
fn uploads_take_any_type_but_only_from_the_page() {
    for path in ["/paste", "/upload", "/fb/api/fs/upload"] {
        assert_eq!(ask(&L, "POST", path, &local(&[("origin", PAGE), ("content-type", "image/png")]), true), Ok(true));
        assert_eq!(ask(&L, "POST", path, &local(&[("content-type", "image/png")]), true), Err((403, "bad_origin")), "{path}");
        assert_eq!(ask(&L, "POST", path, &local(&[("origin", "http://evil.example")]), true), Err((403, "bad_origin")));
        assert_eq!(ask(&L, "POST", path, &local(&[("origin", PAGE)]), false), Err((401, "bad_session")));
    }
}

#[test]
fn a_websocket_needs_the_exact_origin() {
    let ws = |h: &[(&'static str, &'static str)], path| ask(&L, "GET", path, &local(h), true);
    assert_eq!(ws(&[("origin", PAGE), ("upgrade", "websocket")], "/ws"), Ok(true));
    assert_eq!(ws(&[("upgrade", "websocket")], "/ws"), Err((403, "bad_origin")));
    assert_eq!(ws(&[], "/fb/api/ws"), Err((403, "bad_origin")));
    assert_eq!(ws(&[("upgrade", "WebSocket")], "/api/hello"), Err((403, "bad_origin")), "any path that upgrades");
    let t = tailnet(&[("origin", TS_PAGE), ("upgrade", "websocket")]);
    assert_eq!(ask(&L, "GET", "/ws", &t, true), Ok(false));
    assert_eq!(ask(&L, "GET", "/ws", &t, false), Err((401, "bad_session")));
}

#[test]
fn an_origin_present_on_any_request_must_be_exact() {
    assert_eq!(ask(&L, "GET", "/", &local(&[("origin", "http://evil.example")]), false), Err((403, "bad_origin")));
    assert_eq!(ask(&L, "GET", "/fb/api/ls", &local(&[("origin", PAGE)]), true), Ok(true));
}

#[test]
fn the_host_is_the_loopback_address_exactly_a_tailnet_name_or_an_allowed_one() {
    for host in ["localhost:48080", "127.0.0.1", "127.0.0.1:48081", "[::1]:48080", "127.0.0.1:48080.evil.example", "",
                 "evil.example", "evil.example:48080", "mac.local", "ts.net", ".ts.net", "x..ts.net", "evilts.net", "devbox.example"] {
        assert_eq!(ask(&L, "GET", "/", &[("host", host)], false), Err((403, "bad_host")), "{host}");
    }
    assert_eq!(ask(&L, "GET", "/", &[], false), Err((403, "bad_host")), "no Host");
    for host in [TS, "mac.tail1234.ts.net:443", "Terminum.Tail1234.TS.net"] {
        assert_eq!(ask(&L, "GET", "/", &[("host", host)], false), Ok(false), "{host}: a tailnet name is never the Mac's own browser");
    }
    let allow = vec!["Devbox.Example".to_string()];
    assert_eq!(ask(&Listener { port: PORT, allow: &allow }, "GET", "/", &[("host", "devbox.example:8080")], false), Ok(false));
}

#[test]
fn paths_that_route_differently_than_they_read_are_refused() {
    let h = tailnet(&[("origin", TS_PAGE), ("content-type", "application/json")]);
    for path in ["/fb/api/%6fs/reveal", "/fb/api%2Fos/reveal", "/api/%6fs/reveal", "/fb/api/fs/../os/reveal", "/fb/./api/os/x", "/a/.."] {
        assert_eq!(ask(&L, "POST", path, &h, true), Err((400, "bad_path")), "{path}");
        assert_eq!(ask(&L, "GET", path, &local(&[]), true), Err((400, "bad_path")), "{path}");
    }
    assert_eq!(ask(&L, "GET", "/fb/renderers.js", &h, false), Ok(false));
    assert_eq!(ask(&L, "GET", "/a.b/..c", &h, false), Ok(false), "dots inside a name are a name");
}

#[test]
fn host_names_lose_their_port_and_brackets() {
    assert_eq!(host_name("192.0.2.5:8765"), "192.0.2.5");
    assert_eq!(host_name("[::1]:8765"), "::1");
    assert_eq!(host_name("[::1]"), "::1");
    assert_eq!(host_name("fe80::1"), "fe80::1");
    assert_eq!(host_name("Mac.Local"), "mac.local");
}

#[test]
fn the_sign_in_comes_from_the_header_or_the_subprotocol_never_the_query() {
    let header = |v: &'static str| move |n: &str| (n == "x-tm-session").then(|| v.to_string());
    assert_eq!(signin_credential(header("abc")), Some("abc".into()));
    assert_eq!(signin_credential(header("")), None);
    let proto = |v: &'static str| move |n: &str| (n == "sec-websocket-protocol").then(|| v.to_string());
    assert_eq!(signin_credential(proto("tm, abc")), Some("abc".into()));
    assert_eq!(signin_credential(proto("chat, abc")), None);
    assert_eq!(signin_credential(proto("tm")), None);
    assert!(needs_signin("/custom.css") && needs_signin("//custom.css"), "the user's stylesheet is theirs");
}

#[test]
fn a_denial_is_the_error_body_and_quotes_at_most_200_characters() {
    let long = format!("http://{}", "a".repeat(500));
    let d = decide(&L, "GET", "/", |n| match n {
        "host" => Some("127.0.0.1:48080".into()),
        "origin" => Some(long.clone()),
        _ => None,
    }, false, false).unwrap_err();
    assert_eq!(d.body()["error"], "bad_origin");
    assert!(d.message.len() < 260);
}
