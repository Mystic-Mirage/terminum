// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! One tmux control client (`tmux -C … attach`, or the same over ssh for a host): a child
//! process whose stdin takes commands, one a line, and whose stdout carries notices and the
//! answers (`%begin … %end|%error`) in the order the commands were written (AC-01, AC-03).
//!
//! A line holding several commands (`a ; b`) is answered by several blocks, and a failing
//! one stops the rest, so every command is a line of its own: callers pipeline lines, never
//! join them. A command left unanswered for `ANSWER_WAIT` while nothing else arrives kills the
//! client (AC-14); on a slow link bytes keep arriving, and killing it would only send them again
//! (AC-132).

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

/// How long a caller waits for an answer; with nothing arriving meanwhile, the client is hung
/// and is killed (AC-14).
pub const ANSWER_WAIT: Duration = Duration::from_secs(10);
/// A command this old means a hung client even while other bytes arrive (AC-132).
const ANSWER_MOST: Duration = Duration::from_secs(60);
/// A server whose oldest command waited this long is said to stall (AC-03), and nothing more is
/// asked of it for the list while it does (AC-132).
pub const STALL: Duration = Duration::from_millis(3000);
/// What a control client over ssh says when nothing ever came from it.
pub const SSH_NO_ANSWER: &str = "ssh: no answer from the host";
const NOT_ANSWERED: &str = "tmux did not answer";
const NOT_CONNECTED: &str = "tmux is not connected";
const NOT_KEEPING_UP: &str = "tmux is not keeping up: too many commands wait for it";

/// Whether an answer's error says the link failed, not tmux: nothing is known of what was asked (AC-132).
pub fn no_answer(error: &str) -> bool {
    [NOT_ANSWERED, NOT_CONNECTED, NOT_KEEPING_UP].contains(&error)
}

/// stderr kept to say why a client ended (ssh's "Permission denied (publickey)").
const STDERR_KEPT: usize = 4096;
/// How long the reason waits for stderr's end once stdout ended (a child left holding it).
const STDERR_WAIT: Duration = Duration::from_secs(1);
/// Most bytes of commands written to a client and not yet taken by its writer (stdin full: a
/// slow ssh): a command past it fails at once rather than piling up (the reader never writes).
const LINES_MAX: usize = 16 << 20;

/// What a control client tells besides answers. `%output` is a wake-up: the screen comes from
/// `capture-pane`; only a clipboard write in it is read (clip.rs, AC-06).
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// a pane printed something: `%N`
    Output(String),
    /// windows, panes, layouts or sessions changed: the list is read again
    Changed,
    /// tmux paused a pane's output (flow control): `%N`
    Pause(String),
    /// a program in pane `%N` wrote this to the clipboard (OSC 52): its base64, not decoded yet
    Copy(String, Vec<u8>),
}

/// An answer: the block's lines, or tmux's error text.
pub type Answer = Result<Vec<String>, String>;

struct Pending {
    reply: oneshot::Sender<Answer>,
    at: Instant,
}

/// A running control client. Dropping the last handle kills the process.
pub struct Control {
    lines: mpsc::UnboundedSender<(String, oneshot::Sender<Answer>)>,
    /// bytes in `lines` (LINES_MAX)
    queued: Arc<AtomicUsize>,
    alive: Arc<AtomicBool>,
    /// why it ended (ssh's or tmux's last words), once it has
    pub ended: Arc<Mutex<Option<String>>>,
    /// set once the first answer arrived: the client attached
    pub attached: Arc<AtomicBool>,
    /// tmux said `%exit` with no reason: its session or server ended on purpose (its last
    /// window closed, `kill-server`), not a crash, a kill or a lost link
    pub clean: Arc<AtomicBool>,
    pid: Option<u32>,
    pending: Arc<Mutex<VecDeque<Pending>>>,
}

