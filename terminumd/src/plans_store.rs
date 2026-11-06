// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! state/plans.json: the panes' plans and the last archived, with the identity of each tmux
//! server they were made in (as unread.json, page/unread.rs): a restart of terminumd keeps
//! them, a tmux server started anew (its pane ids start again at %0) takes its plans with it.
//! A file that cannot be read is set aside as plans.json.bak, never written over.

use std::path::Path;

use serde_json::{json, Value};

use super::{Plan, Plans};
use crate::app::Shared;

const FILE: &str = "plans.json";
const VERSION: u64 = 1;

impl Plans {
    /// The plans a terminumd before this one kept; none without a file.
    pub fn load(dir: &Path) -> Plans {
        crate::auth::store::read_or_set_aside(&dir.join(FILE), |v| {
            if v["version"] != VERSION {
                return Err(format!("version {} is not {VERSION}", v["version"]));
            }
            let p = Plans::default();
            *p.cur.lock() = serde_json::from_value(v["current"].clone()).map_err(|e| e.to_string())?;
            *p.archived.lock() = serde_json::from_value::<Vec<(String, Plan)>>(v["archived"].clone()).map_err(|e| e.to_string())?;
            *p.servers.lock() = serde_json::from_value(v["servers"].clone()).map_err(|e| e.to_string())?;
            Ok(p)
        }).unwrap_or_default()
    }

    pub(super) fn json(&self) -> Value {
        json!({"version": VERSION, "current": *self.cur.lock(), "archived": *self.archived.lock(), "servers": *self.servers.lock()})
    }

    /// A server's list was read (`prefix` its keys' start): a server of another identity than
    /// the plans were made in started anew, and its plans go. True when anything changed.
    pub fn seen(&self, prefix: &str, identity: &str) -> bool {
        let was = self.servers.lock().insert(prefix.to_string(), identity.to_string());
        if was.as_deref() == Some(identity) {
            return false;
        }
        if was.is_some() {
            self.cur.lock().retain(|k, _| !k.starts_with(prefix));
        }
        true
    }

    /// plans.json as the plans are now.
    pub fn write(&self, dir: &Path) {
        let _one = WRITING.lock();
        if let Err(e) = crate::auth::store::write_private(&dir.join(FILE), &self.json()) {
            tracing::warn!(event = "plans.unsaved", error = %e);
        }
    }
}

/// One write of plans.json at a time.
static WRITING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// plans.json written off the async workers; the last write holds the last change.
pub fn save(app: &Shared) {
    if app.cfg.agent {
        return;
    }
    let a = app.clone();
    match tokio::runtime::Handle::try_current() {
        Ok(rt) => drop(rt.spawn_blocking(move || a.plans.write(&a.cfg.dir))),
        Err(_) => a.plans.write(&a.cfg.dir),
    }
}

/// Pane `key`'s hook event (sock.rs `/hook`), after the hooks applied it: a prompt starts a turn,
/// a session of another id archives the plan, the plan tool's op applies; at a tool's end, the
/// reminder due. A change is saved, and the list read again.
pub fn hook_event(app: &Shared, key: &str, ev: &Value) -> Option<String> {
    let now = crate::auth::store::unix_now();
    let (changed, remind) = apply(&app.plans, key, ev, || app.hooks.working(key), now);
    if changed || remind.is_some() {
        save(app);
    }
    if changed {
        app.dirty.notify_one();
        crate::page::plans::tell(&app.pages, &[key.to_string()]);
    }
    // a human's move rides the same event (AC-114): the event the agent reads as context
    let carries = !ev["sub"].as_str().is_some_and(|s| !s.is_empty()) && ev["event"].as_str() == Some(remind_event(ev["agent"].as_str().unwrap_or("")));
    if carries { crate::tasks::tell::with_lines(app, key, remind) } else { remind }
}

/// `ev` applied to the plans: (changed, the reminder due).
fn apply(p: &Plans, key: &str, ev: &Value, working: impl FnOnce() -> bool, now: u64) -> (bool, Option<String>) {
    let (agent, name) = (ev["agent"].as_str().unwrap_or(""), ev["event"].as_str().unwrap_or(""));
    let session = ev["session"].as_str().filter(|s| !s.is_empty());
    let mut changed = match name {
        "UserPromptSubmit" => { p.turn(key, agent, session, now); true }
        "SessionStart" => session.is_some_and(|s| p.session(key, Some(s))),
        _ => false,
    };
    // a subagent's plan tool and tool calls are its own: the session's plan is the main agent's
    let sub = ev["sub"].as_str().is_some_and(|s| !s.is_empty());
    if ev["plan"].is_object() && !sub {
        p.hook(key, agent, session, &ev["plan"], now);
        changed = true;
    }
    // the reminder rides the event whose output reaches the model: Claude's sync PostToolBatch, Codex's PostToolUse
    let remind = (!sub && name == remind_event(agent)).then(|| p.post_tool(key, working(), now)).flatten();
    (changed, remind)
}

/// The one event of `agent` that may carry a reminder: Claude Code delivers no async hook's
/// output in an interactive session, so its reminder waits for the synchronous PostToolBatch.
pub fn remind_event(agent: &str) -> &'static str {
    if agent == "claude" { "PostToolBatch" } else { "PostToolUse" }
}

/// Before terminumd stops: the plans on disk now.
pub fn flush(app: &Shared) {
    if !app.cfg.agent {
        app.plans.write(&app.cfg.dir);
    }
}

