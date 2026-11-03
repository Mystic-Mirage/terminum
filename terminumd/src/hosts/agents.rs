// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A host's coding agents' progress turned on or off (AC-99): `terminum hosts agents <host>
//! on|off` and the host menu. On: the helper copied (as Files does, without registering Files),
//! terminum's hooks and MCP server installed in the host's agents, `"agents": "enabled"` stored
//! only when one took them. Off: they are taken out, the record cleared, the helper removed only
//! when Files does not use it. The ssh steps are behind [`Ops`] so the decisions are tested.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{Map, Value};
use tokio::sync::Notify;

use crate::app::Shared;

type Got = Vec<(String, String)>;

/// Said when the host's record changed while an operation ran on the recipe it started with.
pub(super) const CHANGED: &str = "the host's record changed meanwhile; nothing was saved, try again";

/// The steps that reach the host or settings.json, all over the recipe read once at the start.
pub(crate) trait Ops {
    async fn install(&self) -> Result<Got, String>;
    async fn remove(&self) -> Result<Got, String>;
    async fn remove_helper(&self) -> Result<(), String>;
    fn save(&self, f: &mut dyn FnMut(&mut Map<String, Value>)) -> Result<(), String>;
    /// The link follows the stored setting (link::changed).
    fn changed(&self);
    /// The recipe this operation started with.
    fn recipe(&self) -> &[String];
}

/// Each agent's line: `<agent>: <result as shown>`.
pub fn lines(host: &str, got: &[(String, String)]) -> Vec<String> {
    got.iter().map(|(a, r)| format!("{a}: {}", super::agentsinstall::shown(host, r))).collect()
}

/// At least one agent took terminum's hooks (or uses the host's own terminum).
pub fn took(got: &[(String, String)]) -> bool {
    got.iter().any(|(_, r)| matches!(r.as_str(), "installed" | "needs-trust" | "skipped-own"))
}

/// Files uses the helper: it stays when agents go off.
fn files_use_helper(rec: &Value) -> bool {
    matches!(rec["files"].as_str(), Some("enabled" | "wanted"))
}

/// On: nothing is saved when the record changed during the ssh steps (a stale operation never
/// writes over a newer record). Off: saved and the link closed first, so /hook and /agent stop at
/// once; the host's entries go after that, bounded, with their own result.
pub(crate) async fn set_with(ops: &impl Ops, host: &str, on: bool) -> Result<Vec<String>, String> {
    if on {
        let got = ops.install().await.map_err(|e| format!("Could not set up coding agents' progress on {host}: {e}"))?;
        let shown = lines(host, &got);
        if !took(&got) {
            return Err(format!("No coding agent on {host} took terminum's hooks: {}", shown.join("; ")));
        }
        // judged under the settings file's write lock: the record a newer edit left is kept
        let mut saved = false;
        ops.save(&mut |h| {
            if let Some(r) = h.get_mut(host).filter(|r| r["ssh"] == Value::from(ops.recipe())) {
                r["agents"] = Value::from("enabled");
                saved = true;
            }
        })?;
        if !saved {
            return Err(format!("Could not set up coding agents' progress on {host}: {CHANGED}"));
        }
        ops.changed();
        return Ok(shown);
    }
    let mut keep_helper = true;
    ops.save(&mut |h| {
        if let Some(r) = h.get_mut(host) {
            r["agents"] = Value::Null;
            keep_helper = files_use_helper(r);
        }
    })?;
    ops.changed();
    let got = ops.remove().await.map_err(|e| format!("Turned off here; could not take terminum's hooks out of {host}: {e}"))?;
    let mut shown = lines(host, &got);
    if !keep_helper {
        match ops.remove_helper().await {
            Ok(()) => shown.push(format!("helper removed from {host}")),
            Err(e) => return Err(format!("Turned off here; {}; could not remove the helper from {host}: {e}", shown.join("; "))),
        }
    }
    Ok(shown)
}

/// The running terminumd's steps for `host`, over the recipe read when the operation began.
struct Live<'a> {
    app: &'a Shared,
    host: &'a str,
    recipe: Vec<String>,
}

