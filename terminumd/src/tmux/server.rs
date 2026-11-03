// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! One tmux server terminumd is the control client of: the private local one (`tmux -L
//! terminum`, started by terminumd with its own config, OQ-01) or a remote host's over ssh
//! (AC-81). Locally there is a control client per session (its panes' `%output` arrives only
//! there, AS-06) for notices, typing and screens, and a bulk client (`no-output`) for the
//! list, history pages and search, so a bulk read never queues ahead of an echo (OQ-02).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot, watch, Notify};

use super::control::{Answer, Control, Notice};
pub use super::command::{host_attach_command, host_command, plain};

/// How a server is reached.
#[derive(Clone, Debug)]
pub enum Link {
    /// this Mac: `tmux -L <socket>`, started with `conf` when it is not running
    Local { tmux: PathBuf, socket: String, conf: PathBuf },
    /// a host: `ssh <args> -- <remote tmux command>` (args checked by hosts::recipe)
    Ssh { ssh: PathBuf, args: Vec<String> },
}

/// The problem said when tmux is not installed.
pub const NO_TMUX: &str = if cfg!(target_os = "macos") { "tmux is needed for terminals: brew install tmux" } else { "tmux is needed for terminals: sudo apt install tmux (or your system's package manager)" };

/// The session a host's control client holds (OQ-81).
pub const HOST_SESSION: &str = "terminum";

/// A host's session client started (AC-89): its session and how the start went.
pub type Started = (String, Result<(), String>);

/// What the list says about a server.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum Status {
    Attached,
    Connecting,
    Reconnecting { why: String, retry_s: u64 },
    Failed { why: String },
    Off,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Arc<Control>>,
    bulk: Option<Arc<Control>>,
    /// pane id -> its session, from the last list
    panes: HashMap<String, String>,
}

pub struct Server {
    /// "local" or the host's name
    pub name: String,
    /// the socket part of a pane key: the -L name locally, "default" on a host
    pub socket: String,
    pub link: Link,
    inner: Mutex<Inner>,
    wakes: Mutex<HashMap<String, watch::Sender<u64>>>,
    printed: Mutex<HashMap<String, u64>>,
    /// the list must be read again (a window, pane, session or layout changed)
    pub dirty: Arc<Notify>,
    /// a control client ended (the manager reconnects or reports)
    pub ended: Arc<Notify>,
    pub status: Mutex<Status>,
    /// bumped whenever control clients are replaced: shown panes capture anew
    pub generation: AtomicU64,
    /// `extended-keys` is on, as asked once per generation (AS-10)
    extended: Mutex<Option<(u64, bool)>>,
    /// a host's `HOME`, as read once per generation
    home: Mutex<Option<(u64, String)>>,
    /// its identity (machine, socket, pid), as read once per generation
    identity: Mutex<Option<(u64, String)>>,
    /// tmux's `#{socket_path}`, read once per generation: a hook event names its server by it
    socket_path: Mutex<Option<(u64, String)>>,
    /// the user's own sessions attached by name (AC-89): on a server of the user's own on
    /// this Mac all it lists; on a host besides `terminum`; `None` for the private server
    pub only: Mutex<Option<Vec<String>>>,
    /// a host whose record changed in settings.json and still runs on its previous recipe: why
    pub held: Mutex<Option<String>>,
    /// a host's session clients started again after an end: session -> (delay s, not before, started)
    pub tries: Mutex<HashMap<String, (u64, std::time::Instant, std::time::Instant)>>,
    /// its last client ended with tmux's own `%exit` while one pane at most was shown: the
    /// user closed the last one (or ran kill-server on it), nothing to make again (AC-15)
    closed_by_user: AtomicBool,
    /// panes a page's mouse release went to lately: what their program copies goes to it (AC-06)
    copies: notices::Armed,
}

