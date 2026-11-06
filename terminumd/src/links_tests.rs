// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;
use crate::tmux::server::tests::known_socket_path;
use crate::tmux::server::Link as Via2;
use axum::routing::post;
use serde_json::json;
use tokio::sync::Notify;

fn host(path: &str) -> Arc<Server> {
    let s = Server::new("devbox", "default", Via2::Ssh { ssh: "ssh".into(), args: vec!["devbox.example".into()] }, Arc::new(Notify::new()));
    known_socket_path(&s, path);
    s
}

fn local(name: &str, path: Option<&str>) -> Arc<Server> {
    let s = Server::new("local", name, Via2::Local { tmux: "tmux".into(), socket: name.into(), conf: "/dev/null".into() }, Arc::new(Notify::new()));
    if let Some(p) = path {
        known_socket_path(&s, p);
    }
    s
}

/// A link's router with stand-in handlers that read the body as the real ones do.
fn router() -> Router {
    let ok = || post(|Json(_): Json<Value>| async { StatusCode::NO_CONTENT });
    routes(ok(), ok(), Arc::new(Bucket::new()))
}

async fn call(r: &Router, uri: &str, body: String) -> StatusCode {
    use tower::ServiceExt;
    let req = axum::http::Request::post(uri).header("content-type", "application/json").body(axum::body::Body::from(body)).unwrap();
    r.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn a_link_serves_only_hook_and_agent() {
    let r = router();
    assert_eq!(call(&r, "/hook", "{}".into()).await, StatusCode::NO_CONTENT);
    assert_eq!(call(&r, "/agent", "{}".into()).await, StatusCode::NO_CONTENT);
    for other in ["/health", "/password", "/hosts/add", "/tmux/attach", "/restore", "/folders", "/signout"] {
        assert_eq!(call(&r, other, "{}".into()).await, StatusCode::NOT_FOUND, "{other}");
    }
}

#[tokio::test]
async fn a_body_over_64_kb_is_refused() {
    let r = router();
    let big = json!({"prompt": "x".repeat(BODY_MOST)}).to_string();
    assert_eq!(call(&r, "/hook", big).await, StatusCode::PAYLOAD_TOO_LARGE);
    let fits = json!({"prompt": "x".repeat(BODY_MOST - 100)}).to_string();
    assert_eq!(call(&r, "/hook", fits).await, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_flood_is_capped_never_queued() {
    let r = router();
    let mut codes = Vec::new();
    for _ in 0..80 {
        codes.push(call(&r, "/hook", "{}".into()).await);
    }
    let ok = codes.iter().filter(|c| **c == StatusCode::NO_CONTENT).count();
    assert!((50..=52).contains(&ok), "{ok} of 80 went through");
    assert_eq!(codes.iter().filter(|c| **c == StatusCode::TOO_MANY_REQUESTS).count(), 80 - ok);
}

#[test]
fn the_bucket_refills_at_twenty_a_second() {
    let b = Bucket::new();
    let t0 = Instant::now();
    assert_eq!((0..60).filter(|_| b.take(t0)).count(), 50);
    let t1 = t0 + std::time::Duration::from_millis(500);
    assert_eq!((0..60).filter(|_| b.take(t1)).count(), 10);
    let t2 = t1 + std::time::Duration::from_secs(3600);
    assert_eq!((0..60).filter(|_| b.take(t2)).count(), 50, "never more than the burst");
}

#[tokio::test]
async fn a_link_event_is_for_the_host_server_only_with_its_socket_path() {
    let h = host("/tmp/tmux-1000/default");
    let ok = of_link(h.clone(), &json!({"socket": "default", "socket_path": "/tmp/tmux-1000/default", "pane": "%1"})).await;
    assert_eq!(ok.map(|s| s.name.clone()).as_deref(), Some("devbox"), "the host's name, never local: sent history keeps it");
    // a TMUX_TMPDIR server is also named "default": its events are not this server's
    assert!(of_link(h.clone(), &json!({"socket": "default", "socket_path": "/home/alex/tmp/tmux-1000/default"})).await.is_none());
    // the basename alone (an old hook) is not enough over a link
    assert!(of_link(h.clone(), &json!({"socket": "default"})).await.is_none());
    assert!(of_link(h, &json!({"socket": "default", "socket_path": ""})).await.is_none());
}

#[tokio::test]
async fn a_link_event_is_refused_while_its_socket_path_is_unknown() {
    let h = Server::new("devbox", "default", Via2::Ssh { ssh: "ssh".into(), args: vec![] }, Arc::new(Notify::new()));
    assert!(of_link(h, &json!({"socket_path": "/tmp/tmux-1000/default"})).await.is_none(), "no connection: nothing is matched");
}

#[tokio::test]
async fn local_events_match_by_socket_path_else_by_name() {
    let mine = local("terminum", Some("/private/tmp/tmux-501/terminum"));
    let users = local("default", Some("/private/tmp/tmux-501/default"));
    let all = || vec![mine.clone(), users.clone()];
    let by_path = of_local(all(), &json!({"socket": "default", "socket_path": "/private/tmp/tmux-501/default"})).await;
    assert_eq!(by_path.map(|s| s.socket.clone()).as_deref(), Some("default"));
    // the same name under another TMUX_TMPDIR is no server of terminumd's
    assert!(of_local(all(), &json!({"socket": "default", "socket_path": "/Users/alex/tmp/tmux-501/default"})).await.is_none());
    // an old hook names the socket only
    let old = of_local(all(), &json!({"socket": "terminum", "pane": "%2"})).await;
    assert_eq!(old.map(|s| s.socket.clone()).as_deref(), Some("terminum"));
    assert!(of_local(all(), &json!({"socket": "other"})).await.is_none());
}

#[test]
fn stop_drops_only_that_hosts_hook_state() {
    let hooks = crate::tmux::hookstate::Hooks::default();
    let ev = json!({"agent": "claude", "event": "UserPromptSubmit", "session": "s1", "cwd": "/Users/alex"});
    for k in ["tmux:devbox:default:%1", "tmux:devbox2:default:%1", "tmux:local:terminum:%1"] {
        hooks.apply(k, &ev, Instant::now());
        assert!(hooks.has(k), "{k}");
    }
    forget(&hooks, "devbox");
    assert!(!hooks.has("tmux:devbox:default:%1"));
    assert!(hooks.has("tmux:devbox2:default:%1"), "a host whose name starts the same keeps its own");
    assert!(hooks.has("tmux:local:terminum:%1"));
}

#[test]
fn a_link_socket_is_short_stable_and_per_host() {
    let dir = Path::new("/Users/alex/.terminum/state");
    let p = path_in(dir, "devbox.example");
    assert_eq!(p, path_in(dir, "devbox.example"));
    assert_ne!(p, path_in(dir, "devbox2.example"));
    assert_eq!(p.parent(), Some(dir.join("links").as_path()));
    assert_eq!(p.file_name().unwrap().len(), 12 + ".sock".len());
}

/// A fresh socket path for `serve` (short: macOS caps socket paths at 104 bytes).
fn sock(tag: &str) -> PathBuf {
    let p = crate::testutil::temp_root().join(format!("tml-{}-{tag}.sock", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

/// Serves `r` on a new socket with `most` connections and `wait`; the task and its path.
fn serving(tag: &str, r: Router, most: usize, wait: Duration) -> (tokio::task::JoinHandle<()>, PathBuf) {
    let p = sock(tag);
    let l = tokio::net::UnixListener::bind(&p).unwrap();
    (tokio::spawn(serve(l, r, most, wait)), p)
}

/// What the other end sends until it closes, and how long that took (None: still open after 3 s).
async fn till_closed(s: &mut tokio::net::UnixStream) -> Option<(String, Duration)> {
    use tokio::io::AsyncReadExt;
    let began = Instant::now();
    let mut got = Vec::new();
    match tokio::time::timeout(Duration::from_secs(3), s.read_to_end(&mut got)).await {
        Ok(_) => Some((String::from_utf8_lossy(&got).into_owned(), began.elapsed())),
        Err(_) => None,
    }
}

const HEAD: &[u8] = b"POST /hook HTTP/1.1\r\nHost: localhost\r\n";

async fn connect(p: &Path) -> tokio::net::UnixStream {
    let s = tokio::net::UnixStream::connect(p).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;   // accepted, in order
    s
}

#[tokio::test]
async fn connections_over_the_cap_are_closed_at_once() {
    use tokio::io::AsyncWriteExt;
    let (task, p) = serving("cap", router(), 2, Duration::from_secs(5));
    let (_a, mut b) = (connect(&p).await, connect(&p).await);
    let mut over = connect(&p).await;
    let (said, took) = till_closed(&mut over).await.expect("the third connection is closed");
    assert!(said.is_empty() && took < Duration::from_millis(500), "{said:?} {took:?}");
    drop(b.shutdown().await);
    drop(b);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut again = connect(&p).await;
    again.write_all(b"POST /hook HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}").await.unwrap();
    let (said, _) = till_closed(&mut again).await.expect("answered once one closed");
    assert!(said.starts_with("HTTP/1.1 204"), "{said}");
    task.abort();
    let _ = std::fs::remove_file(p);
}

#[tokio::test]
async fn a_stalled_head_or_a_slow_answer_ends_within_the_wait() {
    use tokio::io::AsyncWriteExt;
    let slow = Router::new().route("/hook", post(|| async { tokio::time::sleep(Duration::from_secs(30)).await; StatusCode::NO_CONTENT }));
    let (task, p) = serving("wait", slow, 4, Duration::from_millis(300));
    let mut head = connect(&p).await;
    head.write_all(HEAD).await.unwrap();
    let (said, took) = till_closed(&mut head).await.expect("a head never finished is closed");
    assert!(!said.starts_with("HTTP/1.1 2") && took < Duration::from_millis(1000), "{said:?} {took:?}");
    let mut answer = connect(&p).await;
    answer.write_all(b"POST /hook HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n").await.unwrap();
    let (said, took) = till_closed(&mut answer).await.expect("a slow answer is cut");
    assert!(said.starts_with("HTTP/1.1 408") && took < Duration::from_millis(1000), "{said:?} {took:?}");
    task.abort();
    let _ = std::fs::remove_file(p);
}

#[tokio::test]
async fn stopping_a_link_ends_its_accepted_connections() {
    use tokio::io::AsyncWriteExt;
    let (task, p) = serving("stop", router(), 4, Duration::from_secs(5));
    let mut open = connect(&p).await;
    open.write_all(HEAD).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    task.abort();
    let (said, took) = till_closed(&mut open).await.expect("closed when the link stops");
    assert!(said.is_empty() && took < Duration::from_millis(500), "{said:?} {took:?}");
    let _ = std::fs::remove_file(p);
}
