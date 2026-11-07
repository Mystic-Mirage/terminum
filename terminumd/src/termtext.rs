// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What fbd types into a terminal (AC-12): words quoted for the pane's own shell, never a
//! control or invisible character, never a word a command could take for an option, and
//! never Enter: the user runs the line. Also terminumd's one rule of those characters
//! (`control`, `hides`, `stray`, `clean`): names, labels, titles, plans, links and a program's
//! copies use it.

/// Whether `s` holds a character that acts as a keystroke or hides what is typed (`control`).
pub fn has_control(s: &str) -> bool {
    s.chars().any(control)
}

/// A character that acts as a keystroke (C0, DEL, C1) or hides what is typed (`hides`, and the
/// zero-width joiners): never in a name, a command line or an address.
pub fn control(c: char) -> bool {
    c.is_control() || hides(c) || matches!(c, '\u{200C}' | '\u{200D}')
}

/// What makes a line read otherwise than it runs: bidi controls, zero-width characters, the
/// soft hyphen, word joiners, line and paragraph separators, tag characters (text a model reads
/// and a person does not see); never ZWNJ and ZWJ, which words and emoji need. The page's own
/// copy of the list: web/static/seltext.js `HIDE` (web/test/hides.test.mjs keeps them equal).
pub fn hides(c: char) -> bool {
    matches!(c as u32, 0xAD | 0x061C | 0x180E | 0x200B | 0x200E..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB | 0xE0001 | 0xE0020..=0xE007F)
}

/// A control character or one that `hides`: what `clean` drops, and what a label, a title or a
/// name shown as text refuses.
pub fn stray(c: char) -> bool {
    c.is_control() || hides(c)
}

/// `s` without what is `stray`: with `lines`, tabs and line ends kept (`\r\n`, `\r` and the
/// line and paragraph separators are `\n`).
pub fn clean(s: &str, lines: bool) -> String {
    let s = if lines { s.replace("\r\n", "\n").replace(['\r', '\u{2028}', '\u{2029}'], "\n") } else { s.to_string() };
    s.chars().filter(|c| (lines && matches!(c, '\t' | '\n')) || !stray(*c)).collect()
}

/// Needs no quotes in any shell: not even zsh's `=cmd` (a path lookup) or tcsh's `=N`.
fn bare(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('=') && s.chars().all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c))
}

/// The shell's name as the bridge reports the pane's job ("-zsh", "/bin/bash" → "zsh", "bash").
pub(crate) fn shell_name(job: &str) -> &str {
    job.rsplit('/').next().unwrap_or("").trim_start_matches('-')
}

/// `s` as one word of the shell `job`, or None if that shell cannot take it safely.
pub fn quote(job: &str, s: &str) -> Option<String> {
    // a relative name that starts with "-" would be read as an option by the command
    let s = if s.starts_with('-') { format!("./{s}") } else { s.to_string() };
    if bare(&s) {
        return Some(s);
    }
    match shell_name(job) {
        "bash" | "zsh" | "sh" | "dash" | "ksh" | "mksh" => Some(format!("'{}'", s.replace('\'', "'\\''"))),
        // inside fish's single quotes, \ escapes ' and \ itself
        "fish" => Some(format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))),
        // tcsh and csh expand ! even in single quotes; nu has no escape inside them; xonsh
        // reads \ as an escape: single quotes only for names without these
        _ if !s.contains(['\'', '\\', '!']) => Some(format!("'{s}'")),
        _ => None,
    }
}

