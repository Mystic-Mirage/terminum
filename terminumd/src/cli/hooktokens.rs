// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The tokens of a turn's session (`terminum-dispatcher.md` AC-129), summed beside the transcript on
//! its own machine within `terminum hook`'s time. Three integers leave it: no path, no text.

use std::collections::HashMap;
use std::io::{BufRead, Seek, SeekFrom};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Past these it says nothing rather than overrun the hook's 3 s.
const MOST_BYTES: u64 = 256 << 20;
const MOST_TIME: Duration = Duration::from_millis(1500);
const CODEX_TAIL: u64 = 1 << 20;

#[derive(Default, Clone, Copy)]
struct Sum {
    input: u64,
    output: u64,
    cached: u64,
}

fn int(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or(0)
}

/// Each line holding `needle`, parsed, to `each`; past `until`: None.
fn each_line(reader: impl BufRead, until: Instant, needle: &str, mut each: impl FnMut(usize, &Value)) -> Option<()> {
    for (at, line) in reader.lines().enumerate() {
        let line = line.ok()?;
        if Instant::now() >= until {
            return None;
        }
        if line.contains(needle) && let Ok(v) = serde_json::from_str::<Value>(&line) {
            each(at, &v);
        }
    }
    Some(())
}

/// Claude Code's lines: `message.usage` per message; a message comes in several lines, the last stands.
fn claude(reader: impl BufRead, until: Instant) -> Option<Sum> {
    let mut by_message: HashMap<String, Sum> = HashMap::new();
    each_line(reader, until, "\"usage\"", |at, v| {
        if let Some(u) = v.pointer("/message/usage") {
            let id = v.pointer("/message/id").and_then(Value::as_str).map_or_else(|| at.to_string(), str::to_string);
            by_message.insert(id, Sum { input: int(u, "input_tokens"), output: int(u, "output_tokens"), cached: int(u, "cache_read_input_tokens") + int(u, "cache_creation_input_tokens") });
        }
    })?;
    (!by_message.is_empty()).then(|| by_message.values().fold(Sum::default(), |a, s| Sum { input: a.input + s.input, output: a.output + s.output, cached: a.cached + s.cached }))
}

/// Codex's lines: a `token_count` event holds the session's total; the last stands.
fn codex(reader: impl BufRead, until: Instant) -> Option<Sum> {
    let mut last = None;
    each_line(reader, until, "token_count", |_, v| {
        if let Some(u) = v.pointer("/payload/info/total_token_usage") {
            last = Some(Sum { input: int(u, "input_tokens"), output: int(u, "output_tokens"), cached: int(u, "cached_input_tokens") });
        }
    })?;
    last
}

/// What goes with a turn's end: the three numbers, or `null` when they are not known (a new agent
/// version's format, a file gone, too long a read); nothing when the event names no transcript.
pub fn of_event(agent: &str, input: &Value) -> Option<Value> {
    let path = input.get("transcript_path").and_then(Value::as_str)?;
    let until = Instant::now() + MOST_TIME;
    let read = || -> Option<Sum> {
        // a regular file: a FIFO or a device named there would block past any limit
        let meta = std::fs::metadata(path).ok().filter(|m| m.is_file() && m.len() <= MOST_BYTES)?;
        let mut file = std::fs::File::open(path).ok()?;
        if agent == "codex" {
            // the last total stands: the end of the file holds it, at no cost as a session grows
            file.seek(SeekFrom::Start(meta.len().saturating_sub(CODEX_TAIL))).ok()?;
            let tail = codex(std::io::BufReader::new(&file), until);
            if tail.is_some() {
                return tail;
            }
            file.seek(SeekFrom::Start(0)).ok()?;
            return codex(std::io::BufReader::new(&file), until);
        }
        claude(std::io::BufReader::new(file), until)
    };
    Some(read().map_or(Value::Null, |s| json!({"input": s.input, "output": s.output, "cached": s.cached})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(name: &str, text: &str) -> String {
        let path = crate::testutil::fresh_dir(name).join("t.jsonl");
        std::fs::write(&path, text).unwrap();
        path.display().to_string()
    }

    #[test]
    fn ac129_claude_a_message_counts_once_at_its_last_line() {
        let p = written("tokens-claude", concat!(
            r#"{"type":"user","message":{"content":"secret words"}}"#, "\n",
            r#"{"type":"assistant","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":1,"cache_read_input_tokens":5,"cache_creation_input_tokens":2}}}"#, "\n",
            r#"{"type":"assistant","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":7,"cache_read_input_tokens":5,"cache_creation_input_tokens":2}}}"#, "\n",
            r#"{"type":"assistant","message":{"id":"m2","usage":{"input_tokens":3,"output_tokens":4}}}"#, "\n"));
        let got = of_event("claude", &json!({"transcript_path": p})).unwrap();
        assert_eq!(got, json!({"input": 13, "output": 11, "cached": 7}), "three integers: no text, no path");
    }

    #[test]
    fn ac129_codex_its_last_total_stands() {
        let p = written("tokens-codex", concat!(
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":5,"cached_input_tokens":1,"output_tokens":2}}}}"#, "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":null}}"#, "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":9,"cached_input_tokens":3,"output_tokens":4}}}}"#, "\n"));
        assert_eq!(of_event("codex", &json!({"transcript_path": p})).unwrap(), json!({"input": 9, "output": 4, "cached": 3}));
    }

    #[test]
    fn ac129_what_cannot_be_read_is_said_as_not_known() {
        assert_eq!(of_event("claude", &json!({})), None, "no transcript named: nothing to say");
        assert_eq!(of_event("claude", &json!({"transcript_path": "/no/such/file"})), Some(Value::Null), "the file gone");
        assert_eq!(of_event("claude", &json!({"transcript_path": "/dev/zero"})), Some(Value::Null), "no regular file: never read");
        let p = written("tokens-odd", "{\"another\":\"format\"}\nnot json at all\n");
        assert_eq!(of_event("claude", &json!({"transcript_path": p.clone()})), Some(Value::Null), "a new version's format");
        assert_eq!(of_event("codex", &json!({"transcript_path": p})), Some(Value::Null));
    }
}
