// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A turn's end read from the screen (AC-91 reminders, AC-93 unread): a pane whose agent
//! looked done counts as done only once it stayed so for SETTLE, neither working, waiting nor
//! with subagents running in between. A screen read between two steps of a turn (a title
//! without its spinner for a moment) or while background work goes on ends no turn. A pane whose
//! hooks speak ends its turns by their Stop (hookstate.rs), at the next list read; while its
//! screen still shows work (a shell it started runs on in the background) the end waits.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;

/// How long a pane stays done before its turn counts as ended.
pub const SETTLE: Duration = Duration::from_secs(10);

/// Whether a list row's agent is busy: working, waiting for the user, or with subagents running.
pub fn busy(it: &Value) -> bool {
    it["state"] == "working" || (it["state"] == "waiting" && it["turnOver"] != true) || it["subagents"].as_u64().is_some_and(|n| n > 0)
}

/// The panes that looked done and wait out SETTLE, since when, and whether their hooks said so.
#[derive(Default)]
pub struct Turns {
    pending: Mutex<HashMap<String, (Instant, bool)>>,
}

impl Turns {
    /// A pane whose hooks said its turn ended (Stop, its last subagent's too): it ends at the
    /// next [`Turns::settle`] that finds it not busy, waiting no SETTLE.
    pub fn hook_ended(&self, key: &str, now: Instant) {
        self.pending.lock().insert(key.to_string(), (now.checked_sub(SETTLE).unwrap_or(now), true));
    }

    /// The panes whose hooks ended a turn that waits: their footers are read (layout.rs), each
    /// with whether BACKGROUND_LIMIT passed since the Stop: then only agents in the background
    /// keep the turn going (a shell may never end, a subagent does).
    pub fn hooked(&self, now: Instant) -> HashMap<String, bool> {
        let limit = SETTLE + crate::tmux::agentstate::BACKGROUND_LIMIT;   // its time is SETTLE before the Stop
        self.pending.lock().iter().filter(|(_, (_, hook))| *hook).map(|(k, (at, _))| (k.clone(), now.duration_since(*at) >= limit)).collect()
    }

    /// The panes whose turn just ended: they wait to settle by their own server's list, whichever
    /// server's telling saw it first (AC-132). Those new here, for a read once SETTLE has passed.
    pub fn ended(&self, ended: Vec<String>, now: Instant) -> Vec<String> {
        let mut pending = self.pending.lock();
        ended.into_iter().filter(|k| match pending.entry(k.clone()) {
            std::collections::hash_map::Entry::Vacant(e) => { e.insert((now, false)); true }
            std::collections::hash_map::Entry::Occupied(_) => false,
        }).collect()
    }

