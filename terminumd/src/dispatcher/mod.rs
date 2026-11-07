// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Dispatcher (`docs/specs/terminum-dispatcher.md`): terminum's system user. It starts a task's
//! session on a human's RUN, watches it by hooks' events and a walk on a fixed beat, moves the task
//! as the session lives, and closes what it started. All its clocks and facts are in the store.

pub mod api;
mod gate;
mod hooks;
pub mod plan;
mod procs;
pub mod runs;
pub mod screen;
mod start;
mod walk;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::tasks::api::store;
use crate::tasks::ops;
use crate::tasks::store::DISPATCHER;

pub use hooks::heard;
pub use walk::run as walking;

/// The walk's beat; a gap over GAP adds nothing to a clock (AC-131).
const BEAT: Duration = Duration::from_secs(30);
const GAP: i64 = 120;
/// What terminum typed is known for this long: the hook's prompt is matched to it (§9).
const TYPED_FOR: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct Dispatcher {
    /// panes with activity since the last walk
    active: Mutex<HashSet<String>>,
    /// panes with a live run: the others' hook events cost nothing here
    panes: Mutex<HashSet<String>>,
    /// what terminum typed, by pane
    typed: Mutex<Vec<(String, String, Instant)>>,
    /// runs whose start is being watched
    starting: Mutex<HashSet<String>>,
    /// each run's tree's CPU seconds at the last walk
    cpu: Mutex<HashMap<String, f64>>,
    /// a walk is due now (a session ended)
    soon: Notify,
    /// each pane's subagents, as its hooks said (AC-127)
    subs: Mutex<HashMap<String, HashMap<String, Sub>>>,
    /// each pane's tokens as its last turn's end said; null: not known (AC-129)
    tokens: Mutex<HashMap<String, serde_json::Value>>,
    /// walks in a row over a memory limit, by run (AC-128)
    over: Mutex<HashMap<String, (u32, u32)>>,
    /// what pages were last told of tasks' sessions (`taskusage`), and each pane's CPU seconds then
    usage: Mutex<String>,
    usage_cpu: Mutex<HashMap<String, f64>>,
}

/// A subagent as its hooks said.
#[derive(Clone, Debug)]
struct Sub {
    kind: String,
    last: Instant,
    told: u8,
}

impl Dispatcher {
    /// Activity in pane `key` (§9): a hook's event, a character a human typed.
    pub fn touched(&self, key: &str) {
        if self.has_run(key) {
            self.active.lock().insert(key.to_string());
        }
    }

    fn take_active(&self, key: &str) -> bool {
        self.active.lock().remove(key)
    }

    fn has_run(&self, key: &str) -> bool {
        self.panes.lock().contains(key)
    }

    /// A hook's facts of any pane: its subagents and tokens, for its card (AC-127, AC-129).
    pub fn heard_facts(&self, key: &str, ev: &serde_json::Value) {
        if let Some(tokens) = ev.get("tokens") {
            self.tokens.lock().insert(key.to_string(), tokens.clone());
        }
        let Some(id) = ev["sub"].as_str() else { return };
        let mut subs = self.subs.lock();
        let of_pane = subs.entry(key.to_string()).or_default();
        if ev["event"] == "SubagentStop" {
            of_pane.remove(id);
        } else {
            let sub = of_pane.entry(id.to_string()).or_insert_with(|| Sub { kind: ev["subtype"].as_str().unwrap_or("").to_string(), last: Instant::now(), told: 0 });
            sub.last = Instant::now();
        }
    }

    fn remember_typed(&self, key: &str, text: &str) {
        let mut t = self.typed.lock();
        t.retain(|(_, _, at)| at.elapsed() < TYPED_FOR);
        t.push((key.to_string(), text.to_string(), Instant::now()));
    }

    /// Whether `prompt` in pane `key` is what terminum typed there, on its first 20,000 characters (§9): once.
    fn was_typed(&self, key: &str, prompt: &str) -> bool {
        let first = |s: &str| s.chars().filter(|c| !c.is_whitespace()).take(20_000).collect::<String>();
        let wanted = first(prompt);
        let mut t = self.typed.lock();
        t.retain(|(_, _, at)| at.elapsed() < TYPED_FOR);
        match t.iter().position(|(k, text, _)| k == key && first(text) == wanted) {
            Some(at) => { t.remove(at); true }
            None => false,
        }
    }
}

/// The test clock (§9 Times): only with a test's own app folder.
pub fn scale() -> i64 {
    std::env::var("TM_APP_DIR").ok().and_then(|_| std::env::var("TM_TEST_CLOCK_SCALE").ok()).and_then(|s| s.parse().ok()).filter(|s: &i64| *s > 0).unwrap_or(1)
}

/// Moves `run`'s task and says why, as Dispatcher.
fn move_task(tx: &rusqlite::Transaction, run: &runs::Run, to: &str, words: &str) -> Result<(), String> {
    let at = ops::now();
    ops::move_to(tx, DISPATCHER, &run.task_id, to, at)?;
    ops::log(tx, &run.task, DISPATCHER, "why", words, None, at).map(|_| ())
}

/// Says `words` on `run`'s task, as Dispatcher.
fn say(tx: &rusqlite::Transaction, run: &runs::Run, words: &str) -> Result<(), String> {
    ops::log(tx, &run.task, DISPATCHER, "told", words, None, ops::now()).map(|_| ())
}
