// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A shown pane's frames (AC-02): `%output` wakes the task, which captures the screen once the
//! pane has been quiet SETTLE (a write that came in parts is whole) when it was idle, else at the
//! next 33 ms tick, and sends only the rows that changed; rows that scrolled off are appended to
//! the page's history with their numbers.
//! A pane nobody shows is never captured; one whose terminal its page hides behind Files or View
//! (`screens` off) only has its folder followed, and is sent anew when shown (AC-02).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::Notify;

use super::{Page, CLOSED};
use crate::app::Shared;
use crate::tmux::server::Wake;
use crate::screen::frame::{Fields, Shown, FIELDS};
use crate::screen::history::{Numbering, Step};
use crate::screen::rows;
use crate::tmux::Server;

/// Frames at most 30 a second (AC-02).
pub const FRAME_GAP: Duration = Duration::from_millis(33);
/// After `%output`, the quiet that ends a write: one the terminal took in two reads (Claude Code's
/// cursor up to its spinner row, the row, the cursor back) is captured whole, not half done.
const SETTLE: Duration = Duration::from_millis(6);
/// A shown pane is also looked at this often: a safety net, %output wakes it (AS-06 holds).
const POLL: Duration = Duration::from_secs(1);
/// History rows sent when a pane opens; older ones load on scroll (AC-02).
pub const FIRST_HISTORY: i64 = 1000;

/// What the page's other messages share with its stream.
#[derive(Default)]
pub struct Ctl {
    /// capture now (a key was typed, a resize was made)
    pub wake: Notify,
    paused: AtomicBool,
    resumed: Notify,
    /// the numbering and the last fields, for `more`, `files` and sizing
    pub numbering: Mutex<Numbering>,
    pub fields: Mutex<Option<Fields>>,
    /// the page's history as folds see it (AC-32)
    pub folds: Mutex<Option<super::folds::History>>,
}

impl Ctl {
    pub fn pause(&self, on: bool) {
        self.paused.store(on, Ordering::Relaxed);
        if !on {
            self.resumed.notify_one();
        }
    }
}

