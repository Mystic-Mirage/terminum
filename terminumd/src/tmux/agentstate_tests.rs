// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;
use State::*;

fn rows(r: &[&str]) -> Vec<String> {
    r.iter().map(|s| s.to_string()).collect()
}

fn footer() -> Vec<String> {
    rows(&["────", "❯ ", "────", "  ~/p Opus 5.5 ctx:47%"])
}

fn with_footer(r: &[&str]) -> Vec<String> {
    let mut v = rows(r);
    v.extend(footer());
    v
}

#[test]
fn title_marks() {
    assert_eq!(state_of("◐ Enable web", &footer()), Working);
    assert_eq!(state_of("◓ Enable web", &footer()), Working);
    assert_eq!(state_of("✳ Enable web", &footer()), Done);
    assert!(claude_marked("✳ x") && claude_marked("◑ x") && claude_marked("⠋ x"));
    assert!(!claude_marked("mc [root@devbox.example]") && !claude_marked(""));
}

#[test]
fn the_screen_tells_waiting_and_working() {
    let ask = rows(&["Do you want to proceed?", "❯ 1. Yes", "  2. No", "Esc to cancel"]);
    assert_eq!(state_of("✳ x", &ask), Waiting);
    assert_eq!(state_of("◐ x", &ask), Waiting, "a question wins over the spinner");
    assert_eq!(state_of("", &with_footer(&["✻ Thinking… (12s · esc to interrupt)"])), Working);
    assert_eq!(state_of("", &rows(&["Allow edit to src/a.rs?"])), Waiting);
    assert_eq!(state_of("", &rows(&["Continue? [Y/n]"])), Waiting, "case-insensitive");
    let told = with_footer(&["  \"Would you like to run the following command?\",", "  do you want to (y/n)", "✻ Thinking… (esc to interrupt)"]);
    assert_eq!(state_of("◐ x", &told), Working, "a question's words in the transcript, above the input box, ask nothing");
}

#[test]
fn claude_codes_status_line_without_title_marks() {
    let working = with_footer(&["\u{273b}\0Combobulating\u{2026} (6m\08s\0\u{b7}\0\u{2193}\x0031.5k\0tokens)", ""]);
    let done = with_footer(&["\u{273b} Worked for 2m 12s \u{b7} done 8:17 AM \u{b7} 1 shell still running", ""]);
    assert_eq!(state_of("Demo stand preparation", &working), Working);
    assert_eq!(state_of("Demo stand preparation", &done), Done);
}

#[test]
fn background_subagents_keep_the_turn_going() {
    for line in ["\u{273b} Waiting for 2 background agents to finish", "\u{273b} Waiting for 1 background agent and 1 dynamic workflow to finish",
                 "\u{273b} Waiting for 3 dynamic workflows to finish"] {
        assert_eq!(state_of("\u{2733} Fix it", &with_footer(&[line, ""])), Working, "{line}");
    }
    assert_eq!(state_of("\u{2733} x", &with_footer(&["waiting for the build to finish", ""])), Done, "words of the transcript");
    let after = with_footer(&["\u{273b} Waiting for 2 background agents to finish", "\u{23fa} Both agents are done.", "\u{273b} Worked for 3m 2s", ""]);
    assert_eq!(state_of("\u{2733} x", &after), Done, "a turn after they ended: its own line counts");
}

