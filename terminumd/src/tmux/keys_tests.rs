// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Tests of keys.rs (AC-03): special keys go by tmux's names, text as it is, and what reaches
//! a pane on a private tmux server is what a terminal would have sent.

use super::*;
use crate::testutil::Tmux;

fn keys(names: &[&str]) -> Part {
    Part::Keys(names.iter().map(|s| s.to_string()).collect())
}

/// The commands of every part, in order.
fn commands(target: &str, parts: &[Part], extended: bool) -> Vec<String> {
    in_parts(target, parts, extended).concat()
}

fn text(s: &str) -> Part {
    Part::Text(s.to_string())
}

#[test]
fn keys_by_name() {
    assert_eq!(parts("\x1b[A"), [keys(&["Up"])]);
    assert_eq!(parts("\x1bOD\x1bOD"), [keys(&["Left", "Left"])]);
    assert_eq!(parts("\x1b[1;2D\x1b[1;5C"), [keys(&["S-Left", "C-Right"])]);
    assert_eq!(parts("\x1bOP\x1b[15~\x1b[24~"), [keys(&["F1", "F5", "F12"])]);
    assert_eq!(parts("\x1b[5~\x1b[6~\x1b[3~\x1b[H\x1b[F\x1b[Z"),
               [keys(&["PPage", "NPage", "DC", "Home", "End", "BTab"])]);
    assert_eq!(parts("\x1b[3;5~\x1b[1;2P\x1b[1;8A\x1b[2~"), [keys(&["C-DC", "S-F1", "C-M-S-Up", "IC"])]);
}

#[test]
fn control_bytes_go_by_name() {
    assert_eq!(parts("\r\n\t\x7f\x08\x00\x01\x03\x1a\x1c\x1d\x1e\x1f\x1b"),
               [keys(&["Enter", "C-j", "Tab", "BSpace", "C-h", "C-Space", "C-a", "C-c", "C-z",
                       "C-\\", "C-]", "C-^", "C-_", "Escape"])]);
}

#[test]
fn option_as_meta() {
    assert_eq!(parts("\x1ba\x1bB\x1b.\x1b \x1b;"), [keys(&["M-a", "M-B", "M-.", "M-Space", "M-;"])]);
    assert_eq!(parts("\x1b\r\x1b\x7f\x1b\x01"), [keys(&["M-Enter", "M-BSpace", "M-C-a"])]);
    assert_eq!(parts("\x1bü"), [keys(&["M-ü"])]);
    assert_eq!(parts("\x1b\x1b[A"), [keys(&["Escape", "Up"])]);
}

#[test]
fn shift_and_ctrl_enter_go_by_name() {
    assert_eq!(parts("\x1b[13;2u"), [keys(&["S-Enter"])]);
    assert_eq!(parts("a\x1b[13;5u\r"), [text("a"), keys(&["C-Enter", "Enter"])]);
    assert_eq!(parts("\x1b[013;2u\x1b[9;5u\x1b[13u"), [keys(&["S-Enter", "C-Tab", "Enter"])]);
    assert_eq!(parts("\x1b[13;999u"), [keys(&["Escape"]), text("[13;999u")]);
    assert_eq!(parts("\x1b[97;5u"), [keys(&["Escape"]), text("[97;5u")], "only the keys the page sends");
}

#[test]
fn unknown_sequences_are_escape_then_text() {
    assert_eq!(parts("\x1b[99~"), [keys(&["Escape"]), text("[99~")]);
    assert_eq!(parts("\x1b[1;9A"), [keys(&["Escape"]), text("[1;9A")]);
    assert_eq!(parts("\x1b[200~text\x1b[201~"),
               [keys(&["Escape"]), text("[200~text"), keys(&["Escape"]), text("[201~")]);
    assert_eq!(parts("\x1bOx"), [keys(&["Escape"]), text("Ox")]);
    assert_eq!(parts("\x1b["), [keys(&["Escape"]), text("[")]);
}

#[test]
fn a_batch_splits_into_runs_in_order() {
    assert_eq!(parts("\x1b[A\x1b[Ahi \x1b[B"), [keys(&["Up", "Up"]), text("hi "), keys(&["Down"])]);
    assert_eq!(parts("    "), [text("    ")], "held spaces are one piece of text");
    assert_eq!(parts("ls\r"), [text("ls"), keys(&["Enter"])]);
    assert_eq!(parts("\x1b[A ls"), [keys(&["Up"]), text(" ls")]);
    assert_eq!(parts(""), []);
    assert_eq!(parts("ünï 字 ✳"), [text("ünï 字 ✳")]);
    assert_eq!(parts("a\u{9b}b\u{85}"), [text("ab")], "C1 characters are dropped");
}

