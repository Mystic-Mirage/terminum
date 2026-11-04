// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A coding agent's own transcript as a source of its pane's links (AC-102): Claude Code in its
//! full-screen TUI leaves tmux no history, so what scrolled away is only in the session's
//! transcript, `<claude dir>/projects/<folder>/<session>.jsonl` (the session as the pane's hooks
//! last said, else as its Claude Code process's `sessions/<pid>.json` says). Only its http and
//! https addresses are taken, of the conversation alone: the user's and the agent's messages, not
//! its tool calls and their output (code it wrote or read, commands), its thinking or a
//! compaction's summary. Only the addresses found are kept, in memory, so a read again takes only
//! what was added.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;
use serde::de::{Deserializer, SeqAccess, Visitor};

use super::linklist::{Found, Link};

/// The newest part of a transcript read, at most.
pub const READ_MOST: u64 = 128 << 20;
/// Transcripts whose addresses are kept, at most.
const KEPT: usize = 8;

/// Sessions whose transcript path is remembered, at most.
const PATHS: usize = 64;

/// A transcript read: which file it was (device, inode), how far (a line's end), the addresses
/// found up to there, the last answer (by count, the addresses told, how many) and when it was
/// last asked for.
struct Kept {
    file: (u64, u64),
    at: u64,
    found: Found,
    told: Option<(bool, Vec<Link>, usize)>,
    used: u64,
}

/// The transcripts read, by path; one read at a time, so a second ask waits and takes what the
/// first read rather than reading it all again.
static READ: Mutex<(u64, Option<HashMap<PathBuf, Kept>>)> = Mutex::new((0, None));
/// Each session's transcript found, by Claude Code's folder and session.
static FOUND: Mutex<Option<HashMap<(PathBuf, String), PathBuf>>> = Mutex::new(None);

/// Claude Code's folder: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| crate::config::home().join(".claude"))
}

/// The session Claude Code runs as process `pid` or one below it (3 levels: a wrapper, a shell),
/// with the process whose record says it.
pub fn session_of(dir: &Path, pid: u32) -> Option<(u32, String)> {
    let mut level = vec![pid];
    for _ in 0..3 {
        for &p in &level {
            if let Some(id) = record(dir, p) {
                return Some((p, id));
            }
        }
        level = level.iter().flat_map(|p| children(*p)).collect();
        if level.is_empty() {
            return None;
        }
    }
    None
}

/// The session process `pid`'s `sessions/<pid>.json` says (`sessionId` and `procStart` alone are
/// read), when that record is of the process running now (a record left by an ended Claude Code
/// names another session).
pub fn record(dir: &Path, pid: u32) -> Option<String> {
    let v = serde_json::from_slice::<serde_json::Value>(&std::fs::read(dir.join("sessions").join(format!("{pid}.json"))).ok()?).ok()?;
    running(pid, v["procStart"].as_str()).then(|| v["sessionId"].as_str().map(str::to_string)).flatten()
}

/// The start of process `pid` as Claude Code's `procStart` gives it on Linux (`/proc/<pid>/stat`'s
/// 22nd field: clock ticks since boot), None elsewhere.
pub fn proc_start(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(19).map(str::to_string)
}

/// Whether process `pid` is the Claude Code its record (with `start`) was written by: by its start
/// where the system says it, else by its program's name.
fn running(pid: u32, start: Option<&str>) -> bool {
    match proc_start(pid) {
        Some(now) => start == Some(now.as_str()),
        None => std::process::Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]).output().ok()
            .is_some_and(|o| matches!(String::from_utf8_lossy(&o.stdout).trim().rsplit('/').next(), Some("claude" | "node"))),
    }
}

/// The processes whose parent is `pid` (`pgrep -P`: macOS and Linux).
fn children(pid: u32) -> Vec<u32> {
    std::process::Command::new("pgrep").args(["-P", &pid.to_string()]).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect())
        .unwrap_or_default()
}

/// The transcript of session `id` under `dir`, None when there is none (or the id is no id): where
/// it was found last while it is there, else looked for in each project's folder.
pub fn transcript(dir: &Path, id: &str) -> Option<PathBuf> {
    if id.is_empty() || id.len() > 100 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    let key = (dir.to_path_buf(), id.to_string());
    let last = FOUND.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().get(&key).cloned();
    if let Some(p) = last.filter(|p| p.is_file()) {
        return Some(p);
    }
    let name = format!("{id}.jsonl");
    let found = std::fs::read_dir(dir.join("projects")).ok()?.flatten().map(|e| e.path().join(&name)).find(|p| p.is_file());
    let mut g = FOUND.lock().unwrap_or_else(|e| e.into_inner());
    let paths = g.get_or_insert_default();
    if paths.len() >= PATHS {
        paths.clear();
    }
    match &found {
        Some(p) => paths.insert(key, p.clone()),
        None => paths.remove(&key),
    };
    found
}

/// The addresses of the transcript at `path` as `Found::told` gives them, or why it cannot be read:
/// what was read before and the lines added since (all of its newest READ_MOST when it is new,
/// another file or shorter); the last answer again when none were added.
pub fn read(path: &Path, by_count: bool) -> Result<(Vec<Link>, usize), String> {
    read_most(path, READ_MOST, by_count)
}

