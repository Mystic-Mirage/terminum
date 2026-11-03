// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A shown pane's history on the page (AC-02, AC-32): the rows sent when it opens (reset),
//! the rows that scroll off (append), older pages on scroll (`more`), and folded rows the
//! page asks to see (`unfold`). Rows come from `capture-pane -S … -E …` ranges, numbered
//! absolutely (screen::history); folded rows are sent as null until asked for.

use std::sync::Arc;

use serde_json::{json, Value};

use super::stream::{Ctl, FIRST_HISTORY};
use super::find::shown;
use super::Page;
use crate::app::Shared;
use crate::screen::frame::Fields;
use crate::screen::rows;
use crate::tmux::control::Control;

/// Rows per `more` (AC-02).
const PAGE: i64 = 1000;
/// Rows one `unfold` may ask for (AC-32).
const UNFOLD_MAX: i64 = 20_000;

/// Rows `from..to` (absolute) of `pane` in the page format; exactly `to - from` of them (tmux
/// joined rows of a range never reach past its end). `top` and `history` are the screen's top
/// and tmux's history_size as the page counts them. tmux counts a capture's rows back from its
/// history's end when it runs it, and the pane may have printed since the frame that numbered
/// them (a burst of output over ssh): so the capture reaches `margin` rows above the range and
/// comes with the history's size read before and after it, and the rows asked for are taken
/// from where those that scrolled meanwhile moved them. Where the count cannot say how far they
/// moved (a history that shrank, or at its `limit`, where tmux drops the oldest tenth at once,
/// and the count may come back to the frame's: then the screen's top rows show whether anything
/// moved; drawn over in place, they are found nowhere and the count holds), the last frame's
/// screen rows (`screen`, hashes, the first at `top`) are found in a capture down to the screen
/// and number the rows. A width that changed (tmux re-wrapped) or rows found nowhere is an
/// error, never rows of another place shown as these (AC-02).
#[allow(clippy::too_many_arguments)]
pub async fn capture(client: &Control, pane: &str, top: i64, history: i64, limit: i64, screen: &[u64], from: i64, to: i64, width: usize) -> Result<Vec<Value>, String> {
    let rows = rows_at(client, pane, top, history, limit, screen, from, to, width).await?;
    Ok(rows.iter().map(|r| r.as_ref().map_or_else(|| json!({"r": [], "e": true}), |r| rows::encode(r, None))).collect())
}

/// `capture`'s rows as parsed, `None` for those tmux no longer has (search reads their text, AC-34).
#[allow(clippy::too_many_arguments)]
pub async fn rows_at(client: &Control, pane: &str, top: i64, history: i64, limit: i64, screen: &[u64], from: i64, to: i64, width: usize) -> Result<Vec<Option<rows::Row>>, String> {
    if from >= to {
        return Ok(Vec::new());
    }
    let probe = format!("display -p -t {pane} '#{{history_size}} #{{pane_width}}'");
    let read = sizes;
    let want = (to - from) as usize;
    let mut margin = MARGIN;
    let full = |h: i64| h >= limit - limit / 10;           // at tmux's limit: trimmed a tenth at a time
    let tops = screen.len().min(ANCHOR_ROWS);
    for _ in 0..3 {
        // offsets as the frame counted them, reaching `margin` rows above the range
        let (s, e) = (from - top - margin, to - top - 1);
        let mut cmds = vec![probe.clone(), format!("capture-pane -p -e -J -t {pane} -S {s} -E {e}"), probe.clone()];
        let check = full(history) && tops > 0;
        if check {
            cmds.push(format!("capture-pane -p -e -J -t {pane} -S 0 -E {}", tops - 1));   // the screen's top rows now
        }
        let mut answers = client.run_all(cmds).await.into_iter();
        let (before, lines, after) = (read(answers.next())?, answers.next().ok_or("no answer")??, read(answers.next())?);
        if before.1 != width || after.1 != width {
            return Err("the pane changed width while its lines were read".into());
        }
        let moved = before.0 - history;                    // rows that scrolled off since the frame
        // the same count at the limit may be a trim and as many rows again: not when the screen's top is the frame's
        let trimmed = moved == 0 && check && full(before.0) && {
            let now = rows::capture_rows(&answers.next().ok_or("no answer")??.join("\n"), width);
            !now.iter().map(|r| crate::screen::history::hash(&rows::row_text(r))).eq(screen[..tops].iter().copied())
        };
        if moved < 0 || (moved > 0 && full(before.0)) {
            return anchored(client, &probe, pane, top, screen, from, to, width).await;
        }
        // found where a trim moved them; found nowhere, the top rows were drawn over in place (a
        // progress bar's redraw), nothing moved, and the count numbers them
        if trimmed && let Ok(rows) = anchored(client, &probe, pane, top, screen, from, to, width).await {
            return Ok(rows);
        }
        if before.0 != after.0 || moved > margin {
            margin = 2 * moved.max(after.0 - history) + MARGIN;   // it went on meanwhile: once more, reaching further
            continue;
        }
        // the capture's first row: s rows back from tmux's end then (never past its oldest)
        let first = top + moved + s.max(-before.0);
        // a search's tens of thousands of rows: parsed off the async workers, beside typing and frames
        let rows = tokio::task::spawn_blocking(move || rows::capture_rows(&lines.join("\n"), width)).await.map_err(|e| e.to_string())?;
        // rows tmux no longer has (its oldest came after `from`) are empty, before those it has
        return Ok(placed(rows, first, from, want));
    }
    Err("the pane's history kept moving while its lines were read".into())
}