/// A server's list was read: its plans stay only while it is the server they were made in.
pub async fn seen(app: &Shared, server: &crate::tmux::Server) {
    let Some(identity) = server.identity().await else { return };
    if app.plans.seen(&server.key(""), &identity) {
        save(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const K: &str = "tmux:local:terminum:%1";

    fn planned(p: &Plans) {
        p.hook(K, "claude", Some("s1"), &json!({"op": "all", "items": [{"text": "a", "status": "completed"}, {"text": "b"}]}), 100);
    }

    #[test]
    fn a_plan_survives_a_restart_in_a_private_file() {
        let d = crate::testutil::fresh_dir("plans-restart");
        let p = Plans::default();
        planned(&p);
        p.seen("tmux:local:terminum:", "id1");
        p.write(d.as_path());
        let mode = std::fs::metadata(d.as_path().join(FILE)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let back = Plans::load(d.as_path());
        assert_eq!(back.row(K, false, 200).map(|r| (r.done, r.total)), Some((1, 2)));
        assert!(!back.seen("tmux:local:terminum:", "id1"), "the same server keeps its plans");
        assert!(back.row(K, false, 200).is_some());
    }

    #[test]
    fn a_tmux_server_started_anew_takes_its_plans() {
        let p = Plans::default();
        planned(&p);
        assert!(p.seen("tmux:local:terminum:", "id1"));
        assert!(p.seen("tmux:local:terminum:", "id2"));
        assert_eq!(p.row(K, false, 200), None);
    }

    #[test]
    fn a_file_that_does_not_parse_is_set_aside_not_written_over() {
        let d = crate::testutil::fresh_dir("plans-bad");
        std::fs::write(d.as_path().join(FILE), b"{not json").unwrap();
        let p = Plans::load(d.as_path());
        assert_eq!(p.row(K, false, 0), None);
        assert_eq!(std::fs::read(d.as_path().join("plans.json.bak")).unwrap(), b"{not json");
        std::fs::write(d.as_path().join(FILE), br#"{"version": 9}"#).unwrap();
        Plans::load(d.as_path());
        assert!(!d.as_path().join(FILE).exists(), "another version is set aside too");
    }

    #[test]
    fn a_gone_pane_or_a_new_program_archives_its_plan() {
        let p = Plans::default();
        planned(&p);
        assert!(!p.retain("tmux:local:terminum:", &[K.to_string()]), "a live pane keeps its plan");
        assert!(!p.retain("tmux:local:other:", &[]), "another server's panes are not judged");
        assert!(p.retain("tmux:local:terminum:", &[]));
        assert_eq!((p.row(K, false, 200), p.archived()), (None, 1));
        planned(&p);
        assert!(!p.session(K, Some("s1")), "the same session keeps it");
        assert!(p.session(K, None), "the program changed");
        assert_eq!(p.archived(), 2);
        assert!(!p.session(K, None), "nothing left to archive");
    }

    #[test]
    fn a_subagents_plan_op_and_tool_end_leave_the_sessions_plan() {
        let p = Plans::default();
        planned(&p);
        let ev = |sub: &str, event: &str| json!({"agent": "claude", "session": "s1", "event": event, "sub": sub,
            "plan": {"op": "all", "items": [{"text": "sub's own"}]}});
        assert_eq!(apply(&p, K, &ev("a1", "PostToolUse"), || true, 100_000), (false, None));
        assert_eq!(p.row(K, false, 100_000).map(|r| r.total), Some(2), "the main agent's plan is kept");
        let (changed, remind) = apply(&p, K, &ev("", "PostToolUse"), || true, 100_000);
        assert!(changed && remind.is_none(), "the main agent's own op applies (it is fresh)");
        assert_eq!(p.row(K, false, 100_000).map(|r| r.total), Some(1));
        let main = json!({"agent": "claude", "session": "s1", "event": "PostToolBatch"});
        assert!(apply(&p, K, &main, || true, 200_000).1.is_some(), "the main agent is reminded");
    }

    #[test]
    fn claude_is_reminded_on_its_sync_batch_codex_on_its_tool_end() {
        let p = Plans::default();
        planned(&p);
        let ev = |agent: &str, event: &str| json!({"agent": agent, "session": "s1", "event": event});
        for other in ["PostToolUse", "PostToolUseFailure", "Stop", "PreToolUse"] {
            assert_eq!(apply(&p, K, &ev("claude", other), || true, 100_000).1, None, "claude {other}: no reminder");
        }
        assert_eq!(p.cur.lock()[K].reminded_at, None, "claude's async PostToolUse never consumes the reminder");
        assert!(apply(&p, K, &ev("claude", "PostToolBatch"), || true, 100_000).1.is_some(), "claude's PostToolBatch reminds");
        assert_eq!(apply(&p, K, &ev("claude", "PostToolBatch"), || true, 100_010).1, None, "once per window");
        let q = Plans::default();
        q.hook(K, "codex", Some("s1"), &json!({"op": "all", "items": [{"text": "a"}, {"text": "b"}]}), 100);
        assert_eq!(apply(&q, K, &ev("codex", "PostToolBatch"), || true, 100_000).1, None);
        assert!(apply(&q, K, &ev("codex", "PostToolUse"), || true, 100_000).1.is_some(), "codex's PostToolUse reminds");
    }

    #[test]
    fn no_file_is_no_plans() {
        let d = crate::testutil::fresh_dir("plans-none");
        assert_eq!(Plans::load(d.as_path()).archived(), 0);
    }
}
