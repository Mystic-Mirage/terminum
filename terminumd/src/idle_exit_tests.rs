// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The idle agents' exit (AC-153): what a look decides, and the keys on a real private tmux with a
//! stand-in agent (perl: any program not a shell; the list names the kind) drawing Claude Code's
//! box or Codex's composer, typed into the pane's bare shell as a user starts an agent.

use super::*;
use serde_json::json;

fn pane(seen: Option<u64>) -> Pane {
    Pane { seen, command: "claude".into(), ..Pane::default() }
}

#[test]
fn ac153_tmux_fields_are_read_in_order() {
    let p = Pane::parse("1700000000\t\t0\t2.1.318\t");
    assert_eq!(p, Pane { seen: Some(1_700_000_000), run: String::new(), in_mode: false, command: "2.1.318".into(), start: String::new() });
    assert_eq!(Pane::parse("\tr1\t1\tclaude\t\"claude\"").seen, None, "never stamped");
}

#[test]
fn ac153_a_pane_is_due_only_after_its_days_unopened_and_safe_to_type_into() {
    let now = 100 * DAY;
    assert_eq!(due(&pane(None), now, 3), Due::Stamp, "never seen: its days start now, nothing ends at once");
    assert_eq!(due(&pane(None), now, 0), Due::Stamp, "stamped while off too");
    assert_eq!(due(&pane(Some(now - 3 * DAY + 1)), now, 3), Due::Wait);
    assert_eq!(due(&pane(Some(now - 3 * DAY)), now, 3), Due::Go);
    assert_eq!(due(&pane(Some(0)), now, 0), Due::Wait, "0 days: off");
    assert!(matches!(due(&Pane { run: "r1".into(), ..pane(Some(0)) }, now, 1), Due::Skip(w) if w.contains("Dispatcher")));
    assert!(matches!(due(&Pane { start: "claude --resume".into(), ..pane(Some(0)) }, now, 1), Due::Skip(w) if w.contains("own program")));
    assert!(matches!(due(&Pane { in_mode: true, ..pane(Some(0)) }, now, 1), Due::Skip(_)));
    assert!(matches!(due(&Pane { command: "zsh".into(), ..pane(Some(0)) }, now, 1), Due::Skip(_)));
    for start in ["", "/bin/zsh -l", "\"bash\"", "/bin/sh"] {
        assert_eq!(due(&Pane { start: start.into(), ..pane(Some(0)) }, now, 1), Due::Go, "{start:?}: a shell's pane");
    }
    for start in ["bash -c claude", "zsh -lic claude", "sh -c 'claude; exec sh'"] {
        assert!(matches!(due(&Pane { start: start.into(), ..pane(Some(0)) }, now, 1), Due::Skip(_)), "{start:?}: its shell ends with the agent, and the pane");
    }
}

/// A stand-in agent drawing `kind`'s empty box (or `draft` in it). `how`: `cc` ends on the second
/// Ctrl+C, `exit` only on the line /exit, `stuck` never. Codex's says "ctrl + c again to quit" after
/// the first, as Codex does.
struct Stage {
    tm: crate::testutil::Tmux,
    server: Arc<Server>,
    pane: String,
    key: String,
    app: Shared,
    root: std::path::PathBuf,
    kind: &'static str,
}

