// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What of a sent message is kept (AC-90): the messages themselves are rows of the message
//! history in the tasks' store (`crate::messages`). A coding agent's prompt comes exactly from
//! its hooks (`terminum hook`) and takes the line the page told for the same text (`hint`);
//! otherwise the page says what stood at the cursor when Enter went. Kept out: a line starting
//! with a space (a shell's "not in history"), an empty one, the same text again from the same
//! pane within AGAIN, values of words named like secrets and secrets' known shapes (replaced by
//! "[redacted]"). Beside it, per project (a host and a folder) in
//! `~/.terminum/sessions/<id>/` (0600; `meta.json` names the project): where the agents' answers
//! ended (`ends.jsonl`), the quotes' numbers given and the quotes asked about (AC-37).

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

/// Answer ends kept per project; the file is cut back to it once it holds TRIM, or to its newest
/// BYTES / 2 once it is larger than BYTES.
pub const KEEP: usize = 2000;
const TRIM: usize = 2500;
const BYTES: usize = 4 << 20;
/// The most characters of one entry.
pub const TEXT: usize = 20000;
const AGAIN: Duration = Duration::from_secs(10);
/// Lines the page told for a hooked pane's Enter, kept per pane for its hook's prompt.
const HINTS: usize = 4;
/// Quotes asked about kept per pane (AC-37), and the most of one's text kept.
const ASKED: usize = 100;
const ASKED_TEXT: usize = 4000;

pub struct Sent {
    root: PathBuf,
    /// per pane: the last text kept and when (one Enter seen twice is kept once)
    last: Mutex<HashMap<String, (String, Instant)>>,
    /// per hooked pane: (the loose first line of what the page saw, its line), newest last
    hints: Mutex<HashMap<String, VecDeque<(String, i64)>>>,
    write: Mutex<()>,
}

/// A project's folder name: hex of the hash of its host and path.
pub fn id(host: &str, path: &str) -> String {
    crate::auth::crypto::hex(&crate::auth::crypto::sha256(format!("{host}\0{path}").as_bytes())[..8])
}

#[path = "redaction.rs"]
mod redaction;
pub use redaction::redact;

impl Sent {
    pub fn new(root: PathBuf) -> Sent {
        Sent { root, last: Mutex::default(), hints: Mutex::default(), write: Mutex::default() }
    }

    /// A project's folder: its answers' ends, quotes and asked quotes.
    fn dir(&self, host: &str, path: &str) -> PathBuf {
        self.root.join(id(host, path))
    }


    /// Where the project's agents' answers ended (page/answers.rs), beside its sent history.
    fn ends_file(&self, host: &str, path: &str) -> PathBuf {
        self.dir(host, path).join("ends.jsonl")
    }

    /// Keeps that pane `key`'s agent's answer ended at `line` with `text` (AC-90). Blocking.
    pub fn add_end(&self, host: &str, path: &str, key: &str, line: i64, text: &str) -> Result<(), String> {
        let entry = json!({"at": crate::auth::store::unix_now(), "key": key, "line": line, "text": text});
        let _w = self.write.lock();
        let file = self.ends_file(host, path);
        self.project_dir(&file, host, path)?;
        append(&file, &(entry.to_string() + "\n")).map_err(|e| format!("{}: {e}", file.display()))?;
        trim(&file).map_err(|e| format!("{}: {e}", file.display()))
    }

    /// The `most` newest answer ends of pane `key`: (text, line, false), newest first. Blocking.
    pub fn ends_of_pane(&self, host: &str, path: &str, key: &str, most: usize) -> Vec<(String, i64, bool)> {
        of_key(&std::fs::read_to_string(self.ends_file(host, path)).unwrap_or_default(), key, most)
    }

