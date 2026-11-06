// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Agent mode (AC-82): terminumd on a remote host, reached by the Mac's terminumd through a
//! Unix socket that ssh forwards. The Mac starts it as `ssh <recipe> -- <dest>
//! terminumd-agent --agent --socket <path>` and writes a fresh token as the first line of its
//! stdin (never a command line); the agent lives exactly as long as that ssh connection: it
//! exits when stdin closes. With `--link <sock> --mac <macid>` it also points that Mac's
//! link in `~/.terminum/run` at the forwarded hook socket before it says ready (runlink.rs).

use std::io::{BufRead, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tokio::net::UnixListener;

/// Which terminumd sources this binary is (a hash of terminumd/src and its manifests, set by
/// the build): the Mac's terminumd and an agent work together when their ids are equal.
pub const AGENT_ID: &str = match option_env!("TM_AGENT_ID") {
    Some(a) => a,
    None => "dev",
};
/// The Host header requests to an agent carry (they come through the Mac's terminumd).
pub const HOST: &str = "terminumd-agent";

/// The agent's private folder (socket, scratch state); removed on any exit.
static DIR: OnceLock<PathBuf> = OnceLock::new();
/// `--link <sock> --mac <macid>`: the Mac's forwarded hook socket in `~/.terminum/run` and
/// that Mac's id; the socket and its link go on any exit (runlink.rs).
static LINK: OnceLock<(PathBuf, String)> = OnceLock::new();

pub struct Setup {
    pub dir: PathBuf,
    pub token: String,
    pub listener: UnixListener,
}

/// `--agent --socket <path>`: the socket path, or None when not in agent mode.
pub fn socket_arg() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|a| a == "--agent") {
        return None;
    }
    match args.iter().position(|a| a == "--socket").and_then(|i| args.get(i + 1)) {
        Some(p) => Some(PathBuf::from(p)),
        None => fail("--agent needs --socket <path>"),
    }
}

/// `--link <sock> --mac <macid>`, both or neither.
fn link_args() -> Option<(PathBuf, String)> {
    let args: Vec<String> = std::env::args().collect();
    let val = |k: &str| args.iter().position(|a| a == k).map(|i| args.get(i + 1).cloned().unwrap_or_default());
    match (val("--link"), val("--mac")) {
        (None, None) => None,
        (Some(l), Some(m)) if !l.is_empty() && !m.is_empty() => Some((PathBuf::from(l), m)),
        _ => fail("--link <path> and --mac <macid> go together"),
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("terminumd-agent: {msg}");
    std::process::exit(2);
}

/// Read the token, make the private folder, bind the socket, say "ready" on stdout.
pub fn start(socket: &Path) -> Setup {
    let mut token = String::new();
    std::io::stdin().lock().read_line(&mut token).unwrap_or_else(|e| fail(&format!("no token on stdin: {e}")));
    let token = token.trim().to_string();
    if token.len() < 16 {
        fail("the token on stdin is too short");
    }
    if let Some((sock, mac)) = link_args() {
        // a forward sshd refused (no socket) or a run folder failing its checks: the link is
        // refused and the helper goes on for Files; the Mac retries later (remote contract)
        match crate::runlink::setup(&crate::runlink::dir(), &mac, &sock) {
            Ok(()) => {
                LINK.set((sock, mac)).ok();
                println!("terminumd-agent link: up");
            }
            Err(e) => {
                eprintln!("terminumd-agent: link: {e}");
                println!("terminumd-agent link: refused");
            }
        }
    }
    let dir = socket.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| fail("the socket needs a folder"));
    private_dir(dir).unwrap_or_else(|e| fail(&format!("{}: {e}", dir.display())));
    DIR.set(dir.to_path_buf()).ok();
    let std_listener = std::os::unix::net::UnixListener::bind(socket).unwrap_or_else(|e| fail(&format!("{}: {e}", socket.display())));
    let _ = std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600));
    std_listener.set_nonblocking(true).unwrap_or_else(|e| fail(&e.to_string()));
    let listener = UnixListener::from_std(std_listener).unwrap_or_else(|e| fail(&e.to_string()));
    println!("terminumd-agent ready {AGENT_ID} {} {} {}-{}", hostname(), user(), std::env::consts::OS, std::env::consts::ARCH);
    let _ = std::io::stdout().flush();
    quiet();
    Setup { dir: dir.to_path_buf(), token, listener }
}