    /// Gives back, once, the panes of the server keyed by `server` that have stayed done for SETTLE;
    /// `busy(key)` says whether a pane is busy by that server's list, just read (None: not listed,
    /// gone). Only that list judges its panes: another server's says nothing of them (AC-132). A pane
    /// busy again, or gone, waits no more; one its hooks ended waits while busy (the screen tells
    /// background work its hooks do not).
    pub fn settle(&self, server: &str, busy: impl Fn(&str) -> Option<bool>, now: Instant) -> Vec<String> {
        let mut pending = self.pending.lock();
        let mut out = Vec::new();
        pending.retain(|k, (at, hook)| !k.starts_with(server) || match busy(k) {
            Some(false) if now.duration_since(*at) >= SETTLE => { out.push(k.clone()); false }
            Some(false) => true,
            Some(true) => *hook,
            None => false,
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `ended` as it comes, then a settle by the list just read.
    fn settle(t: &Turns, ended: &[&str], busy: impl Fn(&str) -> Option<bool>, now: Instant) -> Vec<String> {
        t.ended(ended.iter().map(|k| k.to_string()).collect(), now);
        t.settle("", busy, now)
    }

    #[test]
    fn a_turn_ends_once_it_stayed_done() {
        let t = Turns::default();
        let t0 = Instant::now();
        let idle = |_: &str| Some(false);
        assert!(settle(&t, &["a"], idle, t0).is_empty(), "just done: it waits");
        assert!(settle(&t, &[], idle, t0 + SETTLE / 2).is_empty());
        assert!(settle(&t, &["a"], idle, t0 + SETTLE / 2).is_empty(), "seen done again: still since t0");
        assert_eq!(settle(&t, &[], idle, t0 + SETTLE), ["a"], "done for SETTLE: ended, once");
        assert!(settle(&t, &[], idle, t0 + SETTLE * 3).is_empty());
    }

    #[test]
    fn working_again_or_gone_ends_nothing() {
        let t = Turns::default();
        let t0 = Instant::now();
        settle(&t, &["a", "b"], |_| Some(false), t0);
        // a works again (the next step of its turn), b closed
        assert!(settle(&t, &[], |k| (k == "a").then_some(true), t0 + Duration::from_secs(1)).is_empty());
        assert!(settle(&t, &[], |_| Some(false), t0 + SETTLE * 2).is_empty(), "neither waits any more");
    }

    /// AC-132: a list read on one server neither ends nor drops another server's turn.
    #[test]
    fn a_server_settles_only_its_own_panes() {
        let t = Turns::default();
        let t0 = Instant::now();
        t.ended(vec!["tmux:devbox:default:%1".into(), "tmux:local:t:%1".into()], t0);
        let local = "tmux:local:t:";
        assert_eq!(t.settle(local, |_| Some(false), t0 + SETTLE), ["tmux:local:t:%1"], "its own, by its own list");
        assert!(t.settle(local, |_| None, t0 + SETTLE).is_empty(), "the host's pane is not in that list: not gone");
        assert_eq!(t.settle("tmux:devbox:default:", |_| Some(false), t0 + SETTLE), ["tmux:devbox:default:%1"]);
    }

    /// AC-132: each end asks one read, once, whoever saw it first.
    #[test]
    fn an_end_is_new_once() {
        let t = Turns::default();
        let t0 = Instant::now();
        assert_eq!(t.ended(vec!["a".into()], t0), ["a"]);
        assert!(t.ended(vec!["a".into()], t0 + SETTLE / 2).is_empty(), "it waits already");
        assert_eq!(t.settle("", |_| Some(false), t0 + SETTLE), ["a"], "still since the first");
    }

    #[test]
    fn a_hooks_end_comes_at_once_or_once_the_background_work_ended() {
        let t = Turns::default();
        let t0 = Instant::now();
        t.hook_ended("a", t0);
        t.hook_ended("b", t0);
        let a_busy = |k: &str| Some(k == "a");
        assert_eq!(settle(&t, &[], a_busy, t0), ["b"], "not busy: at once");
        assert!(settle(&t, &[], a_busy, t0 + SETTLE * 3).is_empty(), "a shell it started still runs: it waits");
        assert_eq!(settle(&t, &[], |_| Some(false), t0 + SETTLE * 3), ["a"], "the shell ended: its turn ends, once");
        assert!(settle(&t, &[], |_| Some(false), t0 + SETTLE * 4).is_empty());
        t.hook_ended("c", t0);
        assert_eq!(t.hooked(t0).get("c"), Some(&false), "its host's list not read: it waits");
        assert_eq!(t.hooked(t0 + crate::tmux::agentstate::BACKGROUND_LIMIT).get("c"), Some(&true), "ten minutes on: only its agents count");
        assert!(settle(&t, &[], |_| None, t0).is_empty(), "closed");
        assert!(t.hooked(t0).is_empty());
    }

    #[test]
    fn busy_rows() {
        assert!(busy(&json!({"state": "working"})) && busy(&json!({"state": "waiting"})));
        assert!(busy(&json!({"state": "done", "subagents": 2})), "its subagents still run");
        assert!(!busy(&json!({"state": "waiting", "turnOver": true})), "its turn ended leaving it to the user");
        assert!(!busy(&json!({"state": "done"})) && !busy(&json!({"state": null})) && !busy(&json!({"state": "failed"})));
    }
}
