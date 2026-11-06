// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What an agent's scope holds beyond its session (AC-98): its project's other sessions' records of
//! agents, on its own host: read, closed and judged for repeats, nothing else. Membership decides for
//! a live pane, never an old stamp; the host decides always, as a project may mix hosts and a host's
//! link reaches only that host's server. The user's records of other sessions are in no scope.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::agentmarks::Place;
use crate::page::bookmarks::Item;
use crate::plans::rules;
use crate::tmux::parse_key;

/// A session's title in an answer, in characters: an agent reads it as data.
const SESSION: usize = 40;
/// A like title: words in common, and their least share of the larger title's, in percent.
const LIKE_WORDS: usize = 3;
const LIKE_SHARE: usize = 60;

fn same_host(a: &str, b: &str) -> bool {
    matches!((parse_key(a), parse_key(b)), (Some((x, ..)), Some((y, ..))) if x == y)
}

/// Whether `i` is an agent's record of another session in `p`'s scope: a member pane's, or orphaned
/// with its project's stamp. A pane in no project has none.
pub fn of_other(i: &Item, p: &Place) -> bool {
    let (Some(project), Some(key)) = (p.project.as_deref(), i.key.as_deref()) else { return false };
    i.by.is_some() && same_host(key, &p.key) && match i.orphaned {
        Some(_) => i.project.as_deref() == Some(project),
        None => key != p.key && p.members.iter().any(|m| m == key),
    }
}

/// A title as it is compared: letter case, runs of spaces and end punctuation aside. Only the end's:
/// inside a title `C++` and `C#`, `v0.30` and `v0.3.0` are different things.
fn compared(title: &str) -> String {
    let t = title.to_lowercase();
    let words = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut = words.trim_end_matches(['.', ',', ';', ':', '!', '?', '…']);
    if cut.is_empty() { words } else { cut.to_string() }
}

pub fn same_title(a: &str, b: &str) -> bool {
    compared(a) == compared(b)
}

/// What a new record is, for telling it from one kept in another session.
pub struct New<'a> {
    pub title: &'a str,
    pub kind: Option<&'a str>,
    pub remind: bool,
    pub file: Option<&'a str>,
}

/// Whether `new` says what `i`, of another session, says: one title, one kind, a reminder as a
/// reminder, one file when both name one. Never a report: each session's report is its own.
pub fn repeats(i: &Item, new: &New) -> bool {
    new.kind != Some("report") && i.kind.as_deref() == new.kind && i.remind == new.remind && same_title(&i.note, new.title)
        && (i.file.is_none() || new.file.is_none() || i.file.as_deref() == new.file)
}

fn words(title: &str) -> BTreeSet<String> {
    title.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// Whether two titles are alike enough for a hint. Only a hint: such titles often differ by the one
/// word that matters.
pub fn alike(a: &str, b: &str) -> bool {
    let (a, b) = (words(a), words(b));
    let common = a.intersection(&b).count();
    common >= LIKE_WORDS && common * 100 >= LIKE_SHARE * a.len().max(b.len())
}

/// A record's session in an answer: its row's title when it was made, which a program may have set.
pub fn session_of(i: &Item) -> String {
    let title: String = rules::clean(&i.title).chars().take(SESSION).collect();
    if title.is_empty() { "untitled".into() } else { title }
}

/// A text quoted for an answer: data among the answer's words.
pub fn quoted(text: &str) -> String {
    Value::from(text).to_string()
}

/// What `summary` says of a record of another session, and where it is.
pub fn with_session(mut summary: Value, i: &Item) -> Value {
    summary["session"] = json!(session_of(i));
    if i.orphaned.is_some() {
        summary["closed"] = json!(true);
    }
    summary
}

#[cfg(test)]
#[path = "agentmarks_scope_tests.rs"]
mod tests;
