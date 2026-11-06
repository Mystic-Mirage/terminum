// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A pane's title and the line below it in the session list (AC-01, AC-39), from one row of
//! `list-panes` (spec §9 "The control client", Sessions). A window the user named
//! (automatic-rename off) shows that name; one tmux names after its program shows the pane's
//! title, else its program; coding agents' status marks are stripped. A pane title that is
//! only the host's name (tmux's default) or a program's name says nothing and is ignored.
//! Ported from iterm-enhancer's `layout.py` (`bare`, `STATUS`, `AUTO_NAME`, `PROGRAM_NAMES`,
//! `agent_of`, `tilde`) for tmux panes.

use super::agentstate::{claude_marked, CODEX_ASKS};

/// Coding agents the list marks with their own icon.
pub const AGENTS: [&str; 2] = ["claude", "codex"];
/// Shells: a pane at one's prompt may move its cursor on a tap (AC-39 "shell").
pub const SHELLS: [&str; 10] = ["bash", "zsh", "sh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "xonsh"];
/// Programs whose name as a title says nothing the program column does not.
const OTHER_PROGRAMS: [&str; 6] = ["ssh", "mosh", "tmux", "screen", "sudo", "login"];

/// One pane's fields of `list-panes -a -F` (spec §9 Sessions, with `#{host}` last).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneFields {
    /// when its window last printed, in tmux's seconds: compared only with itself
    pub window_activity: u64,
    pub session: String,
    pub window_index: u32,
    pub window_id: String,
    pub window_name: String,
    pub automatic_rename: bool,
    pub pane_id: String,
    pub pane_index: u32,
    pub pane_title: String,
    pub command: String,
    pub path: String,
    pub alternate_on: bool,
    pub pane_active: bool,
    /// tmux's `#{host}`: the title tmux gives a pane by default, which means nothing.
    pub host_name: String,
    /// a quick shell's window: the pane it belongs to (`@tm-quick`, AC-88), and its side
    pub quick_for: String,
    pub quick_side: String,
    /// the session's id (`$N`): a host's session client attaches by it (AC-89)
    pub session_id: String,
    /// the title a restore gave the pane (`@tm-title`: its coding agent's, before tmux
    /// restarted, AC-15), shown while a shell runs there
    pub restored_title: String,
}

/// What the list shows for a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    /// The program, the folder (`~` for the home folder), then the other titles.
    pub sub: Vec<String>,
    /// The pane's place under its window's first (1..n), 0 in a window of one pane.
    pub pane: usize,
    pub agent: Option<&'static str>,
    /// A shell at its prompt.
    pub shell: bool,
}

/// The `list-panes -F` format [`parse_list_panes`] reads, fields separated by tabs.
pub const LIST_PANES_FORMAT: &str = "#{session_name}\t#{window_index}\t#{window_id}\t#{window_name}\t\
#{automatic-rename}\t#{pane_id}\t#{pane_index}\t#{pane_title}\t#{pane_current_command}\t\
#{pane_current_path}\t#{alternate_on}\t#{pane_active}\t#{host}\t#{@tm-quick}\t#{@tm-quick-side}\t#{session_id}\t#{@tm-title}\t#{window_activity}";

/// Whether `s` is a name tmux could have given a window after its command (`claude`, `zsh`).
fn auto_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || ".+@-".contains(c))
}

/// One row of `list-panes -F LIST_PANES_FORMAT`; `None` when it has not its 17 fields (16 without
/// the restored title, as tests write them; more: a name holding a tab) or its numbers are not numbers. An empty `#{automatic-rename}` (a tmux that
/// does not say) counts as on when the window's name looks like a command.
pub fn parse_list_panes(line: &str) -> Option<PaneFields> {
    let f: Vec<&str> = line.trim_end_matches(['\r', '\n']).split('\t').collect();
    if !(16..=18).contains(&f.len()) {
        return None;
    }
    let flag = |s: &str| s == "1";
    Some(PaneFields {
        session: f[0].into(),
        window_index: f[1].parse().ok()?,
        window_id: f[2].into(),
        window_name: f[3].into(),
        automatic_rename: if f[4].is_empty() { auto_name(f[3]) } else { flag(f[4]) },
        pane_id: f[5].into(),
        pane_index: f[6].parse().ok()?,
        pane_title: f[7].into(),
        command: f[8].into(),
        path: f[9].into(),
        alternate_on: flag(f[10]),
        pane_active: flag(f[11]),
        host_name: f[12].into(),
        quick_for: f[13].into(),
        quick_side: f[14].into(),
        session_id: f[15].into(),
        restored_title: f.get(16).copied().unwrap_or_default().into(),
        window_activity: f.get(17).and_then(|a| a.parse().ok()).unwrap_or(0),
    })
}

/// Whether a status mark (Claude Code's ✳ ✻ ✶ ◐ ⏺, braille spinners) or a blank in front.
fn status_mark(c: char) -> bool {
    matches!(c, '\u{2800}'..='\u{28ff}' | '\u{25c9}'..='\u{25d3}' | '\u{2722}'..='\u{273d}' | '\u{23f3}'
             | '\u{23fa}' | '\u{231b}' | '\u{b7}' | '*' | '\u{fe0e}' | '\u{fe0f}') || c.is_whitespace()
}

