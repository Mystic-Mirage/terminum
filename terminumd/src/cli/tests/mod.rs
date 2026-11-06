// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Test doubles for the command: a temporary home every path derives from, a terminumd socket
//! served in a thread, fake packages whose terminumd is this test binary answering /health
//! on the socket (no real terminumd, no launchd, no tmux), and a stub launchctl.

mod cmd_tests;
#[cfg(target_os = "macos")]   // macOS asks for folders, nothing else does
mod folders_tests;
mod hooks_tests;
mod hostmode_tests;
mod refresh_tests;
mod import_tests;
mod install_tests;
mod launch_tests;
mod systemd_tests;
mod upgrade_tests;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::ctx::{account_home, Ctx};

/// How much longer than on a quiet machine a fake terminumd may take to answer: the load per
/// core, 1 to 4 (web/test/check.mjs's SLACK for the command's tests): at load 48 on 16 cores
/// the test binary started as a fake terminumd took over 3 s.
pub fn slack() -> u32 {
    let mut load = [0f64; 3];
    // SAFETY: getloadavg writes at most 3 values into the array it is given
    let n = unsafe { libc::getloadavg(load.as_mut_ptr(), 3) };
    let cores = std::thread::available_parallelism().map_or(1, |c| c.get()) as f64;
    if n < 1 { 1 } else { (load[0] / cores).clamp(1.0, 4.0).ceil() as u32 }
}

/// A short temporary home (a Unix socket path holds at most 104 bytes), removed when dropped.
pub struct Home {
    pub dir: PathBuf,
}

