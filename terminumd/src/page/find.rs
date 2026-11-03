// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Reading the shown pane's whole loadable history for search (AC-34) and bookmarks
//! (AC-33): the rows the page could load and the screen, numbered as the page numbers them,
//! read on the bulk client so typing goes on meanwhile.

use std::sync::Arc;

use serde_json::Value;

use super::stream::Ctl;
use super::Page;
use crate::app::Shared;
use crate::screen::rows;
use crate::tmux::Server;

/// (row, text, hard end) of rows `from..to` (absolute) of the shown pane, or why not.
pub async fn rows_of(server: &Server, pane: &str, ctl: &Ctl, keep: i64, around: Option<i64>) -> Result<Vec<(i64, String, bool)>, String> {
    let (top, history, screen, oldest, width, height, limit) = {
        let n = ctl.numbering.lock();
        let f = ctl.fields.lock();
        let (Some(top), Some(f)) = (n.top(), f.as_ref()) else { return Err("the pane is not shown yet".into()) };
        (top, n.history(), n.screen().to_vec(), n.oldest(keep), f.width, f.height as i64, f.limit)
    };
    let (mut first, mut end) = (oldest, top + height);
    if let Some(a) = around {
        (first, end) = super::bookmarks::near(first, end, a);
    }
    if first >= end {
        return Ok(Vec::new());
    }
    let client = server.bulk().ok_or("tmux is not connected")?;
    // numbered as the page's history is, against output that goes on meanwhile (AC-02)
    let rows = super::hist::rows_at(&client, pane, top, history, limit, &screen, first, end, width).await?;
    tokio::task::spawn_blocking(move || {
        rows.into_iter().enumerate().map(|(k, r)| (first + k as i64, r.as_ref().map(rows::row_text).unwrap_or_default(), r.is_none_or(|r| r.hard))).collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// The shown pane's key, server, pane id and stream controls.
pub fn shown(page: &Page) -> Option<(String, Arc<Server>, String, Arc<Ctl>)> {
    let st = page.state.lock();
    st.shown.as_ref().map(|s| (s.key.clone(), s.server.clone(), s.pane.clone(), s.ctl.clone()))
}

/// `find {ref, q}` → `found` (AC-34); reading takes a while, so it runs beside typing.
pub fn start(app: &Shared, page: &Arc<Page>, msg: &Value) {
    let r = super::search::ref_of(msg);
    let q = match super::search::query_of(msg) {
        Ok(q) => q,
        Err(e) => {
            page.send_json(&super::search::refused(&r, &e));
            return;
        }
    };
    let Some((key, server, pane, ctl)) = shown(page) else {
        page.send_json(&super::search::refused(&r, "No session is shown."));
        return;
    };
    let (keep, page) = (app.cfg.history, page.clone());
    tokio::spawn(async move {
        let t0 = std::time::Instant::now();
        match rows_of(&server, &pane, &ctl, keep, None).await {
            Ok(rows) => {
                let lines = rows.len();
                let Ok(((hits, total), q)) = tokio::task::spawn_blocking(move || (super::search::find(&rows, &q), q)).await else { return };
                tracing::info!(event = "search", sid = %key, lines, matches = total, ms = t0.elapsed().as_millis() as u64);
                page.send_json(&super::search::found(&r, &key, &q, &hits, total));
            }
            Err(e) => {
                page.send_json(&super::search::refused(&r, &format!("The history could not be read: {e}")));
            }
        }
    });
}
