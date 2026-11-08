// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The input log's one writer (terminum.md AC-147): rows come over a bounded channel and go into
//! the tasks' store in a transaction a batch, on a thread of its own, never on the async workers
//! or the typing path; it also drops rows older than KEEP_DAYS and beyond KEEP_ROWS.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::Transaction;

use super::Row;
use crate::tasks::store::Store;

/// Rows kept: those of the last KEEP_DAYS, at most KEEP_ROWS.
pub const KEEP_DAYS: i64 = 180;
pub const KEEP_ROWS: i64 = 500_000;
/// How long a batch gathers, and how often old rows go.
const GATHER: Duration = Duration::from_millis(200);
const TRIM_EVERY: Duration = Duration::from_secs(3600);

/// Writes what `rx` brings until every sender is gone; `dropped` counts rows the store refused.
pub fn run(store: Arc<Store>, rx: Receiver<Row>, dropped: Arc<AtomicU64>) {
    let mut trimmed: Option<Instant> = None;
    loop {
        let first = match rx.recv_timeout(TRIM_EVERY) {
            Ok(r) => Some(r),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let mut batch: Vec<Row> = first.into_iter().collect();
        let until = Instant::now() + GATHER;
        while let Some(left) = until.checked_duration_since(Instant::now()).filter(|_| !batch.is_empty()) {
            match rx.recv_timeout(left) {
                Ok(r) => batch.push(r),
                Err(_) => break,
            }
        }
        let trim = trimmed.is_none_or(|t| t.elapsed() >= TRIM_EVERY);
        let n = batch.len();
        match store.with(|tx| {
            insert(tx, &batch)?;
            if trim {
                self::trim(tx, super::now_ms(), KEEP_ROWS)?;
            }
            Ok(())
        }) {
            Ok(()) if trim => trimmed = Some(Instant::now()),
            Ok(()) => {}
            Err(e) => {
                let lost = dropped.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
                if n > 0 {
                    tracing::warn!(event = "inputs.unsaved", n, lost, error = %e);
                }
            }
        }
    }
}

pub fn insert(tx: &Transaction, rows: &[Row]) -> Result<(), String> {
    let mut st = tx.prepare_cached("INSERT INTO inputs (at, ended, sender, component, kind, text, bytes, pane, machine, session, window, program, cwd, project, project_name, run, tasks, agent, page, device, ip, browser, tailnet_user, version, error) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)").map_err(sql)?;
    for r in rows {
        let w = &r.who;
        st.execute(rusqlite::params![r.at, r.ended, w.sender, w.component, r.kind, r.text, r.bytes as i64, r.pane, r.place.machine, r.place.session, r.place.window, r.place.program, r.place.cwd,
            r.place.project, r.place.project_name, w.run, w.tasks, w.agent, w.page.map(|p| p as i64), w.device, w.ip, w.browser, w.tailnet_user, env!("CARGO_PKG_VERSION"), r.error]).map_err(sql)?;
    }
    Ok(())
}

/// Rows older than KEEP_DAYS, then the oldest beyond `rows`, go.
pub fn trim(tx: &Transaction, now_ms: i64, rows: i64) -> Result<(), String> {
    tx.execute("DELETE FROM inputs WHERE at < ?1", [now_ms - KEEP_DAYS * 86_400_000]).map_err(sql)?;
    tx.execute("DELETE FROM inputs WHERE id <= (SELECT id FROM inputs ORDER BY id DESC LIMIT 1 OFFSET ?1)", [rows]).map_err(sql)?;
    Ok(())
}

fn sql(e: rusqlite::Error) -> String {
    format!("The input log: {e}")
}
