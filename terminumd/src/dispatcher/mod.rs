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

use std::sync::atomic::AtomicU64;

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
    /// a walk is due now (a session ended)
    soon: Notify,
    /// what a walk or a hook learned of each pane it watches
    watch: Mutex<HashMap<String, PaneWatch>>,
    /// what pages were last told of tasks' sessions (`taskusage`)
    usage: Mutex<String>,
    /// beats so far: a host with no run is read less often
    beat: AtomicU64,
}

/// What is known of a watched pane beyond the store; a restart learns it anew.
#[derive(Default)]
struct PaneWatch {
    /// its tree's CPU seconds at the last reading, and when
    cpu: Option<(f64, Instant)>,
    /// walks in a row over a memory limit (AC-128)
    over: (u32, u32),
    /// its subagents as its hooks said (AC-127); bounded: a hook's fields are data
    subs: HashMap<String, Sub>,
    /// the tokens of its last turn's end; null: not known (AC-129)
    tokens: Option<serde_json::Value>,
    /// the last line pages were told of it: a host read less often keeps it
    line: Option<serde_json::Value>,
}

/// A subagent as its hooks said.
#[derive(Clone, Debug)]
struct Sub {
    kind: String,
    last: Instant,
    told: plan::SubStep,
}

/// Subagents kept per pane: a made-up hook adds no more.
const MOST_SUBS: usize = 64;

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

    /// A hook's facts of a watched pane: a new agent starts clean; its subagents, shape-checked (AC-127).
    fn heard_facts(&self, key: &str, ev: &serde_json::Value) {
        let mut watch = self.watch.lock();
        let pane = watch.entry(key.to_string()).or_default();
        if ev["event"] == "SessionStart" {
            pane.subs.clear();
        }
        let Some(id) = ev["sub"].as_str().filter(|id| id.len() <= 100 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) else { return };
        if ev["event"] == "SubagentStop" {
            pane.subs.remove(id);
        } else if pane.subs.contains_key(id) || pane.subs.len() < MOST_SUBS {
            let kind: String = crate::plans::rules::clean(ev["subtype"].as_str().unwrap_or("")).chars().take(60).collect();
            pane.subs.entry(id.to_string()).or_insert_with(|| Sub { kind, last: Instant::now(), told: plan::SubStep::Nothing }).last = Instant::now();
        }
    }

    /// A hook's three integers, or null when they are not (AC-129): nothing else of them is kept.
    pub fn heard_tokens(&self, key: &str, ev: &serde_json::Value) {
        let t = &ev["tokens"];
        let numbers = [&t["input"], &t["output"], &t["cached"]].map(serde_json::Value::as_u64);
        let kept = match numbers {
            [Some(input), Some(output), Some(cached)] => serde_json::json!({"input": input, "output": output, "cached": cached}),
            _ => serde_json::Value::Null,
        };
        self.watch.lock().entry(key.to_string()).or_default().tokens = Some(kept);
    }

    /// Whether a turn's end in pane `key` should say its tokens: only a task's session shows them.
    pub fn wants_tokens(&self, key: &str) -> bool {
        self.has_run(key)
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