/// `want` rows from `from` out of captured `rows` whose first is `first`: rows tmux no longer
/// has (its oldest came after `from`) are `None`, before those it has.
fn placed(rows: Vec<rows::Row>, first: i64, from: i64, want: usize) -> Vec<Option<rows::Row>> {
    let lead = ((first - from).max(0) as usize).min(want);
    let mut out: Vec<Option<rows::Row>> = std::iter::repeat_with(|| None).take(lead).collect();
    out.extend(rows.into_iter().skip((from - first).max(0) as usize).take(want - lead).map(Some));
    out.resize_with(want, || None);
    out
}

/// The pane's history_size and width, as the probe beside a capture said them.
fn sizes(a: Option<crate::tmux::control::Answer>) -> Result<(i64, usize), String> {
    let line = a.ok_or("no answer")??.into_iter().next().unwrap_or_default();
    let mut f = line.split(' ').map(|x| x.trim().parse::<i64>().ok());
    match (f.next().flatten(), f.next().flatten()) {
        (Some(h), Some(w)) => Ok((h, w.max(0) as usize)),
        _ => Err(format!("tmux said {line:?} of the pane's history")),
    }
}

/// Rows a capture reaches above its range for those that scroll off meanwhile (one round trip
/// of output: an agent's result over ssh); a capture that finds more reaches further once more.
const MARGIN: i64 = 64;
/// Rows a capture numbered by the screen's rows reaches above its range (the rows it may find
/// them moved by).
const ANCHOR_MARGIN: i64 = 512;
/// The last frame's top screen rows that must be found as they were (at least two not blank).
const ANCHOR_ROWS: usize = 6;

