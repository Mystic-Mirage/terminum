// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;

fn run(status: &str) -> Run {
    Run { status: status.into(), state: "working".into(), kind: "claude".into(), task_id: "T-12".into(), ..Default::default() }
}

fn up(dt: i64) -> Facts {
    Facts { pane: true, marked: true, alive: true, dt, ..Default::default() }
}

fn moved(acts: &[Act]) -> Option<(&'static str, String)> {
    acts.iter().find_map(|a| match a { Act::Move { to, words } => Some((*to, words.clone())), _ => None })
}

#[test]
fn ac123_a_turn_with_no_result_gets_one_request_then_a_human() {
    let mut r = run("IN PROGRESS");
    r.turn_ended = true;
    assert!(step(&mut r, &up(299)).is_empty());
    assert_eq!(step(&mut r, &up(1)), vec![Act::Nudge]);
    assert!(step(&mut r, &up(30)).is_empty(), "once a run");
    let acts = step(&mut r, &up(570));
    assert_eq!(moved(&acts).unwrap(), ("WAIT FOR HUMAN", "The agent stopped without a result".to_string()));
}

#[test]
fn ac123_activity_starts_the_idle_clock_anew() {
    let mut r = run("IN PROGRESS");
    r.turn_ended = true;
    step(&mut r, &up(299));
    step(&mut r, &Facts { active: true, ..up(30) });
    assert_eq!(r.idle_for, 0);
    assert!(step(&mut r, &up(299)).is_empty());
}

#[test]
fn ac123_an_hour_of_nothing_is_done_with_no_result() {
    let mut r = run("IN PROGRESS");
    let acts = step(&mut r, &up(3600));
    let (to, words) = moved(&acts).unwrap();
    assert_eq!(to, "DONE");
    assert!(words.contains("no activity for 60 minutes"));
    let mut waited = run("WAIT FOR HUMAN");
    waited.no_result_wait = true;
    assert_eq!(moved(&step(&mut waited, &up(3600))).unwrap().0, "DONE", "also after it stopped without a result");
    let mut asked = run("WAIT FOR HUMAN");
    assert_eq!(moved(&step(&mut asked, &up(3600))), None, "not a wait for an answer");
}

#[test]
fn ac123_the_exit_code_decides_a_clean_end_from_a_signal() {
    let mut r = run("IN PROGRESS");
    let (to, words) = moved(&step(&mut r, &Facts { exit: Some(0), alive: false, ..up(0) })).unwrap();
    assert_eq!((to, words.as_str()), ("DONE", "The session ended without finish_task."));
    assert_eq!(r.state, "gone");
    let mut r = run("IN PROGRESS");
    let (to, words) = moved(&step(&mut r, &Facts { exit: Some(143), alive: false, ..up(0) })).unwrap();
    assert_eq!((to, words.as_str()), ("FAILED", "The agent's process ended: exit 143."));
    let mut r = run("IN PROGRESS");
    assert_eq!(moved(&step(&mut r, &Facts { alive: false, ..up(0) })).unwrap().0, "FAILED", "dead with no exit recorded");
    assert_eq!(step(&mut r, &Facts { alive: false, ..up(30) }), vec![], "said once");
}

#[test]
fn ac123_a_failed_turn_is_failed_with_its_reason() {
    let mut r = run("IN PROGRESS");
    let acts = step(&mut r, &Facts { failed: Some("rate_limit".into()), ..up(0) });
    assert_eq!(moved(&acts).unwrap(), ("FAILED", "The agent's turn failed: rate_limit".to_string()));
}

#[test]
fn ac124_two_minutes_of_a_question_wait_for_a_human_and_it_returns() {
    let mut r = run("IN PROGRESS");
    let asks = Facts { waiting: Some("Approve Bash".into()), ..up(90) };
    assert!(step(&mut r, &asks).is_empty(), "a short wait moves nothing");
    let acts = step(&mut r, &Facts { dt: 30, ..asks.clone() });
    assert_eq!(moved(&acts).unwrap(), ("WAIT FOR HUMAN", "Approve Bash".to_string()));
    r.status = "WAIT FOR HUMAN".into();
    assert!(step(&mut r, &Facts { dt: 86400, ..asks.clone() }).is_empty(), "no time limit on a wait");
    assert_eq!(r.idle_for, 0, "no idle clock while it waits");
    let acts = step(&mut r, &Facts { active: true, ..up(30) });
    assert_eq!(moved(&acts).unwrap().0, "IN PROGRESS");
}

