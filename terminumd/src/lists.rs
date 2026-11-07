// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The list, server by server (AC-132): each tmux server has one read at a time, on its own task,
//! and what it read is kept on its `Server`. A finished read tells the pages the whole list, the
//! other servers' parts as last read, so a hung or slow server never holds up another's rows.
//! Its own rows stay through a dropped link, dimmed, where they were (AC-81).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::watch;

use crate::app::Shared;
use crate::tmux::server::Server;

/// Rows not read for this long, on a link still up, dim (AC-132).
const STALE_AFTER: Duration = Duration::from_secs(30);
/// The longest a caller waits for a server's read: its commands give up by themselves long before.
const READ_WAIT: Duration = Duration::from_secs(30);

/// What a read of a server found.
pub enum Got {
    /// its list
    Read(Told),
    /// its link is down: these groups say so, its rows as last read go in them
    Down(Vec<Value>),
    /// no way to read it: these groups, nothing kept
    Gone(Vec<Value>),
    /// its link is up, the list did not come or was not asked for (back-pressure)
    Unread,
}

/// A server's list as last read, on its `Server`.
pub struct ServerList {
    state: Mutex<State>,
    /// the last request finished: told after the pages were told
    done: watch::Sender<u64>,
    /// tmux said the list changed: read this server alone
    changed: AtomicBool,
}

#[derive(Default)]
struct State {
    requested: u64,
    running: bool,
    /// a read finished, whatever it found
    tried: bool,
    /// None until something was read
    told: Option<Told>,
    last_good: Arc<Vec<Value>>,
    read_at: Option<Instant>,
    /// `told` holds `last_good` dimmed: nothing to do again while it goes on unread
    dimmed: bool,
}

/// What the pages are told of this server.
#[derive(Clone, Default)]
pub struct Told {
    pub groups: Arc<Vec<Value>>,
    /// shown after every server's rows
    pub absent: Vec<Value>,
    pub keys: Vec<String>,
    /// nothing in it failed
    pub ok: bool,
}

impl Default for ServerList {
    fn default() -> ServerList {
        ServerList { state: Mutex::default(), done: watch::Sender::new(0), changed: AtomicBool::new(false) }
    }
}

impl ServerList {
    /// tmux said this server's list changed.
    pub fn mark_changed(&self) {
        self.changed.store(true, Ordering::Relaxed);
    }

    /// `f` over what the pages are told of it, None until it was read; no list clone is forced.
    pub fn with_told<R>(&self, f: impl FnOnce(Option<&Told>) -> R) -> R {
        f(self.state.lock().told.as_ref())
    }

    #[cfg(test)]
    pub fn told(&self) -> Option<Told> {
        self.state.lock().told.clone()
    }

    /// A read of it finished, whatever it found.
    pub fn tried(&self) -> bool {
        self.state.lock().tried
    }

    /// Keeps what a read found; true when it is this server's own list, fresh (a turn may settle by it).
    fn store(&self, got: Got, now: Instant) -> bool {
        let mut st = self.state.lock();
        st.tried = true;
        let dimmed = std::mem::take(&mut st.dimmed);
        match got {
            Got::Read(told) => {
                let ok = told.ok;
                if ok {
                    st.last_good = told.groups.clone();
                    st.read_at = Some(now);
                }
                st.told = Some(told);
                ok
            }
            Got::Down(groups) => {
                let groups = Arc::new(kept_rows(groups, &st.last_good));
                st.told = Some(Told { groups, ..Default::default() });
                false
            }
            Got::Gone(groups) => {
                st.last_good = Arc::default();
                st.told = Some(Told { groups: Arc::new(groups), ..Default::default() });
                false
            }
            Got::Unread => {
                let stale = st.read_at.is_none_or(|t| now.duration_since(t) >= STALE_AFTER);
                let again = stale && dimmed;
                st.dimmed = stale;
                let groups = if again { Arc::default() } else if stale { Arc::new(stale_rows(&st.last_good)) } else { st.last_good.clone() };
                if let Some(t) = st.told.as_mut() {
                    t.ok = false;
                    if !groups.is_empty() {
                        t.groups = groups;
                    }
                }
                false
            }
        }
    }
}

/// Reads `server` once more: a request during a read runs one more after it. The number to wait for.
pub fn request(app: &Shared, server: &Arc<Server>) -> u64 {
    let mut st = server.list.state.lock();
    st.requested += 1;
    let target = st.requested;
    if !st.running {
        st.running = true;
        tokio::spawn(run(app.clone(), Arc::downgrade(server)));
    }
    target
}

/// Every server is read again; nothing waits for any of them.
pub fn request_all(app: &Shared) {
    for s in app.tmux.all() {
        request(app, &s);
    }
}

/// News about one pane: its server's list alone (AC-132); no such server, all of them.
pub fn pane_changed(app: &Shared, key: &str) {
    match app.tmux.resolve(key) {
        Some((server, _)) => server.list_changed(),
        None => app.dirty.notify_one(),
    }
}

/// News about one host: its list alone (AC-132); no such host, all of them.
pub fn host_changed(app: &Shared, host: &str) {
    match app.tmux.host(host) {
        Some(server) => server.list_changed(),
        None => app.dirty.notify_one(),
    }
}

/// The servers tmux said changed are read again; the others are left alone.
pub fn request_changed(app: &Shared) {
    for s in app.tmux.all() {
        if s.list.changed.swap(false, Ordering::Relaxed) {
            request(app, &s);
        }
    }
}

/// `server`'s list read anew and told to the pages: it has what was just made.
pub async fn refresh_server(app: &Shared, server: &Arc<Server>) {
    let target = request(app, server);
    let mut done = server.list.done.subscribe();
    let _ = tokio::time::timeout(READ_WAIT, done.wait_for(|d| *d >= target)).await;
}

async fn run(app: Shared, server: Weak<Server>) {
    loop {
        let Some(s) = server.upgrade() else { return };
        let target = s.list.state.lock().requested;
        let got = crate::manager::read_one(&app, &s).await;
        let fresh = s.list.store(got, Instant::now());
        crate::manager::publish(&app, &s, fresh).await;
        s.list.done.send_replace(target);
        let mut st = s.list.state.lock();
        if st.requested == target {
            st.running = false;
            return;
        }
    }
}

/// A row of a server that cannot be read: where it was, with nothing said of its agent (AC-99).
fn offline_row(row: &Value) -> Value {
    let mut row = row.clone();
    for k in ["tool", "why", "subagents", "turnOver", "background"] {
        if let Some(o) = row.as_object_mut() {
            o.remove(k);
        }
    }
    row["state"] = Value::Null;
    row["offline"] = json!(true);
    row
}

fn offline_rows(items: &Value) -> Value {
    Value::Array(items.as_array().map(|a| a.iter().map(offline_row).collect()).unwrap_or_default())
}

/// The groups of a host that is down, each with its rows as last read (AC-81).
fn kept_rows(mut down: Vec<Value>, last_good: &[Value]) -> Vec<Value> {
    for g in &mut down {
        if let Some(old) = last_good.iter().find(|o| o["wid"] == g["wid"]) {
            g["items"] = offline_rows(&old["items"]);
        }
    }
    down
}

/// The groups as last read, on a link up that has not given a list for long: dimmed.
fn stale_rows(last_good: &[Value]) -> Vec<Value> {
    last_good.iter().map(|g| {
        let mut g = g.clone();
        g["items"] = offline_rows(&g["items"]);
        g
    }).collect()
}

#[cfg(test)]
#[path = "lists_tests.rs"]
mod tests;
