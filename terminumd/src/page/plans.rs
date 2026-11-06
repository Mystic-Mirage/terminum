// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The Page's view of plans (AC-97): the rows' summaries, the Plan pane's `plan` answer, and
//! `plannew` to every page on a change.

use std::sync::Arc;

use serde_json::{json, Value};

use super::Page;
use crate::app::Shared;
use crate::plans::{Details, Row};

/// Each row's plan summary (absent without one).
pub fn mark(app: &Shared, groups: &mut [Value]) {
    let now = crate::auth::store::unix_now();
    for it in groups.iter_mut().flat_map(|g| g["items"].as_array_mut().into_iter().flatten()) {
        let Some(key) = it["id"].as_str().map(str::to_string) else { continue };
        let row = app.plans.row(&key, app.hooks.working(&key), now);
        put(it, row);
    }
}

/// Row `it`'s `plan`, absent without one.
pub fn put(it: &mut Value, plan: Option<Row>) {
    let Some(o) = it.as_object_mut() else { return };
    match plan {
        Some(r) => { o.insert("plan".into(), json!(r)); }
        None => { o.remove("plan"); }
    }
}

/// Every page hears that the plans of `keys` changed.
pub fn tell(pages: &super::Pages, keys: &[String]) {
    for key in keys {
        pages.tell(&json!({"t": "plannew", "key": key}).to_string());
    }
}

/// The `plan` answer for pane `key`: its plan (null without one).
/// Without a plan, the pane's newest archived one, marked `finished` (its agent exited, AC-97).
pub fn answer(app: &Shared, key: &str) -> Value {
    let now = crate::auth::store::unix_now();
    let (plan, finished) = match app.plans.details(key, app.hooks.working(key), now) {
        Some(d) => (Some(d), false),
        None => (app.plans.last_archived(key, now), true),
    };
    let mut v = reply(key, plan);
    if finished && !v["plan"].is_null() {
        v["plan"]["finished"] = json!(true);
    } else {
        hooks(&mut v, app.hooks.session(key).is_some());
    }
    v
}

/// A plan of a pane terminumd has no hook event from carries `hooks: false` (its agent's
/// hooks are not installed or not trusted, F8).
pub fn hooks(v: &mut Value, hooked: bool) {
    if !hooked && !v["plan"].is_null() {
        v["plan"]["hooks"] = json!(false);
    }
}

/// The `plan` message of pane `key`.
pub fn reply(key: &str, plan: Option<Details>) -> Value {
    let plan = plan.map(|d| json!({
        "title": d.title, "doing": d.doing, "items": d.items.iter().map(|i| json!({"text": i.text, "status": i.status})).collect::<Vec<_>>(),
        "done": d.row.done, "total": d.row.total, "pct": d.row.pct, "eta": d.row.eta, "eta_given": d.eta_given,
        "updated": d.row.updated, "source": d.source, "partial": d.row.partial, "agent": d.agent,
    }));
    json!({"t": "plan", "key": key, "plan": plan})
}

/// `plan` from a page.
pub async fn handle(app: &Shared, page: &Arc<Page>, msg: &Value) {
    page.send_json(&answer(app, msg["key"].as_str().unwrap_or("")));
}

#[cfg(test)]
#[path = "plans_tests.rs"]
mod tests;
