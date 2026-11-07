// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a coding agent's screen says to the typing gate (`terminum-dispatcher.md` §9, AS-125): read
//! from `capture-pane -e`, as only the colors tell a placeholder from a draft. It fails closed: what
//! it does not know is not empty, so a new version's changed screen delays words and never types.

use crate::screen::sgr::{parse_line, Style, FAINT};

#[derive(Clone, Debug, PartialEq)]
pub enum Screen {
    /// A known input box with nothing in it.
    Empty,
    /// A box with this in it: a draft, or what was pasted.
    Holds(String),
    /// The agent's own question stands where the box was: a folder's trust, an update, a login.
    Asks,
    /// Nothing it knows.
    Unknown,
}

/// A row as text: all of it, and what is at full strength: dim is a placeholder or ghost text, no one's words.
struct Row {
    text: String,
    strong: String,
}

/// Whether every escape in `row` is a color: anything else is a screen it does not know.
fn only_colors(row: &str) -> bool {
    let mut parts = row.split('\u{1b}');
    parts.next();
    parts.all(|p| p.strip_prefix('[').is_some_and(|rest| rest.find('m').is_some_and(|m| rest[..m].chars().all(|c| c.is_ascii_digit() || c == ';' || c == ':'))))
}

fn row_of(raw: &str) -> Row {
    let (mut text, mut strong, mut style, mut link) = (String::new(), String::new(), Style::default(), None);
    parse_line(raw, &mut style, &mut link, |c, st, _| {
        text.push(c);
        if st.flags & FAINT == 0 {
            strong.push(c);
        }
    });
    Row { text, strong }
}

fn is_rule(row: &str) -> bool {
    row.trim_start().starts_with("\u{2500}\u{2500}")
}

/// A choice or its footer: what a question that replaces the box shows, in both agents.
fn is_question(rows: &[Row]) -> bool {
    const SAID: [&str; 5] = ["Enter to confirm", "Esc to cancel", "enter continue", "esc skip", "Press Enter to continue"];
    rows.iter().any(|r| SAID.iter().any(|s| r.text.contains(s)))
        || rows.iter().any(|r| {
            let t = r.text.trim_start().trim_start_matches(['\u{276f}', '\u{203a}']).trim_start();
            r.text.trim_start().starts_with(['\u{276f}', '\u{203a}']) && t.starts_with(|c: char| c.is_ascii_digit()) && t.contains(". ")
        })
}

/// Claude Code's box: a rule, `❯ ` and what is typed, a rule; empty holds only its cursor's reverse
/// cell, or a suggestion in dim (with no terminal focus, neither shows its cursor).
fn claude(rows: &[Row]) -> Screen {
    if let Some(bottom) = rows.iter().rposition(|r| is_rule(&r.text))
        && let Some(top) = rows[..bottom].iter().rposition(|r| is_rule(&r.text))
        && rows[top + 1].text.trim_start().starts_with('\u{276f}') {
        let first = rows[top + 1].strong.trim_start().trim_start_matches('\u{276f}');
        let typed: Vec<&str> = std::iter::once(first).chain(rows[top + 2..bottom].iter().map(|r| r.strong.as_str())).map(|r| r.trim_matches(|c: char| c.is_whitespace() || c == '\u{a0}')).filter(|r| !r.is_empty()).collect();
        return if typed.is_empty() { Screen::Empty } else { Screen::Holds(typed.join(" ")) };
    }
    if is_question(rows) { Screen::Asks } else { Screen::Unknown }
}

/// Codex's composer: `› ` and, empty, its placeholder in dim.
fn codex(rows: &[Row]) -> Screen {
    const PLACEHOLDER: &str = "Ask Codex to do anything";
    if is_question(rows) {
        return Screen::Asks;
    }
    let Some(at) = rows.iter().rposition(|r| r.text.trim_start().starts_with('\u{203a}')) else { return Screen::Unknown };
    if rows[at].text.trim_start().trim_start_matches('\u{203a}').trim() == PLACEHOLDER && rows[at].strong.trim_start().trim_start_matches('\u{203a}').trim().is_empty() {
        return Screen::Empty;
    }
    let typed = rows[at].text.trim_start().trim_start_matches('\u{203a}').trim();
    let more: Vec<&str> = rows[at + 1..].iter().map(|r| r.text.trim()).take_while(|r| !r.is_empty()).collect();
    if typed.is_empty() && more.is_empty() {
        return Screen::Unknown;
    }
    Screen::Holds(std::iter::once(typed).chain(more).collect::<Vec<_>>().join(" "))
}

/// The screen of `kind`'s agent, as `capture-pane -e -p` gave it.
pub fn read(kind: &str, raw: &[String]) -> Screen {
    if !raw.iter().all(|r| only_colors(r)) {
        return Screen::Unknown;
    }
    let rows: Vec<Row> = raw.iter().map(|r| row_of(r)).collect();
    match kind {
        "claude" => claude(&rows),
        "codex" => codex(&rows),
        _ => Screen::Unknown,
    }
}

/// Whether `screen` shows `text` was pasted: its first words, or the agent's fold of a long paste.
pub fn shows(screen: &Screen, text: &str) -> bool {
    let Screen::Holds(held) = screen else { return false };
    let squeeze = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let first: String = squeeze(text.lines().find(|l| !l.trim().is_empty()).unwrap_or("")).chars().take(20).collect();
    held.contains("[Pasted") || (!first.is_empty() && squeeze(held).contains(&first))
}

#[cfg(test)]
#[path = "screen_tests.rs"]
mod tests;