impl Stage {
    async fn new(name: &str, kind: &'static str, how: &str, draft: &str) -> Option<Stage> {
        let box_ = match kind {
            "codex" => format!(r#"\xe2\x80\xba {}\n"#, if draft.is_empty() { "\\e[2mAsk Codex to do anything\\e[0m" } else { draft }),
            _ => format!(r#"$r\n\xe2\x9d\xaf {draft}\n$r\n"#),
        };
        let script = format!(r#"system("stty -echo"); $|=1; $n=0; $SIG{{INT}}=sub {{ $n++; exit 0 if "{how}" eq "cc" && $n >= 2; print "\e[6;1Hctrl + c again to quit" if "{kind}" eq "codex" }}; $r="\xe2\x94\x80" x 3; print "\e[2J\e[H{box_}"; while (1) {{ $l = <STDIN>; next unless defined $l; chomp $l; exit 0 if "{how}" eq "exit" && $l eq "/exit"; select(undef, undef, undef, 0.1) if eof(STDIN) }}"#);
        Stage::with(name, kind, (60, 12), &format!("perl -e {}", crate::tmux::args::quote(&script)), |_| true, Duration::from_secs(5)).await
    }

    /// `program` run by the pane's shell, ready once its screen says so.
    async fn with(name: &str, kind: &'static str, size: (u16, u16), program: &str, ready: impl Fn(&str) -> bool, within: Duration) -> Option<Stage> {
        let tm = crate::testutil::Tmux::start(name, size, "/bin/sh")?;
        let pane = tm.read(&["new-window", "-d", "-P", "-F", "#{pane_id}", "/bin/sh"]).trim().to_string();
        tm.read(&["send-keys", "-t", &pane, "-l", &program.replace("{dir}", tm.dir.to_str().unwrap())]);
        tm.read(&["send-keys", "-t", &pane, "Enter"]);
        let tmux = crate::config::find_program("tmux", "TM_TMUX").expect("tmux on PATH");
        let server = Server::new("local", &tm.socket, crate::tmux::server::Link::Local { tmux, socket: tm.socket.clone(), conf: "/dev/null".into() },
                                 Arc::new(tokio::sync::Notify::new()));
        server.sync_clients(&["main".to_string()]).unwrap();
        let until = tokio::time::Instant::now() + Duration::from_secs(3);
        while !server.attached() && tokio::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let key = server.key(&pane);
        let (root, app) = crate::testutil::app(&format!("idle-{name}"));
        *app.layout.write() = json!([{"items": [{"id": key, "agent": kind, "state": "done"}]}]);
        let s = Stage { tm, server, pane, key, app, root, kind };
        let until = tokio::time::Instant::now() + within;
        let drawn = |t: &str| t.contains('\u{276f}') || t.contains('\u{203a}');
        while (is_shell(&s.command()) || !drawn(&s.screen()) || !ready(&s.screen())) && tokio::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(!is_shell(&s.command()) && ready(&s.screen()), "the agent runs: {}\n{}", s.command(), s.screen());
        s.tm.read(&["set-option", "-p", "-t", &s.pane, "@terminum-seen", "1000"]);
        Some(s)
    }
    fn command(&self) -> String {
        self.tm.read(&["display", "-p", "-t", &self.pane, "#{pane_current_command}"]).trim().to_string()
    }
    fn screen(&self) -> String {
        self.tm.read(&["capture-pane", "-p", "-t", &self.pane])
    }
    /// The steps of keys that went, and whether the agent ended.
    async fn end(&self) -> (usize, Result<(), String>) {
        let first = read(&self.server, &self.pane).await.expect("tmux answers");
        assert_eq!(due(&first, now(), 1), Due::Go, "{first:?}");
        end(&self.app, &self.server, &self.pane, &self.key, self.kind, &first).await
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.server.disconnect();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn ac153_two_ctrl_c_end_the_agent_its_shell_and_session_stay() {
    for kind in ["claude", "codex"] {
        let Some(s) = Stage::new(&format!("idle-cc-{kind}"), kind, "cc", "").await else { return };
        assert_eq!(s.end().await, (2, Ok(())), "{kind}: ended by the second Ctrl+C\n{}", s.screen());
        assert!(is_shell(&s.command()), "{kind}: its shell is in front again: {}", s.command());
        assert!(s.tm.read(&["list-sessions", "-F", "#{session_name}"]).contains("main"), "the session stays");
    }
}

#[tokio::test]
async fn ac153_exit_goes_when_ctrl_c_does_not_end_it() {
    for kind in ["claude", "codex"] {
        let Some(s) = Stage::new(&format!("idle-exit-{kind}"), kind, "exit", "").await else { return };
        assert_eq!(s.end().await, (3, Ok(())), "{kind}\n{}", s.screen());
        assert!(is_shell(&s.command()), "{}", s.command());
    }
}

#[tokio::test]
async fn ac153_an_agent_that_stays_is_said_and_left_running() {
    let Some(s) = Stage::new("idle-stuck", "claude", "stuck", "").await else { return };
    let (keys, got) = s.end().await;
    assert!(keys == 3 && got.as_ref().is_err_and(|e| e.contains("still runs")), "{keys} {got:?}");
    assert_eq!(s.command(), "perl");
}

#[tokio::test]
async fn ac153_a_draft_in_the_box_gets_no_key() {
    for kind in ["claude", "codex"] {
        let Some(s) = Stage::new(&format!("idle-draft-{kind}"), kind, "cc", "fix the tests").await else { return };
        let (keys, got) = s.end().await;
        assert!(keys == 0 && got.as_ref().is_err_and(|e| e.contains("draft")), "{kind}: nothing sent, so its days are not restarted: {keys} {got:?}");
        assert_eq!(s.command(), "perl");
    }
}

#[tokio::test]
async fn ac153_a_pane_opened_or_at_work_meanwhile_gets_no_key() {
    let Some(s) = Stage::new("idle-opened", "claude", "cc", "").await else { return };
    let first = read(&s.server, &s.pane).await.unwrap();
    s.tm.read(&["set-option", "-p", "-t", &s.pane, "@terminum-seen", &now().to_string()]);
    let (keys, got) = end(&s.app, &s.server, &s.pane, &s.key, "claude", &first).await;
    assert!(keys == 0 && got.as_ref().is_err_and(|e| e.contains("showed it")), "{keys} {got:?}");
    s.tm.read(&["set-option", "-p", "-t", &s.pane, "@terminum-seen", "1000"]);
    *s.app.layout.write() = json!([{"items": [{"id": s.key, "agent": "claude", "state": "working"}]}]);
    let (keys, got) = s.end().await;
    assert!(keys == 0 && got.as_ref().is_err_and(|e| e.contains("at work")), "{keys} {got:?}");
    assert_eq!(s.command(), "perl");
}

#[tokio::test]
async fn ac153_a_terminals_own_tmux_client_counts_as_opened_terminumds_control_client_does_not() {
    let Some(s) = Stage::new("idle-client", "claude", "stuck", "").await else { return };
    let tmux = crate::config::find_program("tmux", "TM_TMUX").expect("tmux on PATH");
    assert_eq!(attached_panes(&s.server).await, Vec::<String>::new(), "terminumd's control client is no one looking");
    // a terminal's `tmux attach`: `script` gives it a tty of its own; it is ended by its pid
    let attach = [tmux.to_str().unwrap(), "-L", &s.tm.socket, "attach", "-t", "main"];
    let mut script = std::process::Command::new("script");
    if cfg!(target_os = "macos") {
        script.args(["-q", "/dev/null"]).args(attach);
    } else {
        script.args(["-qec", &attach.join(" "), "/dev/null"]);       // util-linux's
    }
    let mut client = script
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let mut shown = Vec::new();
    for _ in 0..50 {
        shown = attached_panes(&s.server).await;
        if !shown.is_empty() { break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = client.kill();
    let _ = client.wait();
    let active = s.tm.read(&["display", "-p", "-t", "main", "#{pane_id}"]).trim().to_string();
    assert_eq!(shown, vec![active], "the pane the terminal shows");
}

/// Real Claude Code (its own config folder, no login, no model call, as web/test/e2e_liveprogs.mjs runs
/// it), when it is on PATH and TM_TEST_NO_CLAUDE is unset: its empty box is read and two Ctrl+C end it.
#[tokio::test]
async fn ac153_real_claude_code_ends_on_two_ctrl_c() {
    if std::env::var_os("TM_TEST_NO_CLAUDE").is_some() || std::process::Command::new("claude").arg("--version").output().is_err() {
        eprintln!("skipped: no claude");
        return;
    }
    let program = r#"mkdir -p '{dir}/cfg' '{dir}/work' && printf '{"hasCompletedOnboarding":true,"projects":{"%s":{"hasTrustDialogAccepted":true}}}' '{dir}/work' > '{dir}/cfg/.claude.json' && cd '{dir}/work' && CLAUDE_CONFIG_DIR='{dir}/cfg' ANTHROPIC_AUTH_TOKEN=no-model-call ANTHROPIC_BASE_URL=http://127.0.0.1:9 DISABLE_AUTOUPDATER=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 claude --safe-mode --tools ''"#;
    let Some(s) = Stage::with("idle-real", "claude", (100, 30), program, |t| t.contains("Safe mode"), Duration::from_secs(40)).await else { return };
    tokio::time::sleep(Duration::from_secs(2)).await;                 // its box drawn whole
    assert_eq!(s.end().await, (2, Ok(())), "ended by the second Ctrl+C, no /exit\n{}", s.screen());
    assert!(is_shell(&s.command()), "{}", s.command());
}
