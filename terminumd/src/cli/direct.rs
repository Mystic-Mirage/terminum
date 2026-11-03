// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Direct mode (TM_NO_LAUNCHD=1, scripts/e2e_install.sh in a temporary HOME): terminumd as a
//! plain background process in its own session, its pid and start time in a pid file, so a
//! reused pid never leads to another process.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::ctx::Ctx;

/// The process's start time as ps tells it: with the pid, it names one process.
pub fn started_at(pid: u32) -> String {
    Command::new("/bin/ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// A child of this process that has exited (reaped now), or None when it is not ours.
fn exited_child(ctx: &Ctx, pid: u32) -> Option<bool> {
    let mut children = ctx.children.lock();
    let child = children.get_mut(&pid)?;
    Some(!matches!(child.try_wait(), Ok(None)))
}

/// The pid in the pid file while it is still the process started then.
pub fn running_pid(ctx: &Ctx) -> Option<u32> {
    let text = std::fs::read_to_string(ctx.pidfile()).ok()?;
    let (pid, since) = text.split_once(' ')?;
    let pid: u32 = pid.parse().ok()?;
    if pid == 0 || pid > libc::pid_t::MAX as u32 || since.trim().is_empty() {
        return None;
    }
    if exited_child(ctx, pid) == Some(true) {
        return None;
    }
    let started = started_at(pid);
    (!started.is_empty() && started == since.trim()).then_some(pid)
}

fn gone(ctx: &Ctx, pid: u32) -> bool {
    if let Some(exited) = exited_child(ctx, pid) {
        return exited;
    }
    // SAFETY: signal 0 only checks that the process exists
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

pub fn stop(ctx: &Ctx) {
    let pid = running_pid(ctx);
    let _ = std::fs::remove_file(ctx.pidfile());
    let Some(pid) = pid else { return };
    for (sig, seconds) in [(libc::SIGTERM, 5.0), (libc::SIGKILL, 2.0)] {
        // SAFETY: signalling the terminumd this install started (checked by its start time)
        if unsafe { libc::kill(pid as libc::pid_t, sig) } != 0 {
            break;
        }
        let deadline = Instant::now() + Duration::from_secs_f64(seconds);
        while !gone(ctx, pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        if gone(ctx, pid) {
            break;
        }
    }
    ctx.children.lock().remove(&pid);
}

pub fn start(ctx: &Ctx) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;
    let program = ctx.program();
    let log = std::fs::OpenOptions::new().append(true).create(true).mode(0o600).open(ctx.log()).map_err(|e| format!("{}: {e}", ctx.log().display()))?;
    let mut cmd = Command::new(&program);
    cmd.arg("serve").stdin(Stdio::null()).stdout(log.try_clone().map_err(|e| e.to_string())?).stderr(log);
    cmd.env_clear().envs(&ctx.env).envs(super::launch::service_env(ctx));
    // SAFETY: setsid and umask::restore are async-signal-safe; terminumd leaves the installer's
    // session and group, and starts with the caller's umask, not the CLI's private one (AC-09)
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            crate::umask::restore();
            Ok(())
        });
    }
    let child = cmd.spawn().map_err(|e| format!("cannot start {}: {e}", program.display()))?;
    let pid = child.id();
    ctx.children.lock().insert(pid, child);
    let line = format!("{pid} {}\n", started_at(pid));
    crate::config::write_private(&ctx.pidfile(), line.as_bytes()).map_err(|e| format!("{}: {e}", ctx.pidfile().display()))
}
