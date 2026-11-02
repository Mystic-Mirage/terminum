// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! `terminum mcp` (AC-97, AC-98): the MCP server a coding agent starts (`terminum hooks
//! install`), newline-delimited JSON-RPC 2.0 on stdin and stdout, read to EOF; stdout carries
//! only the protocol. Its four tools check their arguments here, then go (POST /agent, 2 s) to the
//! first sink that knows the pane tmux names in the agent's environment (TMUX, TMUX_PANE): the
//! local terminumd, else a Mac's link on a host (sinks.rs); terminumd keeps the plan and the
//! notes, this process keeps nothing. Tools and instructions are offered unless the sinks say
//! they do not know the pane (one `probe` to each at initialize, 2 s in all): a terminumd
//! restarting or a host's link down when the agent starts never hides them for its session.

use std::io::{BufRead, Read, Write};
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::sockhttp::Down;

const BUDGET: Duration = Duration::from_secs(2);
/// The longest line read; a longer one is dropped to its newline and answered a parse error.
const MAX_LINE: usize = 1 << 20;
const VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];
const DEFAULT_VERSION: &str = "2025-06-18";

pub const INSTRUCTIONS: &str = "terminum shows the user your plan and keeps what you hand them. Keep a plan: your \
own plan tool (TaskCreate/TaskUpdate, update_plan), else update_session_plan; send doing and eta_minutes at each step \
and every 10 minutes. Only the main agent calls update_session_plan; subagents report to it. Hand add_notes_for_user one record \
per final report, finding outside the task, deferred item or question, never progress; update_notes_for_user one \
that changes. If a tool says you are not in terminum, stop calling.";

pub use crate::plans::rules::NOT_IN_PANE;
use crate::agentmarks::{self as marks, KINDS};
use crate::plans::rules::{self, STATUSES};
pub const DOWN: &str = "terminum is not running; go on without it.";

pub const TOOLS: [&str; 4] = ["update_session_plan", "get_session_plan", "add_notes_for_user", "update_notes_for_user"];

/// The agent's environment: TMUX and TMUX_PANE; `known`: whether a sink knows the pane
/// (None until probed).
pub struct Env {
    pub tmux: Option<String>,
    pub pane: Option<String>,
    pub known: std::cell::Cell<Option<bool>>,
}

/// One POST /agent: (status, answer) or Down.
pub type Forward<'a> = &'a dyn Fn(&Value) -> Result<(u16, Option<Value>), Down>;

/// Serves stdin to EOF; 0, or 2 when a person runs it in a terminal (it would wait for keys).
pub fn main() -> i32 {
    // SAFETY: isatty has no preconditions
    if unsafe { libc::isatty(0) } == 1 {
        eprintln!("terminum mcp is the MCP server a coding agent starts; to set it up: terminum hooks install");
        return 2;
    }
    let env = Env { tmux: std::env::var("TMUX").ok(), pane: std::env::var("TMUX_PANE").ok(), known: Default::default() };
    let forward = |body: &Value| {
        // listed at each call: a Mac's link may come up after the agent started
        let sinks = super::sinks::all();
        // a probe is cheap: 2 s for all the sinks; a call: 2 s for each
        let each = if body["tool"] == "probe" { BUDGET / sinks.len().max(1) as u32 } else { BUDGET };
        super::sinks::first_knowing(&sinks, |p| super::sockhttp::call(p, "POST", "/agent", Some(body), each))
    };
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    while let Some(line) = read_line(&mut stdin) {
        if let Some(out) = answer(&line, &env, &forward)
            && (writeln!(stdout, "{out}").is_err() || stdout.flush().is_err()) {
                break;
            }
    }
    0
}

/// The next line (lossy UTF-8, without its newline), None at EOF or a read error; a line over
/// MAX_LINE comes back as "" plus a marker the parser refuses.
fn read_line(r: &mut impl BufRead) -> Option<String> {
    let mut buf = Vec::new();
    let n = r.by_ref().take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf).ok()?;
    if n == 0 {
        return None;
    }
    if buf.len() > MAX_LINE && buf.last() != Some(&b'\n') {
        let mut rest = Vec::new();
        while r.by_ref().take(MAX_LINE as u64).read_until(b'\n', &mut rest).is_ok_and(|k| k > 0) {
            if rest.last() == Some(&b'\n') {
                break;
            }
            rest.clear();
        }
        return Some("\u{0}line too long".into());
    }
    Some(String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string())
}

