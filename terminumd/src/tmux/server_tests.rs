// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;

#[test]
fn keys_name_host_socket_and_pane() {
    let n = Arc::new(Notify::new());
    let s = Server::new("local", "terminum", Link::Local { tmux: "tmux".into(), socket: "terminum".into(), conf: "/dev/null".into() }, n.clone());
    assert_eq!(s.key("%3"), "tmux:local:terminum:%3");
    let h = Server::new("devbox", "default", Link::Ssh { ssh: "ssh".into(), args: vec!["devbox.example".into()] }, n);
    assert_eq!(h.key("%7"), "tmux:devbox:default:%7");
    assert!(!h.is_local());
}

/// Sets the socket path as a connection would have read it (tests only).
pub fn known_socket_path(s: &Server, path: &str) {
    *s.socket_path.lock() = Some((s.generation.load(Ordering::Relaxed), path.to_string()));
}

/// Whether `w` wakes within 5 ms.
async fn soon(w: &mut Wake) -> bool {
    tokio::time::timeout(std::time::Duration::from_millis(5), w.notified()).await.is_ok()
}

#[tokio::test(start_paused = true)]
async fn one_output_wakes_every_viewer_of_the_pane_and_none_is_lost_between_waits() {
    use std::time::Duration;
    let s = Server::new("local", "tm-test", Link::Local { tmux: "tmux".into(), socket: "tm-test".into(), conf: "/dev/null".into() }, Arc::new(Notify::new()));
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(notices(Arc::downgrade(&s), rx));
    let (mut a, mut b, mut other) = (s.wake("%1"), s.wake("%1"), s.wake("%2"));
    assert!(!soon(&mut a).await, "nothing printed yet: no wake");

    let both = tokio::spawn(async move {
        let (ra, rb) = tokio::join!(tokio::time::timeout(Duration::from_secs(1), a.notified()), tokio::time::timeout(Duration::from_secs(1), b.notified()));
        assert!(ra.is_ok() && rb.is_ok(), "both viewers woken by one output");
        (a, b)
    });
    tokio::task::yield_now().await;
    tx.send(Notice::Output("%1".into())).unwrap();
    let (mut a, mut b) = both.await.unwrap();
    assert!(!soon(&mut other).await, "another pane's viewer sleeps on");

    // output while neither viewer waits (each capturing): kept for each one's next wait, once
    tx.send(Notice::Output("%1".into())).unwrap();
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert!(soon(&mut a).await && soon(&mut b).await, "an output between waits is not lost");
    assert!(!soon(&mut a).await, "and wakes only once");

    // a resize or zoom wakes every shown pane's viewers
    tx.send(Notice::Changed).unwrap();
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert!(soon(&mut a).await && soon(&mut b).await && soon(&mut other).await, "a layout change wakes all");

    drop((a, b, other));
    s.prune_wakes();
    assert!(s.wakes.lock().is_empty(), "wake-ups nobody holds are forgotten");
}
