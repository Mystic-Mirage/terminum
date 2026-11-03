// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::oneshot;

use super::{spawn, WAITING};
use crate::page::{trace, Page, Pages};

fn page() -> (Arc<Page>, tokio::sync::mpsc::Receiver<String>, ()) {
    let dir = std::env::temp_dir().join(format!("tm-reads-{}", std::process::id()));
    let (p, rx) = Pages::default().add(false, trace::Recorder::new(dir)).unwrap();
    (p, rx, ())
}

#[tokio::test]
async fn a_slow_read_never_holds_up_the_caller() {
    let (p, _rx, _d) = page();
    let (_hold, never) = oneshot::channel::<()>();
    assert!(spawn(&p, "more", async move { let _ = never.await; }));
    // the message loop goes on at once: another kind runs while the first still waits
    let (done, got) = oneshot::channel();
    assert!(spawn(&p, "files", async move { let _ = done.send(()); }));
    tokio::time::timeout(Duration::from_secs(1), got).await.expect("a read of another kind waited").unwrap();
}

#[tokio::test]
async fn reads_of_one_kind_run_in_order_and_the_lane_is_bounded() {
    let (p, _rx, _d) = page();
    let (gate, open) = oneshot::channel::<()>();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    assert!(spawn(&p, "more", async move { let _ = open.await; s.lock().push(0); }));
    tokio::time::sleep(Duration::from_millis(20)).await;     // the worker took it: it runs, WAITING may wait
    for i in 1..=WAITING {
        let s = seen.clone();
        assert!(spawn(&p, "more", async move { s.lock().push(i); }));
    }
    assert!(!spawn(&p, "more", async {}), "one more than the lane holds is refused");
    gate.send(()).unwrap();
    for _ in 0..100 {
        if seen.lock().len() == WAITING + 1 { break; }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(*seen.lock(), (0..=WAITING).collect::<Vec<_>>());
    assert!(spawn(&p, "more", async {}), "the lane is free again once its reads ended");
}

#[tokio::test]
async fn a_closed_page_ends_its_waiting_reads() {
    let (p, _rx, _d) = page();
    let (_hold, never) = oneshot::channel::<()>();
    let (running, ran_out) = oneshot::channel::<()>();
    let (waiting, waited_out) = oneshot::channel::<()>();
    assert!(spawn(&p, "unfold", async move { let _r = running; let _ = never.await; }));
    assert!(spawn(&p, "unfold", async move { let _w = waiting; }));
    p.close();
    // each read's future is dropped (its sender with it): the running one and the one behind it
    tokio::time::timeout(Duration::from_millis(500), ran_out).await.expect("a read outlived its closed page").unwrap_err();
    tokio::time::timeout(Duration::from_millis(500), waited_out).await.expect("a waiting read outlived its closed page").unwrap_err();
    assert!(!spawn(&p, "unfold", async {}), "a closed page takes no more reads");
}

#[tokio::test]
async fn closed_returns_at_once_for_a_closed_page_and_wakes_on_close() {
    let (p, _rx, _d) = page();
    let q = p.clone();
    let waiter = tokio::spawn(async move { q.closed().await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!waiter.is_finished());
    p.close();
    tokio::time::timeout(Duration::from_millis(500), waiter).await.expect("close did not wake the waiter").unwrap();
    tokio::time::timeout(Duration::from_millis(50), p.closed()).await.expect("an already closed page waited");
}