    /// The project's folder (0700) with its meta.json, made once.
    fn project_dir(&self, file: &Path, host: &str, path: &str) -> Result<(), String> {
        let dir = file.parent().expect("a folder");
        if !dir.is_dir() {
            crate::config::private_dir(&self.root);
            crate::config::private_dir(dir);
            let meta = serde_json::to_vec(&json!({"host": host, "path": path})).expect("json");
            crate::config::write_private(&dir.join("meta.json"), &meta).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// What of `text` sent in pane `key` is kept (redacted), or None when it is left out (see
    /// the module): one Enter told by both the page and the hook is kept once.
    pub fn accept(&self, key: &str, text: &str) -> Option<String> {
        let text: String = text.trim_end().chars().take(TEXT).collect();
        if text.trim().is_empty() || text.starts_with(' ') {
            return None;
        }
        let text = redact(&text);
        let mut last = self.last.lock();
        if last.get(key).is_some_and(|(t, at)| *t == text && at.elapsed() < AGAIN) {
            return None;
        }
        if last.len() > 1000 {
            last.clear();
        }
        last.insert(key.to_string(), (text.clone(), Instant::now()));
        Some(text)
    }

    /// The page saw `text` sent at `line` in hooked pane `key`: its hook's prompt takes the line.
    pub fn hint(&self, key: &str, text: &str, line: i64) {
        let first = first_loose(text);
        if first.is_empty() {
            return;
        }
        let mut hints = self.hints.lock();
        if hints.len() > 1000 {
            hints.clear();
        }
        let ring = hints.entry(key.to_string()).or_default();
        ring.push_back((first, line));
        if ring.len() > HINTS {
            ring.pop_front();
        }
    }

    /// The line the page told for hooked pane `key`'s prompt `text`: the newest whose first
    /// line's letters and digits begin the prompt's (the page's is what stood in the box).
    pub fn take_hint(&self, key: &str, text: &str) -> Option<i64> {
        let prompt = crate::page::bookmarks_find::loose(text);
        let mut hints = self.hints.lock();
        let ring = hints.get_mut(key)?;
        let i = ring.iter().rposition(|(first, _)| prompt.starts_with(first.as_str()))?;
        ring.remove(i).map(|(_, line)| line)
    }

    /// Pane `key`'s next question number (AC-37 Ask): one more than the highest "Quote #N" it
    /// sent (`sent`, from the message history; a paste Claude Code folds into "[Pasted text]" is
    /// not there without its hooks) or the last given, kept per pane in the project's quotes.json
    /// so it counts on across terminumd's restarts. Blocking.
    pub fn next_question(&self, host: &str, path: &str, key: &str, sent: u64) -> Result<u64, String> {
        let _w = self.write.lock();
        let file = self.dir(host, path).join("quotes.json");
        let mut given = read_object(&file);
        let n = sent.max(given[key].as_u64().unwrap_or(0)) + 1;
        given[key] = json!(n);
        self.project_dir(&file, host, path)?;
        crate::config::write_private(&file, given.to_string().as_bytes()).map_err(|e| format!("{}: {e}", file.display()))?;
        Ok(n)
    }

    /// Keeps a quote pane `key` asked about (AC-37: its number, text and the history line its
    /// selection began on) in the project's asked.json, the newest ASKED of each pane, its text
    /// cut at ASKED_TEXT characters. Blocking.
    pub fn keep_asked(&self, host: &str, path: &str, key: &str, n: u64, text: &str, line: i64) -> Result<(), String> {
        let _w = self.write.lock();
        let file = self.dir(host, path).join("asked.json");
        let mut all = read_object(&file);
        let mut list = all[key].as_array().cloned().unwrap_or_default();
        list.retain(|q| q["n"].as_u64() != Some(n));
        list.push(json!({"n": n, "text": text.chars().take(ASKED_TEXT).collect::<String>(), "line": line}));
        let extra = list.len().saturating_sub(ASKED);
        list.drain(..extra);
        all[key] = json!(list);
        self.project_dir(&file, host, path)?;
        crate::config::write_private(&file, all.to_string().as_bytes()).map_err(|e| format!("{}: {e}", file.display()))
    }

    /// Pane `key`'s quotes asked about, oldest first: [{n, text, line}]. Blocking.
    pub fn asked_of(&self, host: &str, path: &str, key: &str) -> Vec<Value> {
        read_object(&self.dir(host, path).join("asked.json"))[key].as_array().cloned().unwrap_or_default()
    }
}

/// A JSON object kept in `file`, or {} when there is none (or not an object).
fn read_object(file: &Path) -> Value {
    std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// The `most` newest entries of `text` (a .jsonl file's) of pane `key` with a line: (text, line,
/// whether a coding agent's prompt).
fn of_key(text: &str, key: &str, most: usize) -> Vec<(String, i64, bool)> {
    text.lines().rev().filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["key"] == key)
        .filter_map(|v| Some((v["text"].as_str()?.to_string(), v["line"].as_i64()?, v["agent"].as_str().is_some_and(|a| !a.is_empty()))))
        .take(most)
        .collect()
}

/// `text` as the agent's transcript shows it, to be found there: Claude Code's "[Pasted text #3
/// +5 lines]" is how its input box folds a paste (the transcript shows the paste itself), so it
/// is left out; "" when nothing else is left.
pub fn findable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("[Pasted text #") {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        match after.find(']') {
            Some(j) if after[..j].bytes().all(|b| b != b'\n') => rest = &after[j + 1..],
            _ => { out.push_str(after); rest = ""; }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// The numbers N of every "Quote #N" in `text`.
pub fn quote_numbers(text: &str) -> Vec<u64> {
    text.match_indices("Quote #").filter_map(|(i, m)| {
        let digits: String = text[i + m.len()..].chars().take_while(char::is_ascii_digit).take(9).collect();
        digits.parse().ok()
    }).collect()
}

/// The letters and digits of `text`'s first line that has any.
fn first_loose(text: &str) -> String {
    text.lines().map(crate::page::bookmarks_find::loose).find(|l| !l.is_empty()).unwrap_or_default()
}

fn append(file: &Path, line: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(file)?;
    f.write_all(line.as_bytes())
}

/// Cuts the file back to its KEEP newest entries once it holds TRIM, and to its newest half of
/// BYTES once larger (a page reads it whole for each one it shows).
fn trim(file: &Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(file)?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < TRIM && text.len() <= BYTES {
        return Ok(());
    }
    let mut first = lines.len().saturating_sub(KEEP);
    let mut size: usize = lines[first..].iter().map(|l| l.len() + 1).sum();
    while text.len() > BYTES && size > BYTES / 2 && first + 1 < lines.len() {
        size -= lines[first].len() + 1;
        first += 1;
    }
    let kept = lines[first..].join("\n") + "\n";
    crate::config::write_private(file, kept.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_secret_values_are_fully_redacted_before_storage() {
        for text in ["tool --password 'alpha beta' tail", "tool --password \"alpha beta\" tail", "PASSWORD='alpha beta' tool", "PASSWORD=\"alpha beta\" tool"] {
            let redacted = redact(text);
            assert!(!redacted.contains("alpha") && !redacted.contains("beta"), "{redacted}");
        }
        let (s, dir) = store();
        assert_eq!(s.accept("k", "tool --password 'alpha beta' tail").as_deref(), Some("tool --password [redacted] tail"));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn store() -> (Sent, PathBuf) {
        let dir = crate::testutil::temp_root().join(format!("tm-sent-{}-{}", std::process::id(), rand::random::<u32>()));
        (Sent::new(dir.clone()), dir)
    }

    #[test]
    fn left_out_a_space_an_empty_line_the_same_again_and_secrets() {
        let (s, dir) = store();
        assert_eq!(s.accept("k", " secret command"), None);
        assert_eq!(s.accept("k", "   "), None);
        assert_eq!(s.accept("k", "make test  \n").as_deref(), Some("make test"));
        assert_eq!(s.accept("k", "make test"), None, "one Enter told by the hook and the page");
        assert!(s.accept("k2", "make test").is_some(), "another pane");
        assert_eq!(redact("export GITHUB_TOKEN=ghp_abc def"), "export GITHUB_TOKEN=[redacted] def");
        assert_eq!(redact("mysql --password hunter2 -u root"), "mysql --password [redacted] -u root");
        let key = format!("sk-{}", "ant-api03-abcdefghijklmnop");          // no secret's shape in the source
        assert_eq!(redact(&format!("curl -H 'x' {key}")), "curl -H 'x' [redacted]");
        assert_eq!(redact("ask about the token flow"), "ask about the token flow", "a word alone is no secret");
        assert_eq!(redact("git commit --author alex"), "git commit --author alex");
        assert_eq!(redact(&format!("note\n{key}\tok")), "note\n[redacted]\tok", "any whitespace, kept as it was");
        assert_eq!(redact(r#"{"api_key": "abc", "x": 1}"#), r#"{"api_key": [redacted] "x": 1}"#);
        assert_eq!(redact("curl -H 'Authorization: Bearer abc.def'"), "curl -H 'Authorization: Bearer [redacted]");
        let pem = ["-----BEGIN OPENSSH ", "PRIVATE KEY-----\nb3Blbn\n-----END OPENSSH ", "PRIVATE KEY-----"].concat();
        assert_eq!(redact(&format!("key:\n{pem}\ndone")), "key:\n[redacted]\ndone");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A hooked pane's prompt takes the line the page told for the same text, not another
    /// one's (an agent's own turn, a queued prompt).
    #[test]
    fn a_line_handed_to_the_hooks_prompt_by_its_text() {
        let (s, dir) = store();
        s.hint("k1", "fix the\nlint", 300);
        s.hint("k1", "Explain   this", 310);
        assert_eq!(s.take_hint("k1", "Background task done: report"), None, "the agent's own turn takes none");
        assert_eq!(s.take_hint("k2", "fix the lint"), None, "another pane's");
        assert_eq!(s.take_hint("k1", "explain this, then the rest"), Some(310));
        assert_eq!(s.take_hint("k1", "fix the\nlint"), Some(300));
        assert_eq!(s.take_hint("k1", "fix the\nlint"), None, "taken once");
        for i in 0..6 { s.hint("k3", &format!("p{i}"), i); }
        assert_eq!((s.take_hint("k3", "p0"), s.take_hint("k3", "p5")), (None, Some(5)), "the newest four kept");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Ask's numbers count up per pane: from the highest it sent, and from the last given
    /// before it was sent.
    #[test]
    fn quote_numbers_count_up_per_pane() {
        let (s, dir) = store();
        let next = |s: &Sent, k: &str, sent: u64| s.next_question("local", "/p", k, sent).unwrap();
        assert_eq!(next(&s, "k1", 0), 1);
        assert_eq!(next(&s, "k1", 0), 2, "given, not sent yet");
        let again = Sent::new(dir.clone());                              // terminumd started anew
        assert_eq!(next(&again, "k1", 0), 3, "kept across a restart (a folded paste never reaches the history)");
        assert_eq!(next(&s, "k2", 41), 42, "another pane counts its own, from what it sent");
        assert_eq!(next(&s, "k1", 0), 4);
        assert_eq!(quote_numbers("Quote #0007: Quote # 9 Quote #x Question #8"), [7]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Quotes asked about (AC-37) are kept per pane, across a restart: one number once (the
    /// last text), the newest ASKED, a long text cut.
    #[test]
    fn asked_quotes_kept_per_pane() {
        let (s, dir) = store();
        s.keep_asked("local", "/p", "k1", 1, "first", 10).unwrap();
        s.keep_asked("local", "/p", "k2", 1, "other pane", 3).unwrap();
        s.keep_asked("local", "/p", "k1", 1, "first, again", 12).unwrap();
        let again = Sent::new(dir.clone());
        assert_eq!(again.asked_of("local", "/p", "k1"), [json!({"n": 1, "text": "first, again", "line": 12})]);
        for n in 2..=(ASKED as u64 + 1) { s.keep_asked("local", "/p", "k1", n, &"x".repeat(ASKED_TEXT + 5), n as i64).unwrap(); }
        let kept = s.asked_of("local", "/p", "k1");
        assert_eq!((kept.len(), kept[0]["n"].as_u64(), kept[0]["text"].as_str().map(str::len)), (ASKED, Some(2), Some(ASKED_TEXT)));
        assert_eq!(s.asked_of("local", "/p", "k2").len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Answer ends (AC-90) are kept per pane, apart from the message history.
    #[test]
    fn answer_ends_kept_per_pane_apart_from_what_was_sent() {
        let (s, dir) = store();
        s.add_end("local", "/p", "k1", 120, "All 42 pass.").unwrap();
        s.add_end("local", "/p", "k2", 7, "Done here.").unwrap();
        s.add_end("local", "/p", "k1", 300, "The essay is written.").unwrap();
        assert_eq!(s.ends_of_pane("local", "/p", "k1", 10), [("The essay is written.".to_string(), 300, false), ("All 42 pass.".to_string(), 120, false)]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_folded_paste_is_not_looked_for() {
        assert_eq!(findable("[Pasted text #71 +5 lines]"), "");
        assert_eq!(findable("see [Pasted text #3 +12 lines] and fix it"), "see  and fix it");
        assert_eq!(findable("[Image #70] - fix the font"), "[Image #70] - fix the font", "an image's mark is in the transcript too");
        assert_eq!(findable("[Pasted text #1 no end"), "[Pasted text #1 no end");
    }
}
