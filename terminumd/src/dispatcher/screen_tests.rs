// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Screens as measured on Claude Code 2.1.319 and Codex 0.166 (AS-125), with `capture-pane -e`.
use super::*;

fn rows(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

const RULE: &str = "\u{1b}[37m────────────────────────────────────────";

fn claude_box(inner: &str) -> Vec<String> {
    rows(&format!("  Claude Code\n\n{RULE}\n{inner}\n{RULE}\n  ⏵⏵ don't ask on (shift+tab to cycle)"))
}

#[test]
fn as125_claude_empty_is_its_cursor_cell_alone() {
    assert_eq!(read("claude", &claude_box("\u{1b}[39m❯\u{a0}\u{1b}[7m \u{1b}[0m")), Screen::Empty);
}

#[test]
fn as125_claude_its_suggestion_in_dim_is_no_one_s_words() {
    // as measured with no terminal focus: no cursor's cell, every word its own dim run
    let ghost = "\u{1b}[39m❯\u{a0}\u{1b}[2mTry\u{1b}[0m \u{1b}[2m\"how\u{1b}[0m \u{1b}[2mdoes\u{1b}[0m \u{1b}[2m<filepath>\u{1b}[0m \u{1b}[2mwork?\"\u{1b}[0m";
    assert_eq!(read("claude", &claude_box(ghost)), Screen::Empty);
    assert_eq!(read("claude", &claude_box("\u{1b}[39m❯\u{a0}")), Screen::Empty, "nothing at all");
}

#[test]
fn as125_claude_a_draft_is_not_empty() {
    assert_eq!(read("claude", &claude_box("\u{1b}[39m❯\u{a0}a draft not sent\u{1b}[7m \u{1b}[0m")), Screen::Holds("a draft not sent".into()));
    assert_eq!(read("claude", &claude_box("\u{1b}[39m❯\u{a0}/")), Screen::Holds("/".into()));
    assert_eq!(read("claude", &claude_box("\u{1b}[39m❯\u{a0}my draft\u{1b}[2m ghost\u{1b}[0m")), Screen::Holds("my draft".into()), "a draft beside a ghost");
}

#[test]
fn as125_a_draft_in_a_colour_of_its_own_is_a_draft() {
    assert_eq!(read("claude", &claude_box("\u{1b}[39m❯\u{a0}\u{1b}[38;2;200;100;2mmy draft\u{1b}[0m")), Screen::Holds("my draft".into()), "a 2 in a colour is no dim");
}

#[test]
fn as125_claude_a_multi_line_draft_is_held() {
    let screen = rows(&format!("{RULE}\n\u{1b}[39m❯\u{a0}pasted line one\n  pasted line two\n{RULE}"));
    assert_eq!(read("claude", &screen), Screen::Holds("pasted line one pasted line two".into()));
}

#[test]
fn as125_claude_its_trust_question_asks_and_never_reads_as_a_box() {
    let trust = rows(" Accessing workspace:\n\n /private/tmp\n\n ❯ No, exit\n   Yes, I trust this folder\n\n Enter to confirm · Esc to cancel");
    assert_eq!(read("claude", &trust), Screen::Asks);
    let numbered = rows(" Do you want to proceed?\n ❯ 1. Yes\n   2. No");
    assert_eq!(read("claude", &numbered), Screen::Asks);
}

#[test]
fn as125_codex_empty_is_its_placeholder_in_dim() {
    let screen = rows("\u{1b}[2m╭──────╮\u{1b}[0m\n\n\u{1b}[1m›\u{1b}[0m \u{1b}[2mAsk Codex to do anything\u{1b}[0m\n\n  ? for shortcuts");
    assert_eq!(read("codex", &screen), Screen::Empty);
}

#[test]
fn as125_codex_the_same_words_typed_are_a_draft() {
    let screen = rows("\u{1b}[1m›\u{1b}[0m Ask Codex to do anything\n\n  ? for shortcuts");
    assert_eq!(read("codex", &screen), Screen::Holds("Ask Codex to do anything".into()));
}

#[test]
fn as125_codex_its_update_dialog_asks() {
    let screen = rows("  Update available · 0.166.0 → 0.167.0\n\n› 1. Update now (runs `npm install -g @openai/codex`)\n  2. Skip\n  3. Skip until next version\n\n  enter continue · esc skip");
    assert_eq!(read("codex", &screen), Screen::Asks);
}

#[test]
fn as125_what_it_does_not_know_is_not_empty() {
    assert_eq!(read("claude", &rows("$ ls\nfile")), Screen::Unknown);
    assert_eq!(read("codex", &rows("$ ls\nfile")), Screen::Unknown);
    assert_eq!(read("gemini", &claude_box("❯ ")), Screen::Unknown);
    assert_eq!(read("claude", &rows("\u{1b}]8;;x\u{7}link")), Screen::Unknown, "an escape it does not know");
}

#[test]
fn as125_a_paste_shows_by_its_first_words_or_its_fold() {
    assert!(shows(&Screen::Holds("The human ran task T-12: Fix the retry".into()), "The human ran task T-12: Fix the retry loop\nmore"));
    assert!(shows(&Screen::Holds("[Pasted text #1 +40 lines]".into()), "anything long"));
    assert!(!shows(&Screen::Holds("another draft".into()), "The human ran task"));
    assert!(!shows(&Screen::Empty, "The human ran task"));
    assert!(!shows(&Screen::Asks, "The human ran task"));
}
