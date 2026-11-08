// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Dispatcher (`docs/specs/terminum-dispatcher.md`): terminum's system user. It starts a task's
//! session on a human's RUN, watches it by hooks' events and a walk on a fixed beat, moves the task
//! as the session lives, and closes what it started. All its clocks and facts are in the store.

mod acts;
pub mod api;
mod press;
mod gate;
mod hooks;
pub mod plan;
pub mod pool;
pub mod usage;
pub mod procs;
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

pub use hooks::{heard, hearing};
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
    /// panes with a live run: the others' hook events cost nothing here; changed only with the store's lock
    /// held, so a walk's read and a window's opening each see the other whole
    panes: Mutex<HashSet<String>>,
    /// hooks' events, in the order they came (AC-131)
    events: hooks::Events,
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
    /// its agent was not under it at the last walk, with no exit recorded: the wrapper may not have got there
    agent_missed: bool,
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

    pub fn has_run(&self, key: &str) -> bool {
        self.panes.lock().contains(key)
    }

    /// Pane `key` holds `run` from now on, in the transaction that says so (AC-131).
    fn pane_opened(&self, tx: &rusqlite::Transaction, run: &str, key: &str) -> Result<runs::Run, String> {
        let r = runs::change(tx, run, |r| r.pane_key = Some(key.to_string()))?;
        self.panes.lock().insert(key.to_string());
        Ok(r)
    }

    /// The runs a walk goes by, and the panes they hold, in one transaction (AC-131).
    fn read_runs(&self, tx: &rusqlite::Transaction) -> Result<Vec<runs::Run>, String> {
        let all = runs::live(tx)?;
        *self.panes.lock() = all.iter().filter_map(|r| r.pane_key.clone()).collect();
        Ok(all)
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
            pane.subs.entry(id.to_string()).or_insert_with(|| Sub {
                kind: crate::plans::rules::clean(ev["subtype"].as_str().unwrap_or("")).chars().take(60).collect(),
                last: Instant::now(),
                told: plan::SubStep::Nothing,
            }).last = Instant::now();
        }
    }

    /// A hook's four integers, or null when they are not (AC-129); its model goes on to the store only.
    pub fn heard_tokens(&self, key: &str, ev: &serde_json::Value) {
        let kept = usage::Totals::of(&ev["tokens"]).map_or(serde_json::Value::Null, |t| t.shown());
        let known = !kept.is_null();
        self.watch.lock().entry(key.to_string()).or_default().tokens = Some(kept);
        // what grew goes to the store, in its turn with the run's other events (AC-129)
        if known && self.has_run(key) {
            let _ = self.events.sender.send((key.to_string(), ev.clone()));
        }
    }

    fn remember_typed(&self, key: &str, text: &str) {
        let mut t = self.typed.lock();
        t.retain(|(_, _, at)| at.elapsed() < TYPED_FOR);
        t.push((key.to_string(), text.to_string(), Instant::now()));
    }

    /// Whether `prompt` in pane `key` is what terminum typed there, on its first 20,000 characters (§9): once.
    fn was_typed(&self, key: &str, prompt: &str) -> bool {
        let mut t = self.typed.lock();
        match typed_at(&mut t, key, prompt) {
            Some(at) => { t.remove(at); true }
            None => false,
        }
    }

    /// Whether Dispatcher typed `prompt` into pane `key` (its message's sender, AC-147), leaving
    /// it for `was_typed`.
    pub fn typed_it(&self, key: &str, prompt: &str) -> bool {
        typed_at(&mut self.typed.lock(), key, prompt).is_some()
    }
}

/// Where in `t` (what Dispatcher typed lately) `prompt` of pane `key` is.
fn typed_at(t: &mut Vec<(String, String, Instant)>, key: &str, prompt: &str) -> Option<usize> {
    let first = |s: &str| s.chars().filter(|c| !c.is_whitespace()).take(20_000).collect::<String>();
    let wanted = first(prompt);
    t.retain(|(_, _, at)| at.elapsed() < TYPED_FOR);
    t.iter().position(|(k, text, _)| k == key && first(text) == wanted)
}

#[cfg(test)]
mod typed_tests {
    /// A prompt Dispatcher typed is its message (AC-147) and still its own for the gate (§9), once.
    #[tokio::test]
    async fn ac147_a_prompt_dispatcher_typed_is_told_apart_and_left_for_its_gate() {
        let (root, app) = crate::testutil::app("dispatcher-typed");
        let d = &app.dispatcher;
        d.remember_typed("k1", "T-0412: do it");
        assert!(d.typed_it("k1", "T-0412:  do it") && !d.typed_it("k2", "T-0412: do it") && !d.typed_it("k1", "my own words"));
        assert!(d.typed_it("k1", "T-0412: do it"), "asking takes nothing");
        assert!(d.was_typed("k1", "T-0412: do it"));
        assert!(!d.was_typed("k1", "T-0412: do it") && !d.typed_it("k1", "T-0412: do it"), "the gate's once");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// The test clock (§9 Times): only with a test's own app folder.
pub fn scale() -> i64 {
    std::env::var("TM_APP_DIR").ok().and_then(|_| std::env::var("TM_TEST_CLOCK_SCALE").ok()).and_then(|s| s.parse().ok()).filter(|s: &i64| *s > 0).unwrap_or(1)
}

/// Moves one task and says why, as Dispatcher; one already there says nothing.
fn move_one(tx: &rusqlite::Transaction, id: &str, to: &str, words: &str) -> Result<(), String> {
    let t = ops::get(tx, id)?;
    if t.status == to {
        return Ok(());
    }
    let at = ops::now();
    ops::move_to(tx, DISPATCHER, id, to, at)?;
    ops::log(tx, &t.key, DISPATCHER, "why", words, None, at).map(|_| ())
}

/// Moves `run`'s tasks still open: what is done stays done (AC-138).
fn move_task(tx: &rusqlite::Transaction, run: &runs::Run, to: &str, words: &str) -> Result<(), String> {
    run.open.iter().try_for_each(|id| move_one(tx, id, to, words))
}

/// Moves all `run`'s tasks: its agent took them.
fn move_all(tx: &rusqlite::Transaction, run: &runs::Run, to: &str, words: &str) -> Result<(), String> {
    run.ids.iter().try_for_each(|id| move_one(tx, id, to, words))
}

/// Says `words` on what `run` is about now, as Dispatcher.
fn say(tx: &rusqlite::Transaction, run: &runs::Run, words: &str) -> Result<(), String> {
    run.targets().iter().try_for_each(|id| ops::log(tx, &ops::key_of(tx, id)?, DISPATCHER, "told", words, None, ops::now()).map(|_| ()))
}

/// Every page hears of `run`'s tasks.
fn told_all(app: &crate::app::Shared, run: &runs::Run) {
    for id in &run.ids {
        crate::tasks::api::told(app, id);
    }
}
