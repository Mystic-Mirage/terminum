// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A pane's session plan: its items, the step being done and an ETA, from the agent's own plan
//! tool (the `plan` of a hook event, cli/hookplan.rs) or from `terminum mcp`; MCP items count
//! only while the session has no plan of its tool. A new session in the pane (another session
//! id, another agent, the program changed) archives the plan. Percent is the items' alone. A
//! working turn whose plan went stale gets a reminder through the hooks (`reminder`).
//! Times are unix seconds, given by the caller.

use std::collections::HashMap;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Plans archived kept at most.
const ARCHIVED: usize = 50;
/// A finished plan stays on its row this long after its last update once its turn is over.
const FINISHED_SHOWN: u64 = 600;
/// A turn with no plan is reminded once it has worked this long.
const NO_PLAN_AFTER: u64 = 600;
/// The reminder's threshold: a sixth of the turn so far, within these.
const LEAST: u64 = 600;
const MOST: u64 = 1800;
/// The reminder; `{when}` is "was last updated N min ago" or "has no plan yet".
pub const REMINDER: &str = "terminum: this session's plan {when}. Update it now: mark finished items completed with your plan tool (TaskUpdate / update_plan) or call terminum's update_session_plan with the whole plan, the current step and an ETA.";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Tool,
    Mcp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub text: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub agent: String,
    pub session: String,
    pub items: Vec<Item>,
    pub source: Source,
    /// An op named an item it never saw (it began before terminum did): "partial".
    pub incomplete: bool,
    pub title: Option<String>,
    pub doing: Option<String>,
    pub eta_at: Option<u64>,
    pub eta_given: Option<u64>,
    pub updated: u64,
    pub turn_started: Option<u64>,
    pub reminded_at: Option<u64>,
}

impl Plan {
    fn new(agent: &str, session: &str, now: u64) -> Self {
        Plan { agent: agent.into(), session: session.into(), items: Vec::new(), source: Source::Tool, incomplete: false, title: None,
               doing: None, eta_at: None, eta_given: None, updated: now, turn_started: None, reminded_at: None }
    }

    fn done(&self) -> usize {
        self.items.iter().filter(|i| i.status == "completed").count()
    }
    /// Every item completed.
    fn finished(&self) -> bool {
        !self.items.is_empty() && self.done() == self.items.len()
    }
}

/// An MCP `update_session_plan`: each field given replaces the plan's.
#[derive(Clone, Debug, Default)]
pub struct Update {
    pub items: Option<Vec<(String, String)>>,
    pub doing: Option<String>,
    pub eta_minutes: Option<u64>,
    pub title: Option<String>,
}

/// What a session row shows.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub done: usize,
    pub total: usize,
    pub pct: u32,
    pub eta: Option<u64>,
    pub updated: u64,
    pub stale: bool,
    pub partial: bool,
    /// What the agent says it does now: a card shows it (terminum-kanban.md AC-134).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doing: Option<String>,
}

/// What the plan's details show.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Details {
    pub agent: String,
    pub source: Source,
    pub title: Option<String>,
    pub doing: Option<String>,
    pub items: Vec<Item>,
    pub row: Row,
    pub eta_given: Option<u64>,
}

/// pending | in_progress | completed | blocked; another word is pending.
pub fn status(s: &str) -> &'static str {
    rules::STATUSES.iter().find(|x| **x == s).copied().unwrap_or("pending")
}

/// round(100·done/total); 0 without items.
pub fn percent(done: usize, total: usize) -> u32 {
    if total == 0 { 0 } else { ((done * 200 + total) / (total * 2)) as u32 }
}

/// The reminder's threshold for a turn started at `turn_started`.
pub fn threshold(turn_started: Option<u64>, now: u64) -> u64 {
    (now.saturating_sub(turn_started.unwrap_or(now)) / 6).clamp(LEAST, MOST)
}

/// The reminder due on a PostToolUse, if any: `updated` is the plan's (None: no plan).
pub fn reminder(updated: Option<u64>, working: bool, turn_started: Option<u64>, now: u64, reminded_at: Option<u64>) -> Option<String> {
    if !working {
        return None;
    }
    let limit = threshold(turn_started, now);
    let quiet = reminded_at.is_none_or(|r| now.saturating_sub(r) >= limit);
    match updated {
        Some(u) => (now.saturating_sub(u) >= limit && quiet)
            .then(|| REMINDER.replace("{when}", &format!("was last updated {} min ago", now.saturating_sub(u) / 60))),
        None => {
            let started = turn_started?;
            let this_turn = reminded_at.is_some_and(|r| r >= started);
            (now.saturating_sub(started) >= NO_PLAN_AFTER && !this_turn).then(|| REMINDER.replace("{when}", "has no plan yet"))
        }
    }
}

/// An agent the hooks named; "agent" (and "") is one they had no record of (agentapi.rs).
fn known(agent: &str) -> bool {
    !agent.is_empty() && agent != UNKNOWN
}

