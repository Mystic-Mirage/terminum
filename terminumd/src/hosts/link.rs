// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The agents link on the files helper's ssh (remote contract "Transport"): with a host's
//! `agents` on, that ssh also carries `-R <host's ~/.terminum/run>/m-<macid>-<rand8>.sock:<the
//! Mac's link socket>` and the helper gets `--link <that path> --mac <macid>`. The helper says
//! `terminumd-agent link: up` or `link: refused` before its ready line; a refusal is retried
//! with -R after 10 min while files keep working.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::app::Shared;

/// A refused forward is tried again after this long.
pub const RETRY: Duration = Duration::from_secs(600);
/// sshd's remote socket path limit kept with a margin (contract: ≤100 bytes).
const REMOTE_MAX: usize = 100;
/// The Mac's socket path limit (sun_path without its NUL).
const LOCAL_MAX: usize = 103;

/// Whether `host` has coding agents' progress enabled (`"agents": "enabled"` in its record).
pub fn wanted(app: &Shared, host: &str) -> bool {
    app.looks.settings.read().hosts.get(host).is_some_and(|r| r["agents"].as_str() == Some("enabled"))
}

/// Applies a host's changed `agents` setting (the CLI and the host menu call it after saving):
/// the helper's ssh restarts with or without the link, or ends when files are off too.
pub fn changed(app: &Shared, host: &str) {
    super::tunnel::stop(app, host);
    super::tunnel::reconcile(app, host);
}

/// This Mac's id on hosts: a random 12-hex id kept in `<app>/mac-id`, made once; an id that
/// cannot be kept is refused (a new one per connection would leave sockets no sweep removes).
pub fn mac_id(dir: &Path) -> Result<String, String> {
    let f = dir.join("mac-id");
    if let Ok(s) = std::fs::read_to_string(&f) {
        let s = s.trim();
        if !s.is_empty() && s.len() <= 16 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            return Ok(s.to_string());
        }
    }
    let id = crate::auth::crypto::random_hex::<6>();
    std::fs::write(&f, &id).map_err(|e| format!("this Mac's id could not be kept in {}: {e}", f.display()))?;
    Ok(id)
}

/// The remote end of the forward: `<home>/.terminum/run/m-<macid>-<rand8>.sock`, or why not.
pub fn remote_path(home: &str, mac: &str, rand8: &str) -> Result<String, String> {
    if !home.starts_with('/') {
        return Err(format!("the host's home {home:?} is not an absolute path"));
    }
    if !home.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b)) {
        return Err(format!("the host's home {home:?} has characters terminum cannot pass (only A-Z a-z 0-9 / . _ -)"));
    }
    let p = format!("{}/.terminum/run/m-{mac}-{rand8}.sock", home.trim_end_matches('/'));
    if p.len() > REMOTE_MAX {
        return Err(format!("the host's link path {p} is longer than {REMOTE_MAX} bytes"));
    }
    Ok(p)
}

/// The Mac's link socket path, checked: absolute (a bare name is read as host:port) and short.
pub fn local_ok(local: &Path) -> Result<(), String> {
    let s = local.to_string_lossy();
    if !local.is_absolute() || s.contains(':') {
        return Err(format!("{s} is not an absolute path without ':'"));
    }
    if s.len() > LOCAL_MAX {
        return Err(format!("{s} is longer than {LOCAL_MAX} bytes"));
    }
    Ok(())
}

/// The ssh options agents add. With agents off the helper's ssh stays exactly as it was
/// (`ExitOnForwardFailure=yes`, the user's ControlPath): only this case changes them.
/// ControlPath=none: a mux master's -R would outlive us under ControlPersist. Forward
/// failure does not end ssh (a user's own RemoteForward may fail): the helper tells.
pub fn ssh_args(link: Option<(&str, &Path)>) -> Vec<String> {
    let Some((remote, local)) = link else { return vec!["-o".into(), "ExitOnForwardFailure=yes".into()] };
    vec!["-o".into(), "ControlPath=none".into(), "-o".into(), "ExitOnForwardFailure=no".into(), "-R".into(), format!("{remote}:{}", local.display())]
}

/// The helper's flags for the link, each value one single-quoted word of the login shell
/// (`remote` is also limited to safe characters by [`remote_path`]).
pub fn helper_args(remote: &str, mac: &str) -> String {
    let sq = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    format!(" --link {} --mac {}", sq(remote), sq(mac))
}

/// What the helper said about the link.
#[derive(Debug, PartialEq, Eq)]
pub enum Said {
    Up,
    Refused,
}

pub fn said(line: &str) -> Option<Said> {
    match line.trim() {
        "terminumd-agent link: up" => Some(Said::Up),
        "terminumd-agent link: refused" => Some(Said::Refused),
        _ => None,
    }
}

/// The agents state the host listing shows when its sshd refused the forward.
pub fn refused_why(host: &str) -> String {
    format!("forwarding refused by {host}'s sshd (AllowTcpForwarding/AllowStreamLocalForwarding/DisableForwarding, or Dropbear)")
}