pub async fn run(app: Shared, page: Arc<Page>, server: Arc<Server>, pane: String, key: String, ctl: Arc<Ctl>) {
    let (name, theme) = crate::looks::theme_of(&app, &server, &pane).await;
    page.tracer.lock().opening(&key, !server.is_local());
    page.send_json(&json!({"t": "theme", "sid": key, "theme": theme, "name": name}));
    let mut wake = server.wake(&pane);
    let mut shown = Shown::default();
    // the last frame's rows, cursor columns and encodings: an unchanged row is not encoded again
    let mut encodings: Vec<(rows::Row, Option<usize>, String)> = Vec::new();
    let mut last_folds: Option<(Vec<String>, Option<usize>, bool, serde_json::Value)> = None;
    let mut agent_dir = app.hooks.cwd(&key);   // the folder its coding agent works in, as last told (AC-51)
    let mut generation = u64::MAX;
    let mut said_lost = false;
    let mut server_pid: Option<u32> = None;
    let mut stale = false;              // control clients changed or screens were off: everything is sent anew
    loop {
        if page.is_closed() {
            return;
        }
        if ctl.paused.load(Ordering::Relaxed) {
            ctl.resumed.notified().await;
            continue;
        }
        let Some(client) = server.client(&pane) else {
            if !said_lost {
                let msg = if server.is_local() { "tmux on this Mac is not connected".to_string() } else { format!("Connection to {} lost", server.name) };
                page.send_json(&json!({"t": "error", "msg": msg, "sid": key}));
                said_lost = true;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        };
        if said_lost {
            page.send_json(&json!({"t": "error", "msg": "", "sid": key}));
            said_lost = false;
        }
        let g = server.generation.load(Ordering::Relaxed);
        if g != generation {
            // new control clients (a restart, a reconnect): everything is sent anew
            generation = g;
            stale = true;
        }
        // the terminal hidden behind Files or View (once opened): its fields only, for the folder
        let hidden = page.state.lock().screens_off && ctl.fields.lock().is_some();
        let started = Instant::now();
        let prev_cursor = ctl.fields.lock().as_ref().map(|f| (f.cx, f.cy));
        let mut cmds = vec![format!("display -p -t {pane} '{FIELDS}'")];
        if !hidden {
            cmds.push(format!("capture-pane -p -e -J -t {pane}"));
        }
        let answers: Result<Vec<Vec<String>>, String> = client.run_all(cmds).await.into_iter().collect();
        let Ok(answers) = answers else {
            let e = answers.err().unwrap_or_default();
            if e.contains("can't find") || e.contains("no such") {
                page.send_json(&json!({"t": "error", "msg": CLOSED, "sid": key}));
                tracing::info!(event = "pane.closed", key, error = e);
                return;
            }
            tracing::warn!(event = "tmux.error", server = %server.name, cmd = "capture-pane", msg = e);
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        };
        let Some(f) = answers[0].first().and_then(|l| Fields::parse(l)) else {
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        };
        if *server_pid.get_or_insert(f.server_pid) != f.server_pid {
            // the server was started anew: this pane id names another pane now (AC-14)
            page.send_json(&json!({"t": "error", "msg": CLOSED, "sid": key}));
            tracing::info!(event = "pane.closed", key, why = "tmux server restarted");
            return;
        }
        if hidden {
            stale = true;
            follow(&app, &page, &server, &key, &ctl, f, &mut agent_dir).await;
            tokio::select! {
                _ = ctl.wake.notified() => {}
                _ = tokio::time::sleep(POLL) => {}
            }
            continue;
        }
        if std::mem::take(&mut stale) {
            ctl.numbering.lock().forget();
            shown.clear();
        }
        let capture = &answers[1];
        let opened = ctl.fields.lock().is_none();
        if opened {
            // a pane just opened: the active page's grid first, then its first frame at it, never
            // one at the window's old size (AC-04, AC-13)
            *ctl.fields.lock() = Some(f.clone());
            if super::sizing::opening(&app, &page).await {
                continue;
            }
        }
        super::timing::captured(&page, started);
        let screen = rows::screen_rows(&capture.join("\n"), f.width, f.height);
        let texts: Vec<String> = screen.iter().map(rows::row_text).collect();
        let step = ctl.numbering.lock().step(f.history, f.width, &texts, f.alt, f.limit);
        if !matches!(step, Step::Same) {
            // history captures (up to thousands of rows) go on a local server's bulk client, beside
            // this pane's typing; a host's bulk client is its main session's, so there the pane's own
            let bulk = server.is_local().then(|| server.bulk()).flatten();
            let hist = bulk.as_deref().unwrap_or(&*client);
            match step {
                Step::Reset => {
                    shown.clear();
                    super::hist::reset(&app, &page, hist, &ctl, &key, &pane, &f).await;
                }
                Step::Append { from, to } => super::hist::append(&app, &page, hist, &ctl, &key, &pane, &f, from, to).await,
                Step::Same => {}
            }
        }
        let cursor = (f.cursor_on && f.cy < screen.len()).then_some(f.cy);
        // a full-screen program's rows never fold (a coding agent's own alternate screen is an
        // agent's); worked out again only when the screen changed
        let hard: Vec<bool> = screen.iter().map(|r| r.hard).collect();
        let full_screen = f.alt && crate::tmux::titles::agent_of(&f.command).is_none();
        if last_folds.as_ref().is_none_or(|(t, c, a, _)| *t != texts || *c != cursor || *a != full_screen) {
            last_folds = Some((texts.clone(), cursor, full_screen, super::folds::on_screen(&texts, &hard, cursor, full_screen)));
        }
        let folds = last_folds.as_ref().map(|l| l.3.clone()).unwrap_or_default();
        let cols: Vec<Option<usize>> = (0..screen.len()).map(|i| (cursor == Some(i)).then_some(f.cx)).collect();
        let encoded: Vec<String> = screen.iter().zip(&cols).enumerate().map(|(i, (r, &c))| match encodings.get(i) {
            Some((was, at, e)) if was == r && *at == c => e.clone(),
            _ => rows::encode_str(r, c),
        }).collect();
        encodings = screen.into_iter().zip(cols).zip(encoded.iter().cloned()).map(|((r, c), e)| (r, c, e)).collect();
        let moved = prev_cursor != Some((f.cx, f.cy)) || shown.changed(f.cy, &encoded);
        let extra = super::timing::frame(&page, moved);
        if let Some(msg) = shown.frame(&key, &f, encoded, folds, extra)
            && !page.send(msg) {
                return;
            }
        follow(&app, &page, &server, &key, &ctl, f, &mut agent_dir).await;
        if opened {
            super::sizing::activated(&app, &page);       // its grid came after "sub" (or not yet): sized as it comes
        }
        let below: Vec<(&str, bool)> = if full_screen { Vec::new() } else { texts.iter().map(String::as_str).zip(hard.iter().copied()).collect() };
        super::hist::scan_folds(&page, &ctl, &key, &below);
        let last = Instant::now();
        // wait for output, a key, or the poll; then keep frames at most 30 a second
        let how = tokio::select! {
            _ = wake.notified() => "output",
            _ = ctl.wake.notified() => "key",
            _ = tokio::time::sleep(POLL) => "poll",
        };
        let notice = tokio::time::Instant::now();
        super::timing::woken(&page, how);
        let since = last.elapsed();
        if since < FRAME_GAP {
            tokio::time::sleep(FRAME_GAP - since).await;
        }
        // then quiet, output that came while it slept too (its wake kept): never 33 ms past the notice
        if how == "output" {
            settle(&mut wake, notice + FRAME_GAP).await;
        }
    }
}

/// Keeps the pane's last fields; when its folder changed (a cd, by Enter, a hot key or a script),
/// or its agent's, Files follows it (AC-51).
async fn follow(app: &Shared, page: &Page, server: &Arc<Server>, key: &str, ctl: &Ctl, f: Fields, agent_dir: &mut Option<String>) {
    let agent_now = app.hooks.cwd(key);
    let agent_moved = agent_now != *agent_dir;
    *agent_dir = agent_now;
    let moved_to = ctl.fields.lock().as_ref().filter(|old| old.path != f.path || agent_moved).map(|_| f.path.clone());
    *ctl.fields.lock() = Some(f);
    if let Some(cwd) = moved_to {
        page.send_json(&super::client::files_message(app, server, key, &cwd).await);
    }
}

/// Until the pane's output has been quiet SETTLE, or `until`.
async fn settle(wake: &mut Wake, until: tokio::time::Instant) {
    loop {
        let quiet = SETTLE.min(until.saturating_duration_since(tokio::time::Instant::now()));
        if quiet.is_zero() || tokio::time::timeout(quiet, wake.notified()).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Instant;

    fn pane() -> (Arc<Server>, Wake) {
        let link = crate::tmux::server::Link::Local { tmux: "tmux".into(), socket: "tm-test".into(), conf: "/dev/null".into() };
        let server = Server::new("local", "tm-test", link, Arc::new(Notify::new()));
        let wake = server.wake("%1");
        (server, wake)
    }

    #[tokio::test(start_paused = true)]
    async fn a_write_in_parts_is_waited_for_never_past_a_frame() {
        let (server, mut wake) = pane();
        let t0 = Instant::now();
        settle(&mut wake, t0 + FRAME_GAP).await;
        assert_eq!(t0.elapsed(), SETTLE, "no more output: the quiet alone");

        let parts = server.clone();
        tokio::spawn(async move {
            for _ in 0..3 {
                tokio::time::sleep(Duration::from_millis(3)).await;
                parts.woke(Some("%1"));
            }
        });
        let t0 = Instant::now();
        settle(&mut wake, t0 + FRAME_GAP).await;
        assert_eq!(t0.elapsed(), Duration::from_millis(9) + SETTLE, "each part 3 ms after the last: quiet after the third");

        server.woke(Some("%1"));                            // output while the frame gap was slept
        let t0 = Instant::now();
        settle(&mut wake, t0 + FRAME_GAP).await;
        assert_eq!(t0.elapsed(), SETTLE, "that output kept: quiet after it");

        let stream = server.clone();
        let busy = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(1)).await;
                stream.woke(Some("%1"));
            }
        });
        let t0 = Instant::now();
        settle(&mut wake, t0 + FRAME_GAP).await;
        busy.abort();
        assert_eq!(t0.elapsed(), FRAME_GAP, "output that never stops: a frame at the gap");
    }
}
