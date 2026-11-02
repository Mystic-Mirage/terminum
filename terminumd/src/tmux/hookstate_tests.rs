// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;
use serde_json::json;
use std::time::Duration;

const K: &str = "tmux:local:terminum:%3";

fn ev(agent: &str, event: &str, extra: Value) -> Value {
    let mut v = json!({"agent": agent, "event": event, "session": "s1"});
    v.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
    v
}

#[test]
fn late_events_of_another_session_leave_the_current_record_intact() {
    for name in ["SessionEnd", "PreToolUse", "Stop", "SubagentStop"] {
        let (h, t) = (Hooks::default(), Instant::now());
        h.apply(K, &ev("claude", "UserPromptSubmit", json!({"session": "new", "turn": "new-turn", "cwd": "/Users/alex/current"})), t);
        h.apply(K, &ev("claude", "SubagentStart", json!({"session": "new", "turn": "new-turn", "sub": "worker"})), t);
        let before = h.view(K, "claude", t);
        assert!(!h.apply(K, &ev("claude", name, json!({"session": "old", "turn": "old-turn", "cwd": "/Users/alex/old", "sub": "worker"})), t));
        assert_eq!(h.view(K, "claude", t), before, "late {name}");
        assert_eq!(h.cwd(K).as_deref(), Some("/Users/alex/current"));
        assert!(!h.take_ended(K));
    }
}

#[test]
fn rejected_identities_are_distinct_from_accepted_unchanged_events() {
    let (h, t) = (Hooks::default(), Instant::now());
    assert_eq!(h.apply_event(K, &ev("other", "SessionStart", json!({})), t), None);
    assert_eq!(h.apply_event(K, &ev("claude", "SessionStart", json!({"session": "new"})), t), Some(true));
    assert_eq!(h.apply_event(K, &ev("claude", "SomethingNew", json!({"session": "new"})), t), Some(false));
    assert_eq!(h.apply_event(K, &ev("claude", "PostToolUse", json!({"session": "old", "plan": {"op": "all", "items": []}})), t), None);
    assert_eq!(h.apply_event(K, &ev("codex", "SessionEnd", json!({"session": "new"})), t), None);
    assert_eq!(h.apply_event(K, &ev("codex", "UserPromptSubmit", json!({"session": "next"})), t), Some(true));
    assert_eq!(h.session(K), Some(("codex", "next".to_string())));
    assert_eq!(h.apply_event(K, &ev("codex", "SessionEnd", json!({"session": "next"})), t), Some(true));
}

