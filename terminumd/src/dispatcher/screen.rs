// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a coding agent's screen says to the typing gate (`terminum-dispatcher.md` §9, AS-125): read
//! from `capture-pane -e`, as only the colors tell a placeholder from a draft. It fails closed: what
//! it does not know is not empty, so a new version's changed screen delays words and never types.

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

/// A row with its SGR sequences gone; `None` when it holds any other escape.
fn plain(row: &str) -> Option<String> {
    let mut out = String::with_capacity(row.len());
    let mut chars = row.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.next() != Some('[') {
            return None;
        }
        loop {
            match chars.next()? {
                'm' => break,
                '0'..='9' | ';' | ':' => {}
                _ => return None,
            }
        }
    }
    Some(out)
}

fn is_rule(row: &str) -> bool {
    row.trim_start().starts_with("\u{2500}\u{2500}")
}

/// A choice or its footer: what a question that replaces the box shows, in both agents.
fn is_question(rows: &[String]) -> bool {
    const SAID: [&str; 5] = ["Enter to confirm", "Esc to cancel", "enter continue", "esc skip", "Press Enter to continue"];
    rows.iter().any(|r| SAID.iter().any(|s| r.contains(s)))
        || rows.iter().any(|r| {
            let t = r.trim_start().trim_start_matches(['\u{276f}', '\u{203a}', '>']).trim_start();
            r.trim_start().starts_with(['\u{276f}', '\u{203a}']) && t.starts_with(|c: char| c.is_ascii_digit()) && t.contains(". ")
        })
}

/// A row's characters drawn at full strength: dim ones are a placeholder or ghost text, no one's words.
fn at_full_strength(row: &str) -> String {
    let (mut out, mut dim, mut chars) = (String::new(), false, row.chars());
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if !dim {
                out.push(c);
            }
            continue;
        }
        chars.next();
        let mut code = String::new();
        for x in chars.by_ref() {
            if x == 'm' {
                break;
            }
            code.push(x);
        }
        for part in code.split(';') {
            match part {
                "2" => dim = true,
                "0" | "22" | "" => dim = false,
                _ => {}
            }
        }
    }
    out
}

/// Claude Code's box: a rule, `❯ ` and what is typed, a rule; empty holds only its cursor's reverse
/// cell, or a suggestion in dim (with no terminal focus, neither shows its cursor).
fn claude(raw: &[String], rows: &[String]) -> Screen {
    if let Some(bottom) = rows.iter().rposition(|r| is_rule(r))
        && let Some(top) = rows[..bottom].iter().rposition(|r| is_rule(r))
        && rows[top + 1].trim_start().starts_with('\u{276f}') {
        let strong: Vec<String> = raw[top + 1..bottom].iter().map(|r| at_full_strength(r)).collect();
        let first = strong[0].trim_start().trim_start_matches('\u{276f}').to_string();
        let typed: Vec<&str> = std::iter::once(first.as_str()).chain(strong[1..].iter().map(String::as_str)).map(|r| r.trim_matches(|c: char| c.is_whitespace() || c == '\u{a0}')).filter(|r| !r.is_empty()).collect();
        return if typed.is_empty() { Screen::Empty } else { Screen::Holds(typed.join(" ")) };
    }
    if is_question(rows) { Screen::Asks } else { Screen::Unknown }
}

/// Codex's composer: `› ` and, empty, its placeholder in dim.
fn codex(raw: &[String], rows: &[String]) -> Screen {
    const PLACEHOLDER: &str = "Ask Codex to do anything";
    if is_question(rows) {
        return Screen::Asks;
    }
    let Some(at) = rows.iter().rposition(|r| r.trim_start().starts_with('\u{203a}')) else { return Screen::Unknown };
    let typed = rows[at].trim_start().trim_start_matches('\u{203a}').trim();
    if typed == PLACEHOLDER && raw[at].contains("\u{1b}[2m") {
        return Screen::Empty;
    }
    let more: Vec<&str> = rows[at + 1..].iter().map(|r| r.trim()).take_while(|r| !r.is_empty()).collect();
    if typed.is_empty() && more.is_empty() {
        return Screen::Unknown;
    }
    Screen::Holds(std::iter::once(typed).chain(more).collect::<Vec<_>>().join(" "))
}

/// The screen of `kind`'s agent, as `capture-pane -e -p` gave it.
pub fn read(kind: &str, raw: &[String]) -> Screen {
    let Some(rows) = raw.iter().map(|r| plain(r)).collect::<Option<Vec<String>>>() else { return Screen::Unknown };
    match kind {
        "claude" => claude(raw, &rows),
        "codex" => codex(raw, &rows),
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
