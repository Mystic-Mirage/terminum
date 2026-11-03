// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Synthetic Unix peers test timeouts and cancellation without SSH or live state.

use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn unicode_events_survive_every_http_frame_boundary() {
    let line = "data: {\"dirs\":[\"/café/日本\"],\"files\":[],\"moved\":[]}\n";
    for i in 0..line.len() {
        let mut buf = line.as_bytes()[..i].to_vec();
        assert!(take_line(&mut buf).is_none());
        buf.extend_from_slice(&line.as_bytes()[i..]);
        let decoded = take_line(&mut buf).unwrap();
        assert_eq!(decoded, line);
        let event = fs_change("devbox.example", decoded.strip_prefix("data:").unwrap()).unwrap();
        assert!(event.data().contains("/café/日本"));
        assert!(buf.is_empty());
    }
}

async fn peer(tag: &str, headers: Option<&'static [u8]>) -> (Agent, tokio::task::JoinHandle<()>) {
    let socket = std::env::temp_dir().join(format!("fbd-peer-{tag}-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        let _request_len = stream.read(&mut bytes).await.unwrap();
        if let Some(headers) = headers { stream.write_all(headers).await.unwrap(); }
        // The client must close its owned connection after a timeout or cancellation.
        let read = tokio::time::timeout(Duration::from_secs(1), stream.read(&mut bytes)).await.unwrap();
        assert!(matches!(read, Ok(0) | Err(_)));
    });
    (Agent { socket, token: "synthetic-token".into(), generation: 1 }, task)
}

fn request() -> hyper::Request<Body> {
    hyper::Request::builder().uri("/api/file").header(header::HOST, crate::agent::HOST).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn a_stalled_response_header_times_out_and_closes_the_connection() {
    let (agent, peer) = peer("headers", None).await;
    let result = send_with_deadline(&agent, request(), Duration::from_millis(30)).await;
    assert!(matches!(result, Err(e) if e.contains("timed out")));
    peer.await.unwrap();
    std::fs::remove_file(agent.socket).unwrap();
}

#[tokio::test]
async fn a_stalled_finite_body_times_out_and_closes_the_connection() {
    let (agent, peer) = peer("body", Some(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nx")).await;
    let (response, connection) = send_with_deadline(&agent, request(), Duration::from_secs(1)).await.unwrap();
    let body = finite_body(response.into_body(), connection, Duration::from_millis(30));
    assert!(body.collect().await.is_err());
    peer.await.unwrap();
    std::fs::remove_file(agent.socket).unwrap();
}

#[tokio::test]
async fn a_trickling_finite_body_has_a_total_deadline() {
    let socket = std::env::temp_dir().join(format!("fbd-peer-trickle-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _request_len = stream.read(&mut request).await.unwrap();
        stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        for _ in 0..20 {
            if stream.write_all(b"1\r\nx\r\n").await.is_err() { return }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("a trickle must not reset the finite body deadline");
    });
    let agent = Agent { socket, token: "synthetic-token".into(), generation: 1 };
    let (response, connection) = send_with_deadline(&agent, request(), Duration::from_secs(1)).await.unwrap();
    assert!(finite_body(response.into_body(), connection, Duration::from_millis(35)).collect().await.is_err());
    peer.await.unwrap();
    std::fs::remove_file(agent.socket).unwrap();
}

#[tokio::test]
async fn long_lived_streams_outlive_the_header_deadline_and_close_on_drop() {
    let (agent, peer) = peer("stream", Some(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")).await;
    let (response, connection) = send_with_deadline(&agent, request(), Duration::from_millis(30)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(!peer.is_finished(), "the SSE connection has no total request deadline");
    drop(response);
    drop(connection);
    peer.await.unwrap();
    std::fs::remove_file(agent.socket).unwrap();
}

/// An agent answering every request with `status`, HTML with a script and no policy, as a host
/// not to be trusted would.
fn hostile(tag: &str, status: &'static str) -> Agent {
    let socket = std::env::temp_dir().join(format!("fbd-peer-{tag}-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut bytes = [0; 4096];
            let _ = stream.read(&mut bytes).await;
            let body = "<script>steal()</script>";
            let head = format!("HTTP/1.1 {status}\r\nContent-Type: text/html\r\nETag: \"e1\"\r\nContent-Length: {}\r\n\r\n", body.len());
            let _ = stream.write_all(format!("{head}{body}").as_bytes()).await;
        }
    });
    Agent { socket, token: "synthetic-token".into(), generation: 1 }
}

#[tokio::test]
async fn a_hosts_answer_carries_terminumds_type_and_policy() {
    let get = |uri: &str| axum::http::Request::builder().uri(uri).body(Body::empty()).unwrap();
    let agent = hostile("typed", "200 OK");
    let res = forward("devbox.example", &agent, get("/api/raw?path=%2Fsrv%2Fshot.svg")).await.unwrap();
    assert_eq!(res.headers()[header::CONTENT_TYPE], "image/svg+xml", "by the file's name, not the host's text/html");
    assert_eq!(res.headers()[header::CONTENT_SECURITY_POLICY], crate::api_fs::RAW_CSP);
    assert_eq!(res.headers()[header::ETAG], "\"e1\"");
    let res = forward("devbox.example", &agent, get("/api/raw?path=%2Fsrv%2Fpage.html")).await.unwrap();
    assert_eq!(res.headers()[header::CONTENT_TYPE], "text/html", "an HTML file stays one, under the sandbox policy");
    assert_eq!(res.headers()[header::CONTENT_SECURITY_POLICY], crate::api_fs::RAW_CSP);
    let res = forward("devbox.example", &agent, get("/api/ls?path=%2Fsrv")).await.unwrap();
    assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json", "a listing is JSON whatever the host says");
    std::fs::remove_file(agent.socket).unwrap();
    let agent = hostile("typed-404", "404 Not Found");
    let res = forward("devbox.example", &agent, get("/api/raw?path=%2Fsrv%2Fshot.png")).await.unwrap();
    assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json", "an error is JSON, not a picture");
    std::fs::remove_file(agent.socket).unwrap();
}

#[tokio::test]
async fn a_range_goes_to_the_host_and_its_partial_answer_comes_back() {
    let socket = std::env::temp_dir().join(format!("fbd-peer-range-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let seen = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        let n = stream.read(&mut bytes).await.unwrap();
        let head = "HTTP/1.1 206 Partial Content\r\nAccept-Ranges: bytes\r\nContent-Range: bytes 2-5/10\r\nContent-Length: 4\r\n\r\n2345";
        stream.write_all(head.as_bytes()).await.unwrap();
        String::from_utf8_lossy(&bytes[..n]).to_lowercase()
    });
    let agent = Agent { socket, token: "synthetic-token".into(), generation: 1 };
    let req = axum::http::Request::builder().uri("/api/raw?path=%2Fsrv%2Fclip.mp4").header(header::RANGE, "bytes=2-5").body(Body::empty()).unwrap();
    let res = forward("devbox.example", &agent, req).await.unwrap();
    assert!(seen.await.unwrap().contains("range: bytes=2-5"), "the Range reached the host");
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(res.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
    assert_eq!(res.headers()[header::ACCEPT_RANGES], "bytes");
    assert_eq!(res.headers()[header::CONTENT_TYPE], "video/mp4");
    assert_eq!(&res.into_body().collect().await.unwrap().to_bytes()[..], b"2345");
    std::fs::remove_file(agent.socket).unwrap();
}
