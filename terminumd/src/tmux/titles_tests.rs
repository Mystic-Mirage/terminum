// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Tests of titles.rs (AC-01, AC-39): what the session list shows for a pane.

use super::*;

const HOME: &str = "/Users/alex";

fn pane(window_name: &str, auto: bool, title: &str, command: &str, path: &str) -> PaneFields {
    PaneFields {
        session: "main".into(),
        window_index: 1,
        window_id: "@1".into(),
        window_name: window_name.into(),
        automatic_rename: auto,
        pane_id: "%3".into(),
        pane_index: 0,
        pane_title: title.into(),
        command: command.into(),
        path: path.into(),
        alternate_on: false,
        pane_active: true,
        host_name: "mac.example".into(),
        ..Default::default()
    }
}

fn shown(i: &Item) -> (String, Vec<String>) {
    (i.title.clone(), i.sub.clone())
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn ac01_a_named_window_and_its_second_pane() {
    let first = pane("api", false, "mac.example", "claude", "/Users/alex/work/api");
    let second = PaneFields { pane_id: "%4".into(), pane_index: 1, command: "zsh".into(), ..first.clone() };
    let a = item(&first, 1, 2, HOME);
    let b = item(&second, 2, 2, HOME);
    assert_eq!(shown(&a), ("api".into(), strs(&["claude", "~/work/api"])));
    assert_eq!((a.pane, a.agent, a.shell), (1, Some("claude"), false));
    assert_eq!(shown(&b), ("zsh".into(), strs(&["~/work/api"])), "\"zsh · ~/work/api\"");
    assert_eq!((b.pane, b.agent, b.shell), (2, None, true));
}

#[test]
fn ac01_a_subagents_pane_shows_its_own_name_and_keeps_its_type() {
    let lead = pane("2.1.319", true, "\u{2733} Fix the list", "2.1.319", "/Users/alex/p");
    let sub = PaneFields { pane_id: "%4".into(), pane_index: 1, pane_title: "\u{2733} general-purpose".into(),
                           agent_name: "simplify-altitude".into(), ..lead.clone() };
    let i = item(&sub, 2, 2, HOME);
    assert_eq!(i.title, "simplify-altitude");
    assert_eq!(i.agent, Some("claude"));
    assert!(i.sub.contains(&"general-purpose".to_string()), "{:?}", i.sub);
    // alone in a window tmux named after it, too
    assert_eq!(item(&sub, 1, 1, HOME).title, "simplify-altitude");
    // a window the user named keeps that name
    let named = PaneFields { window_name: "review".into(), automatic_rename: false, ..sub.clone() };
    assert_eq!(item(&named, 1, 1, HOME).title, "review");
    // another program started with the same flag is not a subagent
    let other = PaneFields { window_name: "dev".into(), command: "zsh".into(), pane_title: "shell".into(), ..sub.clone() };
    assert_eq!(item(&other, 2, 2, HOME).title, "shell");
}

#[test]
fn ac01_only_a_name_is_taken_from_the_start_command() {
    let row = |name: &str| format!("main\t1\t@1\tw\t1\t%3\t0\tt\tclaude\t/p\t0\t1\th\t\t\t$1\t\t{name}");
    assert_eq!(parse_list_panes(&row("simplify-reuse")).unwrap().agent_name, "simplify-reuse");
    assert_eq!(parse_list_panes(&row("")).unwrap().agent_name, "");
    for bad in ["a=b", "$(id)", "a/b", &"x".repeat(65)] {
        assert_eq!(parse_list_panes(&row(bad)).unwrap().agent_name, "", "{bad}");
    }
}

#[test]
fn a_window_tmux_names_shows_the_panes_title_without_marks() {
    let p = pane("claude", true, "\u{2733} Fix the list", "claude", "/Users/alex/p");
    let i = item(&p, 1, 1, HOME);
    assert_eq!(shown(&i), ("Fix the list".into(), strs(&["claude", "~/p"])));
    assert_eq!((i.pane, i.agent), (0, Some("claude")));
    let spinning = pane("claude", true, "\u{25d0} Fix the list", "claude", "/Users/alex/p");
    assert_eq!(item(&spinning, 1, 1, HOME).title, "Fix the list");
}

#[test]
fn else_its_program() {
    let i = item(&pane("zsh", true, "mac.example", "zsh", "/Users/alex"), 1, 1, HOME);
    assert_eq!(shown(&i), ("zsh".into(), strs(&["~"])));
    assert!(i.shell);
    let empty = item(&pane("", true, "", "", ""), 1, 1, HOME);
    assert_eq!(shown(&empty), ("tmux window".into(), vec![]));
}

#[test]
fn titles_that_say_nothing_are_ignored() {
    for t in ["mac.example", "mac", "ssh", "zsh", "tmux", "Claude", "\u{2733} claude", ""] {
        let i = item(&pane("vim", true, t, "vim", "/Users/alex/p"), 1, 1, HOME);
        assert_eq!(shown(&i), ("vim".into(), strs(&["~/p"])), "{t:?}");
    }
}

#[test]
fn a_name_the_user_gave_stays_the_title() {
    // the live name and the pane's own title after the program and the folder
    let named = item(&pane("api", false, "Fix the list", "claude", "/home/alex/p"), 1, 1, "/home/alex");
    assert_eq!(shown(&named), ("api".into(), strs(&["claude", "~/p", "Fix the list"])));
    let auto = item(&pane("claude", true, "Fix the list", "claude", "/home/alex/p"), 1, 1, "/home/alex");
    assert_eq!(shown(&auto), ("Fix the list".into(), strs(&["claude", "~/p"])));
    // a pane titled "ssh" from before, now running claude in a window the user named
    let stale = item(&pane("Build the databases", false, "ssh", "claude", "/home/alex/p"), 1, 1, "/home/alex");
    assert_eq!(shown(&stale), ("Build the databases".into(), strs(&["claude", "~/p"])));
    // the window's name repeated as the pane's title
    let same = pane("Demo stand preparation and fixes", false, "Demo stand preparation and fixes", "claude", "/home/alex/p");
    let i = item(&same, 1, 1, "/home/alex");
    assert_eq!(shown(&i), ("Demo stand preparation and fixes".into(), strs(&["claude", "~/p"])));
    assert_eq!(i.agent, Some("claude"));
}

#[test]
fn a_windows_other_panes_are_named_for_themselves() {
    let sub = item(&pane("Orange notes", false, "pragmatic", "node", "/Users/alex"), 2, 6, HOME);
    assert_eq!((shown(&sub), sub.pane), (("pragmatic".into(), strs(&["node", "~"])), 2));
    let shell = item(&pane("Orange notes", false, "mac.example", "bash", "/Users/alex"), 6, 6, HOME);
    assert_eq!(shown(&shell), ("bash".into(), strs(&["~"])));
    let nothing = item(&pane("Orange notes", false, "", "", ""), 3, 6, HOME);
    assert_eq!(nothing.title, "Shell");
}

#[test]
fn agents() {
    assert_eq!(item(&pane("codex", true, "", "node", "/x"), 1, 1, HOME).agent, Some("codex"), "by the window's name");
    assert_eq!(item(&pane("work", false, "\u{2733} Plan", "2.1.295", "/x"), 1, 1, HOME).agent, Some("claude"),
               "by Claude Code's marks");
    assert_eq!(item(&pane("work", false, "Plan", "vim", "/x"), 1, 1, HOME).agent, None);
    assert_eq!(agent_of(" Claude "), Some("claude"));
    assert_eq!(agent_of("claudette"), None);
    // Claude Code's process is named by its version: no mark in the title needed
    assert_eq!(agent_of("2.1.302"), Some("claude"));
    assert_eq!(item(&pane("Tune Slicer", false, "Tune Slicer", "2.1.302", "/x"), 1, 1, HOME).agent, Some("claude"), "a window the user named");
    for other in ["2.1", "1.2.3.4", "v2.1.302", "2.1.x", "node"] {
        assert_eq!(agent_of(other), None, "{other}");
    }
}

/// The list names Claude Code's program "claude", never its version (its process's name).
#[test]
fn claude_by_its_name_not_its_version() {
    let it = item(&pane("2.1.304", true, "Fix the build", "2.1.304", "/Users/alex/api"), 1, 1, HOME);
    assert_eq!(it.title, "Fix the build");
    assert_eq!(it.sub, ["claude", "~/api"]);
    assert_eq!(it.agent, Some("claude"));
    let untitled = item(&pane("2.1.304", true, "", "2.1.304", "/Users/alex/api"), 1, 1, HOME);
    assert_eq!((untitled.title.as_str(), untitled.sub.as_slice()), ("claude", ["~/api".to_string()].as_slice()), "the window tmux named after it");
    let named = item(&pane("2.1.304", false, "", "2.1.304", "/Users/alex/api"), 1, 1, HOME);
    assert_eq!(named.title, "2.1.304", "a name the user gave stays");
}

#[test]
fn marks_and_folders() {
    assert_eq!(bare("\u{2733} \u{273b} Title"), "Title");
    assert_eq!(bare("\u{23fa} x"), "x");
    assert_eq!(bare("\u{2733}"), "\u{2733}", "nothing else left");
    assert_eq!(bare("plain * star"), "plain * star");
    assert_eq!(bare("[ ! ] Action Required | Check headers | p"), "Check headers | p", "Codex asking");
    assert_eq!(bare("[ . ] Action Required | Check headers | p"), "Check headers | p", "its blink");
    assert_eq!(bare("[ ! ] Action Required"), "[ ! ] Action Required", "nothing else left");
    assert_eq!(bare("\u{280b} | p"), "p", "Codex's spinner before the thread is named");
    assert_eq!(bare("a | b"), "a | b");
    assert_eq!(tilde("/Users/alex/p", HOME), "~/p");
    assert_eq!(tilde("/Users/alex", "/Users/alex/"), "~");
    assert_eq!(tilde("/Users/alexander/p", HOME), "/Users/alexander/p");
    assert_eq!(tilde("/home/alex/p", HOME), "/home/alex/p", "another home than the host's");
    assert_eq!(tilde("/home/alex/p", ""), "~/p", "no home known");
    assert_eq!(tilde("/Users/", ""), "/Users/");
    assert_eq!(tilde("/opt/x", ""), "/opt/x");
}

#[test]
fn list_panes_rows() {
    let row = "main\t1\t@2\tapi\t0\t%3\t0\t\u{2733} Fix\tclaude\t/Users/alex/work/api\t0\t1\tmac.example\t\t\t$1";
    let p = parse_list_panes(row).unwrap();
    assert_eq!((p.session.as_str(), p.window_index, p.window_id.as_str(), p.window_name.as_str()), ("main", 1, "@2", "api"));
    assert_eq!((p.automatic_rename, p.pane_id.as_str(), p.pane_index, p.pane_title.as_str()), (false, "%3", 0, "\u{2733} Fix"));
    assert_eq!((p.command.as_str(), p.path.as_str(), p.alternate_on, p.pane_active), ("claude", "/Users/alex/work/api", false, true));
    assert_eq!((p.host_name.as_str(), p.session_id.as_str()), ("mac.example", "$1"));
    assert_eq!(parse_list_panes(&format!("{row}\n")), Some(p));
    assert_eq!(parse_list_panes("main\t1\t@2\ta\tb\t0\t%3\t0\tt\tc\t/p\t0\t1\th\t\t\t$1"), None, "a tab in a name");
    assert_eq!(parse_list_panes(&row.replacen("\t1\t", "\tx\t", 1)), None);
    assert_eq!(parse_list_panes(""), None);
    let old = parse_list_panes("s\t1\t@1\tzsh\t\t%1\t0\tt\tzsh\t/\t0\t1\th\t\t\t$1").unwrap();
    assert!(old.automatic_rename, "a tmux that does not say: a command's name is tmux's");
    let named = parse_list_panes("s\t1\t@1\tmy api\t\t%1\t0\tt\tzsh\t/\t0\t1\th\t\t\t$1").unwrap();
    assert!(!named.automatic_rename);
}

#[test]
fn ac01_tmux_says_the_name_a_pane_was_started_with() {
    let started = "env SECRET=x sh -c 'sleep 30' ignored --agent-id a@s --agent-name simplify-reuse --team-name s";
    let Some(s) = crate::testutil::Tmux::start("titles-name", (80, 20), started) else { return };
    let text = s.read(&["list-panes", "-a", "-F", LIST_PANES_FORMAT]);
    assert_eq!(parse_list_panes(text.trim()).unwrap().agent_name, "simplify-reuse", "{text}");
}

#[test]
fn tmux_rows_parse_and_its_default_title_is_ignored() {
    let Some(s) = crate::testutil::Tmux::start("titles", (80, 20), "sleep 30") else { return };
    let text = s.read(&["list-panes", "-a", "-F", LIST_PANES_FORMAT]);
    let p = parse_list_panes(text.lines().next().unwrap()).expect("tmux's row parses");
    assert!(p.pane_id.starts_with('%') && p.window_id.starts_with('@') && !p.host_name.is_empty());
    assert_eq!(p.pane_title, p.host_name, "tmux titles a pane with the host's name");
    let i = item(&p, 1, 1, "");
    assert_ne!(i.title, p.host_name);
    assert!(!i.sub.contains(&p.host_name));
}

#[test]
fn ac15_a_restored_pane_shows_its_agents_title_while_a_shell_runs_there() {
    let p = PaneFields { restored_title: "Fix the login form".into(), ..pane("bash", true, "mac (10.0.0.9) — /Users/alex/api", "bash", "/Users/alex/api") };
    assert_eq!(item(&p, 1, 1, HOME).title, "Fix the login form", "over the shell's own title");
    let named = PaneFields { automatic_rename: false, window_name: "api".into(), ..p.clone() };
    assert_eq!(item(&named, 1, 1, HOME).title, "api", "a window the user named keeps its name");
    let agent = PaneFields { command: "2.1.312".into(), pane_title: "\u{2733} A new talk".into(), ..p };
    assert_eq!(item(&agent, 1, 1, HOME).title, "A new talk", "a coding agent running again titles it itself");
    let line = "main\t1\t@1\tbash\t1\t%3\t0\tt\tbash\t/tmp\t0\t1\tmac\t\t\t$1";
    assert_eq!(parse_list_panes(line).map(|p| p.restored_title), Some(String::new()), "16 fields: none");
    assert_eq!(parse_list_panes(&format!("{line}\tOld talk")).map(|p| p.restored_title), Some("Old talk".into()));
}
