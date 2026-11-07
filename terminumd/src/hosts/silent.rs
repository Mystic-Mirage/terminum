// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A host whose ssh works and whose tmux gives a control client no answer (AC-81): a new binary with
//! the old server still running, a hung server. Unlike a network that is down, each try costs the
//! host a client that stays (orphans.rs), and no number of tries helps: after `TRIES` it is left to
//! the user's Reconnect, with what a plain ssh can tell of why.

use std::time::Duration;

use crate::tmux::orphans;
use crate::tmux::server::{Link, Server};

/// Tries in a row before terminumd stops.
pub const TRIES: u32 = 3;
/// How long a plain command over ssh may take.
const WAIT: Duration = Duration::from_secs(10);

/// What a plain ssh found of a silent tmux: why, for the group, and its server when it answers.
pub struct Found {
    pub why: String,
    running: Option<Running>,
}

#[derive(Debug, PartialEq)]
struct Running {
    version: String,
    pid: u32,
    socket: String,
    clients: Vec<String>,
}

/// `tmux -V`'s line: `tmux 3.8`.
fn client_of(answer: &str) -> Option<String> {
    answer.lines().next()?.trim().strip_prefix("tmux ").map(str::to_string)
}

/// `<version> <pid> <socket>`, then `list-clients`' lines.
fn running_of(answer: &str) -> Option<Running> {
    let mut lines = answer.lines();
    let mut head = lines.next()?.trim().splitn(3, ' ');
    let (version, pid, socket) = (head.next()?, head.next()?.parse().ok()?, head.next()?);
    Some(Running { version: version.to_string(), pid, socket: socket.to_string(), clients: lines.map(str::to_string).collect() })
}

/// What the group says.
fn said(host: &str, client: &str, server: Option<&str>) -> String {
    match server {
        Some(s) if s != client => format!("tmux on {host} does not answer: its client is {client}, its running server {s}. Restart its server, or bring that version back."),
        _ => format!("tmux on {host} does not answer, though ssh does."),
    }
}

/// `remote` over a plain ssh: its stdout, or None when ssh or the command failed.
async fn ask(server: &Server, remote: &str) -> Option<String> {
    let Link::Ssh { args, .. } = &server.link else { return None };
    super::tunnel::ssh(args, &[], remote, None, WAIT).await.ok()
}

/// A client got no answer: None when ssh itself does not work (the network: tries go on). Two
/// commands: a hung server holds up the second, and the first still tells ssh works.
pub async fn look(server: &Server) -> Option<Found> {
    let client = client_of(&ask(server, "tmux -V").await?)?;
    let line = format!("tmux display-message -p '#{{version}} {}' && tmux list-clients -F '{}'", orphans::SERVER_FORMAT, orphans::CLIENTS_FORMAT);
    let running = ask(server, &line).await.as_deref().and_then(running_of);
    let why = said(&server.name, &client, running.as_ref().map(|r| r.version.as_str()));
    Some(Found { why, running })
}

/// The orphaned clients the tries left go, as no control client can end them here.
pub async fn sweep(server: &Server, found: &Found) {
    let Some(r) = &found.running else { return };
    match ask(server, &orphans::over_ssh(r.pid, &r.socket, &r.clients)).await.as_deref().and_then(orphans::ended) {
        Some(ended) => tracing::info!(event = "clients.orphans", server = %server.id(), ended = ?ended),
        None => tracing::warn!(event = "clients.orphans", server = %server.id(), error = "no answer over ssh"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_versions_are_read_from_plain_commands() {
        assert_eq!(client_of("tmux 3.8\n").as_deref(), Some("3.8"));
        assert_eq!(client_of("tmux next-3.9\n").as_deref(), Some("next-3.9"));
        assert_eq!(client_of("bash: tmux: command not found\n"), None, "no tmux: not this case");
        assert_eq!(running_of("3.4 4154211 /tmp/tmux-1000/default\n123 1 attached,control-mode\n"), Some(Running {
            version: "3.4".into(), pid: 4154211, socket: "/tmp/tmux-1000/default".into(), clients: vec!["123 1 attached,control-mode".into()],
        }));
        assert_eq!(running_of("3.4 x /tmp/s\n"), None, "no pid: nothing to sweep by");
        assert_eq!(running_of(""), None);
    }

    #[test]
    fn it_says_the_versions_when_they_differ() {
        let differ = said("devbox", "3.8", Some("3.4"));
        assert!(differ.contains("devbox") && differ.contains("client is 3.8") && differ.contains("running server 3.4") && differ.contains("Restart its server"), "{differ}");
        assert_eq!(said("devbox", "3.8", Some("3.8")), "tmux on devbox does not answer, though ssh does.");
        assert_eq!(said("devbox", "3.8", None), "tmux on devbox does not answer, though ssh does.", "a hung server tells nothing");
    }
}
