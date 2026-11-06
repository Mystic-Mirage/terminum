// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A host's files agent (AC-82–AC-85): terminumd itself in agent mode on the host, reached
//! through `ssh -L <socket on the Mac>:<socket on the host>`, its token on stdin (never a
//! command line); it exits when that ssh ends. Enabling copies the agent the running build
//! carries for the host's platform to ~/.terminum/bin (nothing is copied before the user's
//! click, AC-83); a helper of another build is replaced by this build's (AC-84).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use super::recipe::with_command;
use crate::app::Shared;

/// The agent's folder under the host's $HOME.
const HOME_DIR: &str = ".terminum";
/// The helper's name on hosts set up before the server was named terminumd: its files go
/// when this build's helper is installed or removed.
const LEGACY: &str = "terminusd-agent";
/// What the remote command says when the host has no helper: it is installed (AC-84).
const MISSING: &str = "terminumd-agent missing";
const BACKOFF_MAX: u64 = 30;
/// The page says "helper updated" this long (AC-84).
const UPDATED_NOTE: Duration = Duration::from_secs(60);
/// A failed update is tried again after this long (AC-84 failure).
const UPDATE_RETRY: Duration = Duration::from_secs(600);
/// Each stage's limit (the CLI waits their sum, `host_budget`): the probe, the helper's copy, its
/// "ready" line, its answer through the forward, its removal.
pub(crate) const PROBE_LIMIT: Duration = Duration::from_secs(30);
pub(crate) const COPY_LIMIT: Duration = Duration::from_secs(300);
pub(crate) const READY_LIMIT: Duration = Duration::from_secs(20);
pub(crate) const FORWARD_LIMIT: Duration = Duration::from_secs(5);
pub(crate) const HELPER_REMOVE_LIMIT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(super) struct Agent {
    /// enabling, connecting, updating, up, down; failed (a ticked enabling, AC-83)
    state: String,
    note: String,
    updated_at: Option<Instant>,
    /// agents' link: "" (off), "up", "refused: <why>" (link.rs)
    pub(super) agents: String,
    /// the last refused forward: -R is asked again its back-off later (link.rs `backoff`)
    pub(super) refused_at: Option<Instant>,
    /// refusals in a row since the link was last up
    pub(super) refusals: u32,
    /// the host's $HOME, from the probe
    pub(super) home: Option<String>,
    task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
pub struct Tunnels(pub(super) Mutex<HashMap<String, Agent>>);

fn set(app: &Shared, host: &str, state: &str, note: &str) {
    let mut g = app.tunnels.0.lock();
    let a = g.entry(host.to_string()).or_default();
    a.state = state.to_string();
    a.note = note.to_string();
    drop(g);
    app.dirty.notify_one();
}

/// The agent's state for the panel's offer and `terminum hosts`.
pub fn state(app: &Shared, host: &str) -> String {
    app.tunnels.0.lock().get(host).map_or_else(|| "down".to_string(), |a| a.state.clone())
}

/// The build the helper was just updated to, for 60 s (AC-84).
pub fn updated(app: &Shared, host: &str) -> Option<String> {
    app.tunnels.0.lock().get(host).and_then(|a| a.updated_at).filter(|t| t.elapsed() < UPDATED_NOTE).map(|_| crate::config::build_id().to_string())
}

fn platform(system: &str, machine: &str) -> Option<&'static str> {
    match (system, machine) {
        ("Darwin", "arm64") => Some("macos-aarch64"),
        ("Darwin", "x86_64") => Some("macos-x86_64"),
        ("Linux", "x86_64") => Some("linux-x86_64"),
        ("Linux", "aarch64" | "arm64") => Some("linux-aarch64"),
        _ => None,
    }
}

