// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Bookmarks of sessions that closed, as a server's list says (AC-33).

use crate::app::Shared;

/// The bookmarks whose session `closed` says closed are orphaned (AC-33): saved off the async
/// workers when one is (named again there, under the lock), every
/// page told.
pub(super) async fn orphan(app: &Shared, closed: impl Fn(&str) -> bool + Send + 'static) {
    if super::bookmarks::lost(app.bookmarks.lock().all(), &closed).is_empty() {
        return;
    }
    let a = app.clone();
    told(app, tokio::task::spawn_blocking(move || {
        let mut store = a.bookmarks.lock();
        let ids = super::bookmarks::lost(store.all(), &closed);
        store.orphan(&ids)
    }).await);
}

/// Every page learns of the bookmarks a change made; one not saved is logged.
pub(super) fn told(app: &Shared, done: Result<Result<Vec<super::bookmarks::Item>, super::bookmarks::MarkError>, tokio::task::JoinError>) {
    match done {
        Ok(Ok(items)) => items.iter().for_each(|i| super::marks::tell_mark(app, i)),
        Ok(Err(e)) => tracing::warn!(event = "bookmarks.unsaved", error = %e),
        Err(e) => tracing::warn!(event = "bookmarks.unsaved", error = %e),
    }
}
