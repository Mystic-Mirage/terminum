// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Sessions of the user's own tmux attached by name (AC-89). On this Mac: settings.json
//! `attach: {"<socket>": ["work", …]}` ("default" is plain `tmux`), one `Server` per socket,
//! holding a control client for each named session that runs. On a host: `hosts.<name>.attach`,
//! sessions of the tmux its `terminum` session is on, each over one more ssh connection,
//! started one at a time with back-off. The list shows only those, and a named session that
//! does not run as a group that says so, attached once it appears. terminumd never kills a
//! session there, and makes one only when the whole server is gone (a reboot, the server
//! killed): then the attached sessions are made again from their checkpoint (AC-15,
//! restore_srv.rs); detaching only forgets the name. A place is named by its server's id:
//! `local:<socket>` or `<host>:default`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::app::Shared;
use crate::tmux::server::{Link, Server, Status, HOST_SESSION};
use crate::tmux::{args, layout};

/// At most this many sessions attached, here and on hosts (as many as hosts, terminum-remote.md limits).
const MOST: usize = 32;
/// A host's session client that ended is started again after 1, 2, 4 … this many seconds.
const BACKOFF_MAX: u64 = 30;

/// Where sessions are attached: a server of the user's own on this Mac or a host's tmux.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Local(String),
    Host(String),
}

impl Place {
    /// From a server id (`local:<socket>`, `<host>:default`); "" is the user's plain `tmux`.
    pub fn parse(app: &Shared, id: &str) -> Result<Place, String> {
        let id = id.trim();
        match id.split_once(':') {
            None if id.is_empty() => Ok(Place::Local("default".into())),
            Some(("local", socket)) if crate::settings::is_name(socket) => Ok(Place::Local(socket.into())),
            Some((host, "default")) if app.looks.settings.read().hosts.contains_key(host) => Ok(Place::Host(host.into())),
            Some((host, "default")) => Err(format!("no host named {host}")),
            _ => Err(format!("{id} is not a tmux server (local:<socket> or <host>:default)")),
        }
    }

    pub fn id(&self) -> String {
        match self {
            Place::Local(socket) => format!("local:{socket}"),
            Place::Host(host) => format!("{host}:default"),
        }
    }

    fn server(&self, app: &Shared) -> Option<Arc<Server>> {
        match self {
            Place::Local(socket) => app.tmux.attached(socket),
            Place::Host(host) => app.tmux.host(host),
        }
    }

    /// The sessions attached here, as settings.json has them.
    fn names(&self, app: &Shared) -> Vec<String> {
        let st = app.looks.settings.read();
        match self {
            Place::Local(socket) => st.attach.get(socket).cloned().unwrap_or_default(),
            Place::Host(host) => st.hosts.get(host).map(names_of).unwrap_or_default(),
        }
    }
}

