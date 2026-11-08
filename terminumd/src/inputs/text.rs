// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What an input row says (terminum.md AC-147): the keys sent as text and key names, whether a
//! row is not kept (hidden input), and the browser in a few words.

use crate::tmux::keys::{parts, Part};

/// Programs whose input is a secret: what is typed while one runs is not kept.
const SECRET_PROGRAMS: [&str; 10] = ["sudo", "su", "passwd", "doas", "gpg", "pinentry", "security", "op", "pass", "ssh-add"];
/// Words a prompt for a secret ends with, before its colon.
const SECRET_WORDS: [&str; 4] = ["password", "passphrase", "passcode", "pin"];

/// Typed bytes as a row shows them: text as it is, every other key by its tmux name in angle
/// brackets (`<Enter>`, `<C-c>`, `<BSpace>`, `<Up>`): the keys sent, not the line a program kept.
pub fn rendered(data: &str) -> String {
    let mut out = String::new();
    for p in parts(data) {
        match p {
            Part::Text(t) => out.push_str(&t),
            Part::Keys(names) => {
                for n in names {
                    out.push('<');
                    out.push_str(&n);
                    out.push('>');
                }
            }
        }
    }
    out
}

/// Why a row's text is not kept, or None: a program that reads a secret runs (`program`), the
/// cursor's line asks for one (`line`: "Password:", "[sudo] password for alex:", "Enter
/// passphrase for key …:"), or the first text of a typed row is a space (`spaced`: a shell's
/// "keep out of history", as the sent history does, AC-90).
pub fn not_kept(program: &str, line: &str, spaced: bool) -> Option<String> {
    let program = program.trim().trim_start_matches('-');
    if SECRET_PROGRAMS.contains(&program) {
        return Some(format!("[not kept: {program}]"));
    }
    if asks_secret(line) {
        return Some("[not kept: hidden input]".into());
    }
    spaced.then(|| "[not kept: leading space]".into())
}

/// Whether a screen line ends in a prompt for a secret: one of SECRET_WORDS as a word of its
/// own, then up to a colon with only spaces after it; or sudo's "[sudo]".
fn asks_secret(line: &str) -> bool {
    let line = line.trim_end().to_lowercase();
    if line.contains("[sudo]") {
        return true;
    }
    let Some(before) = line.strip_suffix(':') else { return false };
    let alone = |i: usize, w: &str| {
        let (pre, post) = (before[..i].chars().next_back(), before[i + w.len()..].chars().next());
        !pre.is_some_and(char::is_alphanumeric) && !post.is_some_and(char::is_alphanumeric) && !before[i..].contains(':')
    };
    SECRET_WORDS.iter().any(|w| before.match_indices(w).any(|(i, _)| alone(i, w)))
}

/// The browser and its machine in a few words, from a User-Agent: "Safari 18 · iPhone",
/// "Chrome 141 · Mac"; empty when there is none.
pub fn browser(agent: &str) -> String {
    if agent.trim().is_empty() {
        return String::new();
    }
    let device = [("iPhone", "iPhone"), ("iPad", "iPad"), ("Android", "Android"), ("Macintosh", "Mac"), ("Windows", "Windows"), ("Linux", "Linux")]
        .iter().find(|(k, _)| agent.contains(k)).map_or("", |(_, v)| *v);
    let version = |tag: &str| agent.split(tag).nth(1).map(|v| v.split(['.', ' ', ';']).next().unwrap_or("").to_string());
    let name = [("Edg/", "Edge"), ("EdgiOS/", "Edge"), ("CriOS/", "Chrome"), ("FxiOS/", "Firefox"), ("Firefox/", "Firefox"), ("Chrome/", "Chrome")]
        .iter().find_map(|(tag, n)| version(tag).map(|v| format!("{n} {v}")))
        .or_else(|| agent.contains("Safari/").then(|| format!("Safari {}", version("Version/").unwrap_or_default()).trim_end().to_string()))
        .unwrap_or_else(|| agent.split_whitespace().next().unwrap_or("").chars().take(40).collect());
    if device.is_empty() { name } else { format!("{name} · {device}") }
}

/// The first text typed in `data`, past any keys before it (Ctrl-U, an arrow).
pub fn first_text(data: &str) -> Option<char> {
    parts(data).into_iter().find_map(|p| match p {
        Part::Text(t) => t.chars().next(),
        Part::Keys(_) => None,
    })
}

/// `text` cut to `most` characters.
pub fn cut(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((i, _)) => text[..i].to_string(),
        None => text.to_string(),
    }
}
