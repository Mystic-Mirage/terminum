// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Folds of a pane's history (AC-32): parts the page shows as one line, which terminumd does
//! not send until the user asks. Two thirds of a coding agent's history is the diffs of its
//! edits, the scripts it ran and their output; a file listing can be thousands of nearly equal
//! lines. A port of iterm-enhancer's fold.py: the regular expressions are written out by hand
//! with Python's `str` rules (code points, Unicode whitespace); `History` keeps what one page
//! has, the stream sends the rest.

use serde::Serialize;
use serde_json::Value;

use super::pystr::{chars, is_digit, is_py_space, is_word, py_rstrip, py_strip};
#[cfg(test)]
use super::pystr::text_of;

const CMD_KEEP: usize = 1; // a long command: its line and this many more stay,
const CMD_MIN: usize = 2; // at least this many more fold
const OUT_KEEP: usize = 2; // a long tool output: its first line and this many more stay (a summary, AC-32),
const OUT_MIN: usize = 3; // at least this many more fold
const DIFF_MIN: usize = 6; // an edit's diff folds from this many lines
const SAME_MIN: usize = 20; // nearly equal lines: from 20, the first 3 and last 2 stay
const SAME_HEAD: usize = 3;
const SAME_TAIL: usize = 2;
const BLOB_MIN: usize = 3; // rows of encoded data
const REPEAT_MIN: usize = 5; // one line again and again (an input box redrawn into the history)
const BLANK_MIN: usize = 3; // blank lines in a row before an agent's ⏺ or ⎿ (rows its redraw left empty)
/// Lines looked at around new ones: a fold may start or end there.
pub const CONTEXT: i64 = 1000;
/// Seconds between looks at lines that scrolled into the history.
pub const SCAN_EVERY: f64 = 1.0;

const EDITS: [&str; 6] = ["Updated", "Added", "Removed", "Created", "Wrote", "Deleted"];
const NOTEWORTHY: [&str; 8] = ["error", "fail", "fatal", "exception", "panic", "denied", "refused", "warn"];

/// A fold of `folds`: row indexes, inclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowFold {
    pub from: usize,
    pub to: usize,
    pub label: String,
}

/// A fold told to the page: absolute line numbers, inclusive; serialized {"n", "to", "label"}.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fold {
    pub n: i64,
    pub to: i64,
    pub label: String,
}

// ---------- the patterns ----------

/// `^⏺ [\w.-]+\(`: Claude Code's tool call, "⏺ Bash(…".
fn is_tool(t: &str) -> bool {
    t.strip_prefix("⏺ ").is_some_and(is_call)
}

/// `^  [\w.-]+\(`: a running tool's call while Claude Code blinks its dot off (600 ms of each
/// 1.2 s): a tool's only when a tool's result follows its command.
fn is_blinked_tool(t: &str) -> bool {
    t.strip_prefix("  ").is_some_and(is_call)
}

/// `[\w.-]+\(`: a tool's name and its call.
fn is_call(rest: &str) -> bool {
    let name = rest.find(|c: char| !(is_word(c) || c == '.' || c == '-')).unwrap_or(rest.len());
    name > 0 && rest[name..].starts_with('(')
}

/// `^ {2}⎿ ` (Claude Code) or `^ {2}└ ` (Codex): a tool's result's first line.
fn is_result(t: &str) -> bool {
    t.starts_with("  ⎿ ") || t.starts_with("  └ ")
}

/// How far a result's next lines stand in: Claude Code's 5 columns, Codex's 4.
fn result_indent(t: &str) -> usize {
    if t.starts_with("  └ ") { 4 } else { 5 }
}

/// `^ {2}⎿ +(Updated|Added|Removed|Created|Wrote|Deleted)\b`: the word, when an edit's.
fn edit_of(t: &str) -> Option<&'static str> {
    let rest = t.strip_prefix("  ⎿")?;
    let word = rest.trim_start_matches(' ');
    if word.len() == rest.len() {
        return None;
    }
    EDITS.into_iter().find(|w| word.strip_prefix(w).is_some_and(|after| !after.chars().next().is_some_and(is_word)))
}

