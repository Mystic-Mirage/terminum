// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The banner over the terminal when agents' bookmarks or reminders of a scope went to make room
//! (AC-98): noted by scope and kind when they go, shown on every page when an agent of a pane of
//! that scope ends a turn, kept for pages that connect until a page closes it (not across a
//! restart); one a scope and kind.

use std::collections::HashMap;

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;

use crate::agentmarks::most;
use crate::app::Shared;

#[derive(Default)]
pub struct Full {
    /// (scope, kind: false bookmarks, true reminders) -> the pane that made room (Open shows it)
    pending: Mutex<HashMap<(String, bool), String>>,
    shown: Mutex<Vec<Banner>>,
}

/// One banner: its scope (a project's id, else the pane's key), its kind, the limit, the scope's
/// name as the page says it, and the pane whose turn ended (Open shows it).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Banner {
    pub scope: String,
    pub remind: bool,
    pub most: usize,
    pub label: String,
    pub sid: String,
}

/// Agents' records of `key`'s scope of that kind went for room.
pub fn went(app: &Shared, key: &str, remind: bool) {
    app.marks_full.pending.lock().insert((key.to_string(), remind), key.to_string());
}

/// The panes `ended` ended a turn: their banners show, on every page, a pane named by its row's
/// title now.
pub fn turn_ended(app: &Shared, ended: &[String]) {
    if app.marks_full.pending.lock().is_empty() {
        return;
    }
    let taken: Vec<((String, bool), String)> = {
        let mut p = app.marks_full.pending.lock();
        let keys: Vec<(String, bool)> = p.keys().filter(|(scope, _)| ended.contains(scope)).cloned().collect();
        keys.into_iter().filter_map(|k| p.remove(&k).map(|sid| (k, sid))).collect()
    };
    if taken.is_empty() {
        return;
    }
    let new = {
        let layout = app.layout.read();
        taken.into_iter().map(|((scope, remind), sid)| {
            let title = crate::page::marks::find_item(&layout, &sid).and_then(|it| it["title"].as_str().map(str::to_string));
            let label = format!("session {}", title.filter(|t| !t.is_empty()).unwrap_or_else(|| sid.clone()));
            Banner { scope, remind, most: most(remind), label, sid }
        }).collect()
    };
    if add(&app.marks_full, new) {
        tell(app);
    }
}

/// `new` among those shown, one a scope and kind; whether any was added.
fn add(full: &Full, new: Vec<Banner>) -> bool {
    let mut shown = full.shown.lock();
    let before = shown.len();
    for b in new {
        if !shown.iter().any(|s| s.scope == b.scope && s.remind == b.remind) {
            shown.push(b);
        }
    }
    shown.len() != before
}

fn message(full: &Full) -> String {
    json!({"t": "marksfull", "items": *full.shown.lock()}).to_string()
}

fn tell(app: &Shared) {
    app.pages.tell(&message(&app.marks_full));
}

/// A page that connects gets those shown.
pub fn hello(app: &Shared, page: &crate::page::Page) {
    if !app.marks_full.shown.lock().is_empty() {
        let _ = page.send(message(&app.marks_full));
    }
}

/// A page closed one: gone from every page.
pub fn seen(app: &Shared, scope: &str, remind: bool) {
    let gone = {
        let mut shown = app.marks_full.shown.lock();
        let before = shown.len();
        shown.retain(|b| !(b.scope == scope && b.remind == remind));
        shown.len() != before
    };
    if gone {
        tell(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banner(scope: &str, remind: bool) -> Banner {
        Banner { scope: scope.into(), remind, most: most(remind), label: "p".into(), sid: "k".into() }
    }

    #[test]
    fn one_banner_a_scope_and_kind_until_closed() {
        let full = Full::default();
        assert!(add(&full, vec![banner("p1", false), banner("p1", false)]));
        assert_eq!(full.shown.lock().len(), 1);
        assert!(!add(&full, vec![banner("p1", false)]), "a deletion while it shows changes nothing");
        assert!(add(&full, vec![banner("p1", true), banner("p2", false)]));
        assert_eq!(full.shown.lock().len(), 3);
        let v: serde_json::Value = serde_json::from_str(&message(&full)).unwrap();
        assert_eq!((v["t"].as_str(), v["items"][1]["remind"].as_bool(), v["items"][1]["most"].as_u64()), (Some("marksfull"), Some(true), Some(20)));
    }

    #[tokio::test]
    async fn noted_room_shows_at_a_turn_end_of_its_scope_until_a_page_closes_it() {
        let (root, app) = crate::testutil::app("marksfull");
        let (a, b) = ("tmux:local:tm-test:%1", "tmux:local:tm-test:%2");
        went(&app, a, true);
        turn_ended(&app, &[b.to_string()]);
        assert!(app.marks_full.shown.lock().is_empty(), "another session's turn end shows nothing");
        turn_ended(&app, &[a.to_string()]);
        let shown = app.marks_full.shown.lock().clone();
        assert_eq!(shown, [Banner { scope: a.into(), remind: true, most: most(true), label: format!("session {a}"), sid: a.into() }]);
        turn_ended(&app, &[a.to_string()]);
        assert_eq!(app.marks_full.shown.lock().len(), 1, "shown once: nothing pending");
        seen(&app, a, false);
        assert_eq!(app.marks_full.shown.lock().len(), 1, "the other kind is not this one");
        seen(&app, a, true);
        assert!(app.marks_full.shown.lock().is_empty());
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
