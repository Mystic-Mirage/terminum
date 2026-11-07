// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Bookmarks from the page (AC-33): `mark` (new {ref, sid, n, text, note, remind}; changed
//! {ref, id, note, remind}), `unmark`, `locate {id}` (or `{id, text, n?}`: a sent message's text,
//! AC-90), `remind {id, action: close|resolve}` (AC-91); the list goes to a page with its hello. Kept on the Mac in web-bookmarks.json and
//! told to every signed-in page at once; a bookmark is found again by its text.

use std::sync::Arc;

use serde_json::{json, Value};

use super::bookmarks::{self as bm, NewMark};
use super::Page;
use crate::app::Shared;

fn all(app: &Shared) -> Value {
    let store = app.bookmarks.lock();
    // `most`: the characters a bookmark keeps, so the page shows what is kept (AC-33)
    json!({"t": "marks", "items": store.all().iter().map(bm::Item::wire).collect::<Vec<_>>(), "error": store.broken(), "most": bm::MAX_TEXT})
}

/// What a page learns when it connects.
pub fn hello(app: &Shared, page: &Page) {
    page.send_json(&all(app));
}

pub async fn handle(app: &Shared, page: &Arc<Page>, t: &str, msg: &Value) {
    match t {
        "unmark" => {
            let id = msg["id"].as_str().unwrap_or("").to_string();
            match app.bookmarks.lock().remove(&id) {
                Ok(true) => tell_unmark(app, &id),
                Ok(false) => {}
                Err(e) => super::client::error(page, e.to_string(), None),
            }
        }
        "remind" => {
            // a reminder closed or resolved (AC-91)
            let (id, resolve) = (msg["id"].as_str().unwrap_or(""), msg["action"] == "resolve");
            match app.bookmarks.lock().settle(id, resolve) {
                Ok(item) => tell_mark(app, &item),
                Err(e) => super::client::error(page, e.to_string(), None),
            }
        }
        "locate" => {
            let (app, page, id) = (app.clone(), page.clone(), msg["id"].as_str().unwrap_or("").chars().take(60).collect::<String>());
            match msg["text"].as_str() {
                // a sent message (AC-90): its text in the shown pane, nearest its line, else nearest the end
                Some(text) => {
                    let (text, hint, prompt) = (crate::sent::findable(text).chars().take(bm::PRINT * 4).collect::<String>(), msg["n"].as_i64(), msg["prompt"] == true);
                    tokio::spawn(async move { locate_text(&app, &page, &id, &text, hint, prompt).await });
                }
                None => { tokio::spawn(async move { locate(&app, &page, &id).await }); }
            }
        }
        _ => mark(app, page, msg),
    }
}

/// A new bookmark, or a changed note; a save sent again after no answer came is kept once.
fn mark(app: &Shared, page: &Page, msg: &Value) {
    let r: String = msg["ref"].as_str().map(|s| s.chars().take(40).collect()).unwrap_or_default();
    if let Some(id) = app.bookmarks.lock().ref_id(&r).map(str::to_string) {
        page.send_json(&json!({"t": "marked", "ref": r, "id": id}));
        return;
    }
    let result = if let Some(id) = msg["id"].as_str().filter(|s| !s.is_empty()) {
        bm::note_of(msg).and_then(|note| app.bookmarks.lock().set_note(id, note, msg["remind"].as_bool()))
    } else {
        new_mark(app, page, msg)
    };
    match result {
        Ok(item) => {
            app.bookmarks.lock().remember(&r, &item.id);
            page.send_json(&json!({"t": "marked", "ref": r, "id": item.id}));
            tell_mark(app, &item);
        }
        Err(e) => {
            tracing::warn!(event = "bookmark.refused", reason = %e);
            page.send_json(&json!({"t": "marked", "ref": r, "error": e.to_string()}));
        }
    }
}

fn new_mark(app: &Shared, page: &Page, msg: &Value) -> Result<bm::Item, bm::MarkError> {
    // `alt`: the pane shows a full-screen program's own screen now, which tmux keeps no history of
    let (key, alt) = page.state.lock().shown.as_ref().map(|s| (Some(s.key.clone()), s.ctl.fields.lock().as_ref().is_some_and(|f| f.alt))).unwrap_or((None, false));
    let Some(key) = key.filter(|k| msg["sid"].as_str() == Some(k.as_str())) else {
        return Err(bm::MarkError(bm::NOT_SHOWN.into()));
    };
    let (text, note, n) = bm::mark_of(msg)?;
    let item = find_item(&app.layout.read(), &key).cloned();
    let Some(item) = item else { return Err(bm::MarkError(bm::CLOSED.into())) };
    let project = app.projects.lock().of(&key).map(|p| p.id.clone());   // its project's, else its session's
    let new = NewMark {
        key: Some(key),
        created: None,
        n,
        text,
        note,
        title: item["title"].as_str().unwrap_or("").to_string(),
        host: project_of(&item).0,
        remind: msg["remind"] == true,
        dir: project_of(&item).1,
        project,
        alt,
        ..NewMark::default()
    };
    app.bookmarks.lock().add(new)
}

/// The list's row of pane `key`.
pub fn find_item<'a>(layout: &'a Value, key: &str) -> Option<&'a Value> {
    layout_items(layout.as_array()?).find(|it| it["id"] == key)
}

/// Every row of a layout as it is kept.
pub fn layout_rows(layout: &Value) -> impl Iterator<Item = &Value> {
    layout_items(layout.as_array().map_or(&[], |a| a.as_slice()))
}