/// After "ready" nobody reads stdout or stderr, and ssh closes them when the Mac goes away
/// (a log line written then would fail and panic the exit path, leaving the agent running):
/// from here on they go to `--log <file>` (AC-83: ~/.terminum/logs/agent.log on
/// the host; at 1 MB the file becomes <file>.1), else to /dev/null.
fn quiet() {
    let args: Vec<String> = std::env::args().collect();
    let log = args.iter().position(|a| a == "--log").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let file = log.and_then(|p| {
        if std::fs::metadata(&p).is_ok_and(|m| m.len() > 1 << 20) {
            let _ = std::fs::rename(&p, p.with_extension("log.1"));
        }
        std::fs::OpenOptions::new().create(true).append(true).open(&p).ok()
    });
    let Some(out) = file.or_else(|| std::fs::OpenOptions::new().write(true).open("/dev/null").ok()) else { return };
    use std::os::fd::AsRawFd;
    // SAFETY: dup2 onto the standard descriptors with a valid open descriptor
    unsafe {
        libc::dup2(out.as_raw_fd(), 1);
        libc::dup2(out.as_raw_fd(), 2);
    }
}

/// A new folder only this user can enter. It must not exist yet: the agent removes it on
/// exit, so an existing folder (a home, say) is never taken over.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new().mode(0o700).create(dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => std::io::Error::other("exists: the agent needs a new folder for its socket"),
        _ => e,
    })
}

/// Exit when stdin closes: the ssh connection that carries this agent is gone. Each line on it
/// is the Mac's terminumd saying it is still there (`hosts::link::keep_alive`).
pub fn exit_with_stdin(heard: impl Fn() + Send + 'static, exit: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        read_until_closed(std::io::stdin().lock(), heard);
        exit();
    });
}

fn read_until_closed(mut input: impl BufRead, heard: impl Fn()) {
    let mut sink = String::new();
    while input.read_line(&mut sink).map(|n| n > 0).unwrap_or(false) {
        sink.clear();
        heard();
    }
}

/// The Mac's terminumd sends a line every 15 s for as long as it runs this agent's ssh, and an
/// event stream while its Files are on; neither for 90 s means the connection died without
/// closing stdin (sshd may not notice for hours).
pub fn exit_when_unused(used: impl Fn() -> bool + Send + 'static, exit: impl Fn() + Send + 'static) {
    watch_use(std::time::Duration::from_secs(15), 6, used, exit);
}

fn watch_use(tick: std::time::Duration, ticks_to_exit: u32, used: impl Fn() -> bool + Send + 'static, exit: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let mut idle = 0;
        loop {
            std::thread::sleep(tick);
            idle = if used() { 0 } else { idle + 1 };
            if idle >= ticks_to_exit {
                exit();
                return;
            }
        }
    });
}

/// Remove the private folder (socket included) and the Mac's link; a no-op outside agent mode.
pub fn cleanup() {
    if let Some((sock, mac)) = LINK.get() {
        crate::runlink::unlink(&crate::runlink::dir(), mac, sock);
    }
    if let Some(d) = DIR.get() {
        let _ = std::fs::remove_dir_all(d);
    }
}

fn hostname() -> String {
    crate::config::host_name().and_then(|n| n.split('.').next().map(str::to_string)).unwrap_or_else(|| "unknown".into())
}

fn user() -> String {
    std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_dir_is_new_and_private() {
        use std::os::unix::fs::MetadataExt;
        let base = crate::testutil::temp_root().join(format!("tm-agent-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        assert!(private_dir(&base).is_ok(), "created");
        assert_eq!(std::fs::metadata(&base).unwrap().mode() & 0o777, 0o700);
        assert!(private_dir(&base).is_err(), "an existing folder (a home, say) is never taken over");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn every_line_on_stdin_counts_until_it_closes() {
        let heard = std::sync::atomic::AtomicU32::new(0);
        read_until_closed(&b"\n\n\n"[..], || { heard.fetch_add(1, std::sync::atomic::Ordering::Relaxed); });
        assert_eq!(heard.load(std::sync::atomic::Ordering::Relaxed), 3);
    }

    #[test]
    fn only_a_helper_nothing_uses_exits() {
        use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
        use std::sync::Arc;
        let tick = std::time::Duration::from_millis(5);
        // a fixed wait for the helper that stays; one that exits is waited for, for a loaded machine
        let run = |used: Arc<dyn Fn(u32) -> bool + Send + Sync>, expect_exit: bool| {
            let (polls, exits) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
            let (p, e) = (polls.clone(), exits.clone());
            watch_use(tick, 6, move || used(p.fetch_add(1, Relaxed)), move || { e.fetch_add(1, Relaxed); });
            let mut waited = 0;
            while waited < 40 || (expect_exit && exits.load(Relaxed) == 0 && waited < 2000) {
                std::thread::sleep(tick);
                waited += 1;
            }
            (polls.load(Relaxed), exits.load(Relaxed))
        };
        assert_eq!(run(Arc::new(|_| true), false).1, 0, "in use: stays");
        assert_eq!(run(Arc::new(|_| false), true), (6, 1), "nothing connected: exits after 6 ticks, once");
        let (polls, exits) = run(Arc::new(|n| n % 5 == 4), false);
        assert_eq!(exits, 0, "a line every fifth tick resets the count: {polls} polls");
        assert_eq!(run(Arc::new(|n| n < 3), true), (9, 1), "used, then not: 6 ticks from the last use");
    }
}