#[test]
fn ac03_commands() {
    let p = parts("ls -la\r\x1b[A\t\x03");
    assert_eq!(commands("%3", &p, true), ["send-keys -t %3 -l -- 'ls -la'", "send-keys -t %3 Enter Up Tab C-c"]);
    assert_eq!(commands("%3", &parts("-x it's\x1b;\x1c"), true),
               ["send-keys -t %3 -l -- '-x it'\\''s'", "send-keys -t %3 'M-;' 'C-\\'"]);
    assert_eq!(commands("%3", &parts("\x1b[13;2u"), true), ["send-keys -t %3 S-Enter"]);
    assert_eq!(commands("%3", &parts("a\x1b[A\x1b[13;2u\r\x1b[13;5u"), false),
               ["send-keys -t %3 -l -- 'a'", "send-keys -t %3 Up", "send-keys -t %3 -H 1b 5b 31 33 3b 32 75",
                "send-keys -t %3 Enter", "send-keys -t %3 -H 1b 5b 31 33 3b 35 75"], "extended-keys off: bytes");
    assert!(needs_extended(&parts("x\x1b[13;5u")) && !needs_extended(&parts("x\r")));
    assert_eq!(commands("%3", &[text(""), keys(&[])], true), Vec::<String>::new());
    for c in commands("%3", &parts("a\nb\rc\x00\x1b"), true) {
        assert!(!c.chars().any(|c| c.is_control()), "{c:?}");
    }
}


const NAME_CASES: [(&str, bool); 28] = [
    ("Up", true), ("up", true), ("PgDn", true), ("BTab", true), ("Delete", true), ("Insert", true),
    ("F1", true), ("F12", true), ("F13", false), ("F01", false), ("C-M-S-Up", true), ("^a", true),
    ("M--", true), ("S-a", true), ("c-x", true), ("m-a", true), ("M-ü", true), ("C-\\", true),
    ("C-]", true), ("M-Space", true), ("ü", true), ("C-", false), ("Up2", false), ("^", true),
    ("C-^", true), ("M-C-a", true), ("Enter", true), ("Escape", true)];

#[test]
fn hot_key_names() {
    for (name, ok) in NAME_CASES {
        assert_eq!(names_ok(name), ok, "{name}");
    }
    for bad in ["", " ", "\t", "Ctrl-x", "C-Upp", "hello", "C-M-"] {
        assert!(!names_ok(bad), "{bad:?}");
    }
}

#[test]
fn hot_key_names_agree_with_tmux() {
    let Some(s) = Tmux::start("keynames", (80, 20), "sleep 30") else { return };
    for (name, ok) in NAME_CASES {
        let out = s.tmux(&["bind-key", "-T", "tmtest", name, "display", "x"]);
        assert_eq!(out.status.success(), ok, "tmux and names_ok differ on {name}");
    }
}