fn error(id: &Value, code: i64, message: &str) -> Option<String> {
    Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string())
}

fn result(id: &Value, result: Value) -> Option<String> {
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string())
}

/// The answer to one input line, None for a notification, a response or a blank line.
pub fn answer(line: &str, env: &Env, forward: Forward) -> Option<String> {
    if line.trim().is_empty() {
        return None;
    }
    let Ok(msg) = serde_json::from_str::<Value>(line) else { return error(&Value::Null, -32700, "Parse error") };
    let Some(obj) = msg.as_object() else { return error(&Value::Null, -32600, "Invalid Request: one JSON object a line") };
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // a response to a request of ours (none is sent) is dropped
        return match obj.get("id") {
            Some(id) if !obj.contains_key("result") && !obj.contains_key("error") => error(id, -32600, "Invalid Request: no method"),
            _ => None,
        };
    };
    let id = obj.get("id")?;
    if !(id.is_string() || id.is_i64() || id.is_u64()) {
        return error(&Value::Null, -32600, "Invalid Request: id must be a string or an integer");
    }
    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));
    match method {
        "initialize" => result(id, initialize(&params, knows(env, forward))),
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({"tools": if knows(env, forward) { tools() } else { json!([]) }})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            if !TOOLS.contains(&name) {
                return error(id, -32602, &format!("Unknown tool {name:?}: the tools are {}", TOOLS.join(", ")));
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let session = params.pointer("/_meta/sessionId").and_then(Value::as_str);
            result(id, call(name, &args, session, env, forward))
        }
        _ => error(id, -32601, &format!("Method not found: {method}")),
    }
}

/// Whether the agent's pane may be terminum's: false outside a tmux pane or when the sinks up
/// answer that they do not know it (kept), true when one knows it (kept) or none is up (not
/// kept; an agent lists the tools once, so they stay, answering DOWN until one is back).
fn knows(env: &Env, forward: Forward) -> bool {
    let Some((socket, pane)) = pane_of(env) else { return false };
    if let Some(k) = env.known.get() {
        return k;
    }
    let body = json!({"socket": socket, "socket_path": socket_path(env), "pane": pane, "tool": "probe", "args": {}});
    let k = match forward(&body) {
        Err(Down) => return true,
        Ok(a) => a.0 == 200 && a.1.is_some() && !super::sinks::not_in_pane(&a),
    };
    env.known.set(Some(k));
    k
}

/// TMUX's first field: the tmux server's socket path.
fn socket_path(env: &Env) -> String {
    env.tmux.as_deref().and_then(|t| t.split(',').next()).unwrap_or("").chars().take(300).collect()
}

/// Outside a pane a sink knows (`in_pane` false) the session pays for no instructions and no tools.
fn initialize(params: &Value, in_pane: bool) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
    let version = VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(DEFAULT_VERSION);
    let mut r = json!({"protocolVersion": version, "capabilities": {"tools": {}},
                       "serverInfo": {"name": "terminum", "version": crate::config::build_id()}});
    if in_pane {
        r["instructions"] = json!(INSTRUCTIONS);
    }
    r
}

fn text_result(text: &str, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

/// The tmux socket's name and the pane of the agent's environment, None outside a tmux pane.
pub fn pane_of(env: &Env) -> Option<(String, String)> {
    let socket = env.tmux.as_deref()?.split(',').next()?.rsplit('/').next()?;
    let pane = env.pane.as_deref()?;
    let ok = !socket.is_empty() && socket.len() <= 64 && pane.len() > 1 && pane.len() <= 12
        && pane.starts_with('%') && pane[1..].bytes().all(|b| b.is_ascii_digit());
    ok.then(|| (socket.to_string(), pane.to_string()))
}

/// A tools/call's result: refused here, or terminumd's answer.
pub fn call(tool: &str, args: &Value, session: Option<&str>, env: &Env, forward: Forward) -> Value {
    let args = match checked(tool, args) {
        Ok(a) => a,
        Err(why) => return text_result(&why, true),
    };
    let Some((socket, pane)) = pane_of(env) else { return text_result(NOT_IN_PANE, true) };
    let mut body = json!({"socket": socket, "socket_path": socket_path(env), "pane": pane, "tool": tool, "args": args});
    if let Some(s) = session.map(|s| crate::termtext::clean(s, false).chars().take(100).collect::<String>()).filter(|s| !s.is_empty()) {
        body["session"] = json!(s);
    }
    match forward(&body) {
        Err(Down) => text_result(DOWN, true),
        Ok((200, Some(a))) if a.get("text").is_some_and(Value::is_string) => {
            text_result(a["text"].as_str().unwrap_or(""), a.get("error").and_then(Value::as_bool).unwrap_or(false))
        }
        Ok((status, a)) => text_result(&format!("terminum refused the call: {}", super::sockhttp::message(a.as_ref(), status)), true),
    }
}

/// `v`'s string field `k`, cleaned (rules::clean), of at most `max` characters (None: absent
/// or null).
fn opt_text(v: &Value, k: &str, max: usize, at: &str) -> Result<Option<String>, String> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => match rules::clean(s) {
            c if c.is_empty() => Err(format!("{at}{k} is empty")),
            c if c.chars().count() > max => Err(format!("{at}{k} is over {max} characters")),
            c => Ok(Some(c)),
        },
        Some(_) => Err(format!("{at}{k} must be a string")),
    }
}