fn read_most(path: &Path, most: u64, by_count: bool) -> Result<(Vec<Link>, usize), String> {
    let mut g = READ.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let meta = f.metadata().map_err(|e| e.to_string())?;
    let (file, size) = ((meta.dev(), meta.ino()), meta.len());
    let (n, kept) = &mut *g;
    *n += 1;
    let kept = kept.get_or_insert_default();
    let (mut at, mut found, told) = match kept.remove(path) {
        Some(k) if k.file == file && k.at <= size => (k.at, k.found, k.told),
        _ => (size.saturating_sub(most), Found::default(), None),
    };
    let from = at;
    // a start inside a line (a tail of a long transcript): from the next one
    let cut = at > 0 && {
        f.seek(SeekFrom::Start(at - 1)).map_err(|e| e.to_string())?;
        let mut b = [0u8];
        f.read_exact(&mut b).map_err(|e| e.to_string())?;
        b[0] != b'\n'
    };
    f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
    let mut r = BufReader::new(f);
    let mut line = Vec::new();
    if cut {
        at += r.read_until(b'\n', &mut line).map_err(|e| e.to_string())? as u64;
    }
    loop {
        line.clear();
        let n = r.read_until(b'\n', &mut line).map_err(|e| e.to_string())?;
        if n == 0 || line.last() != Some(&b'\n') {
            break;                    // a line still being written: read when it is whole
        }
        at += n as u64;
        // most lines name no address; those are not parsed; a line of another shape than Entry's
        // is read field by field
        if line.windows(4).any(|w| w.eq_ignore_ascii_case(b"http")) {
            match serde_json::from_slice::<Entry>(&line) {
                Ok(e) => entry(e, &mut found),
                Err(_) => {
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&line) {
                        loose(&v, &mut found);
                    }
                }
            }
        }
    }
    let told = match told {
        Some((b, links, total)) if at == from && b == by_count => (links, total),
        _ => found.told(by_count),
    };
    if kept.len() >= KEPT {
        let oldest = kept.iter().min_by_key(|(_, k)| k.used).map(|(p, _)| p.clone());
        kept.remove(&oldest.unwrap_or_default());
    }
    kept.insert(path.to_path_buf(), Kept { file, at, found, told: Some((by_count, told.0.clone(), told.1)), used: *n });
    Ok(told)
}

/// One line of a transcript: only what is read of it (the rest is skipped, not kept).
#[derive(Deserialize)]
struct Entry<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(rename = "isMeta")]
    meta: Option<bool>,
    #[serde(rename = "isCompactSummary")]
    summary: Option<bool>,
    #[serde(borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow)]
    message: Option<Message<'a>>,
}

#[derive(Deserialize)]
struct Message<'a> {
    #[serde(borrow)]
    content: Option<Content<'a>>,
}

#[derive(Deserialize)]
struct Block<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
}

/// A message's content: its text, or its blocks.
enum Content<'a> {
    Text(Cow<'a, str>),
    Blocks(Vec<Block<'a>>),
}

impl<'de: 'a, 'a> Deserialize<'de> for Content<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<'a>(std::marker::PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = Content<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("text or blocks")
            }
            fn visit_borrowed_str<E: serde::de::Error>(self, s: &'de str) -> Result<Self::Value, E> {
                Ok(Content::Text(Cow::Borrowed(s)))
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Self::Value, E> {
                Ok(Content::Text(Cow::Owned(s.to_string())))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut blocks = Vec::new();
                while let Some(b) = a.next_element()? {
                    blocks.push(b);
                }
                Ok(Content::Blocks(blocks))
            }
        }
        d.deserialize_any(V(std::marker::PhantomData))
    }
}

/// One line of a transcript: a user's or the agent's message, its text alone.
fn entry(e: Entry, found: &mut Found) {
    let texts: Vec<&str> = match &e.message.as_ref().and_then(|m| m.content.as_ref()) {
        Some(Content::Text(s)) => vec![s],
        Some(Content::Blocks(blocks)) => blocks.iter().filter(|b| b.kind.as_deref() == Some("text")).filter_map(|b| b.text.as_deref()).collect(),
        None => Vec::new(),
    };
    said(found, e.kind.as_deref(), e.meta == Some(true) || e.summary == Some(true), e.timestamp.as_deref(), &texts);
}

/// `entry` of a line of another shape: a field of another type is left out.
fn loose(v: &serde_json::Value, found: &mut Found) {
    let content = &v["message"]["content"];
    let texts: Vec<&str> = match content.as_str() {
        Some(s) => vec![s],
        None => content.as_array().into_iter().flatten().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect(),
    };
    let aside = v["isMeta"].as_bool() == Some(true) || v["isCompactSummary"].as_bool() == Some(true);
    said(found, v["type"].as_str(), aside, v["timestamp"].as_str(), &texts);
}

/// The addresses of a user's or the agent's message (`kind`), not a meta line's or a summary's
/// (`aside`), at its time.
fn said(found: &mut Found, kind: Option<&str>, aside: bool, time: Option<&str>, texts: &[&str]) {
    if !matches!(kind, Some("user" | "assistant")) || aside {
        return;
    }
    found.when = time.filter(|t| t.len() <= 40).map(str::to_string);
    for t in texts {
        found.text(t);
    }
}

#[cfg(test)]
#[path = "agentlinks_tests.rs"]
mod tests;
