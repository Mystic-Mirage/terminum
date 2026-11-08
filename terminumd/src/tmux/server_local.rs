// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The local tmux server asked by processes of their own: no control client runs yet or any more:
//! its first start (AC-01), the sessions the user's tmux offers to attach (AC-89).

use super::{plain, Link, Server, HOST_SESSION, NO_TMUX};

impl Server {
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
}
