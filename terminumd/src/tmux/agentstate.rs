// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a coding agent in a pane is doing, for the list's status icon (AC-39): working,
//! waiting for the user, failed, or done. Claude Code puts a spinner in front of its terminal
//! title while it works and ✳ when it is idle (measured: ◐ ◑ ◒ ◓ turning, ✳ at rest); a
//! question or a failure shows only on its screen, so the last rows are read for those. Codex
//! (measured on 0.162) keeps its title while it works and marks it only when it asks.
//! Ported from iterm-enhancer's `agentstate.py`, its regular expressions as small matchers.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Rows of the screen's end that count.
pub const ROWS: usize = 25;
/// A question is near the bottom: older text above it is history.
pub const ASK_ROWS: usize = 12;
/// How long a state stands while the title stays the same.
pub const STATE_EVERY: Duration = Duration::from_secs(6);
/// How long work in the background alone (a dev server, `tail -f`, a monitor: Claude Code's
/// footer counts it) keeps a session working; past it the session is done all the same. Its
/// agents (subagents, teams, workflows) keep it working while they run, however long.
pub const BACKGROUND_LIMIT: Duration = Duration::from_secs(600);
/// Panes remembered before the cache starts anew (panes long closed).
const KEEP: usize = 500;

const WAITING: [&str; 10] = ["do you want to", "would you like to", "esc to cancel", "(y/n)", "[y/n]",
                             "allow command", "allow edit", "allow this", "press enter to",
                             "enter to submit answer"];
const FAILED: [&str; 10] = ["api error", "overloaded_error", "request timed out",
                            "credit balance is too low", "usage limit reached", "claude code crashed",
                            "hit your usage limit", "out of credits", "exceeded retry limit",
                            "experiencing high load"];
/// Codex's own words on its screen (its header, composer, approval and question): a pane whose
/// program is `node` (Codex installed with npm) runs Codex while its screen shows one.
const CODEX_WORDS: [&str; 5] = ["openai codex (v", "ask codex to do anything", "tell codex what to do differently",
                                "enter to submit answer", "tab to add notes"];
/// What Claude Code's footer counts of the work it runs in the background, after the number
/// ("1 shell, 1 monitor", "2 local agents"; measured in its code): its session is not done while
/// one runs. An Artifact comment monitor, a dream or a memory import is no work of a turn.
/// Rows of its footer: the status line, the mode and the counts, a notice.
const FOOTER: usize = 4;
const BACKGROUND: [&str; 9] = ["shell", "monitor", "local agent", "team", "background dynamic workflow",
                               "remote dynamic workflow", "cloud session", "mcp task", "background task"];
/// Those of BACKGROUND that are agents at work: they end on their own. ("← 1 agent" is Claude Code's
/// count of other background sessions waiting on the user in its agents view: not this turn's.)
const AGENTS: [&str; 5] = ["local agent", "team", "background dynamic workflow", "remote dynamic workflow", "cloud session"];
/// Rows of a screen without Claude Code's input box (Codex) read for agents still running.
const AGENT_ROWS: usize = 10;

/// What Claude Code's footer counts in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    None,
    /// shells, monitors, tasks: they may never end (a dev server)
    Tasks,
    /// subagents, teams, workflows: the session's work goes on until they end
    Agents,
}
/// Codex's title while it waits for the user, blinking: "[ ! ] Action Required | <thread> | <folder>".
pub const CODEX_ASKS: [&str; 2] = ["[ ! ] Action Required", "[ . ] Action Required"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Working,
    Waiting,
    Failed,
    Done,
}

impl State {
    /// The layout item's `state`.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Working => "working",
            State::Waiting => "waiting",
            State::Failed => "failed",
            State::Done => "done",
        }
    }
}

/// A spinner frame: ◐◑◒◓, ✢✶✻✽, ·, braille.
fn spinner(c: char) -> bool {
    matches!(c, '\u{25d0}'..='\u{25d3}' | '\u{2722}' | '\u{2736}' | '\u{273b}' | '\u{273d}' | '\u{b7}'
             | '\u{2800}'..='\u{28ff}')
}

fn spinning(title: &str) -> bool {
    title.chars().next().is_some_and(spinner)
}

/// Claude Code's own marks in front of a title (a spinner or ✳).
pub fn claude_marked(title: &str) -> bool {
    spinning(title) || title.starts_with('\u{2733}')
}

/// Whether Codex's title says it waits for the user.
pub fn codex_asks(title: &str) -> bool {
    CODEX_ASKS.iter().any(|m| title.starts_with(m))
}

