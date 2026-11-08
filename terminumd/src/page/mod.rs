// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The pages (browser tabs) connected over `/ws` (AC-01–AC-07): each shows one pane at a
//! time. A page that stops reading is dropped, never waited for (a phone that sleeps).

pub mod answers;
pub mod bookmarks;
pub mod bookmarks_find;
pub mod client;
pub mod diag;
pub mod find;
pub mod folds;
pub mod pystr;
pub mod hist;
pub mod marks;
pub mod plans;
mod lost_marks;
pub mod projects;
pub mod quick;
pub mod reads;
pub mod search;
pub mod sent;
pub mod sessions;
pub mod sizing;
pub mod stream;
pub mod trace;
pub mod timing;
pub mod turns;
pub mod typing;
pub mod unread;
pub mod upload;
pub mod ws;
pub mod zoom;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::mpsc;

/// Pages per terminumd (iterm-enhancer's 100 windows, AC-20 limits): the 101st is refused.
pub const MAX_PAGES: usize = 100;
/// Messages a page may have waiting; a page that falls this far behind is dropped.
const BACKLOG: usize = 512;

pub const CLOSED: &str = "This pane has closed. Pick another one.";

/// One connected page.
pub struct Page {
    pub id: u64,
    /// not the Mac's own browser (through a proxy: a phone over the tailnet): fewer rights (§9 table)
    pub web: bool,
    tx: mpsc::Sender<String>,
    closed: AtomicBool,
    /// wakes whatever waits on this page (reader, writer, stream, reads) when it closes
    woke: tokio::sync::Notify,
    /// history and folder reads off the message loop, one lane per kind
    pub reads: reads::Lanes,
    pub state: Mutex<client::State>,
    /// its part of the latency trace (AC-38)
    pub tracer: Mutex<trace::Tracer>,
    /// its browser and address, for the input log (AC-147)
    pub client: crate::inputs::Client,
    /// the sign-in it presented at the upgrade: the page ends with it (AC-09), never in a log
    credential: String,
}

impl Page {
    /// Queues a message; a page whose queue is full is closed (it reconnects and starts anew).
    pub fn send(&self, text: String) -> bool {
        if self.closed.load(Ordering::Relaxed) {
            return false;
        }
        match self.tx.try_send(text) {
            Ok(()) => true,
            Err(_) => {
                tracing::warn!(event = "page.dropped", page = self.id, reason = "not reading");
                self.close();
                false
            }
        }
    }

    pub fn send_json(&self, v: &Value) -> bool {
        self.send(v.to_string())
    }

    /// The page's socket ends now: the writer wakes (on a message never sent) and closes it.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        let _ = self.tx.try_send(String::new());
        self.woke.notify_waiters();
    }

    /// Returns when the page has closed: one signal for the reader, the writer, the stream
    /// and the reads, so a stuck socket ends all of them.
    pub async fn closed(&self) {
        loop {
            let woke = self.woke.notified();
            tokio::pin!(woke);
            woke.as_mut().enable();
            if self.is_closed() {
                return;
            }
            tokio::select! {
                _ = woke => {}
                _ = self.tx.closed() => {}               // the queue's receiver dropped without close()
            }
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed) || self.tx.is_closed()
    }

    /// Its sign-in is still live: not revoked (Sign out elsewhere), not expired (AC-09).
    pub fn signed_in(&self, signins: &crate::auth::signin::Signins) -> bool {
        signins.check(&self.credential, false)
    }
}

#[derive(Default)]
pub struct Pages {
    next: AtomicU64,
    list: Mutex<HashMap<u64, Arc<Page>>>,
}

impl Pages {
    /// A new page and the receiving end of its queue, or None when there are too many.
    pub fn add(&self, client: crate::inputs::Client, credential: String, recorder: Arc<trace::Recorder>) -> Option<(Arc<Page>, mpsc::Receiver<String>)> {
        let web = client.web;
        let mut list = self.list.lock();
        list.retain(|_, p| !p.is_closed());
        if list.len() >= MAX_PAGES {
            return None;
        }
        let (tx, rx) = mpsc::channel(BACKLOG);
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let page = Arc::new(Page { id, web, tx, closed: AtomicBool::new(false), woke: Default::default(), reads: Default::default(), state: Mutex::new(client::State::default()), tracer: Mutex::new(trace::Tracer::new(recorder)), client, credential });
        list.insert(id, page.clone());
        Some((page, rx))
    }

    pub fn remove(&self, id: u64) {
        self.list.lock().remove(&id);
    }

    pub fn all(&self) -> Vec<Arc<Page>> {
        self.list.lock().values().filter(|p| !p.is_closed()).cloned().collect()
    }

    pub fn count(&self) -> usize {
        self.all().len()
    }

    /// Whether any page is in sight (not hidden, focused or not): the list is refreshed only then.
    pub fn any_in_sight(&self) -> bool {
        self.all().iter().any(|p| p.state.lock().look != client::Look::Hidden)
    }

    /// The same message to every page (encoded once).
    pub fn tell(&self, text: &str) {
        for p in self.all() {
            p.send(text.to_string());
        }
    }

    /// Every page closes: their browsers sign in again (a new password, `terminum signout`).
    pub fn close_all(&self) {
        for p in self.all() {
            p.close();
        }
    }

    /// The pages of a sign-in that ended (Sign out in one browser) close now, not at their next
    /// ping: another tab or device of that sign-in types and reads no more (AC-09).
    pub fn close_signed_out(&self, signins: &crate::auth::signin::Signins) {
        for p in self.all().into_iter().filter(|p| !p.signed_in(signins)) {
            p.close();
        }
    }
}