impl Server {
    /// `dirty` is shared by every server: one list refresh reads them all.
    pub fn new(name: &str, socket: &str, link: Link, dirty: Arc<Notify>) -> Arc<Server> {
        Arc::new(Server {
            name: name.to_string(),
            socket: socket.to_string(),
            link,
            inner: Mutex::default(),
            wakes: Mutex::default(),
            printed: Mutex::default(),
            dirty,
            ended: Arc::new(Notify::new()),
            status: Mutex::new(Status::Connecting),
            generation: AtomicU64::new(0),
            extended: Mutex::new(None),
            home: Mutex::new(None),
            identity: Mutex::new(None),
            socket_path: Mutex::new(None),
            only: Mutex::new(None),
            held: Mutex::new(None),
            tries: Mutex::default(),
            closed_by_user: AtomicBool::new(false),
            copies: Mutex::default(),
        })
    }

    /// One of the user's own tmux servers on this Mac, attached by session (AC-89).
    pub fn is_users(&self) -> bool {
        self.is_local() && self.only.lock().is_some()
    }

    /// Whether `session` is one of the user's own, attached by name (AC-89): never sized by
    /// looking, its windows given their size back.
    pub fn users(&self, session: &str) -> bool {
        self.only.lock().as_ref().is_some_and(|o| o.iter().any(|s| s == session))
    }

    /// Whether the list shows `session`: the user's attached ones, a host's `terminum`
    /// (OQ-81), every session of the private server.
    pub fn shows(&self, session: &str) -> bool {
        self.users(session) || if self.is_local() { self.only.lock().is_none() } else { session == HOST_SESSION }
    }

    /// The session of `pane`, as of the last list.
    pub fn session_of(&self, pane: &str) -> Option<String> {
        self.inner.lock().panes.get(pane).cloned()
    }

    /// The first part of a group id `<prefix>/<session>`: "local" for the private server, the
    /// host's name, `local:<socket>` for an attached server.
    pub fn group_prefix(&self) -> String {
        if self.is_users() { self.id() } else { self.name.clone() }
    }

    pub fn is_local(&self) -> bool {
        matches!(self.link, Link::Local { .. })
    }

    /// The server's own name among all (`<host>:<socket>`, a pane key's middle): this Mac may
    /// have more than one (the private one, the user's own attached, AC-89), so a name alone
    /// does not tell two apart, and their `@N` windows clash.
    pub fn id(&self) -> String {
        format!("{}:{}", self.name, self.socket)
    }

    /// The key of pane `%N` here: `tmux:<host>:<socket>:%N`.
    pub fn key(&self, pane: &str) -> String {
        format!("tmux:{}:{}:{}", self.name, self.socket, pane)
    }

    /// The control client that holds `pane`'s session (typing, screens), else the bulk one.
    pub fn client(&self, pane: &str) -> Option<Arc<Control>> {
        let inner = self.inner.lock();
        inner.panes.get(pane).and_then(|s| inner.sessions.get(s)).filter(|c| c.alive()).cloned().or_else(|| inner.bulk.clone().filter(|c| c.alive()))
    }

    /// The client for the list, history pages and search.
    pub fn bulk(&self) -> Option<Arc<Control>> {
        let inner = self.inner.lock();
        inner.bulk.clone().filter(|c| c.alive()).or_else(|| inner.sessions.values().find(|c| c.alive()).cloned())
    }

    /// Runs one command on the bulk client.
    pub async fn run(&self, line: impl Into<String>) -> Answer {
        match self.bulk() {
            Some(c) => c.run(line).await,
            None => Err(format!("tmux on {} is not connected", self.label())),
        }
    }

    /// Commands on the bulk client, written at once, answered in order.
    pub async fn run_all(&self, lines: Vec<String>) -> Vec<Answer> {
        match self.bulk() {
            Some(c) => c.run_all(lines).await,
            None => lines.iter().map(|_| Err(format!("tmux on {} is not connected", self.label()))).collect(),
        }
    }

    /// Commands on the control client of `pane`'s session, written at once, answered in order.
    pub async fn run_all_on(&self, pane: &str, lines: Vec<String>) -> Vec<Answer> {
        match self.client(pane) {
            Some(c) => c.run_all(lines).await,
            None => lines.iter().map(|_| Err(format!("tmux on {} is not connected", self.label()))).collect(),
        }
    }

    /// The server in words: "this Mac", "your tmux" (`tmux -L <socket>` for another socket), a host.
    pub fn label(&self) -> String {
        match (self.is_users(), self.socket.as_str()) {
            (true, "default") => "your tmux".into(),
            (true, s) => format!("tmux -L {s}"),
            _ if self.is_local() => "this Mac".into(),
            _ => self.name.clone(),
        }
    }