/// `capture` where tmux's count cannot say how far rows moved: one capture from above the range
/// down to the screen's end; the one move at which the last frame's top screen rows are found as
/// they were numbers the rows. Found at two moves (output that repeats itself) or nowhere, it
/// is an error; found below the capture's reach while tmux still has the rows, it reaches
/// further once more.
#[allow(clippy::too_many_arguments)]
async fn anchored(client: &Control, probe: &str, pane: &str, top: i64, screen: &[u64], from: i64, to: i64, width: usize) -> Result<Vec<Option<rows::Row>>, String> {
    let lost = || Err::<Vec<Option<rows::Row>>, String>("the pane's history was trimmed while its lines were read, and its rows were not found".into());
    let n = screen.len().min(ANCHOR_ROWS);
    if n == 0 {
        return lost();
    }
    let mut margin = ANCHOR_MARGIN;
    for _ in 0..2 {
        let s = from - top - margin;
        let mut answers = client.run_all(vec![probe.to_string(), format!("capture-pane -p -e -J -t {pane} -S {s} -E -"), probe.to_string()]).await.into_iter();
        let (before, lines, after) = (sizes(answers.next())?, answers.next().ok_or("no answer")??, sizes(answers.next())?);
        if before.1 != width || after.1 != width {
            return Err("the pane changed width while its lines were read".into());
        }
        if before != after {
            return lost();
        }
        let rows = rows::capture_rows(&lines.join("\n"), width);
        let texts: Vec<String> = rows.iter().map(rows::row_text).collect();
        let hashes: Vec<u64> = texts.iter().map(|t| crate::screen::history::hash(t)).collect();
        // the screen's first row now, in the capture (it starts s rows back from tmux's end, never past its oldest)
        let now_top = (-s).min(before.0).max(0) as usize;
        let at = |k: usize| {
            let j = now_top - k;
            j + n <= hashes.len() && hashes[j..j + n] == screen[..n] && texts[j..j + n].iter().filter(|t| !t.trim().is_empty()).count() >= 2.min(n)
        };
        let found: Vec<usize> = (0..=now_top).filter(|&k| at(k)).take(2).collect();
        let k = match found[..] {
            [k] => k,
            _ => return lost(),                           // nowhere, or at two moves: not which
        };
        let first = top - (now_top - k) as i64;          // the capture's first row, numbered by the screen's rows
        if first > from && (-s) < before.0 {
            margin += first - from + MARGIN;              // rows tmux still has above the capture's reach
            continue;
        }
        return Ok(placed(rows, first, from, (to - from) as usize));
    }
    lost()
}

#[cfg(test)]
#[path = "hist_tests.rs"]
mod tests;

/// Seconds since terminumd started: the folds' clock.
fn now() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

fn keep(app: &Shared) -> i64 {
    app.cfg.history
}

/// The page's history anew: the last rows before the screen, then (the caller) a full frame.
pub async fn reset(app: &Shared, page: &Page, client: &Control, ctl: &Ctl, key: &str, pane: &str, f: &Fields) {
    let began = std::time::Instant::now();
    let (top, history, screen, oldest, truncated) = {
        let n = ctl.numbering.lock();
        (n.top().unwrap_or(0), n.history(), n.screen().to_vec(), n.oldest(keep(app)), n.truncated(keep(app)))
    };
    let first = oldest.max(top - FIRST_HISTORY);
    let lines = match capture(client, pane, top, history, f.limit, &screen, first, top, f.width).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(event = "tmux.error", cmd = "capture-pane", msg = e);
            ctl.numbering.lock().forget();                   // the next frame starts the page's history anew
            Vec::new()
        }
    };
    // folds are terminumd's (AC-32) unless settings.json says "page": {"folds": false}
    let (lines, items) = if app.looks.settings.read().page.folds {
        let mut folds = super::folds::History::new(keep(app) as usize);
        let taken = folds.take(first, lines, true);
        *ctl.folds.lock() = Some(folds);
        taken
    } else {
        *ctl.folds.lock() = None;
        (lines, Vec::new())
    };
    let mut m = json!({"t": "hist", "mode": "reset", "sid": key, "first": first, "oldest": oldest, "truncated": truncated, "lines": lines});
    if !items.is_empty() {
        m["folds"] = json!(items);
    }
    page.tracer.lock().hist("reset", m["lines"].as_array().map_or(0, Vec::len), began);
    page.send_json(&m);
}

/// Rows `from..to` scrolled off the screen: appended to the page's history.
#[allow(clippy::too_many_arguments)]
pub async fn append(app: &Shared, page: &Page, client: &Control, ctl: &Ctl, key: &str, pane: &str, f: &Fields, from: i64, to: i64) {
    let (top, history, screen, oldest, truncated) = {
        let n = ctl.numbering.lock();
        (n.top().unwrap_or(0), n.history(), n.screen().to_vec(), n.oldest(keep(app)), n.truncated(keep(app)))
    };
    let from = from.max(oldest);
    let began = std::time::Instant::now();
    let lines = match capture(client, pane, top, history, f.limit, &screen, from, to, f.width).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(event = "tmux.error", cmd = "capture-pane", msg = e);
            ctl.numbering.lock().forget();                   // no gap: the next frame starts the page's history anew
            return;
        }
    };
    let lines = match ctl.folds.lock().as_mut() {
        Some(h) => h.take(from, lines, false).0,
        None => lines,
    };
    page.tracer.lock().hist("append", lines.len(), began);
    page.send_json(&json!({"t": "hist", "mode": "append", "sid": key, "first": from, "oldest": oldest, "truncated": truncated, "lines": lines}));
}

