// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Remote hosts (terminum-remote.md): a host is its record in settings.json (`hosts.<name>`:
//! its checked ssh recipe, files enabled or declined, the helper's build), never a name it
//! reports (AC-87). terminumd holds each host's `terminum` session over ssh as a control
//! client, reconnecting after a drop with back-off 1, 2, 4 … 30 s (AC-81); a host's files go
//! through its agent once enabled (tunnel.rs, AC-82–AC-85).

pub mod agents;
pub mod agentsinstall;
pub mod link;
pub mod recipe;
pub mod tunnel;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use tokio::sync::Notify;

use crate::app::Shared;
use crate::tmux::server::{Link, Server, Status};

/// At most this many hosts (terminum-remote.md limits).
const MOST: usize = 32;
const BACKOFF_MAX: u64 = 30;
/// ssh has this long to reach tmux's first answer.
const ATTACH_WAIT: Duration = Duration::from_secs(20);

/// A host's running parts.
struct Run {
    retry: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Default)]
pub struct Hosts {
    runs: Mutex<HashMap<String, Run>>,
    /// a record that cannot be used, by name, and why (AC-87 failure)
    broken: Mutex<HashMap<String, String>>,
    /// an attached host's new recipe, by name: tried over ssh before its link is let go, and
    /// its answer once in (AC-87 edge)
    tried: Mutex<HashMap<String, Tried>>,
}

/// A recipe tried and, once in, its answer.
type Tried = (Vec<String>, Option<Result<(), String>>);

fn records(app: &Shared) -> Map<String, Value> {
    app.looks.settings.read().hosts.clone()
}

pub(crate) fn ssh_bin() -> std::path::PathBuf {
    crate::config::find_program("ssh", "TM_SSH").unwrap_or_else(|| "/usr/bin/ssh".into())
}

/// Recipes whose ssh configuration (~/.ssh/config, read by `ssh -G`) sets a ConnectTimeout of its
/// own (a slow ProxyJump): true; false when it does not.
static CONFIG_TIMEOUT: std::sync::LazyLock<Mutex<HashMap<Vec<String>, bool>>> = std::sync::LazyLock::new(Mutex::default);

/// `-o ConnectTimeout=10` for ssh over `recipe`, so a host that never answers ends its link with
/// ssh's words instead of a wait without end; none when the user's ssh configuration sets its
/// own (an `-o` before the recipe would win over it). Not yet learned ([`learn_timeout`]): ours.
pub(crate) fn connect_timeout(recipe: &[String]) -> &'static [&'static str] {
    match CONFIG_TIMEOUT.lock().get(recipe) {
        Some(true) => &[],
        _ => &["-o", "ConnectTimeout=10"],
    }
}

/// Learns once per recipe, off the async workers, whether its ssh configuration sets a
/// ConnectTimeout (`ssh -G`: the configuration it would use, nothing connected).
pub(crate) async fn learn_timeout(recipe: &[String]) {
    if CONFIG_TIMEOUT.lock().contains_key(recipe) {
        return;
    }
    let args = recipe::argv(recipe);
    let own = tokio::task::spawn_blocking(move || {
        std::process::Command::new(ssh_bin()).arg("-G").args(&args).stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null()).output().ok()
            .map(|o| config_sets_timeout(&String::from_utf8_lossy(&o.stdout)))
    }).await.ok().flatten();
    if let Some(own) = own {
        CONFIG_TIMEOUT.lock().insert(recipe.to_vec(), own);
    }
}

/// Whether `ssh -G`'s output sets a connect timeout ("connecttimeout none" when not).
fn config_sets_timeout(g: &str) -> bool {
    g.lines().filter_map(|l| l.strip_prefix("connecttimeout ")).any(|v| v.trim() != "none")
}

