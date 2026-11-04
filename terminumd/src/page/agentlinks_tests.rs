// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;
use serde_json::{Value, json};

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("tm-agentlinks-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("projects/-Users-alex-api")).unwrap();
    d
}

fn lines(v: &[Value]) -> String {
    v.iter().map(|x| x.to_string() + "\n").collect()
}

/// AC-102: the transcript's addresses, the newest first: the user's and the agent's text; not a
/// tool's call or output, the agent's thinking, a meta line or a compaction's summary.
#[test]
fn reads_conversation_only() {
    let d = dir();
    let t = lines(&[
        json!({"type": "user", "message": {"role": "user", "content": "look at https://ci.example/run/7."}}),
        json!({"type": "assistant", "message": {"content": [
            {"type": "thinking", "thinking": "maybe https://thought.example/"},
            {"type": "text", "text": "The PR: https://git.example/pr/4"},
            {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "gh pr view https://git.example/pr/4"}},
            {"type": "tool_use", "id": "t2", "name": "Read", "input": {"file_path": "/Users/alex/api/package-lock.json"}}]}}),
        json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "url:\thttps://git.example/pr/4\nchecks https://ci.example/run/9"}]},
            {"type": "tool_result", "tool_use_id": "t2", "content": "\"resolved\": \"https://registry.example/a.tgz\""}]}}),
        json!({"type": "user", "message": {"content": [{"type": "text", "text": "and https://ci.example/run/7"}]}}),
        json!({"type": "user", "isMeta": true, "message": {"content": "https://meta.example/"}}),
        json!({"type": "user", "isCompactSummary": true, "message": {"content": "Summary: ran https://git.example/pr/4"}}),
        json!({"type": "ai-title", "title": "https://title.example/"}),
    ]);
    let p = d.join("projects/-Users-alex-api/0b1c-2d.jsonl");
    std::fs::write(&p, t + "{not json\n").unwrap();
    assert_eq!(transcript(&d, "0b1c-2d"), Some(p.clone()));
    let (got, total) = read(&p, false).unwrap();
    let got: Vec<(String, usize)> = got.into_iter().map(|l| (l.url, l.count)).collect();
    assert_eq!((got, total), (vec![
        ("https://ci.example/run/7".into(), 2),
        ("https://git.example/pr/4".into(), 1),
    ], 2));
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102 edge: an id that is no id finds nothing; a transcript not there says why.
#[test]
fn no_transcript() {
    let d = dir();
    assert_eq!(transcript(&d, "../x"), None);
    assert_eq!(transcript(&d, ""), None);
    assert_eq!(transcript(&d, "nope"), None);
    assert!(read(&d.join("none.jsonl"), false).is_err());
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102: a pane whose hooks have not named its session yet: its process's (or a child's) record says it.
#[test]
fn session_from_process_record() {
    let d = dir();
    std::fs::create_dir_all(d.join("sessions")).unwrap();
    let me = std::process::id();
    assert_eq!(session_of(&d, me), None);
    let Some(started) = proc_start(me) else { return };    // a system that says no start: by the program's name
    std::fs::write(d.join(format!("sessions/{me}.json")), json!({"pid": me, "sessionId": "0b1c-2d", "procStart": started}).to_string()).unwrap();
    assert_eq!(session_of(&d, me), Some((me, "0b1c-2d".into())));
    assert_eq!(record(&d, me).as_deref(), Some("0b1c-2d"));
    let mut child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
    std::fs::remove_file(d.join(format!("sessions/{me}.json"))).unwrap();
    std::fs::write(d.join(format!("sessions/{}.json", child.id())), json!({"sessionId": "child-1", "procStart": proc_start(child.id())}).to_string()).unwrap();
    let (pid, got) = (child.id(), session_of(&d, me));
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(got, Some((pid, "child-1".into())));
    assert_eq!(record(&d, pid), None, "ended: its record names no session");
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102 edge: a record left by an ended Claude Code whose pid another process has now names no session.
#[test]
fn stale_record_is_no_session() {
    let d = dir();
    std::fs::create_dir_all(d.join("sessions")).unwrap();
    let me = std::process::id();
    std::fs::write(d.join(format!("sessions/{me}.json")), json!({"pid": me, "sessionId": "old-1", "procStart": "1"}).to_string()).unwrap();
    assert_eq!(session_of(&d, me), None);
    std::fs::write(d.join(format!("sessions/{me}.json")), json!({"pid": me, "sessionId": "old-1"}).to_string()).unwrap();
    assert_eq!(session_of(&d, me), None, "the test's program is not claude either");
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102 edge: a command's written-out "\n" after a Markdown link is not part of the address.
#[test]
fn backslash_ends_address() {
    let mut f = Found::default();
    f.text("printf '[Claude Code](https://claude.com/claude-code)\\nEOF\\npython3'");
    f.text("[Claude Code](https://claude.com/claude-code)\\nEOF\\ngh");
    let (got, total) = f.told(false);
    assert_eq!((got[0].url.as_str(), got[0].count, total), ("https://claude.com/claude-code", 2, 1));
}

/// AC-102 edge: Markdown around an address: bold after it, a link whose text is the address; an IPv6 host keeps its brackets.
#[test]
fn markdown_around_address() {
    let mut f = Found::default();
    f.text("**[DEV-7518](https://linear.app/x/issue/DEV-7518):** done; (https://linear.app/x/issue/DEV-7517)** and");
    f.text("[https://a.example/p](https://a.example/p) on http://[::1]:8080/x.");
    let (got, _) = f.told(false);
    let urls: Vec<(&str, usize)> = got.iter().map(|l| (l.url.as_str(), l.count)).collect();
    assert_eq!(urls, vec![("http://[::1]:8080/x", 1), ("https://a.example/p", 2), ("https://linear.app/x/issue/DEV-7517", 1), ("https://linear.app/x/issue/DEV-7518", 1)]);
}

/// AC-102 edge: templates, shell variables, text cut short and table cells are no addresses.
#[test]
fn templates_are_not_addresses() {
    let mut f = Found::default();
    f.text("curl https://api.example/repos/{owner} https://api.example/q?t=$TOKEN https://${HOST}/a https://[my-app].vercel.app/ https://**bold**");
    f.text("see https://docs.example/very/long… | https://a.example/x|https://b.example/y | https://c.example/q?a=$1");
    f.text("https://www.example.**bold**.com https://app.example/[slug]/edit https://d.example/odata?$filter=a https://u:p@e.example:8443/");
    let (got, _) = f.told(false);
    let urls: Vec<&str> = got.iter().map(|l| l.url.as_str()).collect();
    assert_eq!(urls, vec!["https://u:p@e.example:8443/", "https://d.example/odata?$filter=a", "https://b.example/y", "https://a.example/x"]);
}

/// AC-102: each address has the time it was last named; by count the most named come first, then the newest.
#[test]
fn time_and_count_order() {
    let d = dir();
    let at = |t: &str, text: &str| json!({"type": "user", "timestamp": t, "message": {"content": text}});
    let p = d.join("projects/-Users-alex-api/s1.jsonl");
    std::fs::write(&p, lines(&[
        at("2026-11-01T09:00:00.000Z", "https://a.example/ https://b.example/"),
        at("2026-11-02T10:00:00.000Z", "https://a.example/ again"),
        at("2026-11-03T11:00:00.000Z", "https://c.example/"),
    ])).unwrap();
    let got = |by_count| {
        read(&p, by_count).unwrap().0.into_iter().map(|l| (l.url, l.when.unwrap_or_default())).collect::<Vec<_>>()
    };
    let s = |u: &str, t: &str| (u.to_string(), t.to_string());
    assert_eq!(got(false), vec![s("https://c.example/", "2026-11-03T11:00:00.000Z"), s("https://a.example/", "2026-11-02T10:00:00.000Z"), s("https://b.example/", "2026-11-01T09:00:00.000Z")]);
    assert_eq!(got(true)[..2], [s("https://a.example/", "2026-11-02T10:00:00.000Z"), s("https://c.example/", "2026-11-03T11:00:00.000Z")]);
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102: a read again takes the lines added since, and a line still being written only once whole.
#[test]
fn reads_what_was_added() {
    let d = dir();
    let p = d.join("projects/-Users-alex-api/grow.jsonl");
    let msg = |u: &str| json!({"type": "user", "message": {"content": u}}).to_string();
    std::fs::write(&p, format!("{}\n{}", msg("https://a.example/"), &msg("https://b.example/")[..20])).unwrap();
    let urls = || read(&p, false).unwrap().0.into_iter().map(|l| (l.url, l.count)).collect::<Vec<_>>();
    assert_eq!(urls(), vec![("https://a.example/".to_string(), 1)]);
    std::fs::write(&p, format!("{}\n{}\n{}\n", msg("https://a.example/"), msg("https://b.example/"), msg("https://a.example/"))).unwrap();
    assert_eq!(urls(), vec![("https://a.example/".to_string(), 2), ("https://b.example/".to_string(), 1)]);
    std::fs::write(&p, format!("{}\n", msg("https://c.example/"))).unwrap();
    assert_eq!(urls(), vec![("https://c.example/".to_string(), 1)], "a shorter file is read anew");
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102: a transcript replaced by another file, even a longer one, is read anew.
#[test]
fn replaced_transcript_is_read_anew() {
    let d = dir();
    let p = d.join("projects/-Users-alex-api/swap.jsonl");
    let msg = |u: &str| json!({"type": "user", "message": {"content": u}}).to_string() + "\n";
    std::fs::write(&p, msg("https://old.example/")).unwrap();
    assert_eq!(read(&p, false).unwrap().1, 1);
    let tmp = d.join("swap.tmp");
    std::fs::write(&tmp, msg("https://new.example/") + &msg("https://new.example/b")).unwrap();
    std::fs::rename(&tmp, &p).unwrap();
    let urls: Vec<String> = read(&p, false).unwrap().0.into_iter().map(|l| l.url).collect();
    assert_eq!(urls, vec!["https://new.example/b", "https://new.example/"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102 limit: of a long transcript its newest part: a line cut by the start is left out, a line
/// that starts there is not.
#[test]
fn tail_starts_at_whole_line() {
    let d = dir();
    let p = d.join("projects/-Users-alex-api/tail.jsonl");
    let msg = |u: &str| json!({"type": "user", "message": {"content": u}}).to_string() + "\n";
    let (a, b) = (msg("https://a.example/"), msg("https://b.example/"));
    std::fs::write(&p, a.clone() + &b).unwrap();
    let urls = |most| read_most(&p, most, false).unwrap().0.into_iter().map(|l| l.url).collect::<Vec<_>>();
    assert_eq!(urls(b.len() as u64), vec!["https://b.example/"]);
    READ.lock().unwrap().1 = None;
    assert_eq!(urls(b.len() as u64 + 3), vec!["https://b.example/"], "a's end is cut");
    READ.lock().unwrap().1 = None;
    assert_eq!(urls((a.len() + b.len()) as u64), vec!["https://b.example/", "https://a.example/"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102: a transcript moved to another project's folder is found there; one removed is not found.
#[test]
fn moved_transcript_is_found() {
    let d = dir();
    let (a, b) = (d.join("projects/-Users-alex-api/mv-1.jsonl"), d.join("projects/-Users-alex-web/mv-1.jsonl"));
    std::fs::write(&a, "").unwrap();
    assert_eq!(transcript(&d, "mv-1"), Some(a.clone()));
    std::fs::create_dir_all(b.parent().unwrap()).unwrap();
    std::fs::rename(&a, &b).unwrap();
    assert_eq!(transcript(&d, "mv-1"), Some(b.clone()));
    std::fs::remove_file(&b).unwrap();
    assert_eq!(transcript(&d, "mv-1"), None);
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102: asked again with nothing added, the same answer; by the other order, that order.
#[test]
fn same_answer_when_nothing_added() {
    let d = dir();
    let p = d.join("projects/-Users-alex-api/same.jsonl");
    let msg = |u: &str| json!({"type": "user", "message": {"content": u}}).to_string() + "\n";
    std::fs::write(&p, msg("https://a.example/ https://b.example/") + &msg("https://a.example/")).unwrap();
    let urls = |by| read(&p, by).unwrap().0.into_iter().map(|l| l.url).collect::<Vec<_>>();
    assert_eq!(urls(false), vec!["https://a.example/", "https://b.example/"]);
    assert_eq!(urls(false), vec!["https://a.example/", "https://b.example/"]);
    std::fs::write(&p, msg("https://a.example/ https://b.example/") + &msg("https://b.example/") + &msg("https://b.example/")).unwrap();
    assert_eq!(urls(true), vec!["https://b.example/", "https://a.example/"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102 edge: a line with a field of another type keeps its addresses: an object's content, a number's time, a text block among others.
#[test]
fn line_of_other_shape() {
    let d = dir();
    let p = d.join("projects/-Users-alex-api/shape.jsonl");
    std::fs::write(&p, lines(&[
        json!({"type": "user", "timestamp": 7, "message": {"content": "https://a.example/"}}),
        json!({"type": "assistant", "isMeta": "no", "message": {"content": ["x", {"type": "text", "text": "https://b.example/"}]}}),
        json!({"type": "user", "message": {"content": {"text": "https://c.example/"}}}),
        json!({"type": "user", "isMeta": true, "timestamp": 7, "message": {"content": "https://d.example/"}}),
    ])).unwrap();
    let (got, _) = read(&p, false).unwrap();
    let urls: Vec<(&str, Option<&str>)> = got.iter().map(|l| (l.url.as_str(), l.when.as_deref())).collect();
    assert_eq!(urls, vec![("https://b.example/", None), ("https://a.example/", None)]);
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102: a Codex session's transcript is found by its id under its day's folder; another id or no id is not.
#[test]
fn codex_transcript_by_id() {
    let d = dir();
    let day = d.join("sessions/2026/11/04");
    std::fs::create_dir_all(&day).unwrap();
    let id = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
    let p = day.join(format!("rollout-2026-11-04T09-30-00-{id}.jsonl"));
    std::fs::write(&p, "").unwrap();
    std::fs::write(day.join("rollout-2026-11-04T09-31-00-other.jsonl"), "").unwrap();
    assert_eq!(codex_transcript(&d, id), Some(p));
    assert_eq!(codex_transcript(&d, "0199a1b2"), None, "a part of an id");
    assert_eq!(codex_transcript(&d, "../x"), None);
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102: of an older Codex's transcript, the user's and the agent's messages alone, each once
/// (written as an event and as a response item): not the injected context, reasoning, tool calls
/// and their output, or a compaction's summary.
#[test]
fn codex_reads_conversation_only() {
    let d = dir();
    let p = d.join("codex.jsonl");
    let at = |t: &str, kind: &str, payload: Value| json!({"timestamp": t, "type": kind, "payload": payload});
    std::fs::write(&p, lines(&[
        at("2026-11-04T09:00:00.000Z", "session_meta", json!({"id": "s", "cwd": "/Users/alex/api", "instructions": "see https://instructions.example/"})),
        at("2026-11-04T09:00:01.000Z", "response_item", json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "<environment_context>https://env.example/</environment_context>"}]})),
        at("2026-11-04T09:00:02.000Z", "response_item", json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "look at https://ci.example/run/7"}]})),
        at("2026-11-04T09:00:02.000Z", "event_msg", json!({"type": "user_message", "message": "look at https://ci.example/run/7", "images": []})),
        at("2026-11-04T09:00:03.000Z", "response_item", json!({"type": "reasoning", "summary": [{"type": "summary_text", "text": "maybe https://thought.example/"}]})),
        at("2026-11-04T09:00:04.000Z", "event_msg", json!({"type": "agent_reasoning", "text": "maybe https://thought.example/"})),
        at("2026-11-04T09:00:05.000Z", "response_item", json!({"type": "function_call", "name": "shell", "arguments": "{\"command\":[\"curl\",\"https://api.example/\"]}"})),
        at("2026-11-04T09:00:06.000Z", "response_item", json!({"type": "function_call_output", "output": "https://out.example/"})),
        at("2026-11-04T09:00:07.000Z", "event_msg", json!({"type": "agent_message", "message": "The PR: https://git.example/pr/4."})),
        at("2026-11-04T09:00:08.000Z", "response_item", json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "The PR: https://git.example/pr/4."}]})),
        at("2026-11-04T09:00:09.000Z", "compacted", json!({"message": "Summary: https://git.example/pr/4"})),
        at("2026-11-04T09:00:10.000Z", "event_msg", json!({"type": "user_message", "message": "and https://ci.example/run/7"})),
    ])).unwrap();
    let (got, total) = read(&p, false).unwrap();
    let got: Vec<(&str, usize, Option<&str>)> = got.iter().map(|l| (l.url.as_str(), l.count, l.when.as_deref())).collect();
    assert_eq!((got, total), (vec![
        ("https://ci.example/run/7", 2, Some("2026-11-04T09:00:10.000Z")),
        ("https://git.example/pr/4", 1, Some("2026-11-04T09:00:07.000Z")),
    ], 2));
    let _ = std::fs::remove_dir_all(&d);
}

/// AC-102: of a current Codex's transcript (its messages as response items alone), the user's and
/// the agent's text: not the developer's, the context added to the user's input, a refusal, or
/// the same message as a completed item; a message said again is counted again.
#[test]
fn codex_reads_response_items() {
    let d = dir();
    let p = d.join("codex.jsonl");
    let at = |t: &str, payload: Value| json!({"timestamp": t, "type": "response_item", "payload": payload});
    let msg = |role: &str, kind: &str, text: &str| json!({"type": "message", "role": role, "content": [{"type": kind, "text": text}]});
    std::fs::write(&p, lines(&[
        at("2026-11-06T09:00:01.000Z", msg("developer", "input_text", "Never list https://developer.example/")),
        at("2026-11-06T09:00:02.000Z", msg("user", "input_text", "# AGENTS.md instructions for /Users/alex/api\nhttps://agents.example/")),
        at("2026-11-06T09:00:03.000Z", msg("user", "input_text", "  <environment_context>https://env.example/</environment_context>")),
        at("2026-11-06T09:00:04.000Z", msg("user", "input_text", "<skill>https://skill.example/</skill>")),
        at("2026-11-06T09:00:05.000Z", msg("user", "input_text", "Now https://user.example/a")),
        json!({"timestamp": "2026-11-06T09:00:05.000Z", "type": "event_msg", "payload": {"type": "item_completed", "item": {"type": "UserMessage", "content": [{"type": "text", "text": "Now https://user.example/a"}]}}}),
        at("2026-11-06T09:00:06.000Z", json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Done: https://assistant.example/a."}, {"type": "refusal", "refusal": "https://refusal.example/"}]})),
        at("2026-11-06T09:00:07.000Z", msg("user", "input_text", "Now https://user.example/a")),
    ])).unwrap();
    let (got, total) = read(&p, false).unwrap();
    let got: Vec<(&str, usize, Option<&str>)> = got.iter().map(|l| (l.url.as_str(), l.count, l.when.as_deref())).collect();
    assert_eq!((got, total), (vec![
        ("https://user.example/a", 2, Some("2026-11-06T09:00:07.000Z")),
        ("https://assistant.example/a", 1, Some("2026-11-06T09:00:06.000Z")),
    ], 2));
    let _ = std::fs::remove_dir_all(&d);
}
