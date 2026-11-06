// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Where a bookmark shows (AC-33, AC-91): in the session it was made in alone, in a project
//! (AC-92) or not; the project it was made in names it in the list below the sessions. Once that session closed it is orphaned (projects.rs says when), shown
//! only in that list; a restored server's panes keep theirs. A file before version 3 scoped by
//! folder: its bookmarks take the project their session is in now, once.

use std::collections::HashMap;


use super::{Item, MarkError, Store};
use crate::app::Shared;


impl Store {
    /// A file before version 3: each bookmark takes the project its session is in now
    /// (`projects`: pane key -> project id), else stays its session's; saved.
    pub fn take_projects(&mut self, projects: &HashMap<String, String>) -> Result<(), MarkError> {
        if !self.legacy {
            return Ok(());
        }
        for i in self.items.iter_mut() {
            i.project = i.key.as_ref().and_then(|k| projects.get(k)).cloned();
        }
        self.save()?;
        self.legacy = false;
        Ok(())
    }

    /// A file before version 4: a project's record showed in every session of it, so it kept the
    /// key of a pane gone, which a pane may have taken since. Each whose pane is not a member of
    /// its project (`projects`: pane key -> project id) is orphaned, once: it stays in the list below
    /// the sessions, never in a stranger's session; saved.
    pub fn take_sessions(&mut self, projects: &HashMap<String, String>) -> Result<(), MarkError> {
        if !self.unscoped {
            return Ok(());
        }
        let ids: Vec<String> = self.items.iter()
            .filter(|i| i.orphaned.is_none() && i.project.is_some() && i.key.as_ref().and_then(|k| projects.get(k)) != i.project.as_ref())
            .map(|i| i.id.clone()).collect();
        self.orphan(&ids)?;
        self.save()?;
        self.unscoped = false;
        Ok(())
    }

    /// Bookmarks `ids` orphaned now, saved; those it changed.
    pub fn orphan(&mut self, ids: &[String]) -> Result<Vec<Item>, MarkError> {
        let now = crate::auth::store::unix_now() as i64;
        self.edit_all(|i| {
            let gone = i.orphaned.is_none() && ids.contains(&i.id);
            if gone {
                i.orphaned = Some(now);
            }
            gone
        })
    }

    /// The server of `prefix` was made again from its checkpoint (AC-15): its bookmarks take
    /// their panes' new keys (`panes`: old id, new id); those of a pane not made again are
    /// orphaned: a restored pane may take its id. An orphaned one keeps its key.
    pub fn restarted(&mut self, prefix: &str, panes: &[(String, String)]) -> Result<Vec<Item>, MarkError> {
        let now = crate::auth::store::unix_now() as i64;
        self.edit_all(|i| {
            let Some(old) = i.key.as_deref().and_then(|k| k.strip_prefix(prefix)).filter(|_| i.orphaned.is_none()) else { return false };
            match panes.iter().find(|(o, _)| o == old) {
                Some((_, new)) if new != old => { i.key = Some(format!("{prefix}{new}")); true }
                Some(_) => false,
                None => { i.orphaned = Some(now); true }
            }
        })
    }
}

/// At start: the bookmarks of a file before version 3 take their sessions' projects; those of a
/// file before version 4 whose pane is not of their project are orphaned.
pub fn adopt(app: &Shared) {
    let members: HashMap<String, String> = app.projects.lock().list().iter()
        .flat_map(|p| p.members.iter().map(|m| (m.clone(), p.id.clone()))).collect();
    let mut store = app.bookmarks.lock();
    if let Err(e) = store.take_projects(&members).and_then(|()| store.take_sessions(&members)) {
        tracing::warn!(event = "bookmarks.unsaved", error = %e);
    }
}

/// Which of `items` lost their session, one `closed` says closed, whatever their project; those
/// orphaned already are not named again.
pub fn lost(items: &[Item], closed: impl Fn(&str) -> bool) -> Vec<String> {
    items.iter().filter(|i| i.orphaned.is_none() && i.key.as_deref().is_some_and(&closed)).map(|i| i.id.clone()).collect()
}