    /// Whether its tmux was closed by the user since this was last asked (AC-15).
    pub fn took_closed_by_user(&self) -> bool {
        self.closed_by_user.swap(false, Ordering::Relaxed)
    }

    /// The pane -> session map of the last list.
    pub fn set_panes(&self, panes: HashMap<String, String>) {
        self.inner.lock().panes = panes;
    }

    /// The pane `%N`'s wake-up, one per viewer: its revision goes up on each `%output`, so every
    /// viewer wakes, and one that was capturing meanwhile sees the change on its next wait.
    pub fn wake(&self, pane: &str) -> Wake {
        Wake::new(self.wakes.lock().entry(pane.to_string()).or_insert_with(|| watch::channel(0).0).subscribe())
    }

    /// Wakes the viewers of `pane`, or of every shown pane.
    pub fn woke(&self, pane: Option<&str>) {
        let wakes = self.wakes.lock();
        let bump = |tx: &watch::Sender<u64>| tx.send_modify(|r| *r = r.wrapping_add(1));
        match pane {
            Some(p) => wakes.get(p).into_iter().for_each(bump),
            None => wakes.values().for_each(bump),
        }
    }

    /// `%output`s of the pane `%N` so far: a long run of keys goes on once it printed (typing.rs).
    pub fn printed(&self, pane: &str) -> u64 {
        self.printed.lock().get(pane).copied().unwrap_or(0)
    }

    /// Forgets wake-ups nobody holds any more.
    pub fn prune_wakes(&self) {
        self.wakes.lock().retain(|_, tx| tx.receiver_count() > 0);
        self.printed.lock().retain(|p, _| self.wakes.lock().contains_key(p));
    }

    /// Whether any control client is alive.
    pub fn connected(&self) -> bool {
        let inner = self.inner.lock();
        inner.bulk.as_ref().is_some_and(|c| c.alive()) || inner.sessions.values().any(|c| c.alive())
    }

    /// Whether the server has `extended-keys` on (terminum's tmux.conf sets it; a host's tmux
    /// is off by default) and is tmux 3.5+ (which has `extended-keys-format`: 3.4 types
    /// `send-keys S-Enter` as the text "S-Enter"): asked once per connection, an error is off.
    pub async fn extended_keys(&self, client: &Control) -> bool {
        let generation = self.generation.load(Ordering::Relaxed);
        if let Some((g, on)) = *self.extended.lock() && g == generation {
            return on;
        }
        let answers = client.run_all(vec!["show-options -sv extended-keys".into(), "show-options -gv extended-keys-format".into()]).await;
        let on = matches!(&answers[..], [Ok(keys), Ok(_)] if keys.first().is_some_and(|v| v.trim() != "off"));
        *self.extended.lock() = Some((generation, on));
        on
    }

    /// The server's `HOME` (for "~" in a host's paths), read once per connection.
    pub async fn home(&self) -> Option<String> {
        self.per_connection(&self.home, async {
            self.run("show-environment -g HOME").await.ok()?.into_iter().find_map(|l| l.strip_prefix("HOME=").map(str::to_string))
        }).await
    }