fn need_text(v: &Value, k: &str, max: usize, at: &str) -> Result<String, String> {
    opt_text(v, k, max, at)?.ok_or_else(|| format!("{at}{k} is required"))
}

fn one_of(v: &Value, k: &str, allowed: &[&str], at: &str) -> Result<Option<String>, String> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if allowed.contains(&s.as_str()) => Ok(Some(s.clone())),
        Some(other) => Err(format!("{at}{k} {other} is not one of {}", allowed.join(", "))),
    }
}

/// The list `k` of `v` with `min`–`max` objects.
fn list<'a>(v: &'a Value, k: &str, min: usize, max: usize, empty: &str) -> Result<&'a Vec<Value>, String> {
    let a = v.get(k).and_then(Value::as_array).ok_or_else(|| format!("{k} is required: a list of {min}–{max} objects ({empty})"))?;
    if a.is_empty() {
        return Err(format!("{k} is empty: {empty}"));
    }
    if a.len() > max {
        return Err(format!("{k} has {} entries, at most {max}", a.len()));
    }
    if let Some(i) = a.iter().position(|x| !x.is_object()) {
        return Err(format!("{k}[{i}] must be an object"));
    }
    Ok(a)
}

fn put(m: &mut Map<String, Value>, k: &str, v: Option<String>) {
    if let Some(v) = v {
        m.insert(k.into(), json!(v));
    }
}

/// The arguments terminumd gets (only the known fields), or why they are refused.
pub fn checked(tool: &str, args: &Value) -> Result<Value, String> {
    if !args.is_object() {
        return Err("arguments must be an object".into());
    }
    let mut out = Map::new();
    match tool {
        "update_session_plan" => {
            let items = list(args, "items", 1, rules::ITEMS_MOST, "send the whole plan, every item with its status")?;
            let mut kept = Vec::new();
            for (i, it) in items.iter().enumerate() {
                let at = format!("items[{i}].");
                let text = need_text(it, "text", rules::ITEM_TEXT, &at)?;
                let status = one_of(it, "status", &STATUSES, &at)?.ok_or_else(|| format!("{at}status is required: one of {}", STATUSES.join(", ")))?;
                kept.push(json!({"text": text, "status": status}));
            }
            out.insert("items".into(), Value::Array(kept));
            put(&mut out, "doing", opt_text(args, "doing", rules::DOING, "")?);
            put(&mut out, "title", opt_text(args, "title", rules::TITLE, "")?);
            match args.get("eta_minutes") {
                None | Some(Value::Null) => {}
                Some(v) => match rules::eta_minutes(v) {
                    Some(m) => { out.insert("eta_minutes".into(), json!(m)); }
                    None => return Err(format!("eta_minutes {v} is not a whole number of minutes from 1 to {}", rules::ETA_MOST)),
                },
            }
        }
        "get_session_plan" => {}
        "add_notes_for_user" => {
            let notes = list(args, "notes", 1, marks::RECORDS_MOST, "give 1–20 records")?;
            let mut kept = Vec::new();
            for (i, n) in notes.iter().enumerate() {
                let at = format!("notes[{i}].");
                let mut note = Map::new();
                note.insert("title".into(), json!(need_text(n, "title", marks::TITLE, &at)?));
                note.insert("text".into(), json!(need_text(n, "text", marks::TEXT, &at)?));
                put(&mut note, "kind", one_of(n, "kind", &KINDS, &at)?);
                put(&mut note, "file", opt_text(n, "file", marks::FILE, &at)?);
                match n.get("remind") {
                    None | Some(Value::Null) => {}
                    Some(Value::Bool(b)) => { note.insert("remind".into(), json!(b)); }
                    Some(other) => return Err(format!("{at}remind {other} must be true or false")),
                }
                kept.push(Value::Object(note));
            }
            out.insert("notes".into(), Value::Array(kept));
        }
        "update_notes_for_user" => {
            let id = match args.get("id") {
                Some(Value::String(s)) if !s.trim().is_empty() && s.chars().count() <= 64 => json!(s.trim()),
                Some(n @ Value::Number(_)) if n.is_u64() => json!(n.to_string()),
                _ => return Err("id is required: a record's id as add_notes_for_user answered it".into()),
            };
            out.insert("id".into(), id);
            put(&mut out, "status", one_of(args, "status", &marks::STATUSES, "")?);
            put(&mut out, "title", opt_text(args, "title", marks::TITLE, "")?);
            put(&mut out, "text", opt_text(args, "text", marks::TEXT, "")?);
            match args.get("remind") {
                None | Some(Value::Null) => {}
                Some(Value::Bool(b)) => { out.insert("remind".into(), json!(b)); }
                Some(other) => return Err(format!("remind {other} must be true or false")),
            }
            if out.len() == 1 {
                return Err("nothing to change: give status (open, done), title, text or remind".into());
            }
        }
        other => return Err(format!("unknown tool {other}")),
    }
    Ok(Value::Object(out))
}