#[test]
fn background_work_in_the_footer_keeps_the_session_going() {
    let turn = ["\u{273b} Worked for 2m 12s \u{b7} 1 shell still running", "", "\u{2500}\u{2500}\u{2500}\u{2500}", "\u{276f} ", "\u{2500}\u{2500}\u{2500}\u{2500}", "  ~/p Opus 5.5 ctx:47%"];
    let with = |pill: &str| { let mut v = rows(&turn); v.push(pill.to_string()); v };
    for pill in ["  \u{23f5}\u{23f5} bypass permissions on \u{b7} 1 shell, 1 monitor \u{b7} \u{2190} 1 agent      current: 2.1.302",
                 "  1 shell", "  2 monitors \u{b7} ? for shortcuts", "  \u{23f5}\u{23f5} accept edits on \u{b7} 2 local agents",
                 "  1 background dynamic workflow", "  \u{25c7} 1 cloud session", "  3 background tasks", "  1 MCP task", "  2 teams"] {
        assert_eq!(state_of("\u{2733} Fix it", &with(pill)), Working, "{pill}");
    }
    for pill in ["  \u{23f5}\u{23f5} bypass permissions on \u{b7} \u{2190} 1 agent", "  ? for shortcuts", "  1 Artifact comment monitor", "  dreaming"] {
        assert_eq!(state_of("\u{2733} Fix it", &with(pill)), Done, "{pill}: the turn line's shell has ended");
    }
    let above = rows(&["  1 shell", "\u{2500}\u{2500}\u{2500}\u{2500}", "\u{276f} ", "\u{2500}\u{2500}\u{2500}\u{2500}", "  ~/p ctx:47%"]);
    assert_eq!(state_of("\u{2733} x", &above), Done, "words above the input box are the transcript");
    for pill in ["  1 Artifact comment monitor, 1 shell", "  dreaming, 2 shells \u{b7} ? for shortcuts", "  1 teammate \u{b7} 1 shell"] {
        assert_eq!(state_of("\u{2733} Fix it", &with(pill)), Working, "{pill}");
    }
    for pill in ["  1 shell script added", "  3 monitors found", "  1 teammate"] {
        assert_eq!(state_of("\u{2733} Fix it", &with(pill)), Done, "{pill}: no count of Claude Code's");
    }
    let md = rows(&["\u{2500}\u{2500}\u{2500}\u{2500}", "## Notes", "\u{2500}\u{2500}\u{2500}\u{2500}", "  1 shell"]);
    assert_eq!(background_tasks(&md), Background::None, "a rule of the transcript, no input box under it");
    let codex = rows(&["\u{2500}\u{2500}\u{2500}\u{2500}", "\u{203a} Ask Codex to do anything", "\u{2500}\u{2500}\u{2500}\u{2500}", "  1 shell"]);
    assert_eq!(background_tasks(&codex), Background::None, "Codex's composer is no Claude Code input box");
    let typed = rows(&["\u{2500}\u{2500}\u{2500}\u{2500}", "\u{276f} first line", "  second line", "\u{2500}\u{2500}\u{2500}\u{2500}", "  ~/p ctx:47%", "  1 shell"]);
    assert_eq!(background_tasks(&typed), Background::Tasks, "a prompt of two lines in the box");
}

#[test]
fn agents_in_the_background_keep_it_working_however_long() {
    let box_with = |pill: &str| rows(&["\u{273b} Worked for 2m 12s \u{b7} 1 local agent still running", "", "\u{2500}\u{2500}", "\u{276f} ", "\u{2500}\u{2500}", "  ~/p", pill]);
    for pill in ["  \u{23f5}\u{23f5} bypass permissions on \u{b7} 1 shell \u{b7} 2 local agents", "  1 local agent", "  2 teams", "  1 background dynamic workflow", "  \u{25c7} 1 cloud session"] {
        assert_eq!(background_tasks(&box_with(pill)), Background::Agents, "{pill}");
        assert_eq!(read("\u{2733} Fix it", &box_with(pill)), (Working, false), "{pill}: no ten-minute limit");
    }
    for pill in ["  \u{23f5}\u{23f5} bypass permissions on \u{b7} 1 shell \u{b7} \u{2190} 1 agent", "  1 monitor", "  3 background tasks"] {
        assert_eq!(background_tasks(&box_with(pill)), Background::Tasks, "{pill}: a shell may never end; \u{2190} 1 agent is another session");
    }
    assert_eq!(read("\u{2733} Fix it", &box_with("  \u{2190} 1 agent")), (Done, false), "the turn line's agent ended; the agents view's count is no work of this turn");
    // a screen with no Claude Code input box (Codex): its last rows' words
    let codex = |last: &str| rows(&["\u{2022} Spawned 2 agents to review the parser", "", last, "", "\u{203a} Ask Codex to do anything", "  gpt-5.5 high \u{b7} 80% left"]);
    for said in ["  2 agents still running", "  1 subagent is running", "  3 background agents are still running", "  Waiting for 2 agents"] {
        assert_eq!(state_of("api", &codex(said)), Working, "{said}");
    }
    for said in ["  \u{2022} Both agents finished: 2 findings", "  ran 2 agents earlier", "  0 agents running", "  2 agents finished, none running"] {
        assert_eq!(state_of("api", &codex(said)), Done, "{said}");
    }
    let far = { let mut v = codex("  2 agents still running"); v.splice(3..3, (0..12).map(|i| format!("  line {i}"))); v };
    assert_eq!(state_of("api", &far), Done, "only the last ten rows count");
    let above_box = rows(&["  2 agents still running", "\u{2500}\u{2500}", "\u{276f} ", "\u{2500}\u{2500}", "  ~/p"]);
    assert_eq!(state_of("\u{2733} x", &above_box), Done, "Claude Code's transcript: its footer says, not its words");
}