impl Ops for Live<'_> {
    async fn install(&self) -> Result<Got, String> {
        let current = || super::recipe_of(self.app, self.host).is_some_and(|r| r == self.recipe);
        super::agentsinstall::install(&self.recipe, |recipe| async move { super::tunnel::update(&recipe).await }, current).await
    }
    async fn remove(&self) -> Result<Got, String> {
        super::agentsinstall::remove_on(&self.recipe).await
    }
    async fn remove_helper(&self) -> Result<(), String> {
        super::tunnel::remove_helper(&self.recipe).await
    }
    fn save(&self, f: &mut dyn FnMut(&mut Map<String, Value>)) -> Result<(), String> {
        super::save(self.app, f)
    }
    fn changed(&self) {
        super::link::changed(self.app, self.host);
    }
    fn recipe(&self) -> &[String] {
        &self.recipe
    }
}

/// Said by an agents on or off that a later one for the same host ended.
const SUPERSEDED: &str = "a later agents on or off for this host took over; this one was stopped";

/// One agents on or off at a time per host, the last asked winning: a new one stops the one
/// running (dropped mid-step, its ssh killed) and runs after it, so an off given during a long
/// on is never written over by that on's late save, nor its removal raced by that on's install.
#[derive(Default)]
pub struct Turns(Mutex<HashMap<String, Arc<Turn>>>);

#[derive(Default)]
struct Turn {
    running: tokio::sync::Mutex<()>,
    asked: AtomicU64,
    stop: Notify,
}

impl Turns {
    /// `work` in `host`'s turn: it stops the one running and goes after it.
    async fn run<T>(&self, host: &str, work: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
        self.turn(host, true, work).await
    }

    /// `work` in `host`'s turn after the one running, which it leaves be (a refresh); a later
    /// `run` stops it all the same.
    async fn after<T>(&self, host: &str, work: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
        self.turn(host, false, work).await
    }

    /// Whether an on or off of `host` runs or waits now.
    fn busy(&self, host: &str) -> bool {
        self.0.lock().get(host).is_some_and(|t| t.running.try_lock().is_err())
    }

    async fn turn<T>(&self, host: &str, take_over: bool, work: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
        let turn = self.0.lock().entry(host.to_string()).or_default().clone();
        let mine = if take_over {
            let mine = turn.asked.fetch_add(1, Ordering::SeqCst) + 1;
            turn.stop.notify_waiters();
            mine
        } else {
            turn.asked.load(Ordering::SeqCst)
        };
        let _running = turn.running.lock().await;
        let stopped = async {
            loop {
                let woke = turn.stop.notified();
                tokio::pin!(woke);
                woke.as_mut().enable();
                if turn.asked.load(Ordering::SeqCst) != mine {
                    return;
                }
                woke.await;
            }
        };
        tokio::select! {
            biased;
            _ = stopped => Err(SUPERSEDED.into()),
            r = work => r,
        }
    }
}

/// `hosts agents <host> on|off`: the per-agent result lines.
pub async fn set_agents(app: &Shared, host: &str, on: bool) -> Result<Vec<String>, String> {
    if !super::records(app).contains_key(host) {
        return Err(format!("no host named {host}"));
    }
    app.agent_turns.run(host, async {
        // read in its turn: an earlier operation may have changed the record
        let recipe = super::recipe_of(app, host).ok_or_else(|| format!("{host} is not attached"))?;
        set_with(&Live { app, host, recipe }, host, on).await
    }).await
}

/// After AC-84 updated the helper: the host's agents' entries rewritten where they differ, in
/// the host's turn after any on or off running (agents still on then).
pub fn after_update(app: &Shared, host: &str) {
    if !super::link::wanted(app, host) {
        return;
    }
    let (app, host) = (app.clone(), host.to_string());
    tokio::spawn(async move {
        let refresh = async {
            if !super::link::wanted(&app, &host) {
                return Ok(Vec::new());
            }
            super::agentsinstall::refresh(&app, &host).await
        };
        match app.agent_turns.after(&host, refresh).await {
            Ok(got) => tracing::info!(event = "agents.refreshed", host, result = lines(&host, &got).join("; ")),
            Err(why) => tracing::warn!(event = "agents.refresh_failed", host, why),
        }
    });
}

/// The host is being removed with agents on, or while an on or off runs for it: that one
/// stopped, terminum's entries taken out, best effort.
pub fn removed(app: &Shared, host: &str) {
    let Some(recipe) = super::recipe_of(app, host).filter(|_| super::link::wanted(app, host) || app.agent_turns.busy(host)) else { return };
    let (app, host) = (app.clone(), host.to_string());
    tokio::spawn(async move {
        match app.agent_turns.run(&host, super::agentsinstall::remove_on(&recipe)).await {
            Ok(got) => tracing::info!(event = "agents.removed", host, result = lines(&host, &got).join("; ")),
            Err(why) => tracing::warn!(event = "agents.remove_failed", host, why),
        }
    });
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
