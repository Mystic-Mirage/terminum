// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! `terminum hook claude|codex` (AC-39): what a coding agent's hooks run (`terminum hooks
//! install`). It only observes: it reads the event the agent gives on stdin (to its end, so the
//! agent's write never breaks), keeps a few fields of it, cut short (never a tool's output, a
//! file's text or the agent's answer; of a plan tool, its items'
//! words: hookplan.rs), and posts them to every sink at once (the local terminumd's private
//! socket, the Macs' links on a host: sinks.rs) with a budget of 150 ms; it prints at most terminumd's reminder after the agent's reminder event and always
//! exits 0, so the agent goes on as without it.
//! The pane is the one tmux names in the agent's environment (TMUX, TMUX_PANE); outside tmux
//! nothing is sent.

use std::io::{Read, Write};
use std::time::Duration;

use serde_json::{json, Map, Value};

/// The most of the event read (its rest is read and dropped).
const KEEP: usize = 1 << 20;
const BUDGET: Duration = Duration::from_millis(150);

/// Runs the hook for `agent`; always 0 when an agent runs it. Run by a person (an unknown
/// agent, or a terminal for stdin), it says what to run instead: it would wait for the keys.
pub fn main(agent: &str) -> i32 {
    // SAFETY: isatty has no preconditions
    let by_hand = unsafe { libc::isatty(0) } == 1;
    if let Some(why) = refused(agent, by_hand) {
        eprintln!("{why}");
        return 2;
    }
    let mut kept = Vec::new();
    let mut buf = [0u8; 65536];
    let mut stdin = std::io::stdin().lock();
    while let Ok(n) = stdin.read(&mut buf) {
        if n == 0 {
            break;
        }
        if kept.len() < KEEP {
            kept.extend_from_slice(&buf[..n.min(KEEP - kept.len())]);
        }
    }
    let (Ok(input), Ok(tmux), Ok(pane)) = (serde_json::from_slice::<Value>(&kept), std::env::var("TMUX"), std::env::var("TMUX_PANE")) else { return 0 };
    if let Some(ev) = event(agent, &input, &tmux, &pane) {
        let answers = super::sinks::post_all(&super::sinks::all(), "/hook", &ev, BUDGET);
        if let Some(line) = first_reminder(&ev, &answers) {
            // a closed stdout is the agent's choice: never a panic, still 0
            let _ = writeln!(std::io::stdout().lock(), "{line}");
        }
    }
    0
}

/// The reminder line of the first sink whose answer reminds, if any.
pub fn first_reminder(ev: &Value, answers: &[super::sinks::Answer]) -> Option<String> {
    answers.iter().find_map(|a| match a {
        Ok((200, Some(answer))) => remind_line(ev, answer),
        _ => None,
    })
}

/// The one line the agent reads as context when terminumd's answer reminds (AC-97); None
/// without a reminder, or for an event other than the agent's reminder event (Claude's sync
/// PostToolBatch, Codex's PostToolUse: plans_store.rs), whose name the line carries.
pub fn remind_line(ev: &Value, answer: &Value) -> Option<String> {
    let text = answer["remind"].as_str().filter(|t| !t.is_empty())?;
    let name = crate::plans::remind_event(ev["agent"].as_str().unwrap_or(""));
    (ev["event"] == name).then(|| json!({"hookSpecificOutput": {"hookEventName": name, "additionalContext": text}}).to_string())
}

/// Why `terminum hook <agent>` is not an agent's run (`by_hand`: its stdin is a terminal).
pub fn refused(agent: &str, by_hand: bool) -> Option<String> {
    let how = "terminum hook is what a coding agent's hooks run; to set them up: terminum hooks install";
    if crate::tmux::hookstate::agent(agent).is_none() {
        return Some(format!("usage: terminum hook claude|codex (an event on stdin)\n{how}"));
    }
    by_hand.then(|| how.to_string())
}

/// At most `n` characters of `s`, control characters dropped.
fn cut(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\n').take(n).collect()
}

fn text(v: &Value, k: &str, n: usize) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(|s| cut(s, n)).filter(|s| !s.is_empty())
}