#[test]
fn background_work_alone_counts_for_ten_minutes() {
    let shell = rows(&["\u{273b} Worked for 2m 12s", "", "\u{2500}\u{2500}", "\u{276f} ", "\u{2500}\u{2500}", "  ~/p", "  1 shell"]);
    let spinning = read("\u{25d0} Fix it", &shell);
    assert_eq!(read("\u{2733} Fix it", &shell), (Working, true), "the footer alone");
    assert_eq!(spinning, (Working, false), "the agent itself at work: no limit");
    let (mut s, t0) = (States::new(), Instant::now());
    assert_eq!(s.limited("k", (Working, true), t0), (Working, false));
    assert_eq!(s.limited("k", (Working, true), t0 + BACKGROUND_LIMIT / 2), (Working, false), "since it began");
    assert_eq!(s.limited("k", (Working, true), t0 + BACKGROUND_LIMIT), (Done, true), "a dev server: done after ten minutes, marked while it runs");
    assert_eq!(s.limited("k", (Done, false), t0 + BACKGROUND_LIMIT), (Done, false), "its shell ended: no mark");
    assert_eq!(s.limited("k", (Working, true), t0 + BACKGROUND_LIMIT * 2), (Working, false), "a shell again: it counts anew");
    assert_eq!(s.limited("k", spinning, t0 + BACKGROUND_LIMIT * 3), (Working, false), "a new turn");
    assert_eq!(s.limited("k", (Working, true), t0 + BACKGROUND_LIMIT * 3), (Working, false), "its background work counts anew");
    s.store("k", "", t0, 0, (Done, true), false);
    assert!(s.behind("k"), "the mark is kept between reads");
    s.store("k", "", t0, 0, (Done, false), false);
    assert!(!s.behind("k"));
}

/// A turn its hooks ended, done with a shell still running: its footer is read again every
/// STATE_EVERY while it is marked, and not once the mark has gone.
#[test]
fn a_hooked_turn_done_with_a_shell_is_read_again_while_marked() {
    let (mut s, t0) = (States::new(), Instant::now());
    assert_eq!(s.lingers("k", t0), None, "not one");
    s.linger("k", t0, true);
    assert_eq!(s.lingers("k", t0 + STATE_EVERY / 2), Some(false), "marked, not due");
    assert_eq!(s.lingers("k", t0 + STATE_EVERY), Some(true), "due");
    s.linger("k", t0 + STATE_EVERY, false);
    assert_eq!(s.lingers("k", t0 + STATE_EVERY * 2), None, "its shell ended");
    s.linger("tmux:local:terminum:%1", t0, true);
    s.linger("tmux:h:default:%1", t0, true);
    s.retain("tmux:local:terminum:", &[]);
    assert_eq!((s.lingers("tmux:local:terminum:%1", t0), s.lingers("tmux:h:default:%1", t0)), (None, Some(false)), "a closed pane is forgotten, another server's kept");
}

/// What a Stop's footer read says: (keeps the turn going, the mark of work behind its Done).
#[test]
fn a_stops_footer_keeps_the_turn_or_marks_it() {
    use Background::{Agents, Tasks};
    assert_eq!(after_stop(false, Some(Tasks)), (true, Some(false)), "a shell: working for a while");
    assert_eq!(after_stop(true, Some(Tasks)), (false, Some(true)), "ten minutes on: done, marked");
    assert_eq!(after_stop(true, Some(Agents)), (true, Some(false)), "agents: working however long");
    assert_eq!(after_stop(false, Some(Background::None)), (false, Some(false)), "nothing in the background");
    assert_eq!(after_stop(true, Some(Background::None)), (false, Some(false)), "its shell ended: the mark goes");
    assert_eq!(after_stop(true, None), (false, None), "not read: the mark as it was");
}

