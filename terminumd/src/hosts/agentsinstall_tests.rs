// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;

#[test]
fn script_runs_under_sh_with_marker_known_paths_and_caps() {
    let s = script("TMH-0123456789abcdef", "install", false);
    for d in ["\"$HOME/.local/bin\"", "\"$HOME/.claude/local\"", "\"$HOME/.npm-global/bin\"", "\"$HOME\"/.nvm/versions/node/*/bin",
              "\"$HOME/.bun/bin\"", "\"/usr/local/bin\"", "\"/opt/homebrew/bin\""] {
        assert!(s.contains(d), "{d} missing in {s}");
    }
    assert!(s.contains("M=TMH-0123456789abcdef"));
    assert!(s.contains("\"$S\" -lc") && s.contains("\"$S\" -ic"), "login then interactive shell");
    assert!(s.contains(CAP), "cap() in the script");
    assert!(CAP.contains("setsid \"$@\" </dev/null >\"$t\"") && CAP.contains("else \"$@\" </dev/null >\"$t\""), "no input, out to a file");
    assert!(CAP.contains("(sleep 10; pkill -9 -P $p; kill -9 -$p || kill -9 $p)"), "10 s cap without timeout(1): group, else children");
    assert!(CAP.contains("wait $p; r=$?; kill $k 2>/dev/null; kill -9 -$p 2>/dev/null"), "the group goes once it ended");
    assert!(!CAP.contains("set -m"), "job control is off without a terminal in dash and busybox");
    assert!(s.contains("exec \"$H\" hooks install --host-mode --marker \"$M\" \"$@\""));
    assert!(!s.contains("--refresh"));
    assert!(script("TMH-0123456789abcdef", "install", true).contains("--marker \"$M\" --refresh \"$@\""));
    assert!(script("TMH-0123456789abcdef", "remove", false).contains("hooks remove --host-mode"));
    let cmd = remote_command(&s);
    assert!(cmd.starts_with("exec /bin/sh -c '") && cmd.ends_with('\''));
    // every single quote of the script closed, escaped and reopened
    assert_eq!(cmd.matches("'\\''").count(), s.matches('\'').count());
}

#[test]
fn the_script_runs_in_sh_and_frames_the_answer() {
    // the real /bin/sh with a fake home: a claude in ~/.nvm, no helper: the failure lines framed
    let dir = std::env::temp_dir().join(format!("tm-ai-{}", std::process::id()));
    let bin = dir.join(".nvm/versions/node/v22.1.0/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("claude"), "#!/bin/sh\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let m = new_marker().unwrap();
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script(&m, "install", false))
        .env_clear().env("HOME", &dir).env("SHELL", "/bin/sh").env("PATH", "/usr/bin:/bin").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    let got = parse(&text, &m).unwrap();
    assert_eq!(got.len(), 2, "{text}");
    assert!(got.iter().all(|(_, r)| r.starts_with("failed: no helper")), "{got:?}");
}

#[test]
fn parse_reads_only_marker_lines_amid_rc_noise() {
    let m = "TMH-00112233aabbccdd";
    let out = format!("RC-NOISE-top-of-bashrc\r\nWelcome!\nclaude installed\n{m} claude installed\r\nCodex: hooks in\n{m} codex needs-trust\n\
                       {m} gemini installed\nTMH-ffff codex refused: x\n");
    assert_eq!(parse(&out, m).unwrap(), vec![("claude".into(), "installed".into()), ("codex".into(), "needs-trust".into())]);
    let out = format!("{m} codex refused: an entry named terminum runs /opt/x, not /h/.terminum/bin/terminum-host\n");
    assert_eq!(parse(&out, m).unwrap()[0].1, "refused: an entry named terminum runs /opt/x, not /h/.terminum/bin/terminum-host");
    let e = parse("RC-NOISE\nsh: terminumd-agent: not found\n", m).unwrap_err();
    assert!(e.contains("terminumd-agent: not found"), "{e}");
}

#[test]
fn markers_are_fresh_and_shown_results_read() {
    let (a, b) = (new_marker().unwrap(), new_marker().unwrap());
    assert!(a != b && a.len() == 20 && a.starts_with("TMH-"));
    assert_eq!(shown("devbox", "skipped-own"), "uses devbox's own terminum");
    assert_eq!(shown("devbox", "not-found"), "not found");
}

#[test]
fn cap_is_not_stretched_by_a_child_holding_stdout() {
    // the real /bin/sh: an rc-like shell leaves `sleep 3 &` behind holding its stdout
    let t = std::time::Instant::now();
    let out = std::process::Command::new("/bin/sh")
        .arg("-c").arg(format!("{CAP}\nv=$(cap /bin/sh -c 'sleep 3 & echo answer' | cat); echo \"got $v\""))
        .output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "got answer");
    assert!(t.elapsed() < std::time::Duration::from_secs(2), "took {:?}", t.elapsed());
}

#[test]
fn install_stops_after_the_helper_when_the_recipe_changed() {
    let recipe = vec!["alex@devbox.example".to_string()];
    let copied = std::cell::Cell::new(None);
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let e = rt.block_on(install(&recipe, |r| { copied.set(Some(r)); async { Ok(()) } }, || false)).unwrap_err();
    assert_eq!(e, crate::hosts::agents::CHANGED);
    assert_eq!(copied.take(), Some(recipe.clone()), "the helper went over the pinned recipe");
    let e = rt.block_on(install(&recipe, |_| async { Err("copy failed".to_string()) }, || panic!("not asked"))).unwrap_err();
    assert_eq!(e, "copy failed");
}