/// What a tool does, in a few words: a shell's command (its first two words), a file's name,
/// else the tool's name. Never its whole input.
pub fn tool_summary(name: &str, input: &Value) -> String {
    let command = match input.get("command") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(a)) => Some(a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")),
        _ => None,
    };
    let what = if let Some(c) = command {
        // a word with a value (TOKEN=abc, --password=x) keeps only its name
        c.split_whitespace().take(2).map(|w| w.split_once('=').map_or(w.to_string(), |(k, _)| format!("{k}=…"))).collect::<Vec<_>>().join(" ")
    } else if let Some(p) = input.get("file_path").or_else(|| input.get("path")).and_then(Value::as_str) {
        p.rsplit('/').next().unwrap_or(p).to_string()
    } else {
        String::new()
    };
    cut(&crate::sent::redact(&if what.is_empty() { name.to_string() } else { format!("{name}: {what}") }), 80)
}

/// Whether a message ends in a question mark, of any script, past closing marks and Markdown.
fn ends_asking(message: &str) -> bool {
    message.trim_end_matches(|c: char| c.is_whitespace() || "*_`)]»”\"'」』）".contains(c))
        .ends_with(['?', '？', '؟', '\u{37e}', '՞', '፧', '\u{a60f}', '\u{2cfa}'])
}