fn is_blob_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "+/=_-".contains(c)
}

/// A row of encoded data (base64, base64url, hex): digits and letters mixed, not a word or a rule.
pub fn is_blob(text: &str) -> bool {
    let t = py_strip(text);
    let mut seen: Vec<char> = Vec::new();
    for c in t.chars() {
        if !seen.contains(&c) {
            seen.push(c);
        }
    }
    t.len() >= 40 && t.chars().all(is_blob_char) && t.chars().any(|c| c.is_ascii_digit()) && t.chars().any(|c| c.is_ascii_alphabetic()) && seen.len() >= 12
}

/// `error|fail|fatal|…` case aside: a line that says something went wrong.
fn noteworthy(text: &str) -> bool {
    let low: String = text.chars().map(|c| match c {
        'ı' => 'i',
        'ſ' => 's',
        _ => c.to_lowercase().next().unwrap_or(c),
    }).collect();
    NOTEWORTHY.iter().any(|w| low.contains(w))
}

fn is_path_char(c: char) -> bool {
    is_word(c) || ".@+-".contains(c)
}

/// `(?:[~/][\w.@+-]*)+/[\w.@+-]+` → "/p". A chain of segments, each "~" or "/" and its name
/// chars, matches up to its last segment from the second on that is "/" and a name.
fn sub_paths(text: &str) -> String {
    let cs: Vec<char> = text.chars().collect();
    let (mut out, mut i) = (String::new(), 0);
    while i < cs.len() {
        if cs[i] != '~' && cs[i] != '/' {
            out.push(cs[i]);
            i += 1;
            continue;
        }
        let (mut segs, mut j) = (Vec::new(), i); // (start, end) of each segment
        while j < cs.len() && (cs[j] == '~' || cs[j] == '/') {
            let s = j;
            j += 1;
            while j < cs.len() && is_path_char(cs[j]) {
                j += 1;
            }
            segs.push((s, j));
        }
        match (1..segs.len()).rev().find(|&k| cs[segs[k].0] == '/' && segs[k].1 > segs[k].0 + 1) {
            Some(k) => {
                out.push_str("/p");
                cs[segs[k].1..j].iter().for_each(|&c| out.push(c));
            }
            None => cs[i..j].iter().for_each(|&c| out.push(c)),
        }
        i = j;
    }
    out
}