impl Control {
    /// Starts `cmd` (stdin, stdout, stderr piped). Notices go to `notices`; when the process
    /// ends, `notices` gets nothing more and `done` is fired with the reason. `notices` stays
    /// unbounded: its reader (notices.rs) never waits on anything but it, so it keeps up, and
    /// a bound would either stall this reader, which also carries every answer, or drop a
    /// notice (a layout change, a copy) for good; a copy is at most clip.rs `MOST` bytes.
    pub fn spawn(mut cmd: Command, notices: mpsc::UnboundedSender<Notice>, done: oneshot::Sender<String>) -> std::io::Result<Control> {
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        // what ended without a word says: a link that never reached tmux says so
        let via_ssh = std::path::Path::new(cmd.as_std().get_program()).file_name().is_some_and(|n| n.to_string_lossy().contains("ssh"));
        let mut child: Child = cmd.spawn()?;
        let pid = child.id();
        let mut stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let (tx, mut rx) = mpsc::unbounded_channel::<(String, oneshot::Sender<Answer>)>();
        let pending: Arc<Mutex<VecDeque<Pending>>> = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let attached = Arc::new(AtomicBool::new(false));
        let ended: Arc<Mutex<Option<String>>> = Arc::default();
        let clean = Arc::new(AtomicBool::new(false));
        let heard = Arc::new(Mutex::new(Instant::now()));
        let errors: Arc<Mutex<Vec<u8>>> = Arc::default();
        // the watchdog ended it: its stderr's last line (an ssh warning) is not why
        let silent = Arc::new(AtomicBool::new(false));

        // writer: the lines queued now (a frame's display and capture, a batch of keys), their
        // waiters in line, one write; once stdin is closed, new lines fail at once (those
        // waiting end with the reader or their timeout)
        let queued = Arc::new(AtomicUsize::new(0));
        let (p, a, q) = (pending.clone(), alive.clone(), queued.clone());
        tokio::spawn(async move {
            let mut bytes = Vec::new();
            while let Some(first) = rx.recv().await {
                let mut next = Some(first);
                while let Some((line, reply)) = next {
                    q.fetch_sub(line.len(), Ordering::Relaxed);
                    if a.load(Ordering::Relaxed) {
                        p.lock().push_back(Pending { reply, at: Instant::now() });
                        bytes.extend_from_slice(line.as_bytes());
                        bytes.push(b'\n');
                    } else {
                        let _ = reply.send(Err(NOT_CONNECTED.into()));
                    }
                    next = rx.try_recv().ok();
                }
                if !bytes.is_empty() && (stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err()) {
                    a.store(false, Ordering::Relaxed);
                }
                bytes.clear();
            }
        });

        // stderr: kept, for the reason an ssh or tmux ended
        let e = errors.clone();
        let stderr_done = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            while let Ok(n) = stderr.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let mut kept = e.lock();
                kept.extend_from_slice(&buf[..n]);
                let over = kept.len().saturating_sub(STDERR_KEPT);
                kept.drain(..over);
            }
        });

        // reader: notices and answers; at the end every waiter learns why
        let (p, a, att, end, cl, h, hung_up) = (pending.clone(), alive.clone(), attached.clone(), ended.clone(), clean.clone(), heard.clone(), silent.clone());
        tokio::spawn(async move {
            let mut reader = BufReader::with_capacity(1 << 16, stdout);
            let mut block: Option<(String, bool)> = None; // (the %begin's "time num flags", ours)
            let mut lines: Vec<String> = Vec::new();
            let mut buf = Vec::with_capacity(4096);
            let mut exit_reason = None;
            let mut clips = super::clip::Clips::default();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => *h.lock() = Instant::now(),
                }
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                }
                if let Some((id, ours)) = &block {
                    let end = end_of(&buf, id);
                    if end.is_none() {
                        if *ours {
                            lines.push(String::from_utf8_lossy(&buf).into_owned());
                        }
                        continue;
                    }
                    let ok = end == Some(true);
                    if *ours {
                        att.store(true, Ordering::Relaxed);
                        let taken = std::mem::take(&mut lines);
                        if let Some(w) = p.lock().pop_front() {
                            let _ = w.reply.send(if ok { Ok(taken) } else { Err(taken.join("\n")) });
                        }
                    } else {
                        att.store(true, Ordering::Relaxed);
                    }
                    block = None;
                    continue;
                }
                if let Some(rest) = buf.strip_prefix(b"%begin ") {
                    let id = String::from_utf8_lossy(rest).into_owned();
                    let ours = id.rsplit(' ').next().and_then(|f| f.parse::<u32>().ok()).is_some_and(|f| f & 1 == 1);
                    block = Some((id, ours));
                    lines.clear();
                    continue;
                }
                if let Some(n) = notice(&buf) {
                    let copies: Vec<Notice> = match &n {
                        Notice::Output(pane) => clips.line(pane, &buf).into_iter().map(|t| Notice::Copy(pane.clone(), t)).collect(),
                        Notice::Pause(pane) => {
                            clips.forget(pane); // its output is dropped until it goes on
                            Vec::new()
                        }
                        _ => Vec::new(),
                    };
                    let _ = notices.send(n);
                    for c in copies {
                        let _ = notices.send(c);
                    }
                } else if let Some(rest) = buf.strip_prefix(b"%exit") {
                    exit_reason = Some(String::from_utf8_lossy(rest).trim().to_string());
                }
            }
            a.store(false, Ordering::Relaxed);
            cl.store(exit_reason.as_deref() == Some(""), Ordering::Relaxed);
            // no %exit, stdout ended before stderr was read through (ssh's last words come on it):
            // its end first, so a reason is never "no answer" while the words are on their way
            if exit_reason.is_none() {
                let _ = tokio::time::timeout(STDERR_WAIT, stderr_done).await;
            }
            let quiet = if via_ssh && !att.load(Ordering::Relaxed) { SSH_NO_ANSWER } else { "tmux exited" };
            let said_nothing = hung_up.load(Ordering::Relaxed) && via_ssh && !att.load(Ordering::Relaxed);
            let why = exit_reason.filter(|r| !r.is_empty()).or_else(|| last_line(&errors.lock())).filter(|_| !said_nothing).unwrap_or_else(|| quiet.into());
            for w in p.lock().drain(..) {
                let _ = w.reply.send(Err(why.clone()));
            }
            *end.lock() = Some(why.clone());
            let _ = done.send(why);
        });

        // watchdog: a command long unanswered with nothing arriving means a hung client (AC-14)
        let (p, a) = (pending.clone(), alive.clone());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if !a.load(Ordering::Relaxed) {
                    break;
                }
                let late = p.lock().front().is_some_and(|w| hung(w.at.elapsed(), heard.lock().elapsed()));
                if late {
                    tracing::warn!(event = "tmux.timeout", pid = ?pid, "control client timed out");
                    silent.store(true, Ordering::Relaxed);
                    if let Some(pid) = pid {
                        // SAFETY: a signal to our own child process
                        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                    }
                    break;
                }
            }
            let _ = child.wait().await;
        });

        Ok(Control { lines: tx, queued, alive, ended, attached, clean, pid, pending })
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// How long the oldest unanswered command has waited (zero when none waits).
    pub fn waiting(&self) -> Duration {
        self.pending.lock().front().map_or(Duration::ZERO, |w| w.at.elapsed())
    }

    /// Writes one command line now; the answer arrives in order. Lines are never empty
    /// (an empty line detaches a control client) and never hold a newline.
    pub fn send(&self, line: String) -> oneshot::Receiver<Answer> {
        let (tx, rx) = oneshot::channel();
        debug_assert!(!line.trim().is_empty() && !line.contains('\n'));
        if line.trim().is_empty() || line.contains(['\n', '\r']) {
            let _ = tx.send(Err("a tmux command must be one non-empty line".into()));
            return rx;
        }
        let n = line.len();
        if self.queued.fetch_add(n, Ordering::Relaxed) + n > LINES_MAX {
            self.queued.fetch_sub(n, Ordering::Relaxed);
            let _ = tx.send(Err(NOT_KEEPING_UP.into()));
            return rx;
        }
        if self.lines.send((line, tx)).is_err() {
            self.queued.fetch_sub(n, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(Err(NOT_CONNECTED.into()));
            return rx;
        }
        rx
    }

    /// One command and its answer.
    pub async fn run(&self, line: impl Into<String>) -> Answer {
        let rx = self.send(line.into());
        match tokio::time::timeout(ANSWER_WAIT + Duration::from_secs(1), rx).await {
            Ok(Ok(a)) => a,
            Ok(Err(_)) => Err(NOT_CONNECTED.into()),
            Err(_) => Err(NOT_ANSWERED.into()),
        }
    }

    /// Several commands written at once (one round trip), answered in order.
    pub async fn run_all(&self, lines: Vec<String>) -> Vec<Answer> {
        let waiting: Vec<_> = lines.into_iter().map(|l| self.send(l)).collect();
        let mut out = Vec::with_capacity(waiting.len());
        for rx in waiting {
            out.push(match tokio::time::timeout(ANSWER_WAIT + Duration::from_secs(1), rx).await {
                Ok(Ok(a)) => a,
                Ok(Err(_)) => Err(NOT_CONNECTED.into()),
                Err(_) => Err(NOT_ANSWERED.into()),
            });
        }
        out
    }

    /// Ends the client (its process is killed when the last handle goes too).
    pub fn kill(&self) {
        if let Some(pid) = self.pid.filter(|_| self.alive()) {
            // SAFETY: a signal to our own child process
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        }
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Whether a client is hung: its oldest command waited `oldest`, its last line came `quiet` ago.
fn hung(oldest: Duration, quiet: Duration) -> bool {
    oldest > ANSWER_MOST || (oldest > ANSWER_WAIT && quiet > ANSWER_WAIT)
}

/// `Some(true)` for the `%end`, `Some(false)` for the `%error` that closes block `id`.
fn end_of(line: &[u8], id: &str) -> Option<bool> {
    let (ok, rest) = if let Some(r) = line.strip_prefix(b"%end ") {
        (true, r)
    } else {
        let r = line.strip_prefix(b"%error ")?;
        (false, r)
    };
    (rest == id.as_bytes()).then_some(ok)
}

/// A notice line outside blocks, or None for lines that need nothing.
fn notice(line: &[u8]) -> Option<Notice> {
    // only the first two words are read: the rest of a %output line is the pane's bytes
    let mut words = line.splitn(3, |b| *b == b' ');
    let kind = std::str::from_utf8(words.next()?).ok()?;
    let arg = words.next().and_then(|a| std::str::from_utf8(a).ok());
    match kind {
        "%output" | "%extended-output" => arg.filter(|p| p.starts_with('%')).map(|p| Notice::Output(p.to_string())),
        "%pause" => arg.map(|p| Notice::Pause(p.to_string())),
        "%window-add" | "%window-close" | "%unlinked-window-add" | "%unlinked-window-close" | "%window-renamed"
        | "%unlinked-window-renamed" | "%layout-change" | "%session-changed" | "%sessions-changed" | "%session-renamed"
        | "%session-window-changed" | "%window-pane-changed" | "%pane-mode-changed" | "%client-session-changed" => Some(Notice::Changed),
        _ => None,
    }
}

fn last_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes).lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string)
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