/// The agent the running build carries for `platform`: `<build>/agents/<platform>/terminumd`,
/// or this very binary when the platform is this Mac's (a build from a checkout).
fn binary_for(p: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let packaged = exe.parent().map(|d| d.join("agents").join(p).join("terminumd"));
    if let Some(b) = packaged.filter(|b| b.is_file()) {
        return Ok(b);
    }
    let mine = platform(if cfg!(target_os = "macos") { "Darwin" } else { "Linux" }, if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" });
    if mine == Some(p) {
        return Ok(exe);
    }
    Err(format!("no helper for {p} in this build"))
}

/// ssh for `recipe`, its options and destination still to add.
fn ssh_cmd(recipe: &[String]) -> Command {
    crate::hosts::ssh_command(crate::hosts::ssh_bin(), recipe, &[])
}


/// Runs `command` on the host; its stdout, or ssh's last words.
async fn ssh(recipe: &[String], command: &str, stdin: Option<Vec<u8>>, timeout: Duration) -> Result<String, String> {
    let mut c = ssh_cmd(recipe);
    with_command(&mut c, recipe, command);
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = c.spawn().map_err(|e| format!("ssh: {e}"))?;
    let mut input = child.stdin.take().expect("piped");
    let feed = tokio::spawn(async move {
        if let Some(b) = stdin {
            let _ = input.write_all(&b).await;
        }
        drop(input);
    });
    let out = super::bounded::output(&mut child, super::bounded::ANSWER_MAX, timeout).await;
    feed.abort();
    let out = out?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr }).trim().lines().last().unwrap_or("").to_string();
        return Err(if err.is_empty() { format!("ssh exited {}", out.status) } else { err });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The host's platform (or why it has no helper) and $HOME; read after a random marker line,
/// so whatever the host's rc files print cannot be taken for the answer.
pub(super) async fn probe(recipe: &[String]) -> Result<(Result<&'static str, String>, String), String> {
    let marker = format!("tm-probe-{}", crate::auth::crypto::random_hex::<6>());
    let out = ssh(recipe, &super::link::probe_command(&marker), None, PROBE_LIMIT).await?;
    let (s, m, home) = super::link::parse_probe(&out, &marker).ok_or("the host did not answer the probe")?;
    Ok((platform(&s, &m).ok_or_else(|| format!("no helper for {s} {m}")), home))
}

/// Copies `binary` to ~/.terminum/bin/terminumd-agent-<id> through a temporary name, points
/// bin/terminumd-agent at it, keeps one older version, checks it answers with `id`.
async fn install(recipe: &[String], binary: &PathBuf, id: &str) -> Result<(), String> {
    if !(id == "dev" || (id.len() == 12 && id.bytes().all(|b| b.is_ascii_hexdigit()))) {
        return Err(format!("not an agent id: {id}"));
    }
    let unique = crate::auth::crypto::random_hex::<12>();
    let script = [
        format!("set -e; d=\"$HOME/{HOME_DIR}\"; mkdir -p \"$d/bin\" \"$d/logs\"; chmod 700 \"$d\""),
        "mkdir -p -m 700 \"$d/run\"; chmod 700 \"$d/run\"".into(),
        format!("tmp=\"$d/bin/.terminumd-agent.{unique}.tmp\"; link=\"$d/bin/.terminumd-agent.{unique}.new\""),
        "trap 'rm -f \"$tmp\" \"$link\"' EXIT; cat > \"$tmp\"; chmod 755 \"$tmp\"".into(),
        format!("got=$(\"$tmp\" --agent-id); [ \"$got\" = \"{id}\" ] || {{ echo \"helper answers $got, expected {id}\" >&2; exit 1; }}"),
        format!("mv -f \"$tmp\" \"$d/bin/terminumd-agent-{id}\"; ln -s \"terminumd-agent-{id}\" \"$link\""),
        "mv -f \"$link\" \"$d/bin/terminumd-agent\"".into(),
        format!("ls -1t \"$d/bin\" | grep '^terminumd-agent-' | grep -v '^terminumd-agent-{id}$' | tail -n +2 \
         | while read -r f; do rm -f \"$d/bin/$f\"; done"),
        format!("rm -f \"$d/bin/{LEGACY}\" \"$d/bin/{LEGACY}-\"* \"$d/bin/.{LEGACY}.\"*"),
        format!("\"$d/bin/terminumd-agent-{id}\" --agent-id"),
    ]
    .join("; ");
    let bytes = tokio::fs::read(binary).await.map_err(|e| format!("{}: {e}", binary.display()))?;
    let out = ssh(recipe, &script, Some(bytes), COPY_LIMIT).await?;
    let got = answer(&out);
    if got != id {
        return Err(format!("the copied helper says {got}, expected {id}"));
    }
    Ok(())
}