/// Whether a screen is Codex's (for a `node` pane); the title is not asked, as a killed Codex
/// can leave its mark behind for the next program.
pub fn codex_screen(rows: &[String]) -> bool {
    any_of(&rows.join("\n"), &CODEX_WORDS)
}

fn any_of(text: &str, words: &[&str]) -> bool {
    let low = text.to_lowercase();
    words.iter().any(|w| low.contains(w))
}

/// Claude Code's status line while it works, "✻ Combobulating… (6m 8s · ↓ 31.5k tokens)": at a
/// line's start, blanks, a mark (✢…✽, ·, *), blanks, 1 to 80 characters of one line, "…",
/// blanks, "(" (the regular expression of `agentstate.py`, matched by hand).
fn status_line(tail: &[char]) -> bool {
    let ws = |c: char| c.is_whitespace();
    let mark = |c: char| matches!(c, '\u{2722}'..='\u{273d}' | '\u{b7}' | '*');
    (0..tail.len()).filter(|&p| p == 0 || tail[p - 1] == '\n').any(|p| {
        let mut i = p;
        while i < tail.len() && ws(tail[i]) {
            i += 1;
        }
        if !tail.get(i).copied().is_some_and(mark) || !tail.get(i + 1).copied().is_some_and(ws) {
            return false;
        }
        // the 1..80 characters may begin anywhere in the blanks after the first one
        let mut start = i + 2;
        loop {
            if text_then_paren(tail, start) {
                return true;
            }
            if start < tail.len() && ws(tail[start]) {
                start += 1;
            } else {
                return false;
            }
        }
    })
}

/// Whether `tail[start..]` is 1 to 80 characters of one line, "…", blanks, then "(".
fn text_then_paren(tail: &[char], start: usize) -> bool {
    (start + 1..=start + 80).take_while(|&k| k <= tail.len() && tail[k - 1] != '\n').any(|k| {
        if tail.get(k) != Some(&'\u{2026}') {
            return false;
        }
        let mut j = k + 1;
        while j < tail.len() && tail[j].is_whitespace() {
            j += 1;
        }
        tail.get(j) == Some(&'(')
    })
}

/// Whether `row` is a turn's closing line: Claude Code's "✻ Worked for 2m 12s · …" (a mark, a
/// verb, " for ", a duration) or Codex's "  Worked for 43s • 11:04" (no mark).
pub fn turn_line(row: &str) -> bool {
    let t = row.trim_start();
    let t = t.strip_prefix(|c: char| matches!(c, '\u{2722}'..='\u{273d}' | '\u{b7}' | '*')).map_or(t, |r| r.strip_prefix(' ').unwrap_or(r));
    let rest = t.to_lowercase();
    rest.split_once(" for ").is_some_and(|(verb, after)| !verb.is_empty() && !verb.contains(' ') && after.starts_with(|c: char| c.is_ascii_digit()))
}

/// Whether Claude Code's newest turn line says its subagents still run: "✻ Waiting for 2
/// background agents to finish" (also "1 background agent and 1 dynamic workflow", "dynamic
/// workflows"; measured in its code), not a later "✻ Worked for 3m 2s" (a turn after they ended).
/// Its turn goes on until they end.
fn background_work(tail: &str) -> bool {
    let mark = |c: char| matches!(c, '\u{2722}'..='\u{273d}' | '\u{b7}' | '*');
    let turn_line = |l: &str| -> Option<bool> {
        let mut c = l.trim_start().chars();
        if !c.next().is_some_and(mark) || c.next() != Some(' ') {
            return None;
        }
        let rest = c.as_str().to_lowercase();
        if rest.starts_with("waiting for") && rest.contains("to finish") && (rest.contains(" background agent") || rest.contains(" dynamic workflow")) {
            return Some(true);
        }
        // "Worked for 2m 12s", "Cooked for 9s": its verb, " for ", a duration
        let (verb, after) = rest.split_once(" for ")?;
        (!verb.contains(' ') && after.starts_with(|c: char| c.is_ascii_digit())).then_some(false)
    };
    tail.lines().rev().find_map(turn_line).unwrap_or(false)
}

