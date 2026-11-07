// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a server's control clients tell (control.rs `Notice`): wake-ups of shown panes, lists
//! to read again, flow control, and what a full-screen program copied after a page's mouse
//! release (AC-06).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::{mpsc, watch};

use super::{Notice, Server};

/// How long after a page's mouse release a program's clipboard write is that page's (AC-06).
const COPY_WAIT: Duration = Duration::from_secs(2);

/// What a program's copy is handed to.
pub type OnCopy = Arc<dyn Fn(String) + Send + Sync>;
/// Panes armed by a release: until when, and for whom.
pub(super) type Armed = Mutex<HashMap<String, (Instant, OnCopy)>>;

impl Server {
    /// Lines for the control client of `pane`'s session, in order after what it was sent before;
    /// nothing waits for them (their answers do not matter): what was asked goes first even if
    /// this server hangs (AC-132).
    pub fn send_on(&self, pane: &str, lines: Vec<String>) {
        if let Some(c) = self.client(pane) {
            for line in lines {
                drop(c.send(line));
            }
        }
    }

    /// What was asked of it is still on its way (AC-132): no more on top of it.
    pub fn backed_up(&self) -> bool {
        self.bulk().is_some_and(|c| c.waiting() >= crate::tmux::control::STALL)
    }

    /// The client to ask for what is read over and over (screens for the list, last lines), unless
    /// it is backed up.
    pub fn reader(&self) -> Option<Arc<crate::tmux::control::Control>> {
        self.bulk().filter(|c| c.waiting() < crate::tmux::control::STALL)
    }

    /// Its list is to be read again, its own alone (lists.rs).
    pub(super) fn list_changed(&self) {
        self.list.mark_changed();
        self.changed.notify_one();
    }

    /// A page's mouse release went to the pane `%N`: what its program copies within
    /// `COPY_WAIT` goes to `to` (the latest page's release wins).
    pub fn arm_copy(&self, pane: &str, to: OnCopy) {
        let mut armed = self.copies.lock();
        let now = Instant::now();
        armed.retain(|_, (until, _)| *until > now);
        armed.insert(pane.to_string(), (now + COPY_WAIT, to));
    }

    /// A clipboard write of the pane `%N` (its base64): decoded for its page when armed, else
    /// dropped as it came (AC-06).
    fn copied(&self, pane: &str, b64: &[u8]) {
        let to = {
            let mut armed = self.copies.lock();
            let now = Instant::now();
            armed.retain(|_, (until, _)| *until > now); // a page's arm goes once it is over
            armed.get(pane).map(|(_, f)| f.clone())
        };
        match to {
            Some(f) => {
                if let Some(text) = crate::tmux::clip::text(b64) {
                    f(text);
                }
            }
            None => tracing::debug!(event = "clip.unasked", server = %self.name, pane),
        }
    }
}

/// One viewer's wake-up of a shown pane (`Server::wake`).
pub struct Wake(watch::Receiver<()>);

impl Wake {
    pub(super) fn new(rx: watch::Receiver<()>) -> Wake {
        Wake(rx)
    }

    /// Until the pane printed since this viewer last woke (at once if it already has).
    pub async fn notified(&mut self) {
        if self.0.changed().await.is_err() {
            std::future::pending::<()>().await;     // its server is gone: only the poll looks again
        }
    }
}

pub(super) async fn notices(server: std::sync::Weak<Server>, mut rx: mpsc::UnboundedReceiver<Notice>) {
    while let Some(n) = rx.recv().await {
        let Some(s) = server.upgrade() else { return };
        match n {
            Notice::Output(pane) => {
                *s.printed.lock().entry(pane.clone()).or_insert(0) += 1;
                s.woke(Some(&pane));
            }
            Notice::Changed => {
                s.list_changed();
                // a resize or a zoom redraws shown panes: capture them now
                s.woke(None);
            }
            Notice::Copy(pane, b64) => s.copied(&pane, &b64),
            Notice::Pause(pane) => {
                // flow control on a slow link: shown panes are captured anyway, so go on
                if let Some(c) = s.client(&pane) {
                    drop(c.send(format!("refresh-client -A '{pane}:continue'")));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Link;
    use super::*;

    /// AC-06: a program's copy goes to the page whose release armed its pane, within COPY_WAIT only.
    #[test]
    fn a_copy_goes_only_to_the_page_whose_release_armed_its_pane() {
        let s = Server::new("local", "t", Link::Local { tmux: "tmux".into(), socket: "t".into(), conf: "/dev/null".into() }, Default::default());
        let got: Arc<Mutex<Vec<String>>> = Arc::default();
        let to = |tag: &'static str| -> OnCopy { let g = got.clone(); Arc::new(move |t| g.lock().push(format!("{tag}:{t}"))) };
        let b64 = |t: &str| crate::tmux::clip::encode(t.as_bytes()).into_bytes();
        s.copied("%1", &b64("unasked"));
        s.arm_copy("%1", to("a"));
        s.copied("%2", &b64("another pane"));
        s.copied("%1", &b64("word"));
        s.copied("%1", &b64("line"));
        s.arm_copy("%1", to("b"));
        s.copied("%1", &b64("latest page"));
        s.copies.lock().get_mut("%1").expect("armed").0 = Instant::now() - Duration::from_millis(1);
        s.copied("%1", &b64("too late"));
        assert_eq!(*got.lock(), ["a:word", "a:line", "b:latest page"]);
    }
}
