// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Unit-test helpers: a private tmux server (never the user's) that goes away with the test,
//! also when an assertion fails, and the scratch folder of the whole test process.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

/// Where every test's files go: `tm-unit-<pid>` in the temporary folder, removed when this process
/// exits, after failed and panicked tests too (spec AC-102). A process-wide folder rather than a
/// guard per test: helpers hand out plain paths into their folders (`fresh_dir(x).join(y)`).
pub fn temp_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    extern "C" fn remove() {
        if let Some(root) = ROOT.get() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("tm-unit-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY: registers a function that takes nothing and touches only its own static
        unsafe { libc::atexit(remove) };
        root
    })
}

/// Whether the tmux on PATH is at least `want` (major, minor): 3.4 types `send-keys S-Enter` as
/// text and writes `$` in a window name as `\$`; 3.8 keeps a window name's `\` as `\\`.
pub fn tmux_at_least(want: (u32, u32)) -> bool {
    let v = Command::new("tmux").arg("-V").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let n: Vec<u32> = v.split(|c: char| !c.is_ascii_digit()).filter_map(|p| p.parse().ok()).take(2).collect();
    n.len() == 2 && (n[0], n[1]) >= want
}

pub struct Tmux {
    /// `-L` name: `tm-test-<name>-<pid>`
    pub socket: String,
    /// a scratch folder of its own, removed with it
    pub dir: PathBuf,
}

impl Tmux {
    /// A server whose session "main" (`cols`×`rows`) runs `cmd`; None when tmux is missing.
    pub fn start(name: &str, (cols, rows): (u16, u16), cmd: &str) -> Option<Tmux> {
        if Command::new("tmux").arg("-V").output().is_err() {
            eprintln!("skipped: tmux is not on PATH");
            return None;
        }
        let socket = format!("tm-test-{name}-{}", std::process::id());
        let dir = temp_root().join(&socket);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Tmux { dir: std::fs::canonicalize(&dir).unwrap(), socket };
        let out = s.tmux(&["new-session", "-d", "-s", "main", "-x", &cols.to_string(), "-y", &rows.to_string(), cmd]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        Some(s)
    }

    pub fn tmux(&self, args: &[&str]) -> Output {
        Command::new("tmux").args(["-u", "-L", &self.socket, "-f", "/dev/null"]).args(args).output().expect("tmux runs")
    }

    /// The output of a command that must succeed.
    pub fn read(&self, args: &[&str]) -> String {
        let out = self.tmux(args);
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    /// Runs `lines` through tmux's command parser, as the control client sends them.
    pub fn source(&self, lines: &[String]) {
        let file = self.dir.join("cmds.conf");
        std::fs::write(&file, lines.join("\n") + "\n").unwrap();
        self.read(&["source-file", file.to_str().unwrap()]);
    }
}

/// An empty folder `tm-<name>-<pid>` under `temp_root` for one test.
pub fn fresh_dir(name: &str) -> PathBuf {
    let dir = temp_root().join(format!("tm-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = self.tmux(&["kill-server"]);
        // where tmux puts `-L` sockets (also when a test killed the server itself)
        let base = std::env::var("TMUX_TMPDIR").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/tmp".into());
        // SAFETY: getuid has no preconditions
        let _ = std::fs::remove_file(PathBuf::from(base).join(format!("tmux-{}", unsafe { libc::getuid() })).join(&self.socket));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// An app over fresh folders (no tmux server is started): (its root, removed by the test; the app).
pub fn app(tag: &str) -> (PathBuf, crate::app::Shared) {
    let root = fresh_dir(tag);
    let (home, dir) = (root.join("home"), root.join("state"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let env = crate::config::Env { port: 0, tmux_socket: format!("tm-test-{tag}"), history: 1000,
        text_max: 1 << 20, list_cache_mb: 1, ttl_days: 1, roots: root.display().to_string() };
    let cfg = crate::app::Config { port: 0, agent: true, service: false, text_max: 1 << 20,
        history: 1000, dir, home: home.clone(), agent_token: String::new() };
    let link = crate::tmux::server::Link::Local { tmux: "unused".into(), socket: env.tmux_socket.clone(), conf: "/dev/null".into() };
    let app = crate::build_app(cfg, &env, crate::looks::Looks::load(&home), link, &env.tmux_socket);
    (root, app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scratch_folder_goes_with_the_process_also_after_a_panic() {
        if std::env::var_os("TM_TEST_SCRATCH_PANIC").is_some() {
            std::fs::write(fresh_dir("panics").join("f"), "").unwrap();
            println!("root={}", temp_root().display());
            panic!("on purpose");
        }
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "testutil::tests::the_scratch_folder_goes_with_the_process_also_after_a_panic", "--nocapture"])
            .env("TM_TEST_SCRATCH_PANIC", "1").output().unwrap();
        assert!(!out.status.success(), "the child's test panics");
        let text = String::from_utf8_lossy(&out.stdout);
        let root = text.split("root=").nth(1).and_then(|t| t.lines().next()).expect("it says where");
        assert_ne!(Path::new(root), temp_root());
        assert!(!Path::new(root).exists(), "{root} stayed");
    }
}