/// Each maximal run of chars `keep` accepts, `min` long or longer, becomes `to`.
fn sub_runs(text: &str, keep: impl Fn(char) -> bool, min: usize, to: &str) -> String {
    let (mut out, mut run) = (String::new(), String::new());
    let flush = |run: &mut String, out: &mut String| {
        if !run.is_empty() {
            out.push_str(if chars(run) >= min { to } else { run });
            run.clear();
        }
    };
    for c in text.chars() {
        if keep(c) {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// A line's shape: paths, long words and numbers aside. Said in words, not in a name, a
/// failure stays itself (".../errors.js" in a listing is one of many).
pub fn shape(text: &str) -> String {
    let t = sub_paths(text);
    let t = sub_runs(&t, is_blob_char, 24, "t");
    let t = sub_runs(&t, is_digit, 1, "0");
    let t = sub_runs(&t, is_py_space, 1, " ");
    let mut out = py_strip(&t).to_string();
    if noteworthy(&t) {
        out.push('!');
        out.push_str(&t);
    }
    out
}

fn size_of(n: usize) -> String {
    if n < 1024 { format!("{n} B") } else { format!("{:.1} KB", n as f64 / 1024.0) }
}

// ---------- folds ----------

/// Folds of `rows` (a run of a pane's history, oldest first: (text, hard end)), in order. Rows
/// tmux wrapped are one line. Only a part that is whole is folded: one that may go on after
/// the last row is left for when more has come. `newest` false: newer rows exist past `rows`
/// (a window of a long history), so a run of data or of nearly equal lines ends there for now.
pub fn folds<S: AsRef<str>>(rows: &[(S, bool)], newest: bool) -> Vec<RowFold> {
    let stripped: Vec<&str> = rows.iter().map(|(t, _)| py_rstrip(t.as_ref())).collect();
    let mut out = blobs(&stripped, newest); // rows of data: counted as rows; the rest by lines
    let mut spans: Vec<(usize, usize, String)> = Vec::new();
    let mut cur: Option<(usize, usize, String)> = None;
    for (k, (t, eol)) in rows.iter().enumerate() {
        let mut c = cur.take().unwrap_or((k, k, String::new()));
        c.1 = k;
        c.2.push_str(t.as_ref());
        if *eol { spans.push(c) } else { cur = Some(c) }
    }
    spans.extend(cur);
    let lines: Vec<&str> = spans.iter().map(|s| s.2.as_str()).collect();
    for f in line_folds(&lines, newest) {
        let (a, b) = (spans[f.from].0, spans[f.to].1);
        if !out.iter().any(|x| x.from <= b && a <= x.to) {
            out.push(RowFold { from: a, to: b, label: f.label });
        }
    }
    out.sort_by_key(|f| f.from);
    out
}

/// Folds of the rows on a pane's screen (AC-32), as `[[from, to, label], …]`: a coding agent's
/// tool output folds there too, before it scrolls into the history. Never on a full-screen
/// program's rows (vim, mc), nor one holding the cursor; the last part may go on, so it waits.
pub fn on_screen(texts: &[String], hard: &[bool], cursor: Option<usize>, full_screen: bool) -> Value {
    if full_screen {
        return Value::Array(Vec::new());
    }
    let rows: Vec<(&str, bool)> = texts.iter().zip(hard).map(|(t, h)| (t.as_str(), *h)).collect();
    let kept = folds(&rows, true).into_iter().filter(|f| !cursor.is_some_and(|c| f.from <= c && c <= f.to));
    Value::Array(kept.map(|f| serde_json::json!([f.from, f.to, f.label])).collect())
}

/// Runs of BLOB_MIN or more rows of encoded data, the next row after them come.
fn blobs(text: &[&str], newest: bool) -> Vec<RowFold> {
    let (mut out, n, mut i) = (Vec::new(), text.len(), 0);
    while i < n {
        let mut j = i;
        while j < n && is_blob(text[j]) {
            j += 1;
        }
        if j - i >= BLOB_MIN && (j < n || !newest) {
            let data: usize = text[i..j].iter().map(|t| chars(py_strip(t))).sum();
            let hex = text[i..j].iter().all(|t| py_strip(t).chars().all(|c| c.is_ascii_hexdigit()));
            let kind = if hex { "hex" } else { "base64" };
            out.push(RowFold { from: i, to: j - 1, label: format!("⋯ {} of {kind} · {} lines", size_of(data), j - i) });
        }
        i = j.max(i + 1);
    }
    out
}

struct Taken {
    out: Vec<RowFold>,
    taken: Vec<bool>,
}

impl Taken {
    fn fold(&mut self, a: usize, b: usize, label: String) {
        if a <= b && !self.taken[a..=b].iter().any(|t| *t) {
            self.out.push(RowFold { from: a, to: b, label });
            self.taken[a..=b].iter_mut().for_each(|t| *t = true);
        }
    }
}

/// Lines after i that go on (indented at least `indent`, blank lines between them too: a
/// script's), up to the last one that does, and whether a line that does not go on comes after
/// them (blank lines at the history's end may still be inside them).
fn cont(text: &[&str], i: usize, indent: usize) -> (usize, bool) {
    let (mut j, mut last) = (i + 1, i);
    while j < text.len() {
        if !py_strip(text[j]).is_empty() {
            let lead = text[j].chars().take(indent).filter(|c| *c == ' ').count();
            if lead < indent || is_result(text[j]) {
                break;
            }
            last = j;
        }
        j += 1;
    }
    (last + 1, j < text.len())
}

fn line_folds(lines: &[&str], newest: bool) -> Vec<RowFold> {
    let text: Vec<&str> = lines.iter().map(|t| py_rstrip(t)).collect();
    let n = text.len();
    let mut f = Taken { out: Vec::new(), taken: vec![false; n] };
    for (i, t) in text.iter().enumerate() {
        let blinked = !is_tool(t) && is_blinked_tool(t);
        if is_tool(t) || blinked {
            // the command a tool ran, its lines
            let (j, ends) = cont(&text, i, 4);
            if blinked && !text[j.min(n)..].iter().find(|r| !py_strip(r).is_empty()).is_some_and(|r| is_result(r)) {
                continue;                               // indented text that only looks like a call
            }
            if ends && j - (i + 1) >= CMD_KEEP + CMD_MIN {
                let k = j - (i + 1) - CMD_KEEP;
                f.fold(i + 1 + CMD_KEEP, j - 1, format!("⋯ {k} more lines of the command"));
            }
        } else if is_result(t) {
            // a tool's result: a diff, or output
            let (j, ends) = cont(&text, i, result_indent(t));
            let k = j - (i + 1);
            if !ends {
                continue;
            }
            match edit_of(t) {
                Some(w) if k >= DIFF_MIN => {
                    let what = if w == "Created" || w == "Wrote" { "of the file" } else { "of diff" };
                    f.fold(i + 1, j - 1, format!("⋯ {k} lines {what}"));
                }
                None if k >= OUT_KEEP + OUT_MIN => {
                    f.fold(i + 1 + OUT_KEEP, j - 1, format!("⋯ {} more lines of output", k - OUT_KEEP));
                }
                _ => {}
            }
        }
    }
    let short = |t: &str| chars(py_strip(t)) <= 3;
    let mut i = 0; // one line again and again, maybe with blank or 1-3 character lines between
    while i < n {
        let t = py_strip(text[i]);
        if chars(t) <= 3 || f.taken[i] || text[i].starts_with('⏺') {
            i += 1;
            continue;
        }
        let (mut j, mut last, mut copies) = (i + 1, i, 1);
        while j < n && !f.taken[j] && (py_strip(text[j]) == t || short(text[j])) {
            if py_strip(text[j]) == t {
                (copies, last) = (copies + 1, j);
            }
            j += 1;
        }
        if copies >= REPEAT_MIN && (j < n || !newest) {
            f.fold(i + 1, last, format!("⋯ {} more copies of this line", copies - 1));
            i = last + 1;
        } else {
            i += 1;
        }
    }
    let mut i = 0; // nearly equal lines: a listing, a log
    while i < n {
        let s = (!py_strip(text[i]).is_empty() && !f.taken[i]).then(|| shape(text[i]));
        let mut j = i + 1;
        if let Some(s) = &s {
            while j < n && !f.taken[j] && !py_strip(text[j]).is_empty() && shape(text[j]) == *s {
                j += 1;
            }
            if j - i >= SAME_MIN && (j < n || !newest) && !text[i].starts_with('⏺') {
                let k = j - i - SAME_HEAD - SAME_TAIL;
                f.fold(i + SAME_HEAD, j - 1 - SAME_TAIL, format!("⋯ {k} similar lines"));
            }
        }
        i = if s.is_some() { j } else { i + 1 };
    }
    let mut i = 0; // blank lines in a row before a coding agent's next message or result (its
    while i < n {
        // redraw left them; a file's own stay)
        let mut j = i;
        while j < n && py_strip(text[j]).is_empty() && !f.taken[j] {
            j += 1;
        }
        if j - i >= BLANK_MIN && j < n && (text[j].starts_with('⏺') || is_result(text[j])) {
            f.fold(i, j - 1, format!("⋯ {} blank lines", j - i));
        }
        i = j.max(i + 1);
    }
    f.out.sort_by_key(|x| x.from);
    f.out
}

#[path = "folds_history.rs"]
mod history;
pub use history::History;

#[cfg(test)]
#[path = "folds_tests.rs"]
mod tests;