/// The agent of an MCP call whose pane the hooks have no record of.
pub const UNKNOWN: &str = "agent";

/// The plans of the panes, by key.
#[derive(Default)]
pub struct Plans {
    cur: Mutex<HashMap<String, Plan>>,
    archived: Mutex<Vec<(String, Plan)>>,
    /// each tmux server's identity by its keys' start (store.rs)
    servers: Mutex<HashMap<String, String>>,
}

impl Plans {
    /// The pane's plan for `agent`'s `session`, the one before archived when it was another's.
    fn entry<'a>(&self, cur: &'a mut HashMap<String, Plan>, key: &str, agent: &str, session: Option<&str>, now: u64) -> &'a mut Plan {
        // an agent the hooks have no record of (terminumd restarted) is the plan's own
        let other = known(agent) && cur.get(key).is_some_and(|p| known(&p.agent) && p.agent != agent
            || session.is_some_and(|s| !p.session.is_empty() && s != p.session));
        if other {
            self.archive_one(cur, key);
        }
        let p = cur.entry(key.to_string()).or_insert_with(|| Plan::new(agent, session.unwrap_or_default(), now));
        // a plan made before the agent or session was known takes them on
        if !known(&p.agent) && known(agent) {
            p.agent = agent.to_string();
        }
        if p.session.is_empty() && let Some(s) = session {
            p.session = s.to_string();
        }
        p
    }

    fn archive_one(&self, cur: &mut HashMap<String, Plan>, key: &str) {
        if let Some(p) = cur.remove(key).filter(|p| !p.items.is_empty() || p.doing.is_some()) {
            let mut a = self.archived.lock();
            a.push((key.to_string(), p));
            let over = a.len().saturating_sub(ARCHIVED);
            a.drain(..over);
        }
    }

    /// The pane's program changed, or its agent began a session (SessionStart) of another id
    /// than the plan's: the plan is archived.
    /// True when it was archived.
    pub fn session(&self, key: &str, session: Option<&str>) -> bool {
        let mut cur = self.cur.lock();
        let other = cur.get(key).is_some_and(|p| session.is_none_or(|s| s != p.session));
        if other {
            self.archive_one(&mut cur, key);
        }
        other
    }

    /// A turn began (UserPromptSubmit). A finished plan was the last request's: archived, the
    /// new request starts without one (it showed 100% from the start).
    pub fn turn(&self, key: &str, agent: &str, session: Option<&str>, now: u64) {
        let mut cur = self.cur.lock();
        if cur.get(key).is_some_and(Plan::finished) {
            self.archive_one(&mut cur, key);
        }
        self.entry(&mut cur, key, agent, session, now).turn_started = Some(now);
    }

    /// Applies a hook event's `plan` op (add, set, all) of the agent's plan tool.
    pub fn hook(&self, key: &str, agent: &str, session: Option<&str>, op: &Value, now: u64) {
        let mut cur = self.cur.lock();
        let p = self.entry(&mut cur, key, agent, session, now);
        let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let well_formed = match s(op, "op").as_deref() {
            Some("all") => true,
            Some("add") => s(op, "id").is_some() && s(op, "text").is_some(),
            Some("set") => s(op, "id").is_some(),
            _ => false,
        };
        if !well_formed {
            return;
        }
        if p.source == Source::Mcp {
            p.items.clear();
            p.source = Source::Tool;
        }
        let before = p.items.clone();
        match op.get("op").and_then(Value::as_str) {
            Some("all") => {
                let list = op.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
                p.items = list.iter().enumerate().filter_map(|(n, i)| {
                    Some(Item { id: (n + 1).to_string(), text: s(i, "text")?, status: status(&s(i, "status").unwrap_or_default()).into() })
                }).collect();
                p.incomplete = false;
            }
            Some("add") => {
                let (Some(id), Some(text)) = (s(op, "id"), s(op, "text")) else { return };
                match p.items.iter_mut().find(|i| i.id == id) {
                    Some(i) => i.text = text,
                    None => p.items.push(Item { id, text, status: "pending".into() }),
                }
            }
            Some("set") => {
                let Some(id) = s(op, "id") else { return };
                let Some(n) = p.items.iter().position(|i| i.id == id) else {
                    p.incomplete = true;
                    p.updated = now;
                    return;
                };
                match s(op, "status").as_deref() {
                    Some("deleted") => { p.items.remove(n); }
                    Some(st) => p.items[n].status = status(st).into(),
                    None => {}
                }
                if let (Some(t), Some(i)) = (s(op, "text"), p.items.iter_mut().find(|i| i.id == id)) {
                    i.text = t;
                }
            }
            _ => return,
        }
        if p.items != before {
            p.eta_at = None;
        }
        p.updated = now;
    }

    /// Applies an MCP update; false when its items were not taken (the session keeps a plan of
    /// its tool). `session` None: the plan's own (Claude Code names none to an MCP server).
    pub fn mcp(&self, key: &str, agent: &str, session: Option<&str>, u: Update, now: u64) -> bool {
        let mut cur = self.cur.lock();
        let p = self.entry(&mut cur, key, agent, session, now);
        let tool = p.source == Source::Tool && !p.items.is_empty();
        let took = match u.items {
            Some(list) if !tool => {
                p.items = list.into_iter().enumerate()
                    .map(|(n, (text, st))| Item { id: (n + 1).to_string(), text, status: status(&st).into() }).collect();
                p.source = Source::Mcp;
                p.incomplete = false;
                p.eta_at = None;
                true
            }
            Some(_) => false,
            None => true,
        };
        if let Some(d) = u.doing {
            p.doing = Some(d).filter(|d| !d.is_empty());
        }
        if let Some(t) = u.title {
            p.title = Some(t).filter(|t| !t.is_empty());
        }
        if let Some(m) = u.eta_minutes {
            p.eta_at = Some(now + m * 60);
            p.eta_given = Some(now);
        }
        p.updated = now;
        took
    }

    /// The reminder due on a PostToolUse of the pane (`working`: its hooks say so); it counts
    /// as given.
    pub fn post_tool(&self, key: &str, working: bool, now: u64) -> Option<String> {
        let mut cur = self.cur.lock();
        let p = cur.get_mut(key)?;
        if !p.items.is_empty() && p.done() == p.items.len() {
            return None;
        }
        let updated = (!p.items.is_empty()).then_some(p.updated.max(p.turn_started.unwrap_or(0)));
        let text = reminder(updated, working, p.turn_started, now, p.reminded_at)?;
        p.reminded_at = Some(now);
        Some(text)
    }

    /// The row's summary of the pane's plan; None without items, and for a finished one whose
    /// turn is over FINISHED_SHOWN after its last update (it stayed at 100% for good). Stale
    /// only while the pane works (`working`: its hooks say so).
    pub fn row(&self, key: &str, working: bool, now: u64) -> Option<Row> {
        self.cur.lock().get(key)
            .filter(|p| working || !p.finished() || now.saturating_sub(p.updated) < FINISHED_SHOWN)
            .and_then(|p| row(p, working, now))
    }

    pub fn details(&self, key: &str, working: bool, now: u64) -> Option<Details> {
        let cur = self.cur.lock();
        let p = cur.get(key)?;
        let r = row(p, working, now).unwrap_or(Row { done: 0, total: 0, pct: 0, eta: p.eta_at, updated: p.updated, stale: false, partial: p.incomplete, doing: None });
        Some(Details { agent: p.agent.clone(), source: p.source, title: p.title.clone(), doing: p.doing.clone(),
                       items: p.items.clone(), row: r, eta_given: p.eta_given })
    }

    /// The pane's newest archived plan (its agent exited), when the pane has no plan of its
    /// own: the Plan pane shows it as a finished session (AC-97).
    pub fn last_archived(&self, key: &str, now: u64) -> Option<Details> {
        if self.cur.lock().contains_key(key) {
            return None;
        }
        let a = self.archived.lock();
        let (_, p) = a.iter().rev().find(|(k, _)| k == key)?;
        let r = row(p, false, now).unwrap_or(Row { done: 0, total: 0, pct: 0, eta: None, updated: p.updated, stale: false, partial: p.incomplete, doing: None });
        Some(Details { agent: p.agent.clone(), source: p.source, title: p.title.clone(), doing: p.doing.clone(),
                       items: p.items.clone(), row: Row { eta: None, ..r }, eta_given: None })
    }

    #[cfg(test)]
    pub fn archived(&self) -> usize {
        self.archived.lock().len()
    }

    /// Archives the plans of the panes of `prefix` (a server's keys) that are not in `live`;
    /// true when any went.
    pub fn retain(&self, prefix: &str, live: &[String]) -> bool {
        let mut cur = self.cur.lock();
        let live: std::collections::HashSet<&str> = live.iter().map(String::as_str).collect();
        let gone: Vec<String> = cur.keys().filter(|k| k.starts_with(prefix) && !live.contains(k.as_str())).cloned().collect();
        for k in &gone {
            self.archive_one(&mut cur, k);
        }
        !gone.is_empty()
    }
}

fn row(p: &Plan, working: bool, now: u64) -> Option<Row> {
    let (done, total) = (p.done(), p.items.len());
    // stale as the reminder judges it: a finished plan never, the age from this turn's start at most
    let since = p.updated.max(p.turn_started.unwrap_or(0));
    (total > 0).then(|| Row { done, total, pct: percent(done, total), eta: p.eta_at, updated: p.updated,
                              stale: working && done < total && now.saturating_sub(since) >= threshold(p.turn_started, now), partial: p.incomplete,
                              doing: p.doing.clone() })
}

#[path = "planrules.rs"]
pub mod rules;

#[path = "plans_store.rs"]
mod store;
pub use store::{carries_context, flush, hook_event, remind_event, save, seen};

#[cfg(test)]
#[path = "plans_tests.rs"]
mod tests;