/// Whether Claude Code's footer counts work in the background: the rows (at most FOOTER) under
/// its input box (a rule, "❯ " and what is typed, a rule), "⏵⏵ bypass permissions on · 1 shell,
/// 1 monitor · ← 1 agent": each item exactly a number and a word of BACKGROUND. It is live; a turn
/// line's "· 1 shell still running" stays when the shell has ended.
pub fn background_tasks(rows: &[String]) -> Background {
    let Some(bottom) = claude_box(rows) else { return Background::None };
    let mut found = Background::None;
    for r in rows[bottom + 1..].iter().take(FOOTER) {
        for item in r.split(" \u{b7} ").flat_map(|part| part.split(", ")) {
            let item = item.trim().trim_start_matches(|c: char| !c.is_alphanumeric()).to_lowercase();   // a mark before ("◇ 1 cloud session")
            let Some((n, what)) = item.split_once(' ') else { continue };
            let what = what.trim_end();
            let is = |w: &&str| what == *w || what.strip_suffix('s') == Some(*w);
            if !n.parse::<u32>().is_ok_and(|n| n > 0) {
                continue;
            }
            if AGENTS.iter().any(is) {
                return Background::Agents;
            }
            if BACKGROUND.iter().any(is) {
                found = Background::Tasks;
            }
        }
    }
    found
}

/// Whether a screen's last rows say agents still run: "2 agents still running", "1 subagent is
/// running", "3 background agents are still running", "waiting for 2 agents" (a coding agent with
/// no Claude Code input box: Codex). A number, at most two words to "agent" or "subagent", then
/// running ("is", "are", "still" between).
fn agents_said(rows: &[String]) -> bool {
    rows[rows.len().saturating_sub(AGENT_ROWS)..].iter().any(|row| {
        let words: Vec<String> = row.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect();
        words.iter().enumerate().any(|(i, w)| {
            if !w.parse::<u32>().is_ok_and(|n| n > 0) {
                return false;
            }
            let rest = &words[i + 1..];
            let Some(a) = rest.iter().take(3).position(|x| matches!(x.as_str(), "agent" | "agents" | "subagent" | "subagents")) else { return false };
            let waiting = i >= 2 && words[i - 2] == "waiting" && words[i - 1] == "for";
            // then only "is", "are", "still" before "running"
            waiting || rest[a + 1..].iter().find(|x| !matches!(x.as_str(), "is" | "are" | "still")).is_some_and(|x| x == "running")
        })
    })
}

/// The bottom rule of Claude Code's input box (a rule, "❯ " and what is typed, a rule), when it is
/// on the screen: a question of its replaces the box, so words above it are the transcript.
/// (Codex's composer: "›", which its menus start with too.)
fn claude_box(rows: &[String]) -> Option<usize> {
    let rule = |r: &String| r.trim_start().starts_with("\u{2500}\u{2500}");
    let bottom = rows.iter().rposition(rule)?;
    let top = rows[..bottom].iter().rposition(rule)?;
    rows[top + 1].trim_start().starts_with('\u{276f}').then_some(bottom)
}

/// After a Stop whose turn waits, what its footer says (`read`; None: it could not be read):
/// whether that keeps the turn going (agents; shells, monitors and tasks until `late`,
/// BACKGROUND_LIMIT on), and whether work runs behind its Done (the mark; None: as it was).
pub fn after_stop(late: bool, read: Option<Background>) -> (bool, Option<bool>) {
    match read {
        Some(Background::Agents) => (true, Some(false)),
        Some(Background::Tasks) => (!late, Some(late)),
        Some(Background::None) => (false, Some(false)),
        None => (false, None),
    }
}

/// The state from the raw title and the screen's last rows.
#[cfg(test)]
pub fn state_of(title: &str, rows: &[String]) -> State {
    read(title, rows).0
}

/// The state, and whether only the footer's work in the background makes it Working.
pub fn read(title: &str, rows: &[String]) -> (State, bool) {
    let rows: Vec<String> = rows.iter().map(|r| r.replace('\0', " ")).collect();   // skipped cells
    let tail = rows[rows.len().saturating_sub(ROWS)..].join("\n");
    // a question near the bottom; with Claude Code's input box there, only under it (its footer)
    let ask_from = rows.len().saturating_sub(ASK_ROWS).max(claude_box(&rows).map_or(0, |b| b + 1));
    // its words as one line: a narrow pane breaks them over rows ("Esc to" / "cancel")
    let asked = rows[ask_from..].join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    if codex_asks(title) || any_of(&asked, &WAITING) {
        return (State::Waiting, false);
    }
    let chars: Vec<char> = tail.chars().collect();
    let last = &rows[rows.len().saturating_sub(ROWS)..];
    let footer = background_tasks(last);
    // agents in the background keep it working however long they run (Claude Code's footer; on
    // a screen without its input box, the last rows' words); shells and monitors for a while
    let agents = footer == Background::Agents || (claude_box(last).is_none() && agents_said(last));
    let working = spinning(title) || any_of(&tail, &["esc to interrupt"]) || status_line(&chars) || background_work(&tail) || agents;
    let background = !working && footer == Background::Tasks;
    if !working && !background && any_of(&tail, &FAILED) {
        return (State::Failed, false);
    }
    (if working || background { State::Working } else { State::Done }, background)
}