/// Folds among the rows that scrolled off, at most every second (AC-32); `screen`: the rows
/// on the screen now, read after them (none for a full-screen program's).
pub fn scan_folds(page: &Page, ctl: &Ctl, key: &str, screen: &[(&str, bool)]) {
    let top = ctl.numbering.lock().top();   // read apart: never two locks held at once
    let found = {
        let mut g = ctl.folds.lock();
        let Some(h) = g.as_mut() else { return };
        let now = now();
        if !h.due(now) {
            return;
        }
        h.scan(now, top, screen)
    };
    if !found.is_empty() {
        page.send_json(&json!({"t": "folds", "sid": key, "items": found}));
    }
}

/// Older rows before `before` (down to `to`, a bookmark's line), never past the oldest
/// the page may load (AC-02, AC-33).
pub async fn older(app: &Shared, page: &Arc<Page>, before: i64, to: Option<i64>) {
    let Some((key, server, pane, ctl)) = shown(page) else { return };
    let Some(client) = server.bulk() else { return };
    let (top, history, screen, oldest, truncated, (width, limit)) = {
        let n = ctl.numbering.lock();
        let Some(top) = n.top() else { return };
        (top, n.history(), n.screen().to_vec(), n.oldest(keep(app)), n.truncated(keep(app)), ctl.fields.lock().as_ref().map_or((80, 0), |f| (f.width, f.limit)))
    };
    let before = before.clamp(oldest, top);
    let first = to.map_or(before - PAGE, |t| t.min(before - PAGE)).max(oldest);
    match capture(&client, &pane, top, history, limit, &screen, first, before, width).await {
        Ok(lines) => {
            let (lines, folds) = match ctl.folds.lock().as_mut() {
                Some(h) => h.take(first, lines, true),
                None => (lines, Vec::new()),
            };
            let mut m = json!({"t": "hist", "mode": "prepend", "sid": key, "first": first, "oldest": oldest, "truncated": truncated, "lines": lines});
            if !folds.is_empty() {
                m["folds"] = json!(folds);
            }
            page.send_json(&m);
        }
        Err(e) => super::client::error(page, format!("Could not load older lines: {e}"), Some(&key)),
    }
}

/// A fold's rows, when the page shows it: only rows of a fold terminumd told of, still in
/// tmux's history, at most 20,000 (AC-32).
pub async fn unfold(app: &Shared, page: &Arc<Page>, a: i64, b: i64) {
    let Some((key, server, pane, ctl)) = shown(page) else { return };
    let Some(client) = server.bulk() else { return };
    let (top, history, screen, oldest, (width, limit)) = {
        let n = ctl.numbering.lock();
        (n.top().unwrap_or(0), n.history(), n.screen().to_vec(), n.oldest(keep(app)), ctl.fields.lock().as_ref().map_or((80, 0), |f| (f.width, f.limit)))
    };
    let a = a.max(oldest);
    let told = ctl.folds.lock().as_ref().is_some_and(|h| h.told(a, b));
    let why = if a > b || b >= top {
        Some("These lines are no longer in this session's history.".to_string())
    } else if !told {
        Some("Not a fold terminumd told of".to_string())
    } else if b - a >= UNFOLD_MAX {
        Some(format!("More than {UNFOLD_MAX} lines: open them with tmux."))
    } else {
        None
    };
    if let Some(why) = why {
        page.send_json(&json!({"t": "lines", "sid": key, "first": a, "lines": [], "error": why}));
        return;
    }
    match capture(&client, &pane, top, history, limit, &screen, a, b + 1, width).await {
        Ok(lines) => {
            page.send_json(&json!({"t": "lines", "sid": key, "first": a, "lines": lines}));
        }
        Err(e) => {
            page.send_json(&json!({"t": "lines", "sid": key, "first": a, "lines": [], "error": e}));
        }
    }
}