/// A host record's attached sessions.
pub fn names_of(rec: &Value) -> Vec<String> {
    rec["attach"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The socket file tmux uses for `-L <socket>`: `$TMUX_TMPDIR` (else /tmp)/tmux-<uid>/<socket>.
pub fn socket_path(socket: &str) -> PathBuf {
    let base = std::env::var_os("TMUX_TMPDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    // SAFETY: getuid has no preconditions
    base.join(format!("tmux-{}", unsafe { libc::getuid() })).join(socket)
}


/// Makes and forgets attached servers to match settings.json (at start and after a change).
pub fn sync(app: &Shared) {
    let wanted = app.looks.settings.read().attach.clone();
    for server in app.tmux.attached_all() {
        if !wanted.contains_key(&server.socket)
            && let Some(old) = app.tmux.remove_attached(&server.socket) {
                let app = app.clone();
                tokio::spawn(async move {
                    crate::page::sizing::release(&app, &old, None).await;
                    old.disconnect();
                });
            }
    }
    let Link::Local { tmux, .. } = &app.tmux.local.link else { return };
    for (socket, sessions) in wanted {
        // made with its sessions: never for a moment a server whose every session is listed
        let server = app.tmux.attached_or(&socket, || {
            let s = Server::new("local", &socket, Link::Local { tmux: tmux.clone(), socket: socket.clone(), conf: "/dev/null".into() }, app.changed.clone());
            *s.only.lock() = Some(sessions.clone());
            s
        });
        *server.only.lock() = Some(sessions);
    }
    app.dirty.notify_one();
}

/// Brings `server`'s control clients in line with the named sessions that run; the groups of
/// those that do not, and whether that is sure (no error: their panes may count as gone).
pub async fn ensure(app: &Shared, server: &Arc<Server>) -> (Vec<Value>, bool) {
    let only = server.only.lock().clone().unwrap_or_default();
    let mut running = if server.connected() {
        server.run("list-sessions -F '#{session_name}'").await.map(Some)
    } else {
        server.local_sessions().await
    };
    if matches!(running, Ok(None)) && !only.is_empty() {
        // the whole server is gone: its attached sessions are made again (once), then read
        crate::restore_srv::users_local(app, server).await;
        running = server.local_sessions().await;
    }
    if running.is_err() && server.connected() {
        return (Vec::new(), false);              // a timeout on a client alive: nothing said of its sessions (AC-132)
    }
    let sure = running.is_ok();
    let (running, why) = match running {
        Ok(Some(list)) => (list, None),
        Ok(None) => (Vec::new(), Some(format!("{} is not running ({})", server.label(), socket_path(&server.socket).display()))),
        Err(e) => (Vec::new(), Some(e)),
    };
    let up: Vec<String> = only.iter().filter(|s| running.contains(s)).cloned().collect();
    if up.is_empty() {
        server.disconnect();
    } else if let Err(e) = server.sync_clients(&up) {
        tracing::warn!(event = "attach.error", socket = %server.socket, error = %e);
    }
    *server.status.lock() = if up.is_empty() { Status::Off } else { Status::Attached };
    let why = why.unwrap_or_else(|| format!("not running on {}: attached when it starts", server.label()));
    (only.iter().filter(|s| !up.contains(s)).map(|s| layout::absent_group(server, s, &why)).collect(), sure)
}

/// A host's attached sessions (AC-89) after its list read: clients for those that run (from
/// `seen`: every session on the host, name and `$id`), one more at a time with back-off; the
/// groups of those that do not, and how many other sessions run there (the host's offer).
pub fn ensure_host(server: &Arc<Server>, seen: &[(String, String)]) -> (Vec<Value>, usize) {
    let only = server.only.lock().clone().unwrap_or_default();
    let up: Vec<(String, String)> = seen.iter().filter(|(n, _)| only.contains(n)).cloned().collect();
    let now = Instant::now();
    let (ended, started) = server.host_sessions(&up, |n| server.tries.lock().get(n).is_none_or(|t| now >= t.1));
    let mut tries = server.tries.lock();
    tries.retain(|n, _| only.contains(n));
    let failed = started.as_ref().filter(|(_, r)| r.is_err()).map(|(n, _)| n.clone());
    for n in ended.into_iter().chain(failed).filter(|n| up.iter().any(|(u, _)| u == n)) {
        let (delay, _, since) = tries.get(&n).copied().unwrap_or((0, now, now));
        // a client that held for a while starts its back-off anew
        let delay = if since.elapsed() > Duration::from_secs(BACKOFF_MAX) || delay == 0 { 1 } else { (delay * 2).min(BACKOFF_MAX) };
        tries.insert(n.clone(), (delay, now + Duration::from_secs(delay), since));
        tracing::warn!(event = "attach.ended", host = %server.name, session = n, retry_s = delay);
    }
    if let Some((n, Ok(()))) = started {
        let t = tries.entry(n).or_insert((0, now, now));
        t.2 = now;
    }
    let why = format!("not running on {}'s tmux: attached when it starts", server.name);
    let absent = only.iter().filter(|s| !up.iter().any(|(n, _)| n == *s)).map(|s| layout::absent_group(server, s, &why)).collect();
    let others = seen.iter().filter(|(n, _)| n != HOST_SESSION && !only.contains(n)).count();
    (absent, others)
}

/// A host's attached sessions while the host is not attached: they show with its state.
pub fn host_waiting(server: &Server, why: &str) -> Vec<Value> {
    let only = server.only.lock().clone().unwrap_or_default();   // released first: a group's fields read it again
    only.iter().map(|s| layout::absent_group(server, s, why)).collect()
}

/// Attaches `sessions` at `place` ("Attach tmux sessions", `terminum tmux attach`): all of them or none.
pub fn add(app: &Shared, place: &Place, sessions: &[String]) -> Result<(), String> {
    let mut names = Vec::new();
    for s in sessions {
        let s = args::check_session_name(s.trim())?;
        if !names.contains(&s) {
            names.push(s);
        }
    }
    if names.is_empty() {
        return Err("Name a session, or tick one".into());
    }
    let have = place.names(app);
    let twin = same_tmux(app, place);
    for s in &names {
        if let Some(other) = twin.iter().find(|o| o.shows(s)) {
            return Err(format!("{s} is listed already from {}: the same tmux", other.label()));
        }
        match place {
            Place::Local(socket) if *socket == app.tmux.local.socket => return Err(format!("{s} is a session of terminum's own tmux: it is in the list already")),
            Place::Host(h) if s == HOST_SESSION => return Err(format!("{HOST_SESSION} is terminum's own session on {h}: it is in the list already")),
            _ if have.contains(s) => return Err(format!("{s} is attached already")),
            _ => {}
        }
    }
    if total(app) + names.len() > MOST {
        return Err(format!("At most {MOST} sessions can be attached."));
    }
    save(app, place, |l| l.extend(names.iter().map(|n| json!(n))))
}

/// The other servers that are the tmux of `place` reached another way (a host that is this Mac,
/// two hosts that are one machine), as far as their identities are known.
fn same_tmux(app: &Shared, place: &Place) -> Vec<Arc<Server>> {
    let Some(me) = place.server(app) else { return Vec::new() };
    let Some(id) = me.identity_known() else { return Vec::new() };
    app.tmux.all().into_iter().filter(|s| s.id() != me.id() && s.identity_known().as_deref() == Some(id.as_str())).collect()
}

/// Forgets `session` at `place`; the session itself goes on, its windows at tmux's own size again.
pub async fn detach(app: &Shared, place: &Place, session: &str) -> Result<(), String> {
    if !place.names(app).iter().any(|s| s == session) {
        return Err(format!("{session} is not attached"));
    }
    if let Some(server) = place.server(app) {
        crate::page::sizing::release(app, &server, Some(session)).await;
    }
    save(app, place, |l| l.retain(|s| s.as_str() != Some(session)))
}

/// A session running at `place` that is not attached, for the form's choices; `ends`: tmux
/// destroys it once its last client goes (`destroy-unattached`), so once terminum lets go of it
/// (detach, terminum stops, a host's link drops) when the user's own terminal left it before.
#[derive(Debug, PartialEq)]
pub struct Running {
    pub name: String,
    pub ends: bool,
}

const LISTED: &str = "#{session_name}\t#{destroy-unattached}";

/// `list-sessions -F LISTED` lines: a name and whether its `destroy-unattached` is on (any value
/// but off: on, keep-last, keep-group).
fn listed(lines: &[String]) -> Vec<Running> {
    lines.iter().filter_map(|l| {
        let (name, ends) = l.rsplit_once('\t')?;
        (!name.is_empty()).then(|| Running { name: name.to_string(), ends: !matches!(ends.trim(), "" | "off" | "0") })
    }).collect()
}

/// The sessions running at `place` that are not attached (none when it is down).
pub async fn running(app: &Shared, place: &Place) -> Vec<Running> {
    let have = place.names(app);
    let lines = match place {
        Place::Local(socket) => {
            let Link::Local { tmux, .. } = &app.tmux.local.link else { return Vec::new() };
            if *socket == app.tmux.local.socket {
                return Vec::new();
            }
            let out = crate::tmux::server::plain(tmux).args(["-L", socket, "list-sessions", "-F", LISTED]).output().await;
            match out {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect(),
                _ => Vec::new(),
            }
        }
        Place::Host(host) => match app.tmux.host(host).filter(|s| s.attached()) {
            Some(s) => s.run(format!("list-sessions -F '{LISTED}'")).await.unwrap_or_default(),
            None => Vec::new(),
        },
    };
    let twin = same_tmux(app, place);
    listed(&lines).into_iter().filter(|r| r.name != HOST_SESSION || matches!(place, Place::Local(_)))
        .filter(|r| !have.contains(&r.name) && !twin.iter().any(|o| o.shows(&r.name))).collect()
}

/// Every attached session: (place, session), this Mac's first.
pub fn every(app: &Shared) -> Vec<(Place, String)> {
    let st = app.looks.settings.read();
    let local = st.attach.iter().flat_map(|(socket, l)| l.iter().map(|s| (Place::Local(socket.clone()), s.clone())));
    let hosts = st.hosts.iter().flat_map(|(h, rec)| names_of(rec).into_iter().map(|s| (Place::Host(h.clone()), s)));
    local.chain(hosts).collect()
}

fn total(app: &Shared) -> usize {
    every(app).len()
}

/// Changes the list at `place` in settings.json and applies it at once (the watcher would
/// within a second).
fn save(app: &Shared, place: &Place, f: impl FnOnce(&mut Vec<Value>)) -> Result<(), String> {
    match place {
        Place::Local(socket) => {
            let path = app.cfg.home.join("settings.json");
            crate::settings::save_object(&path, "attach", |a| {
                let list = a.entry(socket.clone()).or_insert_with(|| json!([]));
                if let Some(l) = list.as_array_mut() {
                    f(l);
                    if l.is_empty() {
                        a.remove(socket);
                    }
                }
            })?;
            let (s, _) = crate::settings::load(&path);
            *app.looks.settings.write() = s;
            sync(app);
            Ok(())
        }
        Place::Host(host) => crate::hosts::update(app, host, |rec| {
            let mut l = rec["attach"].as_array().cloned().unwrap_or_default();
            f(&mut l);
            if let Some(o) = rec.as_object_mut() {
                if l.is_empty() { o.remove("attach") } else { o.insert("attach".into(), json!(l)) };
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-89: a session tmux destroys once its last client goes is told apart in the choices.
    #[test]
    fn listed_sessions_say_whether_they_end_with_their_last_client() {
        let lines: Vec<String> = ["work\toff", "scratch\ton", "keep\tkeep-last", "odd\tname\toff", "\toff", "old"].iter().map(|s| s.to_string()).collect();
        let got: Vec<(String, bool)> = listed(&lines).into_iter().map(|r| (r.name, r.ends)).collect();
        assert_eq!(got, vec![("work".into(), false), ("scratch".into(), true), ("keep".into(), true), ("odd\tname".into(), false)]);
    }
}
