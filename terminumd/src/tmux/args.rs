// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Every argument terminumd puts on a tmux command line (spec §9 "The control client",
//! Arguments; §9 Security; AS-11; AC-07 edge "names and folders on tmux's command line").
//!
//! tmux reads each command line with its own parser: inside single quotes nothing is expanded
//! (no `$VAR`, no `~`, no `\`), so every user string goes as one single-quoted word, a quote
//! written `'\''`. Arguments tmux then expands as formats (`-n` and `-c` of `new-window`,
//! `new-session`, `split-window`; `rename-window`; `select-pane -T`) would run `#(…)` and
//! substitute `#{…}`, so there `#` is doubled first. Control characters never reach a command
//! line: one would end it (a newline) or reach the terminal.

/// Window and session names: at most this many characters (spec §9 limits).
pub const MAX_NAME: usize = 100;

/// The refusal of text holding a control or invisible character.
pub const CONTROL_MSG: &str = "Name contains control or invisible characters";

/// One word for tmux's command parser, taken literally: single-quoted, `'` written `'\''`.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// One word for an argument tmux expands as a format: `#` doubled to `##`, then quoted, so
/// `#(cmd)` runs nothing and `#{…}` stays text (AS-11).
pub fn format_arg(s: &str) -> String {
    quote(&literal_format(s))
}

/// A literal value for a tmux format option, before command-parser quoting (or as argv).
pub fn literal_format(s: &str) -> String {
    s.replace('#', "##")
}

/// A word tmux reads as it is (`%3`, `@2`, `main:1`, a key name such as `Up`), else quoted.
pub fn word(s: &str) -> String {
    let plain = !s.is_empty()
        && !s.starts_with('-')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "%@:._-+".contains(c));
    if plain { s.to_string() } else { quote(s) }
}

/// Refuses text holding a control or invisible character (names, titles, folders; termtext.rs
/// `control`).
pub fn check_text(s: &str) -> Result<(), String> {
    if crate::termtext::has_control(s) { Err(CONTROL_MSG.to_string()) } else { Ok(()) }
}

/// A window or pane name as tmux gets it: trimmed, at most [`MAX_NAME`] characters, no control
/// characters. "" is allowed: it gives the window back to automatic naming.
pub fn check_name(s: &str) -> Result<String, String> {
    let name = s.trim();
    check_text(name)?;
    if name.chars().count() > MAX_NAME {
        return Err(format!("A name is at most {MAX_NAME} characters."));
    }
    Ok(name.to_string())
}

/// A session name: as [`check_name`], and not empty and without ':' or '.' (tmux reads those
/// in a target as window and pane separators and would put '_' in their place).
pub fn check_session_name(s: &str) -> Result<String, String> {
    let name = check_name(s)?;
    if name.is_empty() {
        return Err("invalid session: a session needs a name".to_string());
    }
    if name.contains(':') || name.contains('.') {
        return Err(format!("invalid session: {name} (a session name cannot hold ':' or '.')"));
    }
    Ok(name)
}