/// Every row of the layout's groups.
pub fn layout_items(groups: &[Value]) -> impl Iterator<Item = &Value> {
    groups.iter().flat_map(|g| g["items"].as_array().into_iter().flatten())
}

/// A layout row's project (AC-91): its host (none on this Mac) and folder (none when unknown).
pub fn project_of(item: &Value) -> (Option<String>, Option<String>) {
    let s = |k: &str| item[k].as_str().filter(|v| !v.is_empty()).map(str::to_string);
    (s("host"), s("dir"))
}

/// Every page learns of a new or changed bookmark.
pub fn tell_mark(app: &Shared, item: &bm::Item) {
    app.pages.tell(&json!({"t": "mark", "item": item.wire()}).to_string());
}

/// Every page learns a bookmark is gone.
pub fn tell_unmark(app: &Shared, id: &str) {
    app.pages.tell(&json!({"t": "unmark", "id": id}).to_string());
}

/// Where a bookmark's text is now in the shown pane: near its line first, the whole history
/// only when not there (tmux renumbers when it drops lines or the scrollback is cleared).
async fn locate(app: &Shared, page: &Page, id: &str) {
    let item = app.bookmarks.lock().get(id).cloned();
    let Some(item) = item else {
        page.send_json(&bm::unlocated(id, bm::DELETED));
        return;
    };
    if super::find::shown(page).is_none_or(|s| Some(&s.0) != item.key.as_ref()) {
        page.send_json(&bm::unlocated(id, bm::ELSEWHERE));
        return;
    }
    // an agent's record (AC-98) is found by its title, which the agent's transcript of its call shows
    let (text, hint) = if item.by.is_some() { (&item.note, Some(item.n).filter(|n| *n >= 0)) } else { (&item.text, Some(item.n)) };
    let Some(found) = search(app, page, id, text, hint, item.alt).await else { return };
    if let Some(f) = found.filter(|f| f.n != item.n) {
        // where to look first next time; the marks move there
        if let Ok(moved) = app.bookmarks.lock().set_n(id, f.n) {
            tell_mark(app, &moved);
        }
    }
}

/// Where a sent message's `text` is in the shown pane (AC-90): nearest `hint`, its line when it
/// was sent, else nearest the end; a coding agent's prompt (`prompt`) on a row its transcript
/// opens with "❯ " (bookmarks_find.rs find_each), else wherever its words are.
async fn locate_text(app: &Shared, page: &Page, id: &str, text: &str, hint: Option<i64>, prompt: bool) {
    if prompt && let Some((key, server, pane, ctl)) = super::find::shown(page) {
        let read = super::find::rows_of(&server, &pane, &ctl, app.cfg.history, None).await;
        let (text2, size) = (text.to_string(), super::bookmarks_find::loose(text).chars().take(bm::PRINT).count());
        let at = match read {
            Ok(rows) => tokio::task::spawn_blocking(move || {
                let rows: Vec<(i64, String)> = rows.into_iter().map(|(n, t, _)| (n, t)).collect();
                super::bookmarks_find::find_each(&rows, &[super::bookmarks_find::Wanted { text: text2, hint, prompt: true }])[0]
            }).await.unwrap_or(None),
            Err(_) => None,
        };
        if let Some(n) = at {
            page.send_json(&bm::located(id, &key, Some(&super::bookmarks_find::Found { n, start: 0, size })));
            return;
        }
    }
    let _ = search(app, page, id, text, hint, false).await;
}

/// Looks for `text` in the shown pane, near `hint` first, the whole history only when its whole
/// print is not there (tmux renumbers when it drops lines or the scrollback is cleared), and
/// tells the page `located`; None when it could not be read (the page was told why).
async fn search(app: &Shared, page: &Page, id: &str, text: &str, hint: Option<i64>, alt: bool) -> Option<Option<super::bookmarks_find::Found>> {
    let Some((key, server, pane, ctl)) = super::find::shown(page) else {
        page.send_json(&bm::unlocated(id, bm::ELSEWHERE));
        return None;
    };
    let keep = app.cfg.history;
    // the rows read and searched, the search off the async workers (thousands of rows)
    let read = |around: Option<i64>| {
        let (server, pane, ctl, text) = (server.clone(), pane.clone(), ctl.clone(), text.to_string());
        async move {
            let rows: Vec<(i64, String)> = super::find::rows_of(&server, &pane, &ctl, keep, around).await?.into_iter().map(|(n, t, _)| (n, t)).collect();
            tokio::task::spawn_blocking(move || bm::find(&rows, &text, hint.unwrap_or(i64::MAX))).await.map_err(|e| e.to_string())
        }
    };
    let unread = |e: String| page.send_json(&bm::unlocated(id, &format!("The history could not be read: {e}")));
    let mut found = None;
    if let Some(h) = hint {
        match read(Some(h)).await {
            Ok(at) => found = at,
            Err(e) => { unread(e); return None; }
        }
    }
    if bm::needs_whole(found.as_ref(), text) {
        match read(None).await {
            Ok(at) => found = at.or(found),
            Err(e) => { unread(e); return None; }
        }
    }
    let mut told = bm::located(id, &key, found.as_ref());
    if found.is_none() && alt {
        told["why"] = json!(bm::ALT_GONE);   // not "no longer in its history": it never was
    }
    page.send_json(&told);
    Some(found)
}
