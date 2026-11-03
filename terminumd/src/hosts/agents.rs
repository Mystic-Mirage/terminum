// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A host's coding agents' progress turned on or off (AC-99): `terminum hosts agents <host>
//! on|off` and the host menu. On: the helper copied (as Files does, without registering Files),
//! terminum's hooks and MCP server installed in the host's agents, `"agents": "enabled"` stored
//! only when one took them. Off: they are taken out, the record cleared, the helper removed only
//! when Files does not use it. The ssh steps are behind [`Ops`] so the decisions are tested.

use serde_json::{Map, Value};

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

/// `hosts agents <host> on|off`: the per-agent result lines.
pub async fn set_agents(app: &Shared, host: &str, on: bool) -> Result<Vec<String>, String> {
    if !super::records(app).contains_key(host) {
        return Err(format!("no host named {host}"));
    }
    let recipe = super::recipe_of(app, host).ok_or_else(|| format!("{host} is not attached"))?;
    set_with(&Live { app, host, recipe }, host, on).await
}

/// After AC-84 updated the helper: the host's agents' entries rewritten where they differ.
pub fn after_update(app: &Shared, host: &str) {
    if !super::link::wanted(app, host) {
        return;
    }
    let (app, host) = (app.clone(), host.to_string());
    tokio::spawn(async move {
        match super::agentsinstall::refresh(&app, &host).await {
            Ok(got) => tracing::info!(event = "agents.refreshed", host, result = lines(&host, &got).join("; ")),
            Err(why) => tracing::warn!(event = "agents.refresh_failed", host, why),
        }
    });
}

/// The host is being removed with agents on: terminum's entries taken out, best effort.
pub fn removed(app: &Shared, host: &str) {
    let Some(recipe) = super::recipe_of(app, host).filter(|_| super::link::wanted(app, host)) else { return };
    let host = host.to_string();
    tokio::spawn(async move {
        match super::agentsinstall::remove_on(&recipe).await {
            Ok(got) => tracing::info!(event = "agents.removed", host, result = lines(&host, &got).join("; ")),
            Err(why) => tracing::warn!(event = "agents.remove_failed", host, why),
        }
    });
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