/// The helper's answer: the last non-empty line, so rc files' output before it (AS-84 f), a
/// trailing blank line and a `\r` are left out.
fn answer(out: &str) -> &str {
    out.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("")
}

/// One running tunnel: the ssh child and the agent's id.
struct Running {
    child: Child,
    agent_id: String,
    /// the agent lives as long as its stdin is open, and hears from us on it
    stdin: tokio::process::ChildStdin,
    /// what the helper said about the agents' link
    said: Option<super::link::Said>,
}

/// Whether the helper answers an HTTP request through ssh's -L (any status): a host whose sshd
/// refuses the forward closes each connection at once, while ssh's listener on the Mac stays.
async fn answers(mut stream: tokio::net::UnixStream) -> Result<(), String> {
    let ask = async {
        stream.write_all(b"GET / HTTP/1.1\r\nHost: agent\r\nConnection: close\r\n\r\n").await?;
        let mut head = [0u8; 7];
        stream.read_exact(&mut head).await.map(|_| head)
    };
    match tokio::time::timeout(FORWARD_LIMIT, ask).await {
        Ok(Ok(head)) if &head == b"HTTP/1." => Ok(()),
        Ok(Ok(_)) => Err("not an HTTP answer".into()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!("no answer within {} s", FORWARD_LIMIT.as_secs())),
    }
}

/// Whether the helper that said ready is this build's, the only one Files is checked against
/// and given; none (the caller installs it) or another build (the caller replaces it, AC-84) is
/// not: an older helper may not answer through the forward, and must not stop its update.
fn current(agent_id: &str) -> bool {
    agent_id == crate::agent::AGENT_ID
}

