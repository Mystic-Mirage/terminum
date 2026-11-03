// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The commands that start tmux: a local one (`plain`) and a host's control clients, run by
//! the host's shell over ssh (AC-81, AC-89).

use tokio::process::Command;

use super::server::HOST_SESSION;

/// The remote command of a host's control client, quoted for the host's shell (AC-81):
/// the session `terminum`, created when missing; `-A` without `-D` leaves other clients.
pub fn host_command() -> String {
    format!("tmux -u -C new-session -A -f ignore-size -s {HOST_SESSION}")
}

/// The remote command of a client of the user's session `$N` on a host (AC-89): by id, so no
/// name reaches the host's shell; `-E`: the session's environment is not updated from ssh's.
pub fn host_attach_command(id: &str) -> String {
    let id: String = id.chars().filter(|c| *c == '$' || c.is_ascii_digit()).collect();
    format!("tmux -u -C attach-session -E -f ignore-size -t '{id}'")
}

/// A tmux command with a UTF-8 locale (launchd gives none: tmux would draw `_` for every
/// wide character), without the TMUX of a terminal terminumd may run in (tests), and with the
/// umask terminumd was started with: a server it starts passes it to every terminal (AC-09).
pub fn plain(tmux: &std::path::Path) -> Command {
    let mut c = Command::new(tmux);
    c.env_remove("TMUX").env_remove("TMUX_PANE");
    // SAFETY: umask::restore is async-signal-safe
    unsafe {
        c.pre_exec(|| {
            crate::umask::restore();
            Ok(())
        });
    }
    let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"].iter().any(|k| std::env::var(k).is_ok_and(|v| v.to_uppercase().contains("UTF-8") || v.to_uppercase().contains("UTF8")));
    if !utf8 {
        c.env("LANG", "en_US.UTF-8");
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-89: a host's session client attaches by id, never a name the host's shell would read.
    #[test]
    fn a_hosts_session_is_attached_by_its_id() {
        assert_eq!(host_attach_command("$12"), "tmux -u -C attach-session -E -f ignore-size -t '$12'");
        assert_eq!(host_attach_command("$1'; rm -rf ~"), "tmux -u -C attach-session -E -f ignore-size -t '$1'");
    }

    #[test]
    fn the_host_command_creates_or_attaches_terminum() {
        assert_eq!(host_command(), "tmux -u -C new-session -A -f ignore-size -s terminum");
    }
}
