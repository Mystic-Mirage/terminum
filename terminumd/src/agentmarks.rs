// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Agents' records of an older terminum (AC-98): agents make tasks now; what they kept stays with
//! the user's bookmarks and reminders (page/bookmarks.rs). Here only the oldest file's notes.

use std::path::Path;

use serde_json::Value;

use crate::page::bookmarks::{NewMark, Store};
use crate::plans::rules;

const ALL_MOST: usize = 500;
const TITLE: usize = 120;
const TEXT: usize = 2000;

fn open(i: &crate::page::bookmarks::Item) -> bool {
    i.resolved.is_none()
}

/// The open notes of 0.23's notes.json in `dir` made agents' records once; the file is kept
/// as notes.json.imported. Errors go to the log: the notes stay where they were.
pub fn import_notes(store: &mut Store, dir: &Path) {
    let path = dir.join("notes.json");
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let notes = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["notes"].as_array().cloned()).unwrap_or_default();
    // a note already kept (an import whose file could not be set aside) is not kept twice
    let kept = |n: &NewMark| store.all().iter().any(|i| i.by.is_some() && i.key == n.key && i.project == n.project && i.note.to_lowercase() == n.note.to_lowercase());
    let room = ALL_MOST.saturating_sub(store.all().iter().filter(|i| i.by.is_some() && open(i)).count());
    let new: Vec<NewMark> = notes.iter().filter(|n| n["status"] == "open" && n["orphaned"] != true).filter_map(|n| {
        let body = rules::clean(n["text"].as_str()?);
        let title: String = body.lines().next().unwrap_or("").chars().take(TITLE).collect();
        (!title.is_empty()).then(|| NewMark {
            key: n["key"].as_str().map(str::to_string), n: -1, note: title, text: body.chars().take(TEXT).collect(),
            project: n["project"].as_str().map(str::to_string), by: Some("agent".into()),
            kind: n["kind"].as_str().map(str::to_string), file: n["file"].as_str().map(str::to_string),
            ..NewMark::default()
        })
    }).filter(|n| !kept(n)).collect();
    if new.len() > room {
        tracing::warn!(event = "notes.import", notes = new.len(), room, "more notes than agents' records may be kept: the newest are left in notes.json.imported");
    }
    let new: Vec<NewMark> = new.into_iter().take(room).collect();
    let n = new.len();
    if n > 0 && let Err(e) = store.add_all(new) {
        tracing::warn!(event = "notes.import", error = %e, "the notes of 0.23 were not imported");
        return;
    }
    match std::fs::rename(&path, dir.join("notes.json.imported")) {
        Ok(()) => tracing::info!(event = "notes.import", notes = n, "the notes of 0.23 are agents' bookmarks now"),
        Err(e) => tracing::warn!(event = "notes.import", error = %e, "notes.json was not set aside"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac98_an_older_notes_file_becomes_records_once() {
        let dir = crate::testutil::fresh_dir("notes-import");
        std::fs::write(dir.join("notes.json"), r#"{"notes": [
            {"status": "open", "text": "Fix the parser\nmore", "key": "tmux:local:t:%1", "kind": "bug"},
            {"status": "done", "text": "finished", "key": "tmux:local:t:%1"},
            {"status": "open", "orphaned": true, "text": "gone", "key": "tmux:local:t:%2"}]}"#).unwrap();
        let mut store = Store::load(dir.join("web-bookmarks.json"));
        import_notes(&mut store, &dir);
        let notes: Vec<String> = store.all().iter().map(|i| i.note.clone()).collect();
        assert_eq!(notes, ["Fix the parser"]);
        assert!(!dir.join("notes.json").exists() && dir.join("notes.json.imported").exists());
        import_notes(&mut store, &dir);
        assert_eq!(store.all().len(), 1, "once");
    }
}