#[test]
fn a_turn_is_working_then_done_with_its_tool_and_subagents() {
    let h = Hooks::default();
    let t = Instant::now();
    assert!(h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p1", "prompt": "fix"})), t));
    h.apply(K, &ev("claude", "PreToolUse", json!({"turn": "p1", "tool": "Bash: npm test"})), t);
    h.apply(K, &ev("claude", "SubagentStart", json!({"turn": "p1", "sub": "a1"})), t);
    let v = h.view(K, "2.1.303", t).unwrap();
    assert_eq!((v.state, v.tool.as_str(), v.subagents), (State::Working, "Bash: npm test", 1));
    h.apply(K, &ev("claude", "SubagentStop", json!({"turn": "p1", "sub": "a1"})), t);
    h.apply(K, &ev("claude", "Stop", json!({"turn": "p1"})), t);
    let v = h.view(K, "2.1.303", t).unwrap();
    assert_eq!((v.state, v.subagents, v.tool.as_str()), (State::Done, 0, ""));
    assert!(h.take_ended(K) && !h.take_ended(K), "its end is told once");
}

/// A turn that stops while its subagents run in the background is working until the last stops.
#[test]
fn a_turn_with_subagents_in_the_background_ends_with_the_last() {
    let h = Hooks::default();
    let t = Instant::now();
    h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p1"})), t);
    h.apply(K, &ev("claude", "SubagentStart", json!({"turn": "p1", "sub": "a1"})), t);
    h.apply(K, &ev("claude", "SubagentStart", json!({"turn": "p1", "sub": "a2"})), t);
    h.apply(K, &ev("claude", "Stop", json!({"turn": "p1"})), t);
    assert_eq!(h.view(K, "c", t).map(|v| (v.state, v.subagents)), Some((State::Working, 2)), "Claude Code is done; its subagents are not");
    assert!(!h.take_ended(K), "no end yet: no reminder, no unread");
    let later = t + LEASE + Duration::from_secs(5);
    assert_eq!(h.view(K, "c", later).map(|v| v.state), Some(State::Working), "a background subagent may be quiet longer than a turn");
    h.apply(K, &ev("claude", "PreToolUse", json!({"turn": "p1", "tool": "Bash: ls"})), later);
    h.apply(K, &ev("claude", "SubagentStop", json!({"turn": "p1", "sub": "a1"})), later);
    assert_eq!(h.view(K, "c", later).map(|v| (v.state, v.subagents)), Some((State::Working, 1)));
    h.apply(K, &ev("claude", "SubagentStop", json!({"turn": "p1", "sub": "a2"})), later);
    assert_eq!(h.view(K, "c", later).map(|v| (v.state, v.subagents)), Some((State::Done, 0)));
    assert!(h.take_ended(K), "the last one's stop ends the turn");
    // an interrupt ends it at once, subagents or not
    h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p2"})), later);
    h.apply(K, &ev("claude", "SubagentStart", json!({"turn": "p2", "sub": "b1"})), later);
    h.apply(K, &ev("claude", "Interrupt", json!({"turn": "p2"})), later);
    assert_eq!(h.view(K, "c", later).map(|v| v.state), Some(State::Done));
    assert!(h.take_ended(K));
}

/// The main thread idle (Claude Code's idle notification) or a new prompt while subagents run in
/// the background: still working until the last of them stops, and only then ended.
#[test]
fn subagents_keep_it_working_through_an_idle_notice_and_the_next_prompt() {
    let h = Hooks::default();
    let t = Instant::now();
    h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p1"})), t);
    h.apply(K, &ev("claude", "SubagentStart", json!({"turn": "p1", "sub": "a1"})), t);
    h.apply(K, &ev("claude", "Stop", json!({"turn": "p1"})), t);
    h.apply(K, &ev("claude", "Notification", json!({"kind": "idle_prompt", "message": "Claude is waiting for your input"})), t);
    assert_eq!(h.view(K, "c", t).map(|v| (v.state, v.subagents)), Some((State::Working, 1)), "idle, its subagent is not");
    assert!(!h.take_ended(K));
    h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p2"})), t);
    h.apply(K, &ev("claude", "Stop", json!({"turn": "p2"})), t);
    assert_eq!(h.view(K, "c", t).map(|v| (v.state, v.subagents)), Some((State::Working, 1)), "the next turn ended; the subagent of the one before runs on");
    assert!(!h.take_ended(K));
    h.apply(K, &ev("claude", "SubagentStop", json!({"turn": "p1", "sub": "a1"})), t);
    assert_eq!(h.view(K, "c", t).map(|v| (v.state, v.subagents)), Some((State::Done, 0)));
    assert!(h.take_ended(K), "the last one's stop ends it");
    h.apply(K, &ev("claude", "Notification", json!({"kind": "idle_prompt"})), t);
    assert_eq!(h.view(K, "c", t).map(|v| v.state), Some(State::Done), "no subagents: an idle notice is done");
}

#[test]
fn waiting_for_an_approval_or_an_answer_and_a_failure() {
    let h = Hooks::default();
    let t = Instant::now();
    assert!(h.apply(K, &ev("claude", "PermissionRequest", json!({"tool": "Bash: rm -rf"})), t), "a live session's event makes its record (it went with a link or a restart)");
    h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p1"})), t);
    h.apply(K, &ev("claude", "PermissionRequest", json!({"tool": "Bash: rm -rf"})), t);
    assert_eq!(h.view(K, "c", t).map(|v| (v.state, v.why)), Some((State::Waiting, "Approve Bash: rm -rf".to_string())));
    h.apply(K, &ev("claude", "PreToolUse", json!({"tool": "AskUserQuestion", "message": "Which one?"})), t);
    assert_eq!(h.view(K, "c", t).map(|v| v.why), Some("Which one?".to_string()));
    h.apply(K, &ev("claude", "StopFailure", json!({"error": "rate_limit"})), t);
    assert_eq!(h.view(K, "c", t).map(|v| (v.state, v.why)), Some((State::Failed, "rate_limit".to_string())));
}

#[test]
fn a_late_working_event_of_a_stopped_turn_changes_nothing() {
    let h = Hooks::default();
    let t = Instant::now();
    h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p1"})), t);
    h.apply(K, &ev("claude", "Stop", json!({"turn": "p1"})), t);
    assert!(!h.apply(K, &ev("claude", "PostToolUse", json!({"turn": "p1"})), t), "background hooks come out of order");
    assert!(!h.apply(K, &ev("claude", "PermissionRequest", json!({"turn": "p1", "tool": "Bash"})), t), "nor a late approval");
    assert!(!h.apply(K, &ev("claude", "Notification", json!({"turn": "p1", "kind": "permission_prompt"})), t));
    assert_eq!(h.view(K, "c", t).unwrap().state, State::Done);
    h.apply(K, &ev("claude", "SessionEnd", json!({})), t);
    assert!(!h.apply(K, &ev("claude", "PostToolUse", json!({"turn": "p1"})), t), "a late event of an ended session makes no record");
    assert!(!h.has(K));
}

#[test]
fn leases_run_out_and_a_new_program_or_session_starts_anew() {
    let h = Hooks::default();
    let t = Instant::now();
    h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p1"})), t);
    assert!(h.view(K, "2.1.303", t + LEASE - Duration::from_secs(1)).is_some());
    assert!(h.view(K, "2.1.303", t + LEASE).is_none(), "an interrupt fires no Stop: the screen says again");
    h.apply(K, &ev("claude", "PermissionRequest", json!({"tool": "Bash"})), t);
    assert!(h.view(K, "2.1.303", t + WAIT_LEASE).is_none());
    h.apply(K, &ev("claude", "Stop", json!({})), t);
    assert!(h.view(K, "zsh", t).is_none(), "the program changed: its hooks no longer hold");
    assert!(!h.has(K));
    h.apply(K, &ev("claude", "Stop", json!({})), t);
    h.apply(K, &json!({"agent": "claude", "event": "UserPromptSubmit", "session": "s2"}), t);
    assert_eq!(h.view(K, "c", t).unwrap().state, State::Working, "another session: a record of its own");
    h.apply(K, &ev("claude", "SessionEnd", json!({})), t);
    assert!(h.view(K, "c", t).is_some(), "the old session's end cannot remove s2");
    h.apply(K, &ev("claude", "SessionEnd", json!({"session": "s2"})), t);
    assert!(h.view(K, "c", t).is_none());
}

#[test]
fn codex_done_is_the_screens_and_unknown_events_change_nothing() {
    let h = Hooks::default();
    let t = Instant::now();
    h.apply(K, &ev("codex", "UserPromptSubmit", json!({"turn": "t1"})), t);
    assert_eq!(h.view(K, "node", t).unwrap().agent, "codex");
    h.apply(K, &ev("codex", "Stop", json!({"turn": "t1"})), t);
    assert!(h.view(K, "node", t).is_none(), "Codex has no failure event: done may be a failure on the screen");
    assert!(!h.apply(K, &ev("codex", "SomethingNew", json!({})), t));
    assert!(!h.apply(K, &json!({"agent": "other", "event": "Stop"}), t));
    h.retain("tmux:local:terminum:", &[]);
    assert!(!h.has(K), "a pane that is gone is forgotten");
}

#[test]
fn the_agents_own_folder_is_its_last_own_events_not_a_subagents() {
    let (h, t) = (Hooks::default(), Instant::now());
    assert_eq!(h.cwd(K), None, "no hooks: not known");
    h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p1", "cwd": "/Users/alex/api"})), t);
    assert_eq!(h.cwd(K).as_deref(), Some("/Users/alex/api"));
    assert!(h.apply(K, &ev("claude", "PostToolUse", json!({"turn": "p1", "cwd": "/Users/alex/api/worktrees/fix"})), t), "a move changes what the list shows");
    assert_eq!(h.cwd(K).as_deref(), Some("/Users/alex/api/worktrees/fix"), "a cd in the turn moves it");
    assert!(!h.apply(K, &ev("claude", "PostToolUse", json!({"turn": "p1", "cwd": "/Users/alex/api/worktrees/fix"})), t), "the same folder again: nothing changed");
    h.apply(K, &ev("claude", "PreToolUse", json!({"turn": "p1", "sub": "a1", "cwd": "/Users/alex/elsewhere"})), t);
    h.apply(K, &ev("claude", "Stop", json!({"turn": "p1", "cwd": "relative/not/a/folder"})), t);
    assert_eq!(h.cwd(K).as_deref(), Some("/Users/alex/api/worktrees/fix"), "a subagent's folder and a relative one are not the agent's");
    h.apply(K, &ev("claude", "SessionEnd", json!({})), t);
    assert_eq!(h.cwd(K), None, "the session ended");
}

#[test]
fn working_is_the_hooks_word_and_a_changed_program_is_told_once() {
    let (h, t) = (Hooks::default(), Instant::now());
    assert!(!h.working(K), "no hooks: not working");
    h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p1"})), t);
    assert!(h.working(K));
    h.apply(K, &ev("claude", "Stop", json!({"turn": "p1"})), t);
    assert!(!h.working(K));
    h.view(K, "claude", t);
    assert!(h.take_changed().is_empty(), "the same program");
    assert_eq!(h.view(K, "zsh", t), None);
    assert_eq!(h.take_changed(), vec![K.to_string()]);
    assert!(h.take_changed().is_empty(), "told once");
}

/// The hand check of the 2026-10-31 review as a test: Claude Code's /clear (SessionEnd, then a
/// SessionStart of a new session), its /resume (a SessionStart of the old one), then Codex in
/// the same pane; each late event of a session no longer current changes nothing.
#[test]
fn clear_resume_then_codex_in_one_pane() {
    let (h, t) = (Hooks::default(), Instant::now());
    let at = |s: &str, sid: &str, extra: Value| { let mut e = ev("claude", s, extra); e["session"] = json!(sid); e };
    h.apply(K, &at("UserPromptSubmit", "A", json!({"turn": "a1"})), t);
    h.apply(K, &at("PreToolUse", "A", json!({"turn": "a1", "tool": "Bash"})), t);
    // /clear
    h.apply(K, &at("SessionEnd", "A", json!({"reason": "clear"})), t);
    assert_eq!(h.apply_event(K, &at("SessionStart", "B", json!({"source": "clear"})), t), Some(true));
    assert_eq!(h.session(K), Some(("claude", "B".to_string())));
    assert!(!h.apply(K, &at("PostToolUse", "A", json!({"turn": "a1"})), t), "a late tool end of the cleared session");
    assert!(!h.apply(K, &at("Stop", "A", json!({"turn": "a1"})), t));
    assert_ne!(h.view(K, "claude", t).map(|v| v.state), Some(State::Working), "nothing of A's work shows under B");
    // /resume of A
    assert_eq!(h.apply_event(K, &at("SessionStart", "A", json!({"source": "resume"})), t), Some(true));
    assert_eq!(h.session(K), Some(("claude", "A".to_string())));
    assert!(!h.apply(K, &at("Stop", "B", json!({})), t), "the session left by /resume");
    h.apply(K, &at("UserPromptSubmit", "A", json!({"turn": "a2"})), t);
    assert_eq!(h.view(K, "claude", t).map(|v| v.state), Some(State::Working));
    h.apply(K, &at("Stop", "A", json!({"turn": "a2"})), t);
    h.apply(K, &at("SessionEnd", "A", json!({"reason": "exit"})), t);
    // Codex in the same pane
    assert_eq!(h.apply_event(K, &ev("codex", "UserPromptSubmit", json!({"session": "C", "turn": "c1"})), t), Some(true));
    assert_eq!(h.session(K), Some(("codex", "C".to_string())));
    assert!(!h.apply(K, &at("Stop", "A", json!({"turn": "a2"})), t), "Claude's late Stop after Codex took the pane");
    assert_eq!(h.view(K, "node", t).map(|v| (v.agent, v.state)), Some(("codex", State::Working)));
}

/// A host's link dropped (its panes' records went: links.rs forget) or terminumd restarted while an
/// agent worked: its next event of any kind makes its record again, as working, so the list and the
/// plan answers know its hooks run; a session that ended stays without one.
#[test]
fn any_event_of_a_live_session_makes_its_record_again() {
    let h = Hooks::default();
    let t = Instant::now();
    h.apply(K, &ev("claude", "UserPromptSubmit", json!({"turn": "p1"})), t);
    h.retain("tmux:local:", &[]);                                    // the link's forget
    assert!(!h.has(K));
    assert!(h.apply(K, &ev("claude", "PreToolUse", json!({"turn": "p1", "tool": "mcp__terminum__update_session_plan"})), t));
    assert!(h.has(K));
    assert_eq!(h.view(K, "c", t).unwrap().state, State::Working);
    h.apply(K, &ev("claude", "SessionEnd", json!({})), t);
    assert!(!h.apply(K, &ev("claude", "PostToolUse", json!({"turn": "p1"})), t), "ended: no record");
    assert!(!h.has(K));
    assert!(!h.apply(K, &json!({"agent": "claude", "event": "PostToolUse"}), t), "no session named: no record");
    assert!(!h.has(K));
}