/// The last state per pane key, read again only when the title changed (Claude Code turns its
/// spinner while it works) or [`STATE_EVERY`] passed: the list asks every 2 s for every agent.
#[derive(Debug, Default)]
pub struct States {
    panes: HashMap<String, Pane>,
}

/// What is known of one pane.
#[derive(Debug, Default)]
struct Pane {
    /// its screen as last read
    read: Option<Read>,
    /// since when only work in the background kept it working
    background: Option<Instant>,
    /// a turn its hooks ended, done, its work in the background still running: when its footer was read
    lingering: Option<Instant>,
}

#[derive(Debug)]
struct Read {
    title: String,
    at: Instant,
    state: State,
    /// a `node` pane's screen was Codex's
    codex: bool,
    /// done with work in the background still running (past BACKGROUND_LIMIT)
    behind: bool,
}

impl States {
    pub fn new() -> Self {
        Self::default()
    }

    fn read(&self, key: &str) -> Option<&Read> {
        self.panes.get(key)?.read.as_ref()
    }

    /// Whether pane `key` with `title` needs its screen read at `now`.
    pub fn due(&self, key: &str, title: &str, now: Instant) -> bool {
        self.read(key).is_none_or(|r| r.title != title || now.saturating_duration_since(r.at) >= STATE_EVERY)
    }

    /// The state last stored for `key`.
    pub fn last(&self, key: &str) -> Option<State> {
        self.read(key).map(|r| r.state)
    }

    /// Whether the screen last read for `key` was Codex's.
    pub fn codex(&self, key: &str) -> bool {
        self.read(key).is_some_and(|r| r.codex)
    }

    /// Whether the screen last read for `key` was done with work in the background still running.
    pub fn behind(&self, key: &str) -> bool {
        self.read(key).is_some_and(|r| r.behind)
    }

    /// The state read for `key` at `now` (`background`: Working only through the footer's work
    /// in the background): Done once that has gone on for BACKGROUND_LIMIT, and then whether
    /// that work still runs (its mark).
    pub fn limited(&mut self, key: &str, (state, background): (State, bool), now: Instant) -> (State, bool) {
        if !background {
            if let Some(p) = self.panes.get_mut(key) {
                p.background = None;
            }
            return (state, false);
        }
        let since = *self.panes.entry(key.to_string()).or_default().background.get_or_insert(now);
        if now.saturating_duration_since(since) >= BACKGROUND_LIMIT { (State::Done, true) } else { (state, false) }
    }

    /// Remembers `state`, read at `now` for `title`, whether the screen was Codex's, and whether
    /// work in the background ran behind a Done.
    pub fn store(&mut self, key: &str, title: &str, now: Instant, (state, behind): (State, bool), codex: bool) {
        if self.panes.len() >= KEEP && !self.panes.contains_key(key) {
            self.panes.clear();
        }
        self.panes.entry(key.to_string()).or_default().read = Some(Read { title: title.to_string(), at: now, state, codex, behind });
    }

    /// A turn its hooks ended, done with work in the background still running: Some(whether its
    /// footer is due to be read again at `now`), None when it is not one.
    pub fn lingers(&self, key: &str, now: Instant) -> Option<bool> {
        self.panes.get(key)?.lingering.map(|at| now.saturating_duration_since(at) >= STATE_EVERY)
    }

    /// Forgets the panes of the server keyed by `prefix` that are not `live` (closed).
    pub fn retain(&mut self, prefix: &str, live: &[String]) {
        let live: std::collections::HashSet<&str> = live.iter().map(String::as_str).collect();
        self.panes.retain(|k, _| !k.starts_with(prefix) || live.contains(k.as_str()));
    }

    /// Says whether `key`'s footer, read at `now`, still counts work in the background behind its Done.
    pub fn linger(&mut self, key: &str, now: Instant, on: bool) {
        if on {
            self.panes.entry(key.to_string()).or_default().lingering = Some(now);
        } else if let Some(p) = self.panes.get_mut(key) {
            p.lingering = None;
        }
    }

    /// Forgets a pane's screen (it is read anew).
    #[cfg(test)]
    pub fn forget(&mut self, key: &str) {
        if let Some(p) = self.panes.get_mut(key) {
            p.read = None;
        }
    }
}

#[cfg(test)]
#[path = "agentstate_tests.rs"]
mod tests;
