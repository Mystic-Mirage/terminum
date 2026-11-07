// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! POST /agent on the private socket (AC-97, AC-98): `terminum mcp`'s tool calls for the pane
//! tmux named in the agent's environment, validated as /hook validates its pane. Always 200
//! {"text", "error"}: the text is the tool's answer to the agent.

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};


use crate::app::Shared;
use crate::plans::{Plans, Update};

use crate::plans::rules::{self, NOT_IN_PANE, STATUSES};

/// The caller: its pane key, agent and session; whether terminumd has a hook
/// event of the pane, and whether the call's own session (Codex's _meta.sessionId) is another
/// than the hooked one (a subagent's).
pub struct Caller {
    pub key: String,
    pub agent: String,
    pub session: Option<String>,
    pub hooks: bool,
    pub other: bool,
}

/// The plan answers' last line when no hook event came from the pane (F8).
pub const HOOKS_OFF: &str = "terminum's hooks are not running in this session (Codex: trust them in /hooks); send the whole plan with update_session_plan.";
/// The answer to a plan update from another session than the pane's hooked one.
pub const OTHER_SESSION: &str = "Not applied: this call comes from another session than the pane's main agent (a subagent's). Only the main agent calls update_session_plan; report to it instead.";

/// The answer to a note tool of an agent started before the upgrade (AC-98).
pub const NOTES_GONE: &str = "This tool is gone. Use create_task for what must not get lost.";

pub async fn agent(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let server = crate::links::resolve(&app, crate::links::Via::Local, &body).await;
    agent_on(app, server, body).await
}

/// A tool call for the server it was resolved to (None: no server of the call's).
pub async fn agent_on(app: Shared, server: Option<std::sync::Arc<crate::tmux::server::Server>>, body: Value) -> Json<Value> {
    let pane = body["pane"].as_str().unwrap_or("");
    let Some(server) = server.filter(|s| s.session_of(pane).is_some()) else { return answer(NOT_IN_PANE, true) };
    if body["tool"] == "probe" {
        // `terminum mcp` asks at initialize whether to offer its tools here
        return answer("terminum knows this pane.", false);
    }
    let key = server.key(pane);
    let hooked = app.hooks.session(&key);
    let asked: Option<String> = body["session"].as_str().map(|s| crate::termtext::clean(s, false).chars().take(100).collect());
    let main = hooked.as_ref().map(|(_, s)| s.clone()).filter(|s| !s.is_empty());
    let other = matches!((&main, &asked), (Some(m), Some(a)) if !a.is_empty() && m != a);
    let (hooks, session) = (hooked.is_some(), main.or(asked));
    let caller = Caller { agent: hooked.map_or(crate::plans::UNKNOWN, |(a, _)| a).to_string(), session, key, hooks, other };
    let (tool, args) = (body["tool"].as_str().unwrap_or("").to_string(), body["args"].clone());
    if crate::tasks::tools::NAMES.contains(&tool.as_str()) {
        return task_tool(&app, &caller, tool, args).await;
    }
    let (a, key) = (app.clone(), caller.key.clone());
    let done = tokio::task::spawn_blocking(move || {
        let working = a.hooks.working(&caller.key);
        run(&a.plans, &caller, &tool, &args, working, crate::auth::store::unix_now())
    }).await;
    let Ok((text, error, plan_changed)) = done else { return answer("terminum could not answer the call.", true) };
    if plan_changed {
        crate::plans::save(&app);
        crate::page::plans::tell(&app.pages, std::slice::from_ref(&key));
        app.dirty.notify_one();
    }
    answer(&text, error)
}

/// A task tool (AC-115): every page hears of what it changed.
async fn task_tool(app: &Shared, caller: &Caller, tool: String, args: Value) -> Json<Value> {
    let row = crate::page::marks::find_item(&app.layout.read(), &caller.key).unwrap_or_default();
    let (host, dir) = crate::page::marks::project_of(&row);
    let who = crate::tasks::tools::Who {
        key: caller.key.clone(),
        agent: caller.agent.clone(),
        title: row["title"].as_str().filter(|t| !t.is_empty()).unwrap_or("session").to_string(),
        project: app.projects.lock().of(&caller.key).map(|p| p.id.clone()),
        dir: if host.is_none() { dir } else { None },
    };
    if tool == "get_file" && let Some(host) = host {
        return match host_file(app, &host, who, args).await { Ok(text) => answer(&text, false), Err(e) => answer(&e, true) };
    }
    let a = app.clone();
    let done = tokio::task::spawn_blocking(move || crate::tasks::tools::run(&a.tasks, &a.cfg.dir, &who, &tool, &args)).await;
    match done {
        Ok(Ok((text, changed))) => {
            for task in &changed {
                crate::tasks::api::told(app, task);
            }
            answer(&text, false)
        }
        Ok(Err(e)) => answer(&e, true),
        Err(_) => answer("terminum could not answer the call.", true),
    }
}

/// A task's file for an agent on `host`: put in its ~/.terminum/tmp as a paste's file is, its path said
/// (terminum-kanban.md AC-120).
async fn host_file(app: &Shared, host: &str, who: crate::tasks::tools::Who, args: Value) -> Result<String, String> {
    if !crate::remote::files_on(app, host) {
        return Err("Files are off for this host".into());
    }
    let a = app.clone();
    let (name, data) = tokio::task::spawn_blocking(move || {
        let (name, path) = a.tasks.with(|tx| crate::tasks::tools::task_file(tx, &a.cfg.dir, &who, &args))?;
        std::fs::read(&path).map(|d| (name, d)).map_err(|e| format!("The file could not be read: {e}"))
    }).await.map_err(|_| "terminum could not answer the call.".to_string())??;
    let stored = crate::page::upload::upload_name(&name, &data)?;      // a paste's name: its time, then its own
    let there = crate::remote::paste(app, host, &stored, data.into()).await?;
    Ok(format!("{}\n{there}", crate::tasks::tools::DATA))
}

