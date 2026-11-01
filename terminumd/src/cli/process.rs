// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Bounded CLI subprocesses: own their session and process group, end with the CLI's Ctrl-C, and
//! read pipes without detached threads.

use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

fn nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fd belongs to a live pipe; fcntl changes only that descriptor's flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn read_some<R: Read>(pipe: &mut Option<R>, bytes: &mut Vec<u8>) -> io::Result<()> {
    let Some(reader) = pipe else { return Ok(()) };
    let mut buffer = [0; 4096];
    // A continuously writing child must still let the deadline and the other pipe run.
    for _ in 0..16 {
        match reader.read(&mut buffer) {
            Ok(0) => { *pipe = None; return Ok(()) }
            Ok(n) => bytes.extend_from_slice(&buffer[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn collect(child: &mut Child, limit: Duration) -> Result<Output, String> {
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    nonblocking(out.as_ref().expect("piped").as_raw_fd()).map_err(|e| e.to_string())?;
    nonblocking(err.as_ref().expect("piped").as_raw_fd()).map_err(|e| e.to_string())?;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let deadline = Instant::now() + limit;
    loop {
        read_some(&mut out, &mut stdout).map_err(|e| e.to_string())?;
        read_some(&mut err, &mut stderr).map_err(|e| e.to_string())?;
        // Keep the leader unreaped while a descendant holds either pipe: its pid/group
        // cannot be reused before timeout cleanup signals the group we created.
        if out.is_none() && err.is_none()
            && let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(Output { status, stdout, stderr });
        }
        if Instant::now() >= deadline {
            return Err(format!("no answer within {} s", limit.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The group of the child `run_timeout` waits for in a terminal: a Ctrl-C, a closed terminal or
/// a stop of the CLI ends it too, as its own session never gets the terminal's signals.
static CHILD: AtomicI32 = AtomicI32::new(0);
/// One `run_timeout` at a time: the handlers and CHILD are the process's own.
static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
const PASSED_ON: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

extern "C" fn pass_on(sig: libc::c_int) {
    let group = CHILD.load(Ordering::SeqCst);
    // SAFETY: killpg, signal and raise are async-signal-safe; `group` is the session we made.
    unsafe {
        if group > 0 {
            libc::killpg(group, libc::SIGKILL);
        }
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// Signals of the terminal passed on to the child (CHILD, once spawned: before, they only end
/// the CLI) while it runs; the caller's handlers come back after. Only in a terminal:
/// terminumd's own SIGTERM (no terminal) stays its graceful stop.
fn pass_signals() -> Vec<(libc::c_int, libc::sigaction)> {
    // SAFETY: isatty only reads the descriptor's state.
    if unsafe { libc::isatty(0) } != 1 {
        return Vec::new();
    }
    PASSED_ON.iter().filter_map(|&sig| {
        // SAFETY: sigaction with zeroed structs is the documented way to read and set a handler.
        unsafe {
            let mut new: libc::sigaction = std::mem::zeroed();
            new.sa_sigaction = pass_on as *const () as libc::sighandler_t;
            libc::sigemptyset(&mut new.sa_mask);
            let mut old: libc::sigaction = std::mem::zeroed();
            libc::sigaction(sig, std::ptr::null(), &mut old);
            if old.sa_sigaction == libc::SIG_IGN {
                return None; // ignored (nohup): stays ignored
            }
            (libc::sigaction(sig, &new, std::ptr::null_mut()) == 0).then_some((sig, old))
        }
    }).collect()
}

fn restore_signals(old: Vec<(libc::c_int, libc::sigaction)>) {
    for (sig, act) in old {
        // SAFETY: `act` is the handler sigaction read for `sig`.
        unsafe { libc::sigaction(sig, &act, std::ptr::null_mut()) };
    }
    CHILD.store(0, Ordering::SeqCst);
}

/// Capture both pipes and status within `limit`; errors close readers and end our group. The
/// child has a session of its own (no controlling terminal): a read of /dev/tty fails at once
/// instead of stopping it in the background until the limit.
pub fn run_timeout(mut cmd: Command, limit: Duration) -> Result<Output, String> {
    // SAFETY: setsid is async-signal-safe, as pre_exec requires.
    let cmd = unsafe { cmd.pre_exec(|| if libc::setsid() == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }) };
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let passed = pass_signals();
    let mut child = match cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => { restore_signals(passed); return Err(e.to_string()); }
    };
    CHILD.store(child.id() as libc::pid_t, Ordering::SeqCst);
    let result = collect(&mut child, limit);
    if result.is_err() {
        // SAFETY: setsid made this session's group for the child we started, never a caller's.
        unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
        let _ = child.wait();
    }
    restore_signals(passed);
    result
}