#[test]
fn ac124_a_human_s_move_stands_against_every_signal() {
    let mut r = run("WAIT FOR HUMAN");
    r.held = true;
    r.wait_moved = true;
    r.turn_ended = true;
    assert_eq!(moved(&step(&mut r, &Facts { active: true, ..up(30) })), None);
    assert_eq!(moved(&step(&mut r, &up(7200))), None, "no nudge and no DONE");
    assert_eq!(moved(&step(&mut r, &Facts { failed: Some("x".into()), ..up(0) })), None);
    let acts = step(&mut r, &Facts { exit: Some(1), alive: false, ..up(0) });
    assert_eq!(moved(&acts), None, "nor an end: it is said");
    assert_eq!(acts, vec![Act::Say("Session ended: claude ended, exit 1.".into())]);
}

#[test]
fn ac125_it_closes_two_hours_after_the_end_only_when_idle() {
    let ended = |dt| Facts { exit: Some(0), alive: false, dt, ..up(0) };
    let mut r = run("DONE");
    r.state = "gone".into();
    assert!(step(&mut r, &ended(7199)).is_empty());
    let acts = step(&mut r, &ended(1));
    assert_eq!(acts, vec![Act::Close { words: "Session ended: closed 2 hours after DONE.".into(), keep: true }]);
    assert_eq!(r.state, "gone", "closed only when tmux did it");
    let mut r = run("DONE");
    r.result = true;
    r.turn_ended = true;
    let acts = step(&mut r, &up(7200));
    assert!(matches!(&acts[0], Act::Close { keep: false, .. }), "with a result no screen is kept");
}

#[test]
fn ac125_activity_starts_the_two_hours_again_said_once() {
    let mut r = run("DONE");
    step(&mut r, &up(7000));
    let acts = step(&mut r, &Facts { active: true, ..up(30) });
    assert_eq!(acts, vec![Act::Say("The 2 hours before closing start again: there is activity in the session.".into())]);
    assert_eq!(r.closing_for, 0);
    step(&mut r, &up(30));
    assert!(step(&mut r, &Facts { active: true, ..up(30) }).is_empty(), "once");
}

#[test]
fn ac125_a_human_at_the_shell_keeps_it_open() {
    let mut r = run("DONE");
    r.state = "gone".into();
    let shell = |dt, active| Facts { exit: Some(0), alive: false, active, dt, ..up(0) };
    step(&mut r, &shell(7000, false));
    step(&mut r, &shell(30, true));
    assert_eq!(r.closing_for, 0, "their keys start the clock again");
    assert!(step(&mut r, &shell(7199, false)).is_empty());
}

#[test]
fn ac125_an_agent_still_working_is_never_closed() {
    // its start failed by our reading (a Codex with hooks not trusted): it runs on, no turn's end heard
    let mut r = run("FAILED");
    r.state = "gone".into();
    assert!(step(&mut r, &up(9000)).is_empty());
    r.turn_ended = true;
    assert!(matches!(&step(&mut r, &up(30))[0], Act::Close { .. }), "once its turn is over");
}

#[test]
fn ac125_other_tasks_and_a_human_s_program_keep_it_open() {
    let mut r = run("DONE");
    r.turn_ended = true;
    assert!(step(&mut r, &Facts { others_open: true, ..up(9000) }).is_empty());
    let acts = step(&mut r, &Facts { program: Some("vim".into()), ..up(30) });
    assert_eq!(acts, vec![Act::Say("Session left open: vim runs in it".into())]);
    assert_eq!(r.state, "left");
}

#[test]
fn ac125_a_pane_not_this_run_s_is_never_acted_on() {
    let mut r = run("IN PROGRESS");
    let acts = step(&mut r, &Facts { pane: true, marked: false, alive: true, dt: 30, ..Default::default() });
    assert_eq!(moved(&acts).unwrap(), ("FAILED", "Its pane is no longer this run's: the link is dropped.".to_string()));
    assert_eq!((r.state.as_str(), r.pane_key), ("closed", None));
    let mut gone = run("IN PROGRESS");
    gone.pane_key = Some("tmux:local:s:%1".into());
    step(&mut gone, &Facts::default());
    assert_eq!(gone.pane_key.as_deref(), Some("tmux:local:s:%1"), "a closed window's key stays: RUN goes on by the resume");
    let mut r = run("DONE");
    assert!(matches!(&step(&mut r, &Facts::default())[0], Act::Say(_)), "a closed window of a finished task is only said");
}

#[test]
fn ac131_no_clock_gains_a_gap() {
    let mut r = run("IN PROGRESS");
    r.turn_ended = true;
    step(&mut r, &up(0));
    assert_eq!(r.idle_for, 0);
    let mut d = run("DONE");
    step(&mut d, &up(0));
    assert_eq!(d.closing_for, 0);
}