/// A title without the status marks in front of it (Codex's "[ ! ] Action Required | " and the
/// "| " after its spinner too, so the row does not blink); as it is when nothing else is left.
pub fn bare(title: &str) -> String {
    let rest = title.trim_start_matches(status_mark);
    let rest = CODEX_ASKS.iter().find_map(|m| rest.strip_prefix(m)).unwrap_or(rest);
    match rest.trim_start_matches(|c| c == '|' || status_mark(c)) {
        "" => title.to_string(),
        rest => rest.to_string(),
    }
}

/// Whether `s` is only a program's name (a shell, an agent, ssh…), whatever its case.
pub fn program_name(s: &str) -> bool {
    let low = s.to_lowercase();
    SHELLS.contains(&low.as_str()) || AGENTS.contains(&low.as_str()) || OTHER_PROGRAMS.contains(&low.as_str())
}

/// "claude" or "codex" when the pane's program is one. Claude Code's own process is named by
/// its version ("2.1.302": its installer runs `~/.local/share/claude/versions/2.1.302`), so a
/// program named only so is Claude Code, its title marked or not.
pub fn agent_of(command: &str) -> Option<&'static str> {
    let name = command.trim().to_lowercase();
    if is_version(&name) {
        return Some("claude");
    }
    AGENTS.iter().copied().find(|a| name == *a)
}

/// "1.2.3": three numbers and two dots, nothing else.
pub fn is_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// `path` with the home folder `home` written `~`; without a home known, any
/// `/Users/<name>` or `/home/<name>` (a remote host's).
pub fn tilde(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if !home.is_empty() {
        return match path.strip_prefix(home) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_string(),
        };
    }
    for top in ["/Users/", "/home/"] {
        if let Some(rest) = path.strip_prefix(top) {
            let user_end = rest.find('/').unwrap_or(rest.len());
            if user_end > 0 {
                return format!("~{}", &rest[user_end..]);
            }
        }
    }
    path.to_string()
}

/// Whether a pane title says nothing: empty, "tmux", the window's name, the host's name tmux
/// gives a pane by default (full or short), or a program's name.
fn empty_title(p: &PaneFields) -> bool {
    let t = p.pane_title.as_str();
    let host = p.host_name.as_str();
    t.is_empty() || t == "tmux" || t == p.window_name || (!host.is_empty() && t == host)
        || (!host.is_empty() && host.split('.').next() == Some(t)) || program_name(bare(t).as_str())
}

/// The list's item for pane `p`, the `pane_no`-th (1-based) of `panes_in_window`; a pane after
/// the first goes under it with its own title. `home` is the home folder of the pane's host.
pub fn item(p: &PaneFields, pane_no: usize, panes_in_window: usize, home: &str) -> Item {
    // Claude Code's process (and the window tmux names after it) is named by its version: "claude"
    let named = |s: &str| if is_version(s) { "claude".to_string() } else { s.to_string() };
    let mut job = named(p.command.trim());
    let window_name = if p.automatic_rename { named(&p.window_name) } else { p.window_name.clone() };
    let window_name = window_name.as_str();
    let path = tilde(&p.path, home);
    let program_title = if SHELLS.contains(&job.as_str()) && !p.restored_title.is_empty() {
        p.restored_title.clone()                  // its coding agent's before tmux restarted (AC-15)
    } else if empty_title(p) {
        String::new()
    } else {
        p.pane_title.clone()
    };
    let user_named = !window_name.is_empty() && !p.automatic_rename;
    let mut title = if user_named {
        window_name.to_string()
    } else if !program_title.is_empty() {
        program_title.clone()
    } else if !window_name.is_empty() {
        window_name.to_string()
    } else {
        "tmux window".to_string()
    };
    if job.is_empty() && !user_named {
        job = window_name.to_string();            // tmux names a window after its program
    }
    if pane_no > 1 {
        title = [&program_title, &job, &path].into_iter().find(|t| !t.is_empty()).cloned()
            .unwrap_or_else(|| "Shell".to_string());
    }
    let mut sub = Vec::new();
    if !job.is_empty() && job != title {
        sub.push(job.clone());
    }
    if !path.is_empty() {
        sub.push(path);
    }
    if !program_title.is_empty() && program_title != title && !sub.contains(&program_title) {
        sub.push(program_title);
    }
    let agent = agent_of(&job)
        .or_else(|| AGENTS.iter().copied().find(|a| *a == window_name))
        .or_else(|| claude_marked(&p.pane_title).then_some("claude"));
    Item {
        title: bare(&title),
        sub: sub.iter().map(|s| bare(s)).collect(),
        pane: if panes_in_window > 1 { pane_no } else { 0 },
        agent,
        shell: SHELLS.contains(&job.as_str()),
    }
}

#[cfg(test)]
#[path = "titles_tests.rs"]
mod tests;
