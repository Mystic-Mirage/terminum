// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Reads a page asks for (older history, unfolds, the shown folder) wait on tmux or ssh, so
//! they run off the page's message loop: typing never waits for them (§9). Reads of one kind
//! run one at a time in the order sent; at most `WAITING` wait behind it, and one more is
//! refused in words. A read ends when its page closes.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::Page;

/// Reads of one kind that may wait behind the running one.
pub const WAITING: usize = 4;

type Read = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Each kind's queue, read by its one worker: the order sent is the order run.
#[derive(Default)]
pub struct Lanes(Mutex<HashMap<&'static str, mpsc::Sender<Read>>>);

/// Queues `work` in `kind`'s lane; false when that lane is full or its page closed (nothing queued).
pub fn spawn<F>(page: &Arc<Page>, kind: &'static str, work: F) -> bool
where
    F: Future<Output = ()> + Send + 'static,
{
    let mut lanes = page.reads.0.lock();
    let tx = lanes.entry(kind).or_insert_with(|| worker(page.clone()));
    tx.try_send(Box::pin(work)).is_ok()
}

/// The lane's worker: one read at a time, until the page closes (the read running then and
/// those waiting are dropped).
fn worker(page: Arc<Page>) -> mpsc::Sender<Read> {
    let (tx, mut rx) = mpsc::channel::<Read>(WAITING);
    tokio::spawn(async move {
        loop {
            let work = tokio::select! {
                w = rx.recv() => match w { Some(w) => w, None => break },
                _ = page.closed() => break,
            };
            tokio::select! {
                _ = work => {}
                _ = page.closed() => break,
            }
        }
    });
    tx
}

#[cfg(test)]
#[path = "reads_tests.rs"]
mod tests;
