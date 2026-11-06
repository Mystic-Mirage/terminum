// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Reads a page asks for (older history, unfolds, the shown folder) wait on tmux or ssh, so
//! they run off the page's message loop: typing never waits for them (§9). Reads of one kind
//! on one server run one at a time in the order sent; at most `WAITING` wait behind it, and one
//! more is refused in words. A read ends when its page closes.
//!
//! Actions (a window, a move, a restore) wait likewise, in a lane per server, so a hung server
//! holds up only its own (AC-132). An action is never cut part-way: it and those behind it finish
//! after their page closed.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::Page;

/// Reads of one kind that may wait behind the running one.
pub const WAITING: usize = 4;
/// Actions on one server that may wait behind the running one (a person's clicks, a stalled host).
pub const ACTIONS_WAITING: usize = 16;

type Read = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Each kind's queue, read by its one worker: the order sent is the order run.
#[derive(Default)]
pub struct Lanes(Mutex<HashMap<String, mpsc::Sender<Read>>>);

/// Queues `work` in `kind`'s lane; false when that lane is full or its page closed (nothing queued).
pub fn spawn<F>(page: &Arc<Page>, kind: &str, work: F) -> bool
where
    F: Future<Output = ()> + Send + 'static,
{
    let mut lanes = page.reads.0.lock();
    let tx = lanes.entry(kind.to_string()).or_insert_with(|| worker(page.clone()));
    tx.try_send(Box::pin(work)).is_ok()
}

/// Queues the action `work` in the lane of the server `lane`; false when that lane is full. Its
/// worker holds no page: it ends with the page, once what waits is done.
pub fn spawn_action<F>(page: &Arc<Page>, lane: &str, work: F) -> bool
where
    F: Future<Output = ()> + Send + 'static,
{
    let mut lanes = page.reads.0.lock();
    let tx = lanes.entry(format!("action {lane}")).or_insert_with(|| {
        let (tx, mut rx) = mpsc::channel::<Read>(ACTIONS_WAITING);
        tokio::spawn(async move {
            while let Some(work) = rx.recv().await {
                work.await;
            }
        });
        tx
    });
    tx.try_send(Box::pin(work)).is_ok()
}

/// The lane's worker: one read at a time, until the page closes (the read running then and
/// those waiting are dropped).
fn worker(page: Arc<Page>) -> mpsc::Sender<Read> {
    let (tx, mut rx) = mpsc::channel::<Read>(WAITING);
    tokio::spawn(async move {
        let closed = page.closed();
        tokio::pin!(closed);
        loop {
            let work = tokio::select! {
                w = rx.recv() => match w { Some(w) => w, None => break },
                _ = &mut closed => break,
            };
            tokio::select! {
                _ = work => {}
                _ = &mut closed => break,
            }
        }
    });
    tx
}

#[cfg(test)]
#[path = "reads_tests.rs"]
mod tests;