#[test]
fn typed_bytes_reach_the_pane_as_a_terminal_sends_them() {
    // keys whose tmux encoding (normal cursor keys, no extended keys) is xterm's own
    let data = "ls -la\r\x1b[A\t\x03-x;'#{q}$H~\n\x7f\x08\x00\x01\x1a\x1c\x1d\x1e\x1f\x1b\x1ba\x1b\r\x1b\x7f\
                \x1b[Z\x1b[15~\x1b[1;2D\x1b[1;5C\x1bOP\x1b[3~\x1b[5~ ünï 字\x1b[99~";
    let dir = crate::testutil::temp_root().join(format!("tm-test-keys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("got");
    let cmd = format!("stty raw -echo; exec head -c {} > {}", data.len(), out.display());
    let Some(s) = Tmux::start("keys", (80, 20), &format!("sh -c {}", super::super::args::quote(&cmd))) else { return };
    let wait = |ok: &dyn Fn() -> bool| {
        for _ in 0..100 {
            if ok() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    };
    assert!(wait(&|| s.tmux(&["display", "-p", "-t", "main", "#{pane_current_command}"]).stdout == b"head\n"));
    let file = dir.join("cmds.conf");
    std::fs::write(&file, commands("main", &parts(data), true).join("\n") + "\n").unwrap();
    let r = s.tmux(&["source-file", file.to_str().unwrap()]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert!(wait(&|| std::fs::metadata(&out).map(|m| m.len() as usize >= data.len()).unwrap_or(false)),
            "the pane got {:?}", std::fs::read(&out).ok().map(|b| String::from_utf8_lossy(&b).into_owned()));
    let got = std::fs::read(&out).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(String::from_utf8_lossy(&got), data);
}

/// AS-10: with `extended-keys on` (terminum's tmux.conf), S-Enter reaches a program that asked
/// for extended keys (modifyOtherKeys, as Claude Code does) as CSI u, and a shell as Enter;
/// with it off (a host's tmux by default) the bytes still reach the program.
#[test]
fn shift_enter_is_csi_u_only_for_programs_that_ask() {
    // (the program asks, the server's extended-keys); it reads Enter only from a server that
    // names the keys (extended-keys on, tmux 3.5+: 3.4 has no extended-keys-format) when it did not ask
    for (ask, on) in [(true, true), (false, true), (true, false)] {
        let dir = crate::testutil::temp_root().join(format!("tm-test-xk-{}-{ask}-{on}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let named = on && crate::testutil::tmux_at_least((3, 5));
        let want = if !ask && named { "\r\r" } else { "\x1b[13;2u\x1b[13;5u" };
        let out = dir.join("got");
        // the program asks only once the options are set (tmux drops a request made before)
        let go = dir.join("go");
        let cmd = format!("while [ ! -e {} ]; do sleep 0.02; done; {}stty raw -echo; exec head -c {} > {}", go.display(),
                          if ask { "printf '\\033[>4;2m'; " } else { "" }, want.len(), out.display());
        let Some(s) = Tmux::start("xk", (80, 20), &format!("sh -c {}", super::super::args::quote(&cmd))) else { return };
        s.tmux(&["set", "-g", "extended-keys", if on { "on" } else { "off" }]);
        s.tmux(&["set", "-g", "extended-keys-format", "csi-u"]);
        std::fs::write(&go, "").unwrap();
        let ready = (0..100).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            s.tmux(&["display", "-p", "-t", "main", "#{pane_current_command}"]).stdout == b"head\n"
        });
        assert!(ready);
        for c in commands("main", &parts("\x1b[13;2u\x1b[13;5u"), named) {
            let args: Vec<&str> = c.split(' ').collect();
            assert!(s.tmux(&args).status.success(), "{c}");
        }
        let got = (0..100).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::fs::read(&out).ok().filter(|b| b.len() >= want.len())
        });
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(got.map(|b| String::from_utf8_lossy(&b).into_owned()).as_deref(), Some(want), "asked: {ask}, extended-keys: {on}");
    }
}

#[test]
fn a_long_run_of_keys_goes_in_parts_of_key_part() {
    let left = parts(&"\x1b[D".repeat(250));
    let cmds = commands("%3", &left, true);
    let sizes: Vec<usize> = cmds.iter().map(|c| c.matches(" Left").count()).collect();
    assert_eq!(sizes, [KEY_PART, KEY_PART, KEY_PART, 250 - 3 * KEY_PART], "each part's bytes fit the terminal's buffer with room");
    assert!(KEY_PART * "\x1b[106;5u".len() <= PART_BYTES && PART_BYTES < 800, "even as CSI u a part is under the 1 KB a terminal hands on at once and Claude Code's 800 characters");
    assert_eq!(in_parts("%3", &left, true).len(), 4, "a part a round trip, each once the program read the last");
    assert_eq!(commands("%3", &parts("\x1b[D\x1b[D"), true), ["send-keys -t %3 Left Left"], "a short run stays one command");
}

#[test]
fn long_typed_text_goes_in_parts_of_part_bytes() {
    // a quote Ask types: lines with ⌃J between, 2.3 KB, Cyrillic (2 bytes a character)
    let quote = (0..30).map(|i| format!("> строка {i:02} цитаты из ответа агента")).collect::<Vec<_>>().join("\n");
    let p = parts(&quote);
    let split = in_parts("%3", &p, true);
    assert!(split.len() >= 5, "{} parts", split.len());
    for part in &split {
        let text: usize = part.iter().filter_map(|c| c.strip_prefix("send-keys -t %3 -l -- ")).map(|q| q.len() - 2).sum();
        let keys: usize = part.iter().filter(|c| !c.contains(" -l ")).map(|c| c.split(' ').count() - 3).sum();
        assert!(text + keys * 8 <= PART_BYTES, "{part:?}");
    }
    // one line longer than a part: cut at characters, all of it sent
    let long = "ж".repeat(700);
    let cmds = commands("%3", &parts(&long), true);
    assert_eq!(cmds.len(), 3);
    let back: String = cmds.iter().map(|c| c.trim_start_matches("send-keys -t %3 -l -- ").trim_matches('\'')).collect();
    assert_eq!(back, long);
    assert_eq!(in_parts("%3", &parts("ls -la\r"), true), [["send-keys -t %3 -l -- 'ls -la'", "send-keys -t %3 Enter"]], "what fits is one part");
    assert_eq!(pieces("ab€cd", 3).collect::<Vec<_>>(), ["ab", "€", "cd"], "never inside a character");
    assert_eq!(pieces("", 3).count(), 0);
}