/// The four tools of tools/list.
pub fn tools() -> Value {
    let notes = json!({"type": "string", "enum": KINDS});
    let annotations = json!({"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false});
    let tool = |name: &str, title: &str, description: &str, properties: Value, required: Value| {
        json!({"name": name, "title": title, "description": description, "annotations": annotations,
               "inputSchema": {"type": "object", "properties": properties, "required": required}})
    };
    json!([
        tool("update_session_plan", "Update the session's plan",
            "Store this session's plan, which terminum shows to the user. Send the whole plan each call, never a patch: \
every item with status pending, in_progress, completed or blocked. doing: the current step. eta_minutes: your estimate \
of the minutes left. With your own plan tool its items count; this call then stores doing, eta_minutes and title.",
            json!({
                "items": {"type": "array", "minItems": 1, "maxItems": 50, "description": "The whole plan, in order.",
                    "items": {"type": "object", "required": ["text", "status"], "properties": {
                        "text": {"type": "string", "minLength": 1, "maxLength": 200},
                        "status": {"type": "string", "enum": STATUSES}}}},
                "doing": {"type": "string", "maxLength": 120, "description": "The current step."},
                "eta_minutes": {"type": "integer", "minimum": 1, "maximum": 1440},
                "title": {"type": "string", "maxLength": 80}}),
            json!(["items"])),
        tool("get_session_plan", "Get the session's plan",
            "This session's plan as terminum stores it: [x] completed, [>] in progress, [ ] pending, [!] blocked.",
            json!({}), json!([])),
        tool("add_notes_for_user", "Hand the user records to keep",
            "Keep for the user, instead of only printing it, what must not get lost in the scroll: a final report, a next \
step, a gap or bug seen elsewhere, deferred work, a question. A bookmark of this session; remind: true only if the user \
must act or decide (a blocker, a question, a risky leftover): it comes up at each turn end until resolved. When unsure, \
a bookmark.",
            json!({"notes": {"type": "array", "minItems": 1, "maxItems": 20, "items": {"type": "object", "required": ["title", "text"],
                "properties": {"title": {"type": "string", "minLength": 1, "maxLength": 120, "description": "One line: what it is about."},
                    "text": {"type": "string", "minLength": 1, "maxLength": 2000,
                        "description": "Markdown readable without this chat: what, where (files, paths), why, next step."},
                    "remind": {"type": "boolean"},
                    "kind": notes, "file": {"type": "string", "maxLength": 300}}}}}),
            json!(["notes"])),
        tool("update_notes_for_user", "Update a record kept for the user",
            "Change a record add_notes_for_user answered by id: status done (a reminder resolved, a bookmark deleted) or open, \
a new title, text or remind.",
            json!({"id": {"type": "string"}, "status": {"type": "string", "enum": marks::STATUSES},
                "title": {"type": "string", "minLength": 1, "maxLength": 120}, "text": {"type": "string", "minLength": 1, "maxLength": 2000},
                "remind": {"type": "boolean"}}),
            json!(["id"])),
    ])
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
