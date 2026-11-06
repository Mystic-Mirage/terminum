// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Room in web-bookmarks.json (AC-33, AC-91): the user's own bookmarks and open reminders have
//! limits of their own and are refused beyond them, never deleted for room; a resolved reminder
//! counts in no limit and is deleted for good 30 days after it was resolved. Agents' records
//! make their own room (agentmarks.rs).

use super::{err, Item, MarkError, Store, MAX_ITEMS, MAX_REMINDERS};
use crate::app::Shared;

/// How long a resolved reminder is kept.
pub const RESOLVED_KEEP_S: i64 = 30 * 86400;

/// Whether `i` counts as an open reminder (one resolved counts in no limit).
fn open_reminder(i: &Item) -> bool {
    i.remind && i.resolved.is_none()
}

impl Store {
    /// Room for the user's `bookmarks` new bookmarks and `reminders` new reminders.
    pub fn users_room(&self, bookmarks: usize, reminders: usize) -> Result<(), MarkError> {
        let users = || self.items.iter().filter(|i| i.by.is_none());
        if bookmarks > 0 && users().filter(|i| !i.remind).count() + bookmarks > MAX_ITEMS {
            return err(format!("{MAX_ITEMS} bookmarks are kept at most: delete some first (a session's bookmarks' button, or Bookmarks / Reminders below the sessions)."));
        }
        if reminders > 0 && users().filter(|i| open_reminder(i)).count() + reminders > MAX_REMINDERS {
            return err(format!("{MAX_REMINDERS} open reminders are kept at most: resolve or delete some first (a session's reminders' button, or Bookmarks / Reminders below the sessions)."));
        }
        Ok(())
    }

    /// Reminders resolved more than RESOLVED_KEEP_S before `now` deleted, saved; their ids.
    pub fn purge_resolved(&mut self, now: i64) -> Result<Vec<String>, MarkError> {
        let old = |i: &Item| i.remind && i.resolved.is_some_and(|r| now - r > RESOLVED_KEEP_S);
        let gone: Vec<String> = self.items.iter().filter(|i| old(i)).map(|i| i.id.clone()).collect();
        if !gone.is_empty() {
            self.replace_all(&gone, Vec::new())?;
        }
        Ok(gone)
    }
}

/// `purge_resolved` at start and once a day after, every page told of those gone.
pub async fn purge_daily(app: Shared) {
    loop {
        let a = app.clone();
        let now = crate::auth::store::unix_now() as i64;
        match tokio::task::spawn_blocking(move || a.bookmarks.lock().purge_resolved(now)).await {
            Ok(Ok(gone)) => {
                for id in &gone {
                    super::super::marks::tell_unmark(&app, id);
                }
                if !gone.is_empty() {
                    tracing::info!(event = "reminders.purged", removed = gone.len());
                }
            }
            Ok(Err(e)) => tracing::warn!(event = "reminders.unsaved", error = %e),
            Err(_) => {}
        }
        tokio::time::sleep(std::time::Duration::from_secs(86400)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, remind: bool, resolved: Option<i64>) -> Item {
        Item { id: id.into(), remind, resolved, ..Item::default() }
    }

    #[test]
    fn resolved_reminders_go_after_30_days_and_count_in_no_limit() {
        let now = 100 * 86400;
        let mut s = Store::memory();
        s.items = vec![item("old", true, Some(now - RESOLVED_KEEP_S - 1)), item("recent", true, Some(now - RESOLVED_KEEP_S + 60)),
                       item("open", true, None), item("bookmark", false, None)];
        assert_eq!(s.purge_resolved(now).unwrap(), ["old"]);
        assert_eq!(s.all().iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["recent", "open", "bookmark"]);
        assert!(s.purge_resolved(now).unwrap().is_empty());
        // resolved ones never fill the user's open reminders
        s.items = (0..MAX_REMINDERS).map(|k| item(&k.to_string(), true, Some(now))).collect();
        assert!(s.users_room(0, 1).is_ok());
    }

    #[test]
    fn the_users_kinds_have_limits_of_their_own() {
        let mut s = Store::memory();
        s.items = (0..MAX_REMINDERS).map(|k| item(&format!("r{k}"), true, None)).collect();
        s.items.push(item("b", false, None));
        assert!(s.users_room(1, 0).is_ok(), "a bookmark still fits");
        assert!(s.users_room(0, 1).unwrap_err().to_string().contains("100 open reminders"));
        assert!(s.set_note("b", "n".into(), Some(true)).unwrap_err().to_string().contains("100 open reminders"), "made a reminder: no room");
        assert!(s.set_note("b", "n".into(), Some(false)).is_ok(), "the same kind needs no room");
    }
}