/// The tmux session a project's windows open in (its [+] and RUN alike).
pub fn project_session(name: &str) -> Result<String, String> {
    check_session_name(&name.replace([':', '.'], "-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn quoting_is_literal_for_tmux() {
        assert_eq!(quote("plain"), "'plain'");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote("$HOME ~x a;b \\ \""), "'$HOME ~x a;b \\ \"'");
    }

    #[test]
    fn formats_stay_text() {
        assert_eq!(format_arg("#(rm -rf ~) #{pane_id}"), "'##(rm -rf ~) ##{pane_id}'");
        assert_eq!(format_arg("it's #S"), "'it'\\''s ##S'");
    }

    #[test]
    fn plain_words_stay_bare() {
        assert_eq!(word("%3"), "%3");
        assert_eq!(word("@12"), "@12");
        assert_eq!(word("main:1"), "main:1");
        assert_eq!(word("M-;"), "'M-;'");
        assert_eq!(word("-x"), "'-x'");
        assert_eq!(word(""), "''");
    }

    #[test]
    fn names_are_trimmed_and_checked() {
        assert_eq!(check_name("  build  ").unwrap(), "build");
        assert_eq!(check_name("   ").unwrap(), "", "empty gives the name back to tmux");
        assert_eq!(check_name(&"x".repeat(100)).unwrap().len(), 100);
        assert_eq!(check_name(&"ü".repeat(100)).unwrap().chars().count(), 100, "characters, not bytes");
        assert!(check_name(&"x".repeat(101)).unwrap_err().contains("at most 100"));
    }

    #[test]
    fn refusals() {
        for bad in ["two\nlines", "bell\x07", "c1\u{9b}", "a\u{2028}b", "a\u{2029}b", "bidi\u{202e}",
                    "iso\u{2067}", "zw\u{200b}", "lrm\u{200e}", "bom\u{feff}", "del\x7f", "tab\tx", "nul\0", "soft\u{ad}hyphen", "wj\u{2060}", "zwj\u{200d}"] {
            assert_eq!(check_name(bad).unwrap_err(), CONTROL_MSG, "{bad:?}");
            assert!(check_text(bad).is_err(), "{bad:?}");
        }
        assert!(check_text("ünïcode 字 ✳ #(x) $y").is_ok());
        assert!(check_session_name("").is_err());
        assert!(check_session_name("a:b").unwrap_err().contains("':' or '.'"));
        assert!(check_session_name("a.b").is_err());
        assert_eq!(check_session_name(" scratch ").unwrap(), "scratch");
    }

    const ODD: [&str; 12] = ["a#{pane_id}", "#(id)", "#S", "$HOME", "~x", "a;b", "it's", "x\"y",
                             "back\\slash", "-dash", "two  spaces ", "ünï 字 ✳"];

    #[test]
    fn names_and_folders_round_trip_through_tmux() {
        let Some(s) = crate::testutil::Tmux::start("args", (80, 20), "sleep 60") else { return };
        let ran = s.dir.join("ran");
        let trap = format!("#(touch {})", ran.display());
        let mut lines = Vec::new();
        let mut want = Vec::new();
        for (i, name) in ODD.iter().enumerate() {
            let folder = s.dir.join(name);
            std::fs::create_dir(&folder).unwrap();
            check_text(name).unwrap();
            lines.push(format!("new-window -d -t main:{} -n {} -c {} {}", i + 1, format_arg(name),
                               format_arg(folder.to_str().unwrap()), quote("sleep 60")));
            want.push((i + 1, name.to_string(), folder));
        }
        lines.push(format!("rename-window -t main:0 {}", format_arg(&trap)));
        // the control: quoted only, a format argument is expanded (AS-11)
        lines.push(format!("new-window -d -t main:20 -n {} {}", quote("x#S"), quote("sleep 60")));
        lines.push(format!("select-pane -t main:0 -T {}", format_arg("it's #{host} #(id)")));
        s.source(&lines);

        // tmux 3.4 writes the `$` of a window name and a pane's folder as `\$` (3.5 no longer);
        // tmux 3.8 keeps a window name's `\` as `\\` (not a folder's)
        let shown = |name: &str| if crate::testutil::tmux_at_least((3, 5)) { name.to_string() } else { name.replace('$', "\\$") };
        let named = |name: &str| if crate::testutil::tmux_at_least((3, 8)) { name.replace('\\', "\\\\") } else { shown(name) };
        for (index, name, folder) in &want {
            let target = format!("main:{index}");
            assert_eq!(s.read(&["display", "-p", "-t", &target, "#{window_name}"]), format!("{}\n", named(name)));
            assert_eq!(s.read(&["display", "-p", "-t", &target, "#{pane_current_path}"]),
                       format!("{}\n", shown(&folder.display().to_string())), "folder of {name:?}");
        }
        assert_eq!(s.read(&["display", "-p", "-t", "main:20", "#{window_name}"]), "xmain\n");
        let names = s.read(&["list-windows", "-t", "main", "-F", "#{window_name}"]);
        assert_eq!(names.lines().next(), Some(trap.as_str()));
        assert_eq!(s.read(&["display", "-p", "-t", "main:0", "#{pane_title}"]), "it's #{host} #(id)\n");
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!Path::new(&ran).exists(), "#(…) ran a command");
    }
}