    /// Which tmux server this is, however it is reached: an id terminumd gives it once, a
    /// server option `@terminum-id` (set only where none is, so two connections to one server
    /// read the same), read once per connection. Two hosts that are one machine, or a host that
    /// is this Mac, have the same (a pane is listed once, AC-89).
    pub async fn identity(&self) -> Option<String> {
        self.per_connection(&self.identity, async {
            let mine = crate::auth::crypto::random_hex::<8>();
            let set = format!("set -sqo @terminum-id {mine}");
            let read = self.run_all(vec![set, "show -sv @terminum-id".into()]).await.pop()?.ok()?;
            read.into_iter().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty())
        }).await
    }

    /// The server's full socket path (the first field of a pane's `TMUX`), read once per connection.
    pub async fn socket_path(&self) -> Option<String> {
        self.per_connection(&self.socket_path, async {
            self.run("display -p '#{socket_path}'").await.ok()?.into_iter().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty())
        }).await
    }

    /// The identity as last read (none before the first read or after a reconnect).
    pub fn identity_known(&self) -> Option<String> {
        let generation = self.generation.load(Ordering::Relaxed);
        self.identity.lock().as_ref().filter(|(g, _)| *g == generation).map(|(_, i)| i.clone())
    }

    /// `read`'s answer, kept in `slot` while this connection (generation) lasts.
    async fn per_connection(&self, slot: &Mutex<Option<(u64, String)>>, read: impl std::future::Future<Output = Option<String>>) -> Option<String> {
        let generation = self.generation.load(Ordering::Relaxed);
        if let Some((g, v)) = slot.lock().as_ref() && *g == generation {
            return Some(v.clone());
        }
        let v = read.await?;
        *slot.lock() = Some((generation, v.clone()));
        Some(v)
    }

    pub fn attached(&self) -> bool {
        let inner = self.inner.lock();
        inner.bulk.as_ref().is_some_and(|c| c.alive() && c.attached.load(Ordering::Relaxed))
    }

    /// Why the last control client ended (ssh's or tmux's last words).
    pub fn last_words(&self) -> Option<String> {
        let inner = self.inner.lock();
        inner.bulk.as_ref().and_then(|c| c.ended.lock().clone())
    }

    /// Signals every control client to end now, also those others still hold (terminumd
    /// stops: `exit` runs no destructors).
    pub fn kill_clients(&self) {
        let inner = self.inner.lock();
        for c in inner.sessions.values().chain(inner.bulk.iter()) {
            c.kill();
        }
    }

    /// Kills every control client (the host was switched off, or terminumd stops).
    pub fn disconnect(&self) {
        let mut inner = self.inner.lock();
        inner.sessions.clear();
        inner.bulk = None;
    }

    /// The sessions of a local server, asked without a control client: `None` when no
    /// server runs on the socket.
    pub async fn local_sessions(&self) -> Result<Option<Vec<String>>, String> {
        let Link::Local { tmux, socket, .. } = &self.link else { return Ok(Some(vec![HOST_SESSION.into()])) };
        let out = plain(tmux).args(["-L", socket, "list-sessions", "-F", "#{session_name}"]).output().await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound { NO_TMUX.to_string() } else { format!("tmux: {}: {e}", tmux.display()) }
        })?;
        if out.status.success() {
            return Ok(Some(String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).filter(|s| !s.is_empty()).collect()));
        }
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("no server running") || err.contains("error connecting") || err.contains("No such file") {
            return Ok(None);
        }
        Err(format!("tmux: {}", err.trim()))
    }

    /// A command of the local server by a process of its own (right after a start no control
    /// client runs one), with its config should it start it; each argument one word for tmux.
    pub async fn local(&self, args: &[&str]) -> Result<(), String> {
        let Link::Local { tmux, socket, conf } = &self.link else { return Err("not a local server".into()) };
        let out = plain(tmux).args(["-L", socket, "-f"]).arg(conf).args(args).output().await.map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
    }

    /// Starts the local server with its config and one session `main` in the home folder
    /// (AC-01 first start); the user's default server is never touched.
    pub async fn start_local(&self, session: &str, dir: &str) -> Result<(), String> {
        self.local(&["new-session", "-d", "-s", session, "-c", dir]).await
    }

    /// Brings the control clients in line with `sessions`: one for each, the bulk one
    /// attached to the first; clients of sessions that are gone end.
    pub fn sync_clients(self: &Arc<Self>, sessions: &[String]) -> Result<(), String> {
        let mut spawned = false;
        let mut inner = self.inner.lock();
        inner.sessions.retain(|s, c| sessions.contains(s) && c.alive());
        if !self.is_local() {
            if inner.bulk.as_ref().is_some_and(|c| c.alive()) {
                return Ok(());
            }
            let c = Arc::new(self.spawn(HOST_SESSION, false, host_command())?);
            inner.sessions.insert(HOST_SESSION.into(), c.clone());
            inner.bulk = Some(c);
            drop(inner);
            self.generation.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        for s in sessions {
            if !inner.sessions.contains_key(s) {
                let c = self.spawn(s, false, String::new())?;
                inner.sessions.insert(s.clone(), Arc::new(c));
                spawned = true;
            }
        }
        if !inner.bulk.as_ref().is_some_and(|c| c.alive()) {
            inner.bulk = match sessions.first() {
                Some(first) => Some(Arc::new(self.spawn(first, true, String::new())?)),
                None => None,
            };
            spawned = true;
        }
        drop(inner);
        if spawned {
            self.generation.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    /// A host's clients of the user's sessions (AC-89) in line with `up` (name, `$id`): those
    /// not running or ended go (the ended named in the answer), and one more starts when every
    /// other has answered and `due(name)` (its back-off) allows, named with how its start went;
    /// `terminum`'s client stays.
    pub fn host_sessions(self: &Arc<Self>, up: &[(String, String)], due: impl Fn(&str) -> bool) -> (Vec<String>, Option<Started>) {
        let mut inner = self.inner.lock();
        let mut ended = Vec::new();
        inner.sessions.retain(|s, c| {
            if !c.alive() && s != HOST_SESSION {
                ended.push(s.clone());
            }
            s == HOST_SESSION || (c.alive() && up.iter().any(|(n, _)| n == s))
        });
        if !inner.sessions.values().all(|c| c.attached.load(Ordering::Relaxed)) {
            return (ended, None);
        }
        let Some((name, id)) = up.iter().find(|(n, _)| !inner.sessions.contains_key(n) && due(n)) else { return (ended, None) };
        let started = self.spawn(name, false, host_attach_command(id)).map(|c| {
            inner.sessions.insert(name.clone(), Arc::new(c));
        });
        (ended, Some((name.clone(), started)))
    }

    /// A control client for `session`; `bulk`: it takes no pane output; `remote`: a host's command.
    fn spawn(self: &Arc<Self>, session: &str, bulk: bool, remote: String) -> Result<Control, String> {
        let judged = bulk || (!self.is_local() && session == HOST_SESSION);   // the client the server is judged by
        let flags = if bulk { "ignore-size,no-output" } else { "ignore-size" };
        let cmd = match &self.link {
            // the user's own server (AC-89): never started by us (-N), and its sessions keep
            // their environment (-E: no update-environment from terminumd's)
            Link::Local { tmux, socket, .. } if self.is_users() => {
                let mut c = plain(tmux);
                c.args(["-N", "-C", "-u", "-L", socket, "attach-session", "-E", "-f", flags, "-t"]).arg(format!("={session}"));
                c
            }
            Link::Local { tmux, socket, .. } => {
                let mut c = plain(tmux);
                c.args(["-C", "-u", "-L", socket, "attach-session", "-f", flags, "-t"]).arg(format!("={session}"));
                c
            }
            Link::Ssh { ssh, args } => {
                // ConnectTimeout also bounds the banner exchange: a hung sshd ends the link with ssh's own
                // message instead of a wait without end (unless the user's ssh config sets its own)
                let mut c = crate::hosts::ssh_command(ssh, args, &["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=2"]);
                c.args(crate::hosts::recipe::argv(args)).arg(remote);
                c
            }
        };
        let (ntx, nrx) = mpsc::unbounded_channel();
        let (dtx, drx) = oneshot::channel();
        let control = Control::spawn(cmd, ntx, dtx).map_err(|e| format!("cannot start tmux: {e}"))?;
        let me = Arc::downgrade(self);
        tokio::spawn(notices(me.clone(), nrx));
        let session = session.to_string();
        let clean = control.clean.clone();
        tokio::spawn(async move {
            let why = drx.await.unwrap_or_default();
            if let Some(s) = me.upgrade() {
                // the client the server is judged by (the bulk one; a host's `terminum`) says it
                if judged {
                    let by_user = clean.load(Ordering::Relaxed) && s.inner.lock().panes.len() <= 1;
                    s.closed_by_user.store(by_user, Ordering::Relaxed);
                }
                tracing::info!(event = "client.end", server = %s.name, session, why);
                s.dirty.notify_one();
                s.ended.notify_one();
            }
        });
        if !self.is_local() {
            // a slow link must not back up behind output nobody reads (AS-81)
            drop(control.send("refresh-client -f pause-after=2".into()));
        }
        Ok(control)
    }
}

#[path = "notices.rs"]
mod notices;
use notices::notices;
pub use notices::{OnCopy, Wake};

#[cfg(test)]
#[path = "server_tests.rs"]
pub mod tests;
