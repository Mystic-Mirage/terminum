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

    /// The panes whose turn just ended: they wait to settle by their own server's next list. Another
    /// server's telling may be the first to see it (AC-132).
    pub fn ended(&self, ended: Vec<String>, now: Instant) {
        let mut pending = self.pending.lock();
        for k in ended {
            pending.entry(k).or_insert((now, false));
        }
    }

    /// Takes the panes `ended` just now and gives back, once, those of the server keyed by `server`
    /// that have stayed done for SETTLE; `busy(key)` says whether a pane is busy by that server's list,
    /// just read (None: not listed, gone). Only that list judges its panes: another server's says
    /// nothing of them, whatever it was read for (AC-132). A pane busy again, or gone, waits no more;
    /// one its hooks ended waits while busy (the screen tells background work its hooks do not).
    pub fn settle(&self, server: &str, ended: Vec<String>, busy: impl Fn(&str) -> Option<bool>, now: Instant) -> Vec<String> {
        self.ended(ended, now);
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

    /// How long until the first pane waiting now has stayed done for SETTLE (the loop reads the
    /// list again then), None when none waits; a hook's end is read again with every list, and one
    /// on a server `reachable` denies when it answers: its list is not asked for it (AC-132).
    pub fn due_in(&self, now: Instant, reachable: impl Fn(&str) -> bool) -> Option<Duration> {
        self.pending.lock().iter().filter(|(k, (_, hook))| !hook && reachable(k)).map(|(_, (at, _))| SETTLE.saturating_sub(now.duration_since(*at))).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_turn_ends_once_it_stayed_done() {
        let t = Turns::default();
        let t0 = Instant::now();
        let idle = |_: &str| Some(false);
        assert!(t.settle("", vec!["a".into()], idle, t0).is_empty(), "just done: it waits");
        assert!(t.settle("", vec![], idle, t0 + SETTLE / 2).is_empty());
        assert!(t.settle("", vec!["a".into()], idle, t0 + SETTLE / 2).is_empty(), "seen done again: still since t0");
        assert_eq!(t.due_in(t0 + SETTLE / 2, |_| true), Some(SETTLE / 2), "the loop reads the list again then");
        assert_eq!(t.settle("", vec![], idle, t0 + SETTLE), ["a"], "done for SETTLE: ended, once");
        assert_eq!(t.due_in(t0 + SETTLE, |_| true), None);
        assert!(t.settle("", vec![], idle, t0 + SETTLE * 3).is_empty());
    }

    #[test]
    fn working_again_or_gone_ends_nothing() {
        let t = Turns::default();
        let t0 = Instant::now();
        t.settle("", vec!["a".into(), "b".into()], |_| Some(false), t0);
        // a works again (the next step of its turn), b closed
        assert!(t.settle("", vec![], |k| (k == "a").then_some(true), t0 + Duration::from_secs(1)).is_empty());
        assert!(t.settle("", vec![], |_| Some(false), t0 + SETTLE * 2).is_empty(), "neither waits any more");
    }

    /// AC-132: a list read on one server neither ends nor drops another server's turn.
    #[test]
    fn a_server_settles_only_its_own_panes() {
        let t = Turns::default();
        let t0 = Instant::now();
        t.settle("", vec!["tmux:devbox:default:%1".into(), "tmux:local:t:%1".into()], |_| Some(false), t0);
        let local = "tmux:local:t:";
        assert_eq!(t.settle(local, vec![], |_| Some(false), t0 + SETTLE), ["tmux:local:t:%1"], "its own, by its own list");
        assert!(t.settle(local, vec![], |_| None, t0 + SETTLE).is_empty(), "the host's pane is not in that list: not gone");
        assert_eq!(t.settle("tmux:devbox:default:", vec![], |_| Some(false), t0 + SETTLE), ["tmux:devbox:default:%1"]);
    }

    /// AC-132: an end seen by another server's telling waits for its own list, and a pane on a server
    /// that cannot be reached asks for no list.
    #[test]
    fn an_end_seen_elsewhere_waits_for_its_own_list() {
        let t = Turns::default();
        let t0 = Instant::now();
        t.ended(vec!["tmux:devbox:default:%1".into()], t0);
        assert_eq!(t.due_in(t0, |_| true), Some(SETTLE));
        assert_eq!(t.due_in(t0, |_| false), None, "down: nothing to read it from");
        assert_eq!(t.settle("tmux:devbox:default:", vec![], |_| Some(false), t0 + SETTLE), ["tmux:devbox:default:%1"]);
    }

    #[test]
    fn a_hooks_end_comes_at_once_or_once_the_background_work_ended() {
        let t = Turns::default();
        let t0 = Instant::now();
        t.hook_ended("a", t0);
        t.hook_ended("b", t0);
        assert_eq!(t.due_in(t0, |_| true), None, "the list is read after every hook: no wake-up of its own");
        let a_busy = |k: &str| Some(k == "a");
        assert_eq!(t.settle("", vec![], a_busy, t0), ["b"], "not busy: at once");
        assert!(t.settle("", vec![], a_busy, t0 + SETTLE * 3).is_empty(), "a shell it started still runs: it waits");
        assert_eq!(t.settle("", vec![], |_| Some(false), t0 + SETTLE * 3), ["a"], "the shell ended: its turn ends, once");
        assert!(t.settle("", vec![], |_| Some(false), t0 + SETTLE * 4).is_empty());
        t.hook_ended("c", t0);
        assert_eq!(t.hooked(t0).get("c"), Some(&false), "its host's list not read: it waits");
        assert_eq!(t.hooked(t0 + crate::tmux::agentstate::BACKGROUND_LIMIT).get("c"), Some(&true), "ten minutes on: only its agents count");
        assert!(t.settle("", vec![], |_| None, t0).is_empty() && t.settle("", vec![], |_| Some(false), t0).is_empty(), "closed: no end");
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