/// The fields terminumd gets of `input`, the agent's event, from pane `pane` of the tmux
/// whose socket `tmux` names ("/private/tmp/tmux-501/terminum,123,0"); None when it is no event.
pub fn event(agent: &str, input: &Value, tmux: &str, pane: &str) -> Option<Value> {
    let socket_path = tmux.split(',').next()?;
    let socket = socket_path.rsplit('/').next()?.to_string();
    let name = text(input, "hook_event_name", 40)?;
    if crate::tmux::hookstate::agent(agent).is_none() || socket.is_empty() || !pane.starts_with('%') || !pane[1..].bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut out = Map::new();
    out.insert("agent".into(), json!(agent));
    out.insert("socket".into(), json!(cut(&socket, 64)));
    out.insert("socket_path".into(), json!(cut(socket_path, 300)));
    out.insert("pane".into(), json!(pane));
    out.insert("event".into(), json!(name));
    for (from, to, n) in [("session_id", "session", 100), ("agent_id", "sub", 100), ("agent_type", "subtype", 60),
                          ("notification_type", "kind", 40), ("error", "error", 60), ("reason", "reason", 40), ("cwd", "cwd", 1000), ("model", "model", 80)] {
        if let Some(v) = text(input, from, n) {
            out.insert(to.into(), json!(v));
        }
    }
    if let Some(t) = text(input, "turn_id", 100).or_else(|| text(input, "prompt_id", 100)) {
        out.insert("turn".into(), json!(t));
    }
    let tool = input.get("tool_name").and_then(Value::as_str);
    if let Some(t) = tool {
        out.insert("tool".into(), json!(tool_summary(t, input.get("tool_input").unwrap_or(&Value::Null))));
    }
    // a question it asks the user (Claude Code's AskUserQuestion), else a notification's words
    let asked = tool.filter(|t| matches!(*t, "AskUserQuestion" | "request_user_input" | "request_user_input_async"))
        .and_then(|_| input.pointer("/tool_input/questions/0/question").or_else(|| input.pointer("/tool_input/questions/0/title")))
        .and_then(Value::as_str);
    if let Some(m) = asked.map(|q| cut(q, 200)).or_else(|| text(input, "message", 200)) {
        out.insert("message".into(), json!(m));
    }
    // one bit of its last message: the message itself stays here (AC-39)
    if name == "Stop" && input.get("last_assistant_message").and_then(Value::as_str).is_some_and(ends_asking) {
        out.insert("asks".into(), json!(true));
    }
    if matches!(name.as_str(), "Stop" | "SessionEnd") && let Some(tokens) = super::hooktokens::of_event(agent, input) {
        out.insert("tokens".into(), tokens);
    }
    if let Some(p) = super::hookplan::plan(input) {
        out.insert("plan".into(), p);
    }
    if name == "UserPromptSubmit"
        && let Some(p) = input.get("prompt").and_then(Value::as_str) {
            out.insert("prompt".into(), json!(p.chars().take(crate::sent::TEXT).collect::<String>()));
        }
    Some(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_says_one_bit_of_its_last_message_and_keeps_the_text() {
        const TMUX_ENV: &str = "/tmp/tmux-501/terminum,1,0";
        let stop = |m: &str| json!({"hook_event_name": "Stop", "last_assistant_message": m});
        for m in ["Merge it?", "Слить её в main?", "要合并到 main 吗？", "هل أدمجه؟", "Θέλεις να συνεχίσω\u{37e}", "**Should I push?**\n", "(Keep it?)"] {
            let out = event("codex", &stop(m), TMUX_ENV, "%1").unwrap();
            assert_eq!(out["asks"], true, "{m}");
            assert!(!out.to_string().contains(m.trim()), "the text stays here: {m}");
        }
        for m in ["Done.", "Why did it fail? A null pointer; fixed.", "Готово", "let x = 1;"] {
            assert!(event("claude", &stop(m), TMUX_ENV, "%1").unwrap().get("asks").is_none(), "{m}");
        }
    }

    #[test]
    fn codexs_question_tools_say_their_title() {
        let e = json!({"hook_event_name": "PreToolUse", "tool_name": "request_user_input_async",
                       "tool_input": {"questions": [{"title": "Mask the tokens?", "options": ["Yes", "No"]}]}});
        assert_eq!(event("codex", &e, "/tmp/tmux-501/terminum,1,0", "%1").unwrap()["message"], "Mask the tokens?");
    }

    const TMUX: &str = "/private/tmp/tmux-501/terminum,123,0";

    #[test]
    fn only_a_few_fields_go_and_never_a_tools_output() {
        let input = json!({"hook_event_name": "PostToolUse", "session_id": "s1", "prompt_id": "p1", "cwd": "/Users/alex/api",
            "tool_name": "Bash", "tool_input": {"command": "npm test --watch=false SECRET=abc"}, "tool_response": {"stdout": "all the output"},
            "transcript_path": "/Users/alex/.claude/x.jsonl", "last_assistant_message": "done"});
        let ev = event("claude", &input, TMUX, "%3").unwrap();
        assert_eq!(ev, json!({"agent": "claude", "socket": "terminum", "socket_path": "/private/tmp/tmux-501/terminum", "pane": "%3", "event": "PostToolUse", "session": "s1", "turn": "p1",
            "cwd": "/Users/alex/api", "tool": "Bash: npm test"}));
        assert!(!ev.to_string().contains("output") && !ev.to_string().contains("SECRET") && !ev.to_string().contains("transcript"));
    }

    #[test]
    fn a_question_a_prompt_a_failure_a_file() {
        let ask = json!({"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion", "tool_input": {"questions": [{"question": "Which one?\u{1b}[2J"}]}});
        assert_eq!(event("claude", &ask, TMUX, "%1").unwrap()["message"], "Which one?[2J", "control characters dropped");
        let p = json!({"hook_event_name": "UserPromptSubmit", "prompt": "fix the tests", "turn_id": "t9"});
        let ev = event("codex", &p, TMUX, "%1").unwrap();
        assert_eq!((ev["prompt"].as_str(), ev["turn"].as_str()), (Some("fix the tests"), Some("t9")));
        let f = json!({"hook_event_name": "StopFailure", "error": "rate_limit"});
        assert_eq!(event("claude", &f, TMUX, "%1").unwrap()["error"], "rate_limit");
        assert_eq!(tool_summary("Edit", &json!({"file_path": "/Users/alex/api/src/main.rs", "new_string": "secret"})), "Edit: main.rs");
        assert_eq!(tool_summary("exec_command", &json!({"command": ["bash", "-lc", "ls"]})), "exec_command: bash -lc");
        assert_eq!(tool_summary("Bash", &json!({"command": "export TOKEN=sk-abc123"})), "Bash: export TOKEN=[redacted]", "a value is never sent");
        let key = format!("sk-{}", "ant-api03-abcdefghijkl");               // no secret's shape in the source
        assert_eq!(tool_summary("Bash", &json!({"command": format!("echo {key}")})), "Bash: echo [redacted]");
        assert_eq!(tool_summary("Bash", &json!({"command": "make X=1"})), "Bash: make X=…");
    }

    #[test]
    fn a_plan_tools_items_go_as_plan() {
        let input = json!({"hook_event_name": "PostToolUse", "session_id": "s1", "tool_name": "TodoWrite",
            "tool_input": {"todos": [{"content": "alpha", "status": "pending", "activeForm": "Working on alpha"}]}, "tool_response": {"oldTodos": []}});
        let ev = event("claude", &input, TMUX, "%3").unwrap();
        assert_eq!(ev["plan"], json!({"op": "all", "items": [{"text": "alpha", "status": "pending"}]}));
        assert!(!ev.to_string().contains("oldTodos") && !ev.to_string().contains("activeForm"));
        let bash = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "ls"}, "tool_response": "a"});
        assert!(event("claude", &bash, TMUX, "%3").unwrap().get("plan").is_none());
    }

    #[test]
    fn a_reminder_is_one_line_of_the_events_context() {
        for (agent, name) in [("codex", "PostToolUse"), ("claude", "PostToolBatch")] {
            let ev = json!({"agent": agent, "event": name});
            let line = remind_line(&ev, &json!({"remind": "update it"})).unwrap();
            assert!(!line.contains('\n'));
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap(),
                       json!({"hookSpecificOutput": {"hookEventName": name, "additionalContext": "update it"}}));
            assert_eq!(remind_line(&ev, &json!({})), None, "no reminder: nothing printed");
            assert_eq!(remind_line(&ev, &json!({"remind": ""})), None);
        }
        assert_eq!(remind_line(&json!({"agent": "claude"}), &json!({"remind": "x"})), None, "no event name: nothing printed");
        for (agent, other) in [("claude", "PostToolUse"), ("codex", "PostToolBatch"), ("claude", "Stop"), ("codex", "PreToolUse"), ("claude", "UserPromptSubmit")] {
            assert_eq!(remind_line(&json!({"agent": agent, "event": other}), &json!({"remind": "x"})), None, "{agent} {other}: nothing printed");
        }
    }

    #[test]
    fn any_sinks_reminder_is_printed_none_without_sinks() {
        use super::super::sinks::post_all;
        let ev = json!({"agent": "codex", "event": "PostToolUse"});
        let answers = vec![Err(super::super::sockhttp::Down), Ok((200, Some(json!({})))), Ok((200, Some(json!({"remind": "from a Mac"}))))];
        assert!(first_reminder(&ev, &answers).unwrap().contains("from a Mac"));
        assert_eq!(first_reminder(&ev, &answers[..2]), None);
        let none = post_all(&[], "/hook", &ev, BUDGET);
        assert!(none.is_empty() && first_reminder(&ev, &none).is_none(), "no sinks: nothing posted, nothing printed");
    }

    #[test]
    fn run_by_a_person_it_says_what_to_run() {
        assert!(refused("install", false).unwrap().contains("terminum hooks install"), "terminum hook install: a typo of hooks");
        assert!(refused("claude", true).unwrap().contains("terminum hooks install"), "a terminal for stdin: it would wait for the keys");
        assert_eq!(refused("codex", false), None);
    }

    #[test]
    fn nothing_without_a_pane_an_event_or_a_known_agent() {
        let e = json!({"hook_event_name": "Stop"});
        assert!(event("claude", &e, TMUX, "").is_none());
        assert!(event("claude", &e, TMUX, "%x").is_none());
        assert!(event("other", &e, TMUX, "%1").is_none());
        assert!(event("claude", &json!({"x": 1}), TMUX, "%1").is_none());
        assert!(event("claude", &e, "", "%1").is_none());
    }
}
