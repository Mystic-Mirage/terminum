// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The tokens of a turn's session (`terminum-dispatcher.md` AC-129), summed beside the transcript on
//! its own machine within `terminum hook`'s time. Integers and models' names leave it, the whole and each
//! model's: no path, no text.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Seek, SeekFrom};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::dispatcher::usage::{Totals, MOST_MODEL};

/// Past these it says nothing rather than overrun the hook's 3 s.
const MOST_BYTES: u64 = 256 << 20;
const MOST_TIME: Duration = Duration::from_millis(1500);
const CODEX_TAIL: u64 = 1 << 20;

/// Each model's totals, `""` for none named.
type Sum = BTreeMap<String, Totals>;

fn int(v: &Value, key: &str) -> i64 {
    v[key].as_u64().map_or(0, |n| n.min(i64::MAX as u64) as i64)
}

/// Each line holding `needle`, parsed, to `each`; past `until`: None.
fn each_line<T: DeserializeOwned>(mut reader: impl BufRead, until: Instant, needle: &str, mut each: impl FnMut(usize, T)) -> Option<()> {
    let (mut line, mut at) = (String::new(), 0);
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return Some(());
        }
        if Instant::now() >= until {
            return None;
        }
        if line.contains(needle) && let Ok(v) = serde_json::from_str(&line) {
            each(at, v);
        }
        at += 1;
    }
}

/// Of a Claude Code line only these are parsed: a message's text is skipped unread.
#[derive(Deserialize)]
struct ClaudeLine {
    message: ClaudeMessage,
}

#[derive(Deserialize)]
struct ClaudeMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<ClaudeUsage>,
}

#[derive(Deserialize)]
struct ClaudeUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_creation: Option<CacheCreation>,
}

/// Of `cache_creation_input_tokens`, those kept an hour.
#[derive(Deserialize)]
struct CacheCreation {
    ephemeral_1h_input_tokens: Option<u64>,
}

/// Claude Code's lines: `message.usage` per message, priced by its own model; a message comes in several lines,
/// the last stands.
fn claude(reader: impl BufRead, until: Instant) -> Option<Sum> {
    let mut by_message: HashMap<String, (String, Totals)> = HashMap::new();
    each_line(reader, until, "\"usage\"", |at, line: ClaudeLine| {
        if let Some(u) = line.message.usage {
            let n = |v: Option<u64>| v.map_or(0, |n| n.min(i64::MAX as u64) as i64);
            let cache_write = n(u.cache_creation_input_tokens);
            let hour = n(u.cache_creation.and_then(|c| c.ephemeral_1h_input_tokens)).min(cache_write);
            by_message.insert(line.message.id.unwrap_or_else(|| at.to_string()), (line.message.model.map(|m| m.chars().take(MOST_MODEL).collect()).unwrap_or_default(),
                Totals { input: n(u.input_tokens), output: n(u.output_tokens), cache_write, cache_write_1h: hour, cache_read: n(u.cache_read_input_tokens) }));
        }
    })?;
    if by_message.is_empty() {
        return None;
    }
    let mut sum = Sum::new();
    for (m, t) in by_message.into_values() {
        let of = sum.entry(m).or_default();
        *of = *of + t;
    }
    Some(sum)
}

/// Codex's lines: a `token_count` event holds the session's total; the last stands.
fn codex(reader: impl BufRead, until: Instant) -> Option<Sum> {
    let mut last = None;
    each_line(reader, until, "token_count", |_, v: Value| {
        if let Some(u) = v.pointer("/payload/info/total_token_usage") {
            let tokens = Totals { input: int(u, "input_tokens"), output: int(u, "output_tokens"), cache_read: int(u, "cached_input_tokens"), ..Totals::default() };
            last = Some(Sum::from([(String::new(), tokens)]));
        }
    })?;
    last
}