/// Starts, stops or restarts host connections to match settings.json (at start and after
/// every change of the file or a CLI action). An attached host keeps its link while its new
/// recipe is invalid, or until the new one answers over ssh (AC-87 edge).
pub fn sync(app: &Shared) {
    let recs = records(app);
    let mut runs = app.hosts.runs.lock();
    let mut broken = HashMap::new();
    app.hosts.tried.lock().retain(|name, _| recs.contains_key(name));
    for (name, rec) in &recs {
        let args: Vec<String> = rec["ssh"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let checked = recipe::name_ok(name).and_then(|()| recipe::check(&args));
        let off = rec["off"].as_bool().unwrap_or(false);
        let live = app.tmux.host(name).filter(|s| !off && runs.contains_key(name) && matches!(*s.status.lock(), Status::Attached));
        let args = match checked {
            Ok(a) => a,
            Err(e) => {
                match &live {
                    Some(s) => *s.held.lock() = Some(format!("settings.json: {e}; still connected with the previous recipe")),
                    None => { broken.insert(name.clone(), format!("settings.json: {e}")); }
                }
                continue;
            }
        };
        let attach = crate::attach::names_of(rec);
        let same = app.tmux.host(name).filter(|s| matches!(&s.link, Link::Ssh { args: a, .. } if *a == args));
        if let Some(s) = live.as_ref().filter(|_| same.is_none() && !off)
            && let Some(why) = held_for(app, name, &args) {
                *s.held.lock() = Some(why);
                continue;
            }
        app.hosts.tried.lock().remove(name);
        if let Some(s) = &same {
            *s.held.lock() = None;
        }
        if let Some(s) = same.as_ref().filter(|_| runs.contains_key(name) && !off) {
            // only its attached sessions changed: those let go get their windows' size back
            let gone: Vec<String> = s.only.lock().replace(attach.clone()).unwrap_or_default().into_iter().filter(|x| !attach.contains(x)).collect();
            if !gone.is_empty() {
                let (app, s) = (app.clone(), s.clone());
                tokio::spawn(async move {
                    for x in gone {
                        crate::page::sizing::release(&app, &s, Some(&x)).await;
                    }
                });
            }
            continue;
        }
        if let Some(r) = runs.remove(name) {
            r.task.abort();
        }
        tunnel::stop(app, name);
        if let Some(old) = app.tmux.remove_host(name) {
            let_go(app, old);
        }
        let server = Server::new(name, "default", Link::Ssh { ssh: ssh_bin(), args }, app.dirty.clone());
        *server.only.lock() = Some(attach);
        app.tmux.add_host(server.clone());
        if off {
            *server.status.lock() = Status::Off;
            continue;
        }
        let retry = Arc::new(Notify::new());
        let task = tokio::spawn(hold(app.clone(), server, retry.clone()));
        runs.insert(name.clone(), Run { retry, task });
    }
    for name in runs.keys().cloned().collect::<Vec<_>>() {
        if (!recs.contains_key(&name) || broken.contains_key(&name))
            && let Some(r) = runs.remove(&name) {
                r.task.abort();
            }
    }
    for s in app.tmux.hosts() {
        if !recs.contains_key(&s.name) || broken.contains_key(&s.name) {
            app.tmux.remove_host(&s.name);
            tunnel::stop(app, &s.name);
            let_go(app, s);
        }
    }
    *app.hosts.broken.lock() = broken;
    drop(runs);
    app.dirty.notify_one();
}

/// Why an attached host keeps its link for `args`, its new recipe: tried over ssh first (once
/// for the same recipe), switched to once it answered; None: switch now.
fn held_for(app: &Shared, name: &str, args: &[String]) -> Option<String> {
    let mut tried = app.hosts.tried.lock();
    match tried.get(name) {
        Some((a, Some(Ok(())))) if a == args => None,
        Some((a, Some(Err(e)))) if a == args => Some(format!("the new recipe failed: {e}; still connected with the previous one (terminum hosts retry {name})")),
        Some((a, None)) if a == args => Some("trying the new recipe; still connected with the previous one".into()),
        _ => {
            tried.insert(name.to_string(), (args.to_vec(), None));
            tokio::spawn(probe(app.clone(), name.to_string(), args.to_vec()));
            Some("trying the new recipe; still connected with the previous one".into())
        }
    }
}

/// `tmux -V` on the host over `args`; its answer kept for `name` if that is still the recipe
/// tried, then the hosts matched again.
async fn probe(app: Shared, name: String, args: Vec<String>) {
    let answer = agentsinstall::ssh(&args, "tmux -V", "tmux ").await
        .and_then(|out| if out.contains("tmux ") { Ok(()) } else { Err("no tmux on the host".into()) });
    tracing::info!(event = "host.recipe", host = %name, ok = answer.is_ok(), why = answer.as_ref().err().map(String::as_str).unwrap_or(""));
    {
        let mut tried = app.hosts.tried.lock();
        match tried.get_mut(&name) {
            Some((a, got)) if *a == args => *got = Some(answer),
            _ => return,
        }
    }
    sync(&app);
}

/// A host's server replaced, off or removed: the user's windows sized there get their size
/// back while its link still lives (AC-89), then its clients end.
fn let_go(app: &Shared, old: Arc<Server>) {
    let app = app.clone();
    tokio::spawn(async move {
        let sessions = old.only.lock().clone().unwrap_or_default();
        if old.attached() {
            for s in sessions {
                let _ = tokio::time::timeout(Duration::from_secs(2), crate::page::sizing::release(&app, &old, Some(&s))).await;
            }
        }
        old.disconnect();
    });
}

/// Holds a host's control client: connects, waits while it lives, reconnects with back-off.
async fn hold(app: Shared, server: Arc<Server>, retry: Arc<Notify>) {
    if let Link::Ssh { args, .. } = &server.link {
        learn_timeout(args).await;
    }
    let mut backoff = 1u64;
    loop {
        *server.status.lock() = Status::Connecting;
        app.dirty.notify_one();
        let t0 = std::time::Instant::now();
        let attached = match server.sync_clients(&[]) {
            Ok(()) => wait_attached(&server).await,
            Err(e) => Err(e),
        };
        match attached {
            Ok(()) => {
                tracing::info!(event = "host.attached", host = %server.name, ms = t0.elapsed().as_millis() as u64);
                *server.status.lock() = Status::Attached;
                backoff = 1;
                app.dirty.notify_one();
                tunnel::reconcile(&app, &server.name);
                // judged by its `terminum` client alone: the user's session clients (AC-89)
                // end and come back with it
                while server.attached() {
                    tokio::select! {
                        _ = server.ended.notified() => {}
                        _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                    }
                }
                server.disconnect();
                let why = "connection lost".to_string();
                tracing::warn!(event = "host.lost", host = %server.name, retry_s = backoff);
                *server.status.lock() = Status::Reconnecting { why, retry_s: backoff };
                if server.held.lock().is_some() {
                    // it kept this link for a newer record: that record now, not this one again
                    let app = app.clone();
                    tokio::spawn(async move { sync(&app) });
                }
            }
            Err(why) => {
                tracing::warn!(event = "host.failed", host = %server.name, why, retry_s = backoff);
                *server.status.lock() = Status::Failed { why };
                server.disconnect();
            }
        }
        app.dirty.notify_one();
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
            _ = retry.notified() => {}
        }
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// Until tmux's first answer (attached) or the end of ssh (its last words).
async fn wait_attached(server: &Server) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + ATTACH_WAIT;
    loop {
        if server.attached() {
            return Ok(());
        }
        if !server.connected() {
            return Err(server.last_words().unwrap_or_else(|| "ssh ended".into()));
        }
        if tokio::time::Instant::now() > deadline {
            server.disconnect();
            return Err("no answer from tmux on the host within 20 s".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `hosts add <name> '<destination and options>'` or the page's "Add host": an empty name is
/// the destination's host; `files` records the user's tick to enable its files once attached
/// (AC-83). Returns the host's name.
pub async fn add(app: &Shared, name: &str, text: &str, files: bool) -> Result<String, String> {
    let args = recipe::parse(text)?;
    let recs = records(app);
    let name = if name.is_empty() {
        let made = recipe::default_name(&args)?;
        if recs.get(&made).is_some_and(|r| r["ssh"] != json!(args)) {
            return Err(format!("{made} is already a host: give this one a name"));
        }
        made
    } else {
        recipe::name_ok(name)?;
        name.to_string()
    };
    if recs.len() >= MOST && !recs.contains_key(&name) {
        return Err(format!("At most {MOST} hosts"));
    }
    save(app, |hosts| {
        let mut rec = hosts.get(&name).cloned().unwrap_or_else(|| json!({}));
        rec["ssh"] = json!(args);
        if files && rec["files"] != "enabled" {
            rec["files"] = json!("wanted");
        } else if !files && rec["files"] == "wanted" {
            rec["files"] = Value::Null;
        }
        hosts.insert(name.clone(), rec);
    })?;
    if files {
        tunnel::reconcile(app, &name);
    }
    Ok(name)
}

/// Writes settings.json's hosts and applies them at once (the file watcher would within a second).
fn save(app: &Shared, f: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    let path = app.cfg.home.join("settings.json");
    crate::settings::save_hosts(&path, f)?;
    let (s, _) = crate::settings::load(&path);
    *app.looks.settings.write() = s;
    sync(app);
    Ok(())
}

/// Changes host `name`'s record (its attached sessions, AC-89) and applies it.
pub fn update(app: &Shared, name: &str, f: impl FnOnce(&mut Value)) -> Result<(), String> {
    save(app, |h| {
        if let Some(r) = h.get_mut(name) {
            f(r);
        }
    })
}

/// remove, off, on, retry, enable, decline, files-remove (agents-on|off: [`agents::set_agents`]).
pub async fn action(app: &Shared, name: &str, action: &str) -> Result<(), String> {
    if !records(app).contains_key(name) {
        return Err(format!("no host named {name}"));
    }
    match action {
        "remove" => {
            agents::removed(app, name);
            tunnel::stop(app, name);
            save(app, |h| {
                h.remove(name);
            })
        }
        "off" | "on" => save(app, |h| {
            if let Some(r) = h.get_mut(name) {
                r["off"] = json!(action == "off");
            }
        }),
        "retry" => {
            if let Some(r) = app.hosts.runs.lock().get(name) {
                r.retry.notify_one();
            }
            // a new recipe that failed is tried again
            if app.hosts.tried.lock().remove(name).is_some() {
                sync(app);
            }
            Ok(())
        }
        "enable" => tunnel::enable(app, name).await,
        "decline" => set_files(app, name, Some("declined")),
        // agents on keep the helper: only Files goes
        "files-remove" if link::wanted(app, name) => set_files(app, name, None).map(|()| link::changed(app, name)),
        "files-remove" => tunnel::remove(app, name).await,
        _ => Err(format!("unknown host action: {action}")),
    }
}

/// Records a host's files state ("enabled", "declined" or none) and its helper's build.
pub fn set_files(app: &Shared, name: &str, files: Option<&str>) -> Result<(), String> {
    save(app, |h| {
        if let Some(r) = h.get_mut(name) {
            r["files"] = json!(files);
        }
    })
}

pub fn set_helper(app: &Shared, name: &str, helper: &str) -> Result<(), String> {
    save(app, |h| {
        if let Some(r) = h.get_mut(name) {
            r["helper"] = json!(helper);
        }
    })
}

/// A host's files state as recorded.
pub fn files_state(app: &Shared, name: &str) -> Option<String> {
    app.looks.settings.read().hosts.get(name).and_then(|r| r["files"].as_str().map(str::to_string))
}

/// The host's ssh recipe (checked).
pub fn recipe_of(app: &Shared, name: &str) -> Option<Vec<String>> {
    match &app.tmux.host(name)?.link {
        Link::Ssh { args, .. } => Some(args.clone()),
        Link::Local { .. } => None,
    }
}

/// `terminum hosts`: each host's record and state (AC-87).
pub fn list(app: &Shared) -> Vec<Value> {
    let broken = app.hosts.broken.lock().clone();
    records(app).iter().map(|(name, rec)| {
        let (state, why) = match (broken.get(name), app.tmux.host(name)) {
            (Some(e), _) => ("failed".to_string(), e.clone()),
            (None, Some(s)) => match s.status.lock().clone() {
                Status::Attached => ("attached".into(), s.held.lock().clone().unwrap_or_default()),
                Status::Connecting => ("reconnecting".into(), "connecting".into()),
                Status::Reconnecting { why, .. } => ("reconnecting".into(), why),
                Status::Failed { why } => ("failed".into(), why),
                Status::Off => ("off".into(), String::new()),
            },
            (None, None) => ("off".into(), String::new()),
        };
        json!({"name": name, "ssh": rec["ssh"], "state": state, "why": why, "files": rec["files"], "helper": rec["helper"],
               "agent": tunnel::state(app, name), "agents": link::agents_state(app, name)})
    }).collect()
}

/// The Files panel's offer for a host's pane (AC-83): ask, dismissed, enabling, connecting,
/// updating, up, down.
pub fn offer(app: &Shared, name: &str) -> Value {
    let state = match files_state(app, name).as_deref() {
        Some("declined") => "dismissed".to_string(),
        Some("enabled") => tunnel::state(app, name),
        // ticked in Add host: set up once attached; a host not reaching it is asked again
        Some("wanted") if app.tmux.host(name).is_some_and(|s| matches!(*s.status.lock(), Status::Attached | Status::Connecting)) => "enabling".into(),
        _ if tunnel::state(app, name) == "enabling" => "enabling".into(),
        _ => "ask".into(),
    };
    let mut v = json!({"key": name, "name": name, "state": state});
    if let Some(why) = tunnel::failed(app, name).filter(|_| state == "ask") {
        v["why"] = json!(why);
    }
    if let Some(b) = tunnel::updated(app, name) {
        v["updated"] = json!(b);
    }
    v
}

#[cfg(test)]
mod timeout_tests {
    use super::*;

    #[tokio::test]
    async fn our_connect_timeout_only_where_the_ssh_config_sets_none() {
        assert!(!config_sets_timeout("user alex\nconnecttimeout none\nport 22\n"));
        assert!(config_sets_timeout("connecttimeout 30\n"));
        assert!(!config_sets_timeout(""), "no answer: ours");
        let unknown = vec!["-p".to_string(), "2299".into(), "alex@never-learned.example".into()];
        assert_eq!(connect_timeout(&unknown), ["-o", "ConnectTimeout=10"], "not learned yet: ours");
        let slow = vec!["alex@slow-bastion.example".to_string()];
        CONFIG_TIMEOUT.lock().insert(slow.clone(), true);
        assert!(connect_timeout(&slow).is_empty(), "the user's own wins");
        // the real ssh, which reads ~/.ssh/config and connects nowhere
        let plain = vec!["-p".to_string(), "2222".into(), "alex@devbox.example".into()];
        learn_timeout(&plain).await;
        assert_eq!(connect_timeout(&plain).len(), if CONFIG_TIMEOUT.lock().get(&plain) == Some(&true) { 0 } else { 2 });
    }
}