/// Starts the agent through ssh and waits for its "ready" line (20 s); with `files`, also that
/// the helper answers through the forward.
async fn connect(app: &Shared, host: &str, recipe: &[String], link: Option<&super::link::LinkTo>, files: bool) -> Result<Running, String> {
    let dir = app.cfg.dir.join("agents");
    crate::config::private_dir(&dir);
    let local = dir.join(format!("{}.sock", &crate::auth::crypto::sha256_hex(format!("{host} {}", crate::auth::crypto::random_hex::<6>()).as_bytes())[..12]));
    let _ = std::fs::remove_file(&local);
    let remote = format!("/tmp/tm-{}/s", crate::auth::crypto::random_hex::<6>());
    let own = crate::agent::AGENT_ID;
    let b = format!("\"$HOME/{HOME_DIR}/bin/terminumd-agent");
    let link_args = link.map_or_else(String::new, |l| super::link::helper_args(&l.remote, &l.mac));
    let command = format!(
        "f={b}-{own}\"; [ -x \"$f\" ] || f={b}\"; [ -x \"$f\" ] || {{ echo {MISSING}; exit 0; }}; \
         TM_LOG=info exec \"$f\" --agent --socket {remote} --log \"$HOME/{HOME_DIR}/logs/agent.log\"{link_args}"
    );
    let mut c = ssh_cmd(recipe);
    // agents on only: files' ssh stays exactly as it was when agents are off
    c.args(super::link::ssh_args(link.map(|l| (l.remote.as_str(), l.local.as_path()))));
    c.args(["-o", "ServerAliveInterval=15", "-L"]).arg(format!("{}:{remote}", local.display()));
    with_command(&mut c, recipe, &command);
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = c.spawn().map_err(|e| format!("ssh: {e}"))?;
    let token = crate::auth::crypto::random_hex::<16>();
    let mut stdin = child.stdin.take().expect("piped");
    stdin.write_all(format!("{token}\n").as_bytes()).await.map_err(|e| e.to_string())?;
    let mut out = BufReader::new(child.stdout.take().expect("piped"));
    let mut said = None;
    let ready = tokio::time::timeout(READY_LIMIT, async {
        for _ in 0..50 {
            match super::bounded::line(&mut out, super::bounded::LINE_MAX).await {
                Ok(Some(l)) if l == MISSING => return Some(String::new()),
                Ok(Some(l)) if super::link::said(&l).is_some() => said = super::link::said(&l),
                Ok(Some(l)) => {
                    let w: Vec<&str> = l.split_whitespace().collect();
                    if w.len() == 6 && w[0] == "terminumd-agent" && w[1] == "ready" {
                        return Some(w[2].to_string());
                    }
                }
                _ => return None,
            }
        }
        None
    }).await.ok().flatten();
    let Some(agent_id) = ready else {
        // ended first, so its stderr reaches its end and the tail read is whole
        let _ = child.kill().await;
        let mut err = Vec::new();
        if let Some(e) = child.stderr.take() {
            let _ = tokio::time::timeout(Duration::from_millis(500), async { err = super::bounded::tail(e, super::bounded::ERR_TAIL).await }).await;
        }
        let err = String::from_utf8_lossy(&err);
        return Err(err.trim().lines().last().map_or_else(|| "the helper did not start".to_string(), str::to_string));
    };
    if !current(&agent_id) {
        return Ok(Running { child, agent_id, stdin, said });
    }
    // the socket appears once ssh listens; only a request answered shows the host forwards it
    let mut forwarded = None;
    for _ in 0..100 {
        if let Ok(stream) = tokio::net::UnixStream::connect(&local).await {
            forwarded = Some(stream);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let Some(stream) = forwarded else {
        let _ = child.kill().await;
        return Err("ssh did not forward the helper's socket within 5 s".into());
    };
    if files && let Err(e) = answers(stream).await {
        let _ = child.kill().await;
        return Err(format!("{}; Files cannot reach the helper ({e})", super::link::refused_why(host)));
    }
    if files && crate::hosts::files_state(app, host).as_deref() == Some("enabled") {
        crate::remote::register(app, host, Some((local, token)));
    }
    Ok(Running { child, agent_id, stdin, said })
}

/// Keeps a host's helper connected while its files or its agents are enabled and its control
/// client is up.
async fn keep(app: Shared, host: String) {
    let mut backoff = 1u64;
    loop {
        let enabled = crate::hosts::files_state(&app, &host).as_deref() == Some("enabled") || super::link::wanted(&app, &host);
        let Some(recipe) = crate::hosts::recipe_of(&app, &host).filter(|_| enabled) else { break };
        set(&app, &host, "connecting", &format!("connecting to {host}…"));
        let link = super::link::link_for(&app, &host, &recipe).await;
        let files = crate::hosts::files_state(&app, &host).as_deref() == Some("enabled");
        let got = connect(&app, &host, &recipe, link.as_ref(), files).await;
        if let Ok(run) = &got {
            super::link::heard(&app, &host, run.said.as_ref());
        }
        let wait = match got {
            Ok(mut run) if !current(&run.agent_id) => {
                let _ = run.child.kill().await;
                crate::remote::register(&app, &host, None);
                set(&app, &host, "updating", &format!("updating the helper on {host}…"));
                match update(&recipe).await {
                    Ok(()) => {
                        tracing::info!(event = "agent.updated", host, to = crate::agent::AGENT_ID);
                        let _ = crate::hosts::set_helper(&app, &host, crate::agent::AGENT_ID);
                        app.tunnels.0.lock().entry(host.clone()).or_default().updated_at = Some(Instant::now());
                        super::agents::after_update(&app, &host);
                        Duration::ZERO
                    }
                    Err(e) => {
                        set(&app, &host, "down", &format!("Could not update the helper on {host}: {e}"));
                        UPDATE_RETRY
                    }
                }
            }
            Ok(mut run) => {
                set(&app, &host, "up", "");
                backoff = 1;
                let retry = {
                    let g = app.tunnels.0.lock();
                    let a = g.get(&host);
                    super::link::retry_in(super::link::wanted(&app, &host), a.map_or("", |a| a.agents.as_str()), a.and_then(|a| a.refused_at), a.map_or(0, |a| a.refusals), Instant::now())
                };
                // -R refused: this connection (only this ssh) ends at the retry time, so a fixed
                // sshd brings agents back without waiting for Files' connection to break
                let again = super::link::closed_or_retry(run.child.wait(), retry, &mut run.stdin).await;
                if again {
                    tracing::info!(event = "agent.link_retry", host);
                    let _ = run.child.kill().await;
                }
                crate::remote::register(&app, &host, None);
                if link.is_some() && app.links.path(&host).is_some() {
                    app.links.stop(&app, &host);
                    super::link::set_agents(&app, &host, "", false);
                }
                if again {
                    Duration::ZERO
                } else {
                    set(&app, &host, "down", &format!("{host}: connection closed"));
                    Duration::from_secs(backoff)
                }
            }
            Err(e) => {
                tracing::warn!(event = "agent.failed", host, why = e);
                set(&app, &host, "down", &e);
                Duration::from_secs(backoff)
            }
        };
        tokio::time::sleep(wait).await;
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
    app.tunnels.0.lock().remove(&host);
    crate::remote::register(&app, &host, None);
    app.links.stop(&app, &host);
}

pub(super) async fn update(recipe: &[String]) -> Result<(), String> {
    let p = probe(recipe).await?.0?;
    install(recipe, &binary_for(p)?, crate::agent::AGENT_ID).await
}

/// Brings a host's helper in line with its recorded files state, once its control client is
/// attached: "enabled" keeps its agent connected; "wanted" (ticked in Add host, AC-83) is set
/// up as Enable does, and a failure clears the tick and Files asks again with the reason.
pub fn reconcile(app: &Shared, host: &str) {
    if app.tmux.host(host).is_none_or(|s| matches!(*s.status.lock(), crate::tmux::server::Status::Off)) {
        stop(app, host);
        return;
    }
    let state = crate::hosts::files_state(app, host);
    let attached = app.tmux.host(host).is_some_and(|s| s.attached());
    let mut g = app.tunnels.0.lock();
    if g.get(host).and_then(|a| a.task.as_ref()).is_some_and(|t| !t.is_finished()) {
        return;
    }
    let (shared, name) = (app.clone(), host.to_string());
    // the task is held under the lock, so `stop` (the host removed) aborts it
    let task = match state.as_deref() {
        Some("enabled") => tokio::spawn(keep(shared, name)),
        _ if super::link::wanted(app, host) => tokio::spawn(keep(shared, name)),
        Some("wanted") if attached => tokio::spawn(async move {
            let Err(e) = enable(&shared, &name).await else { return };
            tracing::warn!(event = "agent.enable_failed", host = name, why = e);
            if crate::hosts::recipe_of(&shared, &name).is_none() {
                return;
            }
            let note = match crate::hosts::set_files(&shared, &name, None) {
                Ok(()) => e,
                Err(w) => format!("{e} (and settings.json not written: {w})"),
            };
            set(&shared, &name, "failed", &note);
        }),
        _ => return,
    };
    g.entry(host.to_string()).or_default().task = Some(task);
}

/// Why the enabling the user ticked failed, until it is tried again.
pub fn failed(app: &Shared, host: &str) -> Option<String> {
    app.tunnels.0.lock().get(host).filter(|a| a.state == "failed").map(|a| a.note.clone())
}

/// Ends the host's agent connection (the host was removed or switched off).
pub fn stop(app: &Shared, host: &str) {
    if let Some(a) = app.tunnels.0.lock().remove(host)
        && let Some(t) = a.task {
            t.abort();
        }
    crate::remote::register(app, host, None);
    app.links.stop(app, host);
}

/// Enable (AC-83): the platform, the copy, a first connection; then "files": "enabled".
pub async fn enable(app: &Shared, host: &str) -> Result<(), String> {
    let recipe = crate::hosts::recipe_of(app, host).ok_or_else(|| format!("{host} is not attached"))?;
    {
        // checked and set at once: the tick and a click may start it together
        let mut g = app.tunnels.0.lock();
        let a = g.entry(host.to_string()).or_default();
        if a.state == "enabling" {
            return Ok(());
        }
        a.state = "enabling".into();
        a.note = format!("setting up {host}…");
    }
    app.dirty.notify_one();
    let result = async {
        let p = probe(&recipe).await?.0?;
        install(&recipe, &binary_for(p)?, crate::agent::AGENT_ID).await?;
        let mut trial = connect(app, host, &recipe, None, true).await?;
        let _ = trial.child.kill().await;
        if !current(&trial.agent_id) {
            return Err(format!("the helper on {host} answers as build {:?}", trial.agent_id));
        }
        crate::remote::register(app, host, None);
        Ok::<(), String>(())
    }.await;
    // the host removed or its recipe changed meanwhile: what was consented to is gone
    let result = result.and_then(|()| match crate::hosts::recipe_of(app, host) {
        Some(r) if r == recipe => Ok(()),
        _ => Err(format!("{host} changed while it was set up")),
    });
    match result {
        Ok(()) => {
            crate::hosts::set_files(app, host, Some("enabled"))?;
            let _ = crate::hosts::set_helper(app, host, crate::agent::AGENT_ID);
            stop(app, host); // an agents-only connection restarts with Files
            reconcile(app, host);
            Ok(())
        }
        Err(e) => {
            let mut g = app.tunnels.0.lock();
            if !g.get(host).and_then(|a| a.task.as_ref()).is_some_and(|t| !t.is_finished()) {
                g.remove(host); // an agents-only connection keeps running
            }
            drop(g);
            app.dirty.notify_one();
            Err(format!("Could not enable {host}: {e}"))
        }
    }
}

/// "Remove helper from <host>" (AC-83): the connection closes, the helper and its log go.
pub async fn remove(app: &Shared, host: &str) -> Result<(), String> {
    stop(app, host);
    let recipe = crate::hosts::recipe_of(app, host).ok_or_else(|| format!("{host} is not attached"))?;
    remove_helper(&recipe).await.map_err(|e| format!("Could not remove the helper from {host}: {e}"))?;
    crate::hosts::set_files(app, host, None)
}

/// The helper and its log taken off a host (Files' remove, agents off with Files off).
pub(super) async fn remove_helper(recipe: &[String]) -> Result<(), String> {
    ssh(recipe, &remove_script(), None, HELPER_REMOVE_LIMIT).await.map(drop)
}

fn remove_script() -> String {
    format!(
        "d=\"$HOME/{HOME_DIR}\"; rm -f \"$d/bin/terminumd-agent\" \"$d/bin/terminumd-agent-\"* \"$d/bin/.terminumd-agent.\"* \
         \"$d/bin/{LEGACY}\" \"$d/bin/{LEGACY}-\"* \"$d/logs/agent.log\"*; \
         rmdir \"$d/bin\" \"$d/logs\" \"$d/run\" \"$d\" 2>/dev/null; true"
    )
}

#[cfg(test)]
#[path = "tunnel_tests.rs"]
mod tests;
