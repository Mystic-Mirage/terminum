// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Bounded reading of an ssh child's output: a host's rc files or a broken host may print
//! without end, so its answer is capped in bytes, its stderr kept as a short tail, and a
//! line read for the helper's "ready" is capped in length; over a cap, only this child ends.

use std::time::Duration;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt};
use tokio::process::Child;

/// The most of a host's answer (stdout) kept; the probe, the install check and the agents'
/// results are a few kilobytes.
pub const ANSWER_MAX: usize = 1 << 20;
/// The end of stderr kept for ssh's last words.
pub const ERR_TAIL: usize = 4096;
/// The longest line read while waiting for the helper's "ready".
pub const LINE_MAX: usize = 4096;

/// What a finished child left: exit status, stdout, the tail of stderr.
pub struct Output {
    pub status: std::process::ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Reads all of `r`, failing once it passes `max` bytes.
async fn capped(mut r: impl AsyncRead + Unpin, max: usize) -> Result<Vec<u8>, String> {
    let mut v = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = r.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(v);
        }
        if v.len() + n > max {
            return Err(format!("the host's answer is larger than {} KiB", max / 1024));
        }
        v.extend_from_slice(&buf[..n]);
    }
}

/// Reads all of `r`, keeping only its last `max` bytes.
pub async fn tail(mut r: impl AsyncRead + Unpin, max: usize) -> Vec<u8> {
    let mut v = Vec::new();
    let mut buf = [0u8; 8192];
    while let Ok(n) = r.read(&mut buf).await {
        if n == 0 {
            break;
        }
        v.extend_from_slice(&buf[..n]);
        if v.len() > max {
            v.drain(..v.len() - max);
        }
    }
    v
}

/// Waits for `child` (stdout and stderr piped) within `timeout`, its stdout capped at
/// `max` bytes and its stderr kept as an `ERR_TAIL` tail; over the cap or the time, the
/// child is killed (and only it).
pub async fn output(child: &mut Child, max: usize, timeout: Duration) -> Result<Output, String> {
    let out = child.stdout.take().ok_or("stdout not piped")?;
    let err = child.stderr.take().ok_or("stderr not piped")?;
    let run = async {
        let (stdout, stderr) = tokio::join!(capped(out, max), tail(err, ERR_TAIL));
        let stdout = stdout?;
        let status = child.wait().await.map_err(|e| e.to_string())?;
        Ok::<_, String>(Output { status, stdout, stderr })
    };
    let got = match tokio::time::timeout(timeout, run).await {
        Ok(r) => r,
        Err(_) => Err(format!("ssh timed out after {} s", timeout.as_secs())),
    };
    if got.is_err() {
        let _ = child.kill().await;
    }
    got
}

/// The next line of `r` without its newline (and `\r`), `None` at the end; a line longer
/// than `max` bytes is an error, so a host printing without newlines cannot grow it.
pub async fn line(r: &mut (impl AsyncBufRead + Unpin), max: usize) -> Result<Option<String>, String> {
    let mut v = Vec::new();
    loop {
        let buf = r.fill_buf().await.map_err(|e| e.to_string())?;
        if buf.is_empty() {
            return Ok((!v.is_empty()).then(|| String::from_utf8_lossy(&v).into_owned()));
        }
        let (take, done) = match buf.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (buf.len(), false),
        };
        v.extend_from_slice(&buf[..take]);
        r.consume(take);
        if v.len() > max + 1 {
            return Err(format!("the host printed a line longer than {} KiB", max / 1024));
        }
        if done {
            v.pop();
            if v.last() == Some(&b'\r') {
                v.pop();
            }
            return Ok(Some(String::from_utf8_lossy(&v).into_owned()));
        }
    }
}

#[cfg(test)]
#[path = "bounded_tests.rs"]
mod tests;