#[test]
fn status_line_shapes() {
    let s = |t: &str| status_line(&t.chars().collect::<Vec<_>>());
    assert!(s("  * Baking… (3s)"));
    assert!(s("out\n\u{b7}   Wait\u{2026}   ("));
    assert!(s("✶  …("), "the blanks may start the text");
    assert!(!s("✶ …("), "at least one character before …");
    assert!(!s("x ✻ Baking… ("), "only at a line's start");
    assert!(!s("✻Baking… ("), "a blank after the mark");
    assert!(!s("✻ Bak\ning… ("), "one line");
    assert!(s(&format!("✻ {}… (", "a".repeat(80))));
    assert!(!s(&format!("✻ {}… (", "a".repeat(81))));
    assert!(s("✻ Baking…\n  ("), "blanks after … may cross a line, as \\s does");
}

/// Codex 0.162's composer and footer under its transcript (measured).
fn codex(r: &[&str]) -> Vec<String> {
    let mut v = rows(r);
    v.extend(rows(&["", "› Ask Codex to do anything", "", "  GPT-6.1-Sol high · ~/p", "  ? for shortcuts"]));
    v
}

#[test]
fn codex_measured_screens() {
    let thread = "Write tmux control mode essay | p";
    let working = codex(&["• Ran pwd", "  └ /Users/alex/p", "", "Working (10s • esc to interrupt)"]);
    assert_eq!(state_of(thread, &working), Working, "Codex keeps its title while it works");
    assert_eq!(state_of("⠋ | p", &codex(&[])), Working, "a spinner before the thread is named");
    let done = codex(&["• Created essay.md and verified it contains 400 words.", "", "  Worked for 43s • 11:04"]);
    assert_eq!(state_of(thread, &done), Done);
    let run = rows(&["• Running curl -sI https://example.com", "", "  Would you like to run the following command?",
                     "  $ curl -sI https://example.com", "› 1. Yes, proceed (y)",
                     "  3. No, and tell Codex what to do differently (esc)", "", "  Press enter to confirm or esc to cancel"]);
    assert_eq!(state_of("[ ! ] Action Required | Check headers | p", &run), Waiting);
    let ask = rows(&["  Question 1/1 (1 unanswered)", "  Which language should the script use?", "  › 1. Python",
                     "    2. Bash", "", "  tab to add notes | enter to submit answer | esc to interrupt"]);
    assert_eq!(state_of("[ . ] Action Required | Plan | p", &ask), Waiting, "the title's blink, either frame");
    assert_eq!(state_of(thread, &ask), Waiting, "a question over \"esc to interrupt\", by its screen alone");
    assert_eq!(state_of("[ ! ] Action Required | p", &codex(&[])), Waiting, "the title alone");
}

#[test]
fn a_node_pane_is_codex_by_its_screen() {
    assert!(codex_screen(&codex(&[])), "the composer");
    assert!(codex_screen(&rows(&["  >_ OpenAI Codex (v0.162.1)", "› a draft I have not sent"])), "the header");
    assert!(codex_screen(&rows(&["  3. No, and tell Codex what to do differently (esc)"])), "an approval");
    assert!(codex_screen(&rows(&["  tab to add notes | enter to submit answer | esc to interrupt"])), "a question");
    assert!(!codex_screen(&rows(&["  ready - started server on 0.0.0.0:3000",
                                  "? Would you like to run the app on another port instead? › (Y/n)"])), "a dev server");
    assert!(!codex_screen(&rows(&["  VITE v5.0.0  ready in 300 ms", "  ➜  press h + enter to show help"])));
    assert!(!codex_screen(&[]));
}