impl Home {
    pub fn new() -> Home {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = PathBuf::from(format!("/tmp/tmh-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Home { dir: std::fs::canonicalize(&dir).unwrap() }
    }

    /// A context for this home with every TM_ variable cleared, then `env`; output kept.
    pub fn ctx(&self, env: &[(&str, &str)]) -> Ctx {
        let mut vars: HashMap<String, String> = HashMap::new();
        vars.insert("HOME".into(), self.dir.display().to_string());
        vars.insert("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into());
        for (k, v) in env {
            vars.insert(k.to_string(), v.to_string());
        }
        let mut ctx = Ctx::new(vars, account_home()).unwrap();
        ctx.systemd = false; // launchd's stub on every OS; systemd_tests turn systemd on
        ctx.captured = Some(Mutex::new(Vec::new()));
        ctx.wait = Duration::from_secs(3) * slack();
        for p in [&ctx.root, &ctx.app_dir, &ctx.plist()] {
            assert!(p.starts_with(&self.dir), "{} is outside the test's home", p.display());
        }
        ctx
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// What the context said so far.
pub fn said(ctx: &Ctx) -> Vec<String> {
    ctx.captured.as_ref().map(|c| c.lock().clone()).unwrap_or_default()
}

pub fn write_exec(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

/// A request a served socket got: method, path, JSON body, Content-Type.
pub type Request = (String, String, Option<Value>, Option<String>);
pub type Route = Box<dyn Fn(&Option<Value>) -> (u16, Value) + Send + Sync>;

/// One HTTP request off a connection: (method, path, body, content type).
fn read_request(s: &UnixStream) -> Option<Request> {
    let mut r = BufReader::new(s);
    let mut first = String::new();
    r.read_line(&mut first).ok()?;
    let mut parts = first.split_whitespace();
    let (method, path) = (parts.next()?.to_string(), parts.next()?.to_string());
    let (mut length, mut ctype) = (0usize, None);
    loop {
        let mut line = String::new();
        r.read_line(&mut line).ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            match k.to_ascii_lowercase().as_str() {
                "content-length" => length = v.trim().parse().unwrap_or(0),
                "content-type" => ctype = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    let mut body = vec![0; length];
    r.read_exact(&mut body).ok()?;
    Some((method, path, serde_json::from_slice(&body).ok(), ctype))
}

/// Answers `routes` {"METHOD /path": fn(body)} on `sock` until dropped; records requests.
pub struct Served {
    pub requests: Arc<Mutex<Vec<Request>>>,
    sock: PathBuf,
    stop: Arc<AtomicBool>,
}

pub fn serve(sock: &Path, routes: HashMap<String, Route>) -> Served {
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(sock);
    let listener = UnixListener::bind(sock).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let (rec, halt) = (requests.clone(), stop.clone());
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            if halt.load(Ordering::Relaxed) {
                return;
            }
            let Ok(mut conn) = conn else { continue };
            let Some(req) = read_request(&conn) else { continue };
            // a route named without its query answers any query
            let bare = format!("{} {}", req.0, req.1.split('?').next().unwrap_or_default());
            let (status, answer) = match routes.get(&format!("{} {}", req.0, req.1)).or_else(|| routes.get(&bare)) {
                Some(f) => f(&req.2),
                None => (404, json!({"error": "not_found", "message": "no route"})),
            };
            rec.lock().push(req);
            let data = answer.to_string();
            let _ = write!(conn, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{data}", data.len());
        }
    });
    Served { requests, sock: sock.to_path_buf(), stop }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = UnixStream::connect(&self.sock);
    }
}

pub fn fixed(status: u16, answer: Value) -> Route {
    Box::new(move |_| (status, answer.clone()))
}

/// The health a fake terminumd answers.
pub fn fake_health(build: &str) -> Value {
    json!({"build": build, "pid": std::process::id(), "uptime_s": 1, "port": 48080,
           "password": {"set": true, "error": null}, "servers": [], "hosts": [], "pages": 0,
           "tmux": {"ok": true, "message": ""}})
}

/// Not a test: this test binary playing terminumd in a fake package (TM_FAKE_TERMINUMD=1),
/// serving /health as TM_FAKE_BUILD on the socket until it is killed, its test's home is
/// gone or five minutes have passed: a test that fails before killing it leaves nothing running.
#[test]
fn fake_terminumd() {
    if std::env::var("TM_FAKE_TERMINUMD").as_deref() != Ok("1") {
        return;
    }
    let build = std::env::var("TM_FAKE_BUILD").unwrap();
    let home = std::env::var("HOME").unwrap();
    let root = std::env::var("TM_HOME").unwrap_or_else(|_| format!("{home}/.terminum"));
    let sock = PathBuf::from(std::env::var("TM_APP_DIR").unwrap_or_else(|_| format!("{root}/state"))).join("terminumd.sock");
    let mut routes: HashMap<String, Route> = HashMap::new();
    routes.insert("GET /health".into(), fixed(200, fake_health(&build)));
    eprintln!("level=info event=start build={build}");
    let _served = serve(&sock, routes);
    for _ in 0..300 {
        std::thread::sleep(Duration::from_secs(1));
        if !sock.exists() {
            return;
        }
    }
}

#[test]
fn the_fake_terminumd_ends_when_its_home_goes() {
    let home = Home::new();
    let state = home.path("state");
    std::fs::create_dir_all(&state).unwrap();
    let mut fake = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cli::tests::fake_terminumd", "--nocapture", "--test-threads=1", "-q"])
        .env("TM_FAKE_TERMINUMD", "1")
        .env("TM_FAKE_BUILD", "b1")
        .env("TM_APP_DIR", &state)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let sock = state.join("terminumd.sock");
    let deadline = std::time::Instant::now() + Duration::from_secs(20) * slack();
    while !sock.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(sock.exists(), "the fake never served its socket");
    std::fs::remove_dir_all(&state).unwrap();
    let mut ended = None;
    while ended.is_none() && std::time::Instant::now() < deadline {
        ended = fake.try_wait().unwrap();
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = fake.kill();
    let _ = fake.wait();
    assert!(ended.is_some(), "the fake outlived its home");
}

/// A package folder as scripts/package.py makes it; its terminumd says `answers` to
/// --version and, run, serves the socket as `build` (crash: exits at once with an error).
pub fn package(home: &Home, build: &str, answers: Option<&str>, crash: bool) -> PathBuf {
    let answers = answers.unwrap_or(build);
    let pkg = home.path(&format!("pkg-{build}-{answers}-{crash}"));
    let _ = std::fs::remove_dir_all(&pkg);
    let exe = std::env::current_exe().unwrap();
    let run = if crash {
        "echo '2026-10-15T09:00:00.000Z level=error event=listen msg=\"port 48080 is in use (set `port` in ~/.terminum/settings.json)\"'\nexit 1".to_string()
    } else {
        format!("exec /usr/bin/env TM_FAKE_TERMINUMD=1 TM_FAKE_BUILD='{build}' '{}' --exact cli::tests::fake_terminumd --nocapture --test-threads=1 -q", exe.display())
    };
    let text = format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{answers}'; exit 0; fi\n{run}\n");
    for plat in ["macos-aarch64", "macos-x86_64", "linux-x86_64", "linux-aarch64"] {
        write_exec(&pkg.join("agents").join(plat).join("terminumd"), &text);
    }
    std::fs::write(pkg.join("BUILD"), format!("{build}\n")).unwrap();
    std::fs::write(pkg.join("release-signers"), "# none\n").unwrap();
    pkg
}

/// A launchctl that records its calls in <dir>/calls and keeps "loaded" as a file; a file
/// `refuse` makes bootstrap fail, `fail_kickstart` makes kickstart fail, and `unbound` (a
/// count) makes that many bootstraps log what terminumd says when launchd could not bind
/// the port (`unbound_log`: the log; `port`).
pub fn stub_launchctl(dir: &Path) -> PathBuf {
    let path = dir.join("launchctl");
    write_exec(
        &path,
        "#!/bin/sh\nd=$(dirname \"$0\")\necho \"$*\" >> \"$d/calls\"\ncase $1 in\n\
         print) [ -f \"$d/loaded\" ] && exit 0; exit 113 ;;\n\
         bootstrap) if [ -f \"$d/refuse\" ]; then echo 'Bootstrap failed: 5: Input/output error' >&2; exit 5; fi; touch \"$d/loaded\"\n\
           n=$(cat \"$d/unbound\" 2>/dev/null || echo 0)\n\
           if [ \"$n\" -gt 0 ]; then echo $((n - 1)) > \"$d/unbound\"\n\
             printf 'ERROR terminumd::sock: msg=\"port %s was held by another program when launchd loaded terminum: free it, then run `terminum restart`\"\\n' \"$(cat \"$d/port\")\" >> \"$(cat \"$d/unbound_log\")\"; fi ;;\n\
         bootout) rm -f \"$d/loaded\" ;;\n\
         kickstart) [ -f \"$d/fail_kickstart\" ] && exit 1 ;;\nesac\nexit 0\n",
    );
    path
}

/// The verbs the stub launchctl was called with, `print` left out.
pub fn verbs(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("calls")).unwrap_or_default().lines().filter_map(|l| l.split_whitespace().next()).filter(|v| *v != "print").map(str::to_string).collect()
}