/// The text typed for `insert` (paths, then a space) or `cd` (the folder), or the reason
/// it is not typed.
pub fn line(job: &str, action: &str, paths: &[String], path: Option<&str>) -> Result<String, String> {
    let shell = match shell_name(job) {
        "" => "this shell",
        s => s,
    };
    let words = match (action, path) {
        ("insert", _) if !paths.is_empty() => paths.iter().map(String::as_str).collect::<Vec<_>>(),
        ("cd", Some(p)) => vec![p],
        _ => return Err("insert needs paths, cd needs path".into()),
    };
    if words.iter().any(|w| has_control(w)) {
        return Err("Name contains control or invisible characters — not sent to the terminal".into());
    }
    let quoted = words.iter().map(|w| quote(job, w)).collect::<Option<Vec<_>>>()
        .ok_or_else(|| format!("Name cannot be typed safely into {shell} — not sent to the terminal"))?;
    Ok(if action == "cd" { format!("cd {}", quoted[0]) } else { quoted.join(" ") + " " })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn invisible_and_control_characters() {
        for s in ["a\u{3}b", "x\r", "\u{7f}", "a\u{85}b", "a\u{202e}b", "a\u{200b}b", "a\u{200d}b", "\u{2066}x", "\u{2028}", "\u{feff}"] {
            assert!(has_control(s), "{s:?}");
        }
        assert!(!has_control("naïve file — ü 文件.txt"));
        assert_eq!(clean("a\r\nb\rc\u{2028}d\te\u{202e}\u{e0041}\u{7}", true), "a\nb\nc\nd\te");
        assert_eq!(clean("a\nb\u{2028}c\u{200d}", false), "abc\u{200d}");
        assert!(stray('\u{ad}') && stray('\u{7}') && !stray('\u{200d}') && !stray('é'), "a label keeps ZWJ, refuses a soft hyphen");
    }

    #[test]
    fn quoting_per_shell() {
        assert_eq!(quote("zsh", "src/db/pool.rs").unwrap(), "src/db/pool.rs");
        assert_eq!(quote("-zsh", "it's a\\b").unwrap(), "'it'\\''s a\\b'");
        assert_eq!(quote("/usr/local/bin/fish", "it's a\\b").unwrap(), "'it\\'s a\\\\b'");
        assert_eq!(quote("tcsh", "my file").unwrap(), "'my file'");
        assert_eq!(quote("tcsh", "it's"), None);
        assert_eq!(quote("nu", "a\\b"), None);
        assert_eq!(quote("", "wow!"), None, "an unknown shell gets the strict rule");
        assert_eq!(quote("bash", "-rf").unwrap(), "./-rf");
        assert_eq!(quote("bash", "-my file").unwrap(), "'./-my file'");
        assert_eq!(quote("zsh", "=ls").unwrap(), "'=ls'", "zsh would type /bin/ls");
        assert_eq!(quote("zsh", "a=b").unwrap(), "a=b");
    }

    #[test]
    fn lines_never_press_enter() {
        assert_eq!(line("zsh", "cd", &[], Some("/Users/alex/my dir")).unwrap(), "cd '/Users/alex/my dir'");
        assert_eq!(line("zsh", "insert", &["a".into(), "b c".into()], None).unwrap(), "a 'b c' ");
        assert!(line("tcsh", "insert", &["it's".into()], None).unwrap_err().contains("into tcsh"));
        assert!(line("zsh", "cd", &[], Some("/a\u{202e}b")).unwrap_err().contains("invisible"));
        assert!(line("zsh", "cd", &[], None).is_err());
    }

    /// Each shell installed here reads the quoted word back as exactly the name, and runs nothing.
    #[test]
    fn round_trip_through_real_shells() {
        let names = ["plain", "=ls", "=1", "my file", "it's", "a\\'; touch PWNED; #", "$(touch PWNED)", "`touch PWNED`", "a\"b", "wow!", "-rf", "ü 文件", "*", "~x", "a;b|c&d"];
        for sh in ["bash", "zsh", "sh", "dash", "fish", "tcsh"] {
            if Command::new("sh").args(["-c", &format!("command -v {sh}")]).output().map_or(true, |o| !o.status.success()) {
                continue;
            }
            let dir = crate::testutil::temp_root().join(format!("fbd-quote-{sh}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            for name in names {
                let Some(q) = quote(sh, name) else { continue };
                let out = Command::new(sh).current_dir(&dir).args(["-c", &format!("printf '%s' {q}")]).output().unwrap();
                let want = if name.starts_with('-') { format!("./{name}") } else { name.to_string() };
                assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{sh}: {name:?} quoted as {q}");
            }
            assert!(!dir.join("PWNED").exists(), "{sh} ran a command from a name");
            std::fs::remove_dir_all(&dir).ok();
        }
    }
}
