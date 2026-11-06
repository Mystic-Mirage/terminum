// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A host whose ssh works and whose tmux gives a control client no answer (AC-81): a new binary with
//! the old server still running, a hung server. Unlike a network that is down, each try costs the
//! host a client that stays (orphans.rs), and no number of tries helps: after `TRIES` it is left to
//! the user's Reconnect, with what a plain ssh can tell of why.

use std::process::Stdio;
use std::time::Duration;

use crate::tmux::orphans;
use crate::tmux::server::{Link, Server};

/// Tries in a row before terminumd stops.
pub const TRIES: u32 = 3;
/// How long a plain command over ssh may take.
const WAIT: Duration = Duration::from_secs(10);

/// What a plain ssh found: the client's version, and the running server's when it answers.
#[derive(Debug, PartialEq)]
struct Seen {
    client: String,
    server: Option<Running>,
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

/// The answer to `LOOK`: `<version> <pid> <socket>`, then `list-clients`' lines.
fn running_of(answer: &str) -> Option<Running> {
    let mut lines = answer.lines();
    let mut head = lines.next()?.trim().splitn(3, ' ');
    let (version, pid, socket) = (head.next()?, head.next()?.parse().ok()?, head.next()?);
    Some(Running { version: version.to_string(), pid, socket: socket.to_string(), clients: lines.map(str::to_string).collect() })
}

const LOOK: &str = "tmux display-message -p '#{version} #{pid} #{socket_path}' && tmux list-clients -F '#{client_pid} #{client_control_mode} #{client_flags}'";

/// What the group says.
fn said(host: &str, seen: &Seen) -> String {
    match &seen.server {
        Some(r) if r.version != seen.client => format!("tmux on {host} does not answer: its client is {}, its running server {}. Restart its server, or bring that version back.", seen.client, r.version),
        _ => format!("tmux on {host} does not answer, though ssh does."),
    }
}

/// `remote` over a plain ssh: its stdout, or None when ssh or the command failed.
async fn ask(server: &Server, remote: &str) -> Option<String> {
    let Link::Ssh { ssh, args } = &server.link else { return None };
    let mut c = super::ssh_command(ssh, args, &[]);
    c.args(super::recipe::argv(args)).arg(remote).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = c.spawn().ok()?;
    let out = super::bounded::output(&mut child, super::bounded::ANSWER_MAX, WAIT).await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A client got no answer: None when ssh itself does not work (the network: tries go on), else why
/// tmux does not answer. The orphaned clients the tries left go here, as no control client can.
pub async fn look(server: &Server) -> Option<String> {
    let client = client_of(&ask(server, "tmux -V").await?)?;
    let running = ask(server, LOOK).await.as_deref().and_then(running_of);
    if let Some(r) = &running {
        let opt = orphans::answer_option();
        let line = format!("{}; tmux show-options -gqv {opt}; tmux set-option -gu {opt}", orphans::script(r.pid, &r.socket, &orphans::listed(&r.clients), &opt));
        match ask(server, &line).await {
            Some(ended) => tracing::info!(event = "clients.orphans", server = %server.id(), ended = ended.trim()),
            None => tracing::warn!(event = "clients.orphans", server = %server.id(), error = "no answer over ssh"),
        }
    }
    Some(said(&server.name, &Seen { client, server: running }))
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
        let running = |v: &str| Some(Running { version: v.into(), pid: 1, socket: "/s".into(), clients: Vec::new() });
        let differ = said("devbox", &Seen { client: "3.8".into(), server: running("3.4") });
        assert!(differ.contains("devbox") && differ.contains("client is 3.8") && differ.contains("running server 3.4") && differ.contains("Restart its server"), "{differ}");
        assert_eq!(said("devbox", &Seen { client: "3.8".into(), server: running("3.8") }), "tmux on devbox does not answer, though ssh does.");
        assert_eq!(said("devbox", &Seen { client: "3.8".into(), server: None }), "tmux on devbox does not answer, though ssh does.", "a hung server tells nothing");
    }
}