/// What goes with a turn's end: the whole and each model's numbers, or `null` when they are not known (a new
/// agent version's format, a file gone, too long a read); nothing when the event names no transcript.
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
    Some(read().map_or(Value::Null, |s| {
        let mut v = s.values().fold(Totals::default(), |a, t| a + *t).json();
        v["models"] = s.iter().map(|(m, t)| {
            let mut e = t.json();
            e["model"] = json!((!m.is_empty()).then_some(m));
            e
        }).collect();
        v
    }))
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
        let whole = json!({"input": 13, "output": 11, "cache_write": 2, "cache_write_1h": 0, "cache_read": 5});
        let mut one = whole.clone();
        one["model"] = Value::Null;
        let mut said = whole;
        said["models"] = json!([one]);
        assert_eq!(got, said, "integers only: no text, no path");
    }

    #[test]
    fn ac129_claude_a_line_is_read_for_its_id_and_usage_only() {
        let p = written("tokens-claude-typed", concat!(
            r#"{"type":"user","message":{"content":[{"text":"the word \"usage\" in a prompt"}]}}"#, "\n",
            r#"{"type":"summary","message":"\"usage\" in a string"}"#, "\n",
            r#"{"message":{"content":[{"type":"text","text":"long"}],"usage":{"input_tokens":2,"output_tokens":1,"cache_read_input_tokens":null,"cache_creation":{"ephemeral_5m_input_tokens":9}}}}"#, "\n",
            r#"{"message":{"usage":{"input_tokens":3,"output_tokens":1}}}"#, "\n"));
        let got = of_event("claude", &json!({"transcript_path": p})).unwrap();
        assert_eq!((got["input"].as_i64(), got["output"].as_i64(), got["cache_write"].as_i64()), (Some(5), Some(2), Some(0)), "no id: each line its own; no usage: not counted");
    }

    #[test]
    fn ac129_codex_its_last_total_stands() {
        let p = written("tokens-codex", concat!(
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":5,"cached_input_tokens":1,"output_tokens":2}}}}"#, "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":null}}"#, "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":9,"cached_input_tokens":3,"output_tokens":4}}}}"#, "\n"));
        let got = of_event("codex", &json!({"transcript_path": p})).unwrap();
        assert_eq!((got["input"].as_i64(), got["output"].as_i64(), got["cache_read"].as_i64()), (Some(9), Some(4), Some(3)));
        assert_eq!(got["models"], json!([{"model": null, "input": 9, "output": 4, "cache_write": 0, "cache_write_1h": 0, "cache_read": 3}]), "its model is not read");
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

    #[test]
    fn ac129_claude_its_model_goes_with_the_numbers() {
        let p = written("tokens-model", concat!(
            r#"{"type":"assistant","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":10,"output_tokens":1}}}"#, "\n"));
        assert_eq!(of_event("claude", &json!({"transcript_path": p})).unwrap()["models"][0]["model"], "claude-opus-5-5");
    }

    #[test]
    fn ac129_claude_each_model_and_its_hour_s_cache_apart() {
        let p = written("tokens-models", concat!(
            r#"{"message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":30,"cache_creation":{"ephemeral_5m_input_tokens":10,"ephemeral_1h_input_tokens":20}}}}"#, "\n",
            r#"{"message":{"id":"m2","model":"claude-haiku-5-5","usage":{"input_tokens":3,"output_tokens":2}}}"#, "\n",
            r#"{"message":{"id":"m3","model":"claude-opus-5-5","usage":{"input_tokens":5,"output_tokens":1}}}"#, "\n"));
        let got = of_event("claude", &json!({"transcript_path": p})).unwrap();
        assert_eq!((got["input"].as_i64(), got["cache_write_1h"].as_i64()), (Some(18), Some(20)), "the whole");
        assert_eq!(got["models"], json!([
            {"model": "claude-haiku-5-5", "input": 3, "output": 2, "cache_write": 0, "cache_write_1h": 0, "cache_read": 0},
            {"model": "claude-opus-5-5", "input": 15, "output": 2, "cache_write": 30, "cache_write_1h": 20, "cache_read": 0}]));
    }
}