#[test]
fn codex_failures_and_a_cancel() {
    let limit = codex(&["■ You\u{2019}ve hit your usage limit. Visit https://chatgpt.com/settings/usage to purchase more credits"]);
    assert_eq!(state_of("t | p", &limit), Failed);
    assert_eq!(state_of("t | p", &codex(&["■ exceeded retry limit, last status: 503"])), Failed);
    assert_eq!(state_of("t | p", &codex(&["■ Codex is currently experiencing high load."])), Failed);
    let cancel = codex(&["✗ You canceled the request to run curl -sI https://example.com",
                         "■ Conversation interrupted - use /feedback if something went wrong"]);
    assert_eq!(state_of("t | p", &cancel), Done, "a turn the user stopped is not a failure");
    let quoted = codex(&["› Why did it say \"usage limit reached\"?", "Working (3s • esc to interrupt)"]);
    assert_eq!(state_of("t | p", &quoted), Working, "an error's words in the user's own prompt, while it works");
}

#[test]
fn a_failure_that_ended_the_turn() {
    let err = with_footer(&["⎿  API Error: 529 {\"type\":\"overloaded_error\"}"]);
    assert_eq!(state_of("✳ x", &err), Failed);
    assert_eq!(state_of("◐ x", &err), Working, "still going after an error: not failed");
    assert_eq!(state_of("", &rows(&["Usage limit reached · resets 5pm"])), Failed);
}

#[test]
fn only_the_last_rows_count() {
    let mut old = rows(&["Do you want to proceed?"]);
    old.extend(rows(&["output"; 40]));
    assert_eq!(state_of("✳ x", &old), Done);
    let mut near = rows(&["Do you want to proceed?"]);
    near.extend(rows(&["output"; 14]));
    assert_eq!(state_of("✳ x", &near), Done, "a question 15 rows up is history");
    let mut err = rows(&["API Error"]);
    err.extend(rows(&["output"; 25]));
    assert_eq!(state_of("✳ x", &err), Done, "a failure 26 rows up is history");
    assert_eq!(state_of("", &[]), Done);
}

/// AC-132: an idle agent's screen costs a slow link nothing: it is read again when its window
/// printed, once more after that (a second holds several prints), and once in a while all the same.
#[test]
fn a_screen_that_printed_nothing_is_read_only_rarely() {
    let mut s = States::new();
    let t0 = Instant::now();
    let at = |secs| t0 + Duration::from_secs(secs);
    let every = STATE_EVERY.as_secs();
    s.store("k", "t", t0, 100, (Done, false), false);
    assert!(s.due("k", "t", at(every), 100), "first read: once more");
    s.store("k", "t", at(every), 100, (Done, false), false);
    assert!(!s.due("k", "t", at(2 * every), 100), "printed nothing");
    assert!(!s.due("k", "t", at(every + 1), 101), "printed, but STATE_EVERY has not passed");
    assert!(s.due("k", "new", at(every + 1), 100), "a new title at once");
    assert!(s.due("k", "t", at(2 * every), 101), "printed");
    s.store("k", "t", at(2 * every), 101, (Done, false), false);
    assert!(s.due("k", "t", at(3 * every), 101), "once more after it printed");
    s.store("k", "t", at(3 * every), 101, (Done, false), false);
    assert!(!s.due("k", "t", at(4 * every), 101));
    assert!(s.due("k", "t", at(3 * every) + STATE_QUIET, 101), "once in a while all the same");
}

#[test]
fn the_screen_is_read_again_only_on_a_new_title_or_after_a_while() {
    let mut s = States::new();
    let t0 = Instant::now();
    let mut reads = 0;
    // the pane prints all along (each step a new mark)
    for (n, (title, at)) in [("✳ x", 0), ("✳ x", 1), ("◐ x", 2), ("◐ x", 3), ("◐ x", 3 + STATE_EVERY.as_secs())].into_iter().enumerate() {
        let now = t0 + Duration::from_secs(at);
        let activity = n as u64;
        if s.due("tmux:local:terminum:%1", title, now, activity) {
            reads += 1;
            s.store("tmux:local:terminum:%1", title, now, activity, (Waiting, false), false);
        }
        assert_eq!(s.last("tmux:local:terminum:%1"), Some(Waiting));
    }
    assert_eq!(reads, 3);
    assert!(s.due("tmux:local:terminum:%2", "✳ x", t0, 0), "another pane");
    s.forget("tmux:local:terminum:%1");
    assert_eq!(s.last("tmux:local:terminum:%1"), None);
}