/// How long after its `refusals`-th refusal in a row -R is asked again: RETRY, then twice as
/// long each time up to 16 times (2 h 40 min), so a host whose sshd keeps refusing does not cut
/// a healthy Files connection every 10 minutes for good.
pub fn backoff(refusals: u32) -> Duration {
    RETRY * (1 << refusals.saturating_sub(1).min(4))
}

/// Whether to ask for -R now: never refused, or refused at least its back-off ago.
pub fn try_forward(refused_at: Option<Instant>, refusals: u32, now: Instant) -> bool {
    refused_at.is_none_or(|t| now.duration_since(t) >= backoff(refusals))
}

/// When a connection whose agents' -R was refused must end so -R is asked again: its back-off
/// after the refusal (AC-85: 10 minutes first), even while Files keeps the connection healthy.
pub fn retry_in(wanted: bool, agents: &str, refused_at: Option<Instant>, refusals: u32, now: Instant) -> Option<Duration> {
    let t = refused_at.filter(|_| wanted && agents.starts_with("refused"))?;
    Some(backoff(refusals).saturating_sub(now.duration_since(t)))
}

/// Waits for `closed` (the ssh child's end) or, with `retry`, that long; true when the retry
/// time came first.
pub async fn closed_or_retry(closed: impl std::future::Future, retry: Option<Duration>) -> bool {
    match retry {
        None => {
            closed.await;
            false
        }
        Some(d) => tokio::select! {
            _ = closed => false,
            () = tokio::time::sleep(d) => true,
        },
    }
}

/// The probe's command: a marker line, then `uname -sm` and $HOME, so rc noise before it
/// cannot be read as the answer.
pub fn probe_command(marker: &str) -> String {
    format!("echo {marker}; uname -sm; printf '%s\\n' \"$HOME\"")
}

/// (system, machine, home) from the lines after the marker.
pub fn parse_probe(out: &str, marker: &str) -> Option<(String, String, String)> {
    let mut lines = out.lines().skip_while(|l| l.trim() != marker).skip(1);
    let mut w = lines.next()?.split_whitespace();
    let (s, m) = (w.next()?.to_string(), w.next()?.to_string());
    let home = lines.next()?.trim_end_matches('\r').to_string();
    Some((s, m, home))
}

/// The agents' link a connection carries: the host's -R path, the Mac's socket, this Mac's id.
pub struct LinkTo {
    pub remote: String,
    pub local: PathBuf,
    pub mac: String,
}

/// The host's agents' link state: "off", "up", "connecting" or "refused: <why>".
pub fn agents_state(app: &Shared, host: &str) -> String {
    if !wanted(app, host) {
        return "off".into();
    }
    let g = app.tunnels.0.lock();
    g.get(host).map(|a| a.agents.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| "connecting".into())
}

/// Sets the link state shown; `refused` starts the -R back-off.
pub(super) fn set_agents(app: &Shared, host: &str, state: &str, refused: bool) {
    let mut g = app.tunnels.0.lock();
    let a = g.entry(host.to_string()).or_default();
    a.agents = state.to_string();
    if refused {
        a.refused_at = Some(Instant::now());
        a.refusals += 1;
    } else if state == "up" {
        a.refusals = 0;
    }
    drop(g);
    app.dirty.notify_one();
}

/// The agents' link for this connection, when agents are on and -R is not in its back-off;
/// a path too long or a listener that cannot start refuses agents with why (files go on).
pub(super) async fn link_for(app: &Shared, host: &str, recipe: &[String]) -> Option<LinkTo> {
    if !wanted(app, host) {
        app.links.stop(app, host);
        return None;
    }
    let (refused_at, refusals, home) = app.tunnels.0.lock().get(host).map(|a| (a.refused_at, a.refusals, a.home.clone())).unwrap_or_default();
    if !try_forward(refused_at, refusals, Instant::now()) {
        return None;
    }
    let home = match home {
        Some(h) => h,
        None => match super::tunnel::probe(recipe).await {
            Ok((_, h)) => {
                app.tunnels.0.lock().entry(host.to_string()).or_default().home = Some(h.clone());
                h
            }
            Err(e) => {
                set_agents(app, host, &format!("refused: {e}"), true);
                return None;
            }
        },
    };
    let made = mac_id(&app.cfg.dir).and_then(|mac| {
        let remote = remote_path(&home, &mac, &crate::auth::crypto::random_hex::<4>())?;
        let local = app.links.start(app, &app.cfg.dir, host)?;
        local_ok(&local).map(|()| LinkTo { remote, local, mac })
    });
    match made {
        Ok(l) => Some(l),
        Err(e) => {
            app.links.stop(app, host);
            set_agents(app, host, &format!("refused: {e}"), true);
            None
        }
    }
}

/// Records what the helper said about the link; a refusal stops the Mac's listener.
pub(super) fn heard(app: &Shared, host: &str, said: Option<&Said>) {
    match said {
        Some(Said::Up) => set_agents(app, host, "up", false),
        Some(Said::Refused) => {
            app.links.stop(app, host);
            set_agents(app, host, &format!("refused: {}", refused_why(host)), true);
        }
        None => {}
    }
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
