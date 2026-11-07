// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Where terminumd keeps things and what the environment overrides (spec §9 Configuration):
//! `~/.terminum` (settings, themes, tmux.conf, logs) and its `state/` folder (sign-ins,
//! workspaces, bookmarks, the CLI socket), both private to the user (AC-09).

use std::path::{Path, PathBuf};

/// The build this binary is (AC-16): set by `make` from the sources; "dev" otherwise.
const BUILD: &str = match option_env!("TM_BUILD") {
    Some(b) => b,
    None => "dev",
};
/// Keeps BUILD whole in the binary's bytes, where `scripts/agents.py` looks for it on a target
/// it cannot run: the optimizer may otherwise build the string from immediates.
#[used]
static BUILD_IN_BYTES: &str = BUILD;

/// The day this binary was built (YYYY-MM-DD), set by `make` and the release; "" otherwise.
pub const BUILD_DATE: &str = match option_env!("TM_BUILD_DATE") {
    Some(d) => d,
    None => "",
};

/// The build terminumd answers as: the compiled one unless a test plays another (TM_BUILD_ID).
pub fn build_id() -> &'static str {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| std::env::var("TM_BUILD_ID").unwrap_or_else(|_| BUILD.to_string()))
}

pub fn env<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// This machine's host name, lowercased (`gethostname`).
pub fn host_name() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its length; gethostname NUL-terminates within it
    if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).to_lowercase())
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// `TM_HOME`, default `~/.terminum`: settings.json, themes/, custom.css, hotkeys.json,
/// tmux.conf, logs/. Made private (0700) when terminumd creates it.
pub fn tm_home() -> PathBuf {
    let dir = std::env::var_os("TM_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".terminum"));
    private_dir(&dir);
    dir
}

/// `TM_APP_DIR`, default `~/.terminum/state` (tests use their own): private (0700) and
/// ours, or terminumd does not start; what is in it is only for this user (AC-09).
pub fn app_dir() -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let dir = std::env::var_os("TM_APP_DIR").map(PathBuf::from).unwrap_or_else(|| tm_home().join("state"));
    private_dir(&dir);
    let meta = std::fs::metadata(&dir).expect("app dir");
    // SAFETY: geteuid has no preconditions
    if meta.uid() != unsafe { libc::geteuid() } {
        eprintln!("error: {} belongs to another user", dir.display());
        std::process::exit(2);
    }
    dir
}

/// Creates `dir` (and parents) and makes it 0700.
pub fn private_dir(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("error: {}: {e}", dir.display());
        std::process::exit(2);
    }
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
}

/// A program on PATH, or in the places Homebrew puts it (launchd's PATH is short).
pub fn find_program(name: &str, env_key: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(env_key).map(PathBuf::from) {
        return Some(p);
    }
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':')
        .map(PathBuf::from)
        .chain(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from))
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The strings of a JSON array; none for anything else.
pub fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// Writes `bytes` to `path` through a temporary file renamed over it, mode 0600.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_replacing(path, bytes, 0o600, None)
}

/// The one way terminum replaces a file: a temporary file of its own beside it (a unique name,
/// `mode` as umask allows, or `keep`'s permissions exactly), synced, then renamed over it.
pub fn write_replacing(path: &Path, bytes: &[u8], mode: u32, keep: Option<std::fs::Permissions>) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(".{name}.{}.{}.new", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_file(&tmp);
    let written = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        if let Some(p) = keep {
            std::fs::set_permissions(&tmp, p)?;
        }
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Runtime settings that come from the environment.
#[derive(Clone, Debug)]
pub struct Env {
    pub port: u16,
    pub tmux_socket: String,
    pub history: usize,
    pub text_max: u64,
    pub list_cache_mb: usize,
    pub ttl_days: u64,
    pub roots: String,
}

impl Env {
    pub fn read(port_default: u16, socket_default: &str) -> Env {
        Env {
            port: env("TM_PORT", port_default),
            tmux_socket: std::env::var("TM_TMUX_SOCKET").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| socket_default.to_string()),
            history: env("TM_HISTORY", 10_000),
            text_max: env("TM_TEXT_MAX_BYTES", 10u64 << 20),
            list_cache_mb: env("TM_LIST_CACHE_MB", 128),
            ttl_days: env("TM_WORKSPACE_TTL_DAYS", 14),
            roots: std::env::var("TM_WRITABLE_ROOTS").unwrap_or_else(|_| "$HOME:/tmp".into()),
        }
    }
}

/// A test's private store: `TM_TEST_HOOKS` with its own `TM_APP_DIR`; an installed terminumd's service passes neither (`cli/launch.rs`).
pub fn test_store() -> bool {
    std::env::var("TM_TEST_HOOKS").is_ok() && std::env::var("TM_APP_DIR").is_ok()
}