#[test]
fn the_cache_stays_small() {
    let mut s = States::new();
    let now = Instant::now();
    for n in 0..KEEP + 1 {
        s.store(&format!("k{n}"), "", now, 0, (Done, false), false);
    }
    assert!(s.panes.len() <= KEEP);
    assert_eq!(s.last(&format!("k{KEEP}")), Some(Done), "the newest is kept");
}

/// A question in a narrow pane (a window split with an agent team's panes): Claude Code breaks
/// its footer over rows ("Esc to" / "cancel"), which still says it waits.
#[test]
fn a_question_wrapped_in_a_narrow_pane_waits() {
    let r = rows(&["  4. Don't push", "     Commit and deploy", "     only.", "  5. Type something.", "──────────────────────",
                   "  6. Chat about this", "Enter to select · ↑/↓", "to navigate · Esc to", "cancel"]);
    assert_eq!(state_of("✳ A coding app", &r), Waiting);
}

#[test]
fn a_programs_own_error_line_says_failed_or_a_safety_stop() {
    for line in ["■ stream disconnected before completion: reset", "■ Selected model is at capacity. Try again.", "■ rate limit exceeded: wait",
                 "■ Goal budget reached - the turn was stopped.", "⎿  API Error: 500 server error"] {
        assert_eq!(read("t", &rows(&["› fix it", line, "› Ask Codex to do anything"])).0, State::Failed, "{line}");
    }
    for line in ["■ This request has been flagged for possible cybersecurity risk.", "⎿  API Error: Opus can't help with this. Start a new session to continue.",
                 "Chat stopped as a precaution", "■ This content can't be shown"] {
        assert_eq!(read("t", &rows(&["› fix it", line, "› Ask Codex to do anything"])).0, State::Waiting, "{line}");
    }
}

#[test]
fn the_same_words_in_the_transcript_or_before_a_later_turn_say_nothing() {
    let quoted = "The docs quote: This request has been flagged for possible cybersecurity risk.";
    assert_eq!(read("t", &rows(&[quoted, "Worked for 3m 2s", "› Ask Codex to do anything"])).0, State::Done);
    assert_eq!(read("t", &rows(&[quoted, "› Ask Codex to do anything"])).0, State::Done, "not a program's error line");
    assert_eq!(read("t", &rows(&["■ rate limit exceeded", "› again", "• done", "Worked for 12s • 2:32 PM", "› Ask Codex to do anything"])).0, State::Done, "a later turn went fine");
    assert_eq!(read("t", &rows(&["■ Conversation interrupted - use /feedback", "› Ask Codex to do anything"])).0, State::Done, "the user's own cancel");
}

#[test]
fn a_screen_says_when_agents_run() {
    assert!(agents_run(&rows(&["✻ Waiting for 2 background agents to finish"])));
    assert!(agents_run(&rows(&["• 2 agents still running", "› Ask Codex to do anything"])));
    assert!(!agents_run(&rows(&["✻ Worked for 3m 2s", "─────", "❯ ", "─────", "  ⏵⏵ bypass permissions on"])), "a ghost");
}

#[test]
fn claude_codes_list_of_agents_counts_all_but_main() {
    let screen = rows(&["❯ ", "  ~/nodejs-rasp Opus 5.5 (high)", "  ⏵⏵ bypass permissions on · 3 shells · ← 1 agent", "",
                        "  ● main", "  ○ rust-flakes      Fix flaky Rust viewer/tui tests", "  ○ node-flakes      Fix flaky Node timeout tests",
                        "  ○ feedscale-flake  Fix feed-scale retained-bytes flake", "", ""]);
    assert_eq!(listed_agents(&screen), 3);
    assert_eq!(listed_agents(&rows(&["● Reading the file", "○ a list of an agent's answer", ""])), 0, "no main: the transcript's");
    assert_eq!(listed_agents(&rows(&["● main", "text after it"])), 0, "not at the bottom");
    assert_eq!(listed_agents(&[]), 0);
}

#[test]
fn safeguards_retry_dialog_waits() {
    assert_eq!(read("t", &rows(&["Session paused", "Opus's safeguards flagged this message."])).0, State::Waiting);
}