fn answer(text: &str, error: bool) -> Json<Value> {
    Json(json!({"text": text, "error": error}))
}

/// One tool call: (answer, is an error, whether the plan changed: the list is read again, the plans saved).
pub fn run(plans: &Plans, c: &Caller, tool: &str, args: &Value, working: bool, now: u64) -> (String, bool, bool) {
    let r = match tool {
        "update_session_plan" if c.other => Err(OTHER_SESSION.to_string()),
        "update_session_plan" => update_plan(plans, c, args, now).map(|t| (t, true)),
        "get_session_plan" => Ok((plan_text(plans, &c.key, working, now), false)),
        // an agent started before the upgrade still holds the old list (AC-98)
        "add_notes_for_user" | "update_notes_for_user" | "get_notes_for_user" | "delete_notes_for_user" => Err(NOTES_GONE.to_string()),
        other => Err(format!("unknown tool {other}: update_session_plan, get_session_plan, {}", crate::tasks::tools::NAMES.join(", "))),
    };
    let r = r.map(|(t, ch)| match tool {
        "update_session_plan" | "get_session_plan" if !c.hooks => (format!("{t}\n{HOOKS_OFF}"), ch),
        _ => (t, ch),
    });
    match r {
        Ok((t, ch)) => (t, false, ch),
        Err(e) => (e, true, false),
    }
}

fn text_of(v: &Value, k: &str, max: usize) -> Result<Option<String>, String> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let s = rules::clean(s);
            if s.chars().count() > max { Err(format!("{k} is over {max} characters")) } else { Ok(Some(s)) }
        }
        Some(_) => Err(format!("{k} must be a string")),
    }
}

fn update_plan(plans: &Plans, c: &Caller, args: &Value, now: u64) -> Result<String, String> {
    let items = args["items"].as_array().ok_or("items is required: send the whole plan, every item with its status")?;
    if items.is_empty() || items.len() > rules::ITEMS_MOST {
        return Err(format!("items has {} entries: send the whole plan, 1–{} items", items.len(), rules::ITEMS_MOST));
    }
    let mut list = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let text = text_of(it, "text", rules::ITEM_TEXT).map_err(|e| format!("items[{i}].{e}"))?.filter(|t| !t.is_empty())
            .ok_or_else(|| format!("items[{i}].text is required"))?;
        let status = it["status"].as_str().filter(|s| STATUSES.contains(s))
            .ok_or_else(|| format!("items[{i}].status {} is not one of {}", it["status"], STATUSES.join(", ")))?;
        list.push((text, status.to_string()));
    }
    let eta_minutes = match &args["eta_minutes"] {
        Value::Null => None,
        v => Some(rules::eta_minutes(v).ok_or_else(|| format!("eta_minutes {v} is not a whole number of minutes from 1 to {}", rules::ETA_MOST))?),
    };
    let u = Update { items: Some(list), doing: text_of(args, "doing", rules::DOING)?, eta_minutes, title: text_of(args, "title", rules::TITLE)? };
    let took = plans.mcp(&c.key, &c.agent, c.session.as_deref(), u, now);
    let d = plans.details(&c.key, false, now).ok_or("The plan was not stored.")?;
    let eta = d.row.eta.map(|at| format!(", ETA ~{}", clock(at))).unwrap_or_default();
    let (done, total) = (d.row.done, d.row.total);
    Ok(if took {
        format!("Plan stored: {done} of {total} completed ({}%){eta}.", d.row.pct)
    } else {
        format!("Doing and ETA stored; items follow your own plan tool ({done} of {total} completed), so the items sent were not taken{eta}.")
    })
}

/// `at` (unix s) as local HH:MM.
#[allow(deprecated)] // the libc crate marks musl's time_t deprecated ahead of its move to 64 bits
fn clock(at: u64) -> String {
    let t = at as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r writes into a valid tm
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}

/// The stored plan as lines: "[x] / [>] / [ ] / [!]" items, then doing, ETA and updated.
pub fn plan_text(plans: &Plans, key: &str, working: bool, now: u64) -> String {
    let Some(d) = plans.details(key, working, now).filter(|d| !d.items.is_empty() || d.row.doing.is_some()) else { return "No plan yet.".into() };
    let mut out: Vec<String> = d.title.iter().cloned().collect();
    for it in &d.items {
        let mark = match it.status.as_str() { "completed" => "[x]", "in_progress" => "[>]", "blocked" => "[!]", _ => "[ ]" };
        out.push(format!("{mark} {}", it.text));
    }
    if let Some(doing) = &d.row.doing {
        out.push(format!("Doing: {doing}"));
    }
    if let Some(eta) = d.row.eta {
        out.push(format!("ETA: ~{}", clock(eta)));
    }
    out.push(format!("Updated {} min ago; {} of {} completed.", now.saturating_sub(d.row.updated) / 60, d.row.done, d.row.total));
    out.join("\n")
}

#[cfg(test)]
#[path = "agentapi_tests.rs"]
mod tests;
