// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! AC-08 / AC-14: the launchd agent's plist and the launchctl calls (a stub launchctl), the
//! guard that keeps tests off the live agent, direct mode's pid file, and the reason
//! terminumd gives in its log when it does not come up.

use std::time::Duration;

use super::*;
use crate::cli::{direct, launch};

fn stubbed(home: &Home, env: &[(&str, &str)]) -> (Ctx, PathBuf) {
    let dir = home.path("lc");
    std::fs::create_dir_all(&dir).unwrap();
    let mut ctx = home.ctx(env);
    ctx.launchctl = stub_launchctl(&dir);
    ctx.wait = Duration::from_millis(300); // no terminumd answers after the stub's bootstrap
    (ctx, dir)
}

#[test]
fn invalid_pid_records_never_resolve_to_signal_targets() {
    let home = Home::new();
    let ctx = home.ctx(&[]);
    std::fs::create_dir_all(&ctx.app_dir).unwrap();
    // Only inspect running_pid: calling stop on an invalid record would be unsafe before the fix.
    for text in ["0 ".to_string(), format!("{} ", u32::MAX), format!("{} ", i32::MAX as u32 + 1), format!("{} ", std::process::id())] {
        std::fs::write(ctx.pidfile(), text).unwrap();
        assert_eq!(direct::running_pid(&ctx), None);
    }
}

#[test]
fn timed_out_commands_end_their_descendants_and_output_readers() {
    let home = Home::new();
    let pidfile = home.path("descendant.pid");
    let mut command = std::process::Command::new("/bin/sh");
    command.args(["-c", "sleep 30 & echo $! > \"$TM_TEST_CHILD_PID\"; wait"]).env("TM_TEST_CHILD_PID", &pidfile);
    let result = launch::run_timeout(command, Duration::from_millis(200));
    assert!(result.is_err());
    let pid: libc::pid_t = std::fs::read_to_string(pidfile).unwrap().trim().parse().unwrap();
    let state = std::process::Command::new("/bin/ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().unwrap();
    let running = !state.stdout.is_empty() && !String::from_utf8_lossy(&state.stdout).trim().starts_with('Z');
    if running {
        // SAFETY: this is the sleep child whose pid our own fixture recorded; cleanup before asserting.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    assert!(!running, "the timed-out command left its descendant and pipe reader running");
}

#[test]
fn bounded_commands_keep_both_output_streams_and_exit_status() {
    let mut command = std::process::Command::new("/bin/sh");
    command.args(["-c", "printf stdout; printf stderr >&2; exit 7"]);
    let output = launch::run_timeout(command, Duration::from_secs(2)).unwrap();
    assert_eq!((output.stdout.as_slice(), output.stderr.as_slice(), output.status.code()), (b"stdout".as_slice(), b"stderr".as_slice(), Some(7)));
}

#[test]
fn a_command_has_no_terminal_to_stop_on() {
    // its own session: /dev/tty cannot be opened, so a read of it fails at once instead of
    // stopping the command in the background until the limit
    let mut command = std::process::Command::new("/bin/sh");
    command.args(["-c", "if (: < /dev/tty) 2>/dev/null; then echo tty; else echo none; fi; ps -o sess= -p $$ >/dev/null; [ \"$(ps -o pgid= -p $$ | tr -d ' ')\" = $$ ] && echo leader"]);
    // stopped on a tty read, it would run out its limit: an error, not this output
    let output = launch::run_timeout(command, Duration::from_secs(20)).unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "none\nleader\n");
}

/// The plist as plutil reads it (macOS only).
#[cfg(target_os = "macos")]
fn plist_json(path: &Path) -> Value {
    let lint = std::process::Command::new("/usr/bin/plutil").arg("-lint").arg(path).output().unwrap();
    assert!(lint.status.success(), "{}", String::from_utf8_lossy(&lint.stdout));
    let out = std::process::Command::new("/usr/bin/plutil").args(["-convert", "json", "-o", "-"]).arg(path).output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

#[cfg(target_os = "macos")]
#[test]
fn the_plist() {
    let home = Home::new();
    let (ctx, _) = stubbed(&home, &[("PATH", "/usr/bin:/opt/homebrew/bin:/opt/other/bin:relative")]);
    launch::start(&ctx).unwrap();
    let root = home.path(".terminum");
    assert_eq!(ctx.plist(), home.path("Library/LaunchAgents/io.terminum.terminumd.plist"));
    let p = plist_json(&ctx.plist());
    assert_eq!(p["Label"], "io.terminum.terminumd");
    assert_eq!(p["ProgramArguments"], json!([root.join("daemon/terminumd").display().to_string(), "serve"]));
    assert_eq!((p["RunAtLoad"].clone(), p["KeepAlive"].clone(), p["ThrottleInterval"].clone()), (json!(true), json!(true), json!(1)));
    assert_eq!(p["StandardOutPath"], root.join("logs/terminumd.log").display().to_string());
    assert_eq!(p["StandardErrorPath"], p["StandardOutPath"]);
    assert_eq!(p["Sockets"]["Listeners"], json!({"SockNodeName": "127.0.0.1", "SockServiceName": "48080", "SockFamily": "IPv4", "SockType": "stream"}));
    let path: Vec<&str> = p["EnvironmentVariables"]["PATH"].as_str().unwrap().split(':').collect();
    assert_eq!(path[..2], ["/opt/homebrew/bin", "/usr/local/bin"]);
    assert_eq!(path.last(), Some(&"/opt/other/bin"), "the installing shell's own folders follow");
    let mut unique = path.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), path.len(), "each folder once");
    assert!(p["EnvironmentVariables"].get("TM_HOME").is_none(), "default folders are not pinned");
    assert_eq!(mode(&root.join("logs/terminumd.log")), 0o600, "made private before launchd opens it");
    assert_eq!(mode(&root.join("state")), 0o700);
    assert_eq!(launch::plist_port(&ctx), Some(48080));
}

#[test]
fn the_port_launchd_holds_comes_from_settings() {
    let home = Home::new();
    let (ctx, _) = stubbed(&home, &[]);
    std::fs::create_dir_all(&ctx.root).unwrap();
    std::fs::write(ctx.root.join("settings.json"), r#"{"port": 47911}"#).unwrap();
    assert!(launch::plist(&ctx, launch::port(&ctx)).contains("<string>47911</string>"));
    launch::start(&ctx).unwrap();
    assert_eq!(launch::plist_port(&ctx), Some(47911));
}

#[test]
fn first_start_loads_then_restart_kickstarts() {
    let home = Home::new();
    let (ctx, dir) = stubbed(&home, &[]);
    launch::start(&ctx).unwrap();
    assert_eq!(verbs(&dir), ["bootstrap"]);
    std::fs::remove_file(dir.join("calls")).unwrap();
    launch::start(&ctx).unwrap();
    assert_eq!(verbs(&dir), ["kickstart"], "the plist is the same: a restart takes the new `current`, launchd keeps the port");
    let calls = std::fs::read_to_string(dir.join("calls")).unwrap();
    assert!(calls.contains("kickstart -k gui/"), "{calls}");
}

#[test]
fn a_loaded_agent_is_only_kickstarted_even_when_its_plist_would_change() {
    let home = Home::new();
    let (ctx, dir) = stubbed(&home, &[]);
    launch::start(&ctx).unwrap();
    let before = std::fs::read_to_string(ctx.plist()).unwrap();
    std::fs::remove_file(dir.join("calls")).unwrap();
    let (mut other, _) = stubbed(&home, &[("PATH", "/opt/other/bin")]);
    other.launchctl = ctx.launchctl.clone();
    launch::start(&other).unwrap();
    assert_eq!(verbs(&dir), ["kickstart"], "upgrades and rollbacks never give the port up");
    assert_eq!(std::fs::read_to_string(ctx.plist()).unwrap(), before, "the plist changes with terminum restart");
    launch::reload(&other).unwrap();
    assert!(std::fs::read_to_string(ctx.plist()).unwrap().contains("/opt/other/bin"));
}

/// The stub's bootstraps find the port unbindable, as launchd does when another program holds
/// it; terminumd is served (answers) meanwhile.
fn unbound(home: &Home) -> (Ctx, PathBuf, u16) {
    let (ctx, dir) = stubbed(home, &[]);
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    std::fs::create_dir_all(&ctx.root).unwrap();
    std::fs::write(ctx.root.join("settings.json"), format!("{{\"port\": {port}}}")).unwrap();
    std::fs::write(dir.join("unbound"), "1000").unwrap();
    std::fs::write(dir.join("port"), port.to_string()).unwrap();
    std::fs::write(dir.join("unbound_log"), ctx.log().display().to_string()).unwrap();
    (ctx, dir, port)
}

#[test]
fn a_port_launchd_could_not_bind_fails_at_once_with_the_reason() {
    let home = Home::new();
    let (ctx, dir, port) = unbound(&home);
    let _squatter = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    assert_eq!(launch::start(&ctx).unwrap_err(), crate::activation::unbound_message(port));
    assert_eq!(verbs(&dir), ["bootstrap"], "no retry: the user frees it, then runs terminum restart");
}

#[test]
fn the_agent_of_the_old_name_is_unloaded_before_this_one_is_loaded() {
    let home = Home::new();
    let (ctx, dir) = stubbed(&home, &[]);
    let legacy = ctx.agents.join("io.terminum.terminusd.plist");
    let aside = ctx.agents.join(".io.terminum.terminusd.plist.retired");
    std::fs::create_dir_all(&ctx.agents).unwrap();
    std::fs::write(&legacy, "<plist/>").unwrap();
    launch::migrate_legacy(&ctx).unwrap();
    launch::start(&ctx).unwrap();
    let calls = std::fs::read_to_string(dir.join("calls")).unwrap();
    let first = calls.lines().find(|l| !l.starts_with("print")).unwrap_or_default();
    assert!(first.starts_with("bootout gui/") && first.ends_with("/io.terminum.terminusd"), "{calls}");
    assert_eq!(verbs(&dir).last().map(String::as_str), Some("bootstrap"));
    assert!(!legacy.exists() && aside.exists(), "set aside: not loaded at login, kept until the new build answers");
    std::fs::remove_file(dir.join("calls")).unwrap();
    launch::migrate_legacy(&ctx).unwrap();
    launch::start(&ctx).unwrap();
    assert!(!std::fs::read_to_string(dir.join("calls")).unwrap().contains("terminusd"), "once only");

    // the new build did not come up: the old agent is loaded again
    std::fs::remove_file(dir.join("calls")).unwrap();
    assert!(launch::restore_legacy(&ctx).unwrap());
    assert_eq!(verbs(&dir), ["bootout", "bootstrap"]);
    assert!(std::fs::read_to_string(dir.join("calls")).unwrap().contains("io.terminum.terminusd.plist"));
    assert!(legacy.exists() && !aside.exists() && !ctx.plist().exists());
    assert!(!launch::restore_legacy(&ctx).unwrap(), "nothing set aside: nothing to bring back");
}

#[test]
fn the_old_names_log_goes_in_front_of_the_new_one() {
    let home = Home::new();
    let (ctx, _dir) = stubbed(&home, &[]);
    std::fs::create_dir_all(ctx.log_dir()).unwrap();
    std::fs::write(ctx.log_dir().join("terminusd.log"), "old 1\nold 2\n").unwrap();
    std::fs::write(ctx.log(), "new 1\n").unwrap();
    launch::migrate_legacy(&ctx).unwrap();
    assert_eq!(std::fs::read_to_string(ctx.log()).unwrap(), "old 1\nold 2\nnew 1\n");
    assert!(!ctx.log_dir().join("terminusd.log").exists());
    assert_eq!(mode(&ctx.log()), 0o600);
}

#[test]
fn the_old_agents_files_go_once_the_new_build_answers() {
    let home = Home::new();
    let (ctx, _dir) = stubbed(&home, &[]);
    let aside = ctx.agents.join(".io.terminum.terminusd.plist.retired");
    std::fs::create_dir_all(&ctx.agents).unwrap();
    std::fs::write(&aside, "<plist/>").unwrap();
    let pid = ctx.root.join("state/terminusd.pid");
    std::fs::create_dir_all(pid.parent().unwrap()).unwrap();
    std::fs::write(&pid, "1").unwrap();
    launch::forget_legacy(&ctx);
    assert!(!aside.exists() && !pid.exists());
}

#[test]
fn a_failed_kickstart_loads_again() {
    let home = Home::new();
    let (ctx, dir) = stubbed(&home, &[]);
    launch::start(&ctx).unwrap();
    std::fs::remove_file(dir.join("calls")).unwrap();
    std::fs::write(dir.join("fail_kickstart"), "").unwrap();
    launch::start(&ctx).unwrap();
    assert_eq!(verbs(&dir), ["kickstart", "bootout", "bootstrap"]);
}

#[test]
fn restart_always_loads_again() {
    let home = Home::new();
    let (ctx, dir) = stubbed(&home, &[]);
    launch::start(&ctx).unwrap();
    std::fs::remove_file(dir.join("calls")).unwrap();
    launch::reload(&ctx).unwrap();
    assert_eq!(verbs(&dir), ["bootout", "bootstrap"], "the port is bound anew");
}

#[test]
fn a_refused_bootstrap_says_what_launchd_answered() {
    let home = Home::new();
    let (ctx, dir) = stubbed(&home, &[]);
    std::fs::write(dir.join("refuse"), "").unwrap();
    let msg = launch::start(&ctx).unwrap_err();
    assert!(msg.contains("Input/output error"), "{msg}");
    assert!(verbs(&dir).len() > 1, "retried while launchd may still be unloading");
}

#[test]
fn stop_unloads_and_removes_the_agent() {
    let home = Home::new();
    let (ctx, dir) = stubbed(&home, &[]);
    launch::start(&ctx).unwrap();
    launch::stop(&ctx).unwrap();
    assert_eq!(verbs(&dir).last().map(String::as_str), Some("bootout"));
    assert!(!ctx.plist().exists());
}

#[test]
fn moved_folders_reach_terminumd() {
    let home = Home::new();
    let root = home.path("r").display().to_string();
    let la = home.path("la").display().to_string();
    let ctx = home.ctx(&[("TM_HOME", &root), ("TM_LAUNCH_AGENTS", &la)]);
    let env: HashMap<String, String> = launch::service_env(&ctx).into_iter().collect();
    assert_eq!(env["TM_HOME"], root);
    assert!(!env.contains_key("TM_APP_DIR"));
    assert_eq!(ctx.plist().parent(), Some(home.path("la").as_path()));
    assert_eq!(ctx.sock(), home.path("r/state/terminumd.sock"));
}

#[test]
fn launchd_is_never_asked_for_a_home_that_is_not_the_real_one() {
    let home = Home::new();
    let ctx = home.ctx(&[]);
    assert_eq!(ctx.launchctl, Path::new("/bin/launchctl"), "the real one: refused before it runs");
    let msg = launch::start(&ctx).unwrap_err();
    assert!(msg.contains("TM_NO_LAUNCHD=1"), "{msg}");
    assert!(launch::stop(&ctx).is_err());
}

#[test]
fn only_test_labels_may_replace_the_live_one() {
    let home = Home::new();
    let env = |l: &str| {
        let mut m = HashMap::new();
        m.insert("HOME".to_string(), home.dir.display().to_string());
        m.insert("TM_LAUNCHD_LABEL".to_string(), l.to_string());
        Ctx::new(m, account_home())
    };
    assert_eq!(env("io.terminum.test-42").unwrap().label, "io.terminum.test-42");
    for bad in ["io.terminum.terminumd2", "com.apple.x", "io.terminum.test-", "io.terminum.test-a/b"] {
        assert!(env(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_reused_pid_is_not_stopped() {
    let home = Home::new();
    let ctx = home.ctx(&[("TM_NO_LAUNCHD", "1")]);
    std::fs::create_dir_all(&ctx.app_dir).unwrap();
    let me = std::process::id();
    std::fs::write(ctx.pidfile(), format!("{me} Thu Jan  1 00:00:00 1970\n")).unwrap();
    assert_eq!(direct::running_pid(&ctx), None, "another process now has that pid");
    direct::stop(&ctx); // would kill this test process if it signalled the pid
    std::fs::write(ctx.pidfile(), format!("{me} {}\n", direct::started_at(me))).unwrap();
    assert_eq!(direct::running_pid(&ctx), Some(me));
}

fn with_log(text: &str) -> (Home, Ctx) {
    let home = Home::new();
    let ctx = home.ctx(&[]);
    std::fs::create_dir_all(ctx.log_dir()).unwrap();
    std::fs::write(ctx.log(), text).unwrap();
    (home, ctx)
}

#[test]
fn the_last_error_is_the_reason() {
    let (_h, ctx) = with_log(concat!(
        "2026-10-15T09:00:00Z level=error event=x msg=\"an old one\"\n",
        "2026-10-15T09:00:01Z level=error event=listen msg=\"port 48080 is in use (set `port` in ~/.terminum/settings.json)\"\n",
        "2026-10-15T09:00:01Z level=info event=exit\n"
    ));
    assert_eq!(launch::log_reason(&ctx, None).as_deref(), Some("port 48080 is in use (set `port` in ~/.terminum/settings.json)"));
    assert_eq!(launch::describe(&ctx, None, None), "terminumd does not answer: port 48080 is in use (set `port` in ~/.terminum/settings.json)");
}

#[test]
fn terminumds_own_error_lines_give_the_reason() {
    let msg = crate::activation::unbound_message(48080);
    let (_h, ctx) = with_log(&format!("error: {msg}\n2026-10-15T09:00:01.000000Z ERROR terminumd::sock: event=\"start.failed\" msg=\"{msg}\"\n"));
    assert_eq!(launch::log_reason(&ctx, None), Some(msg));
    let (_h, ctx) = with_log("2026-10-15T09:00:01Z INFO terminumd: event=start NOERRORS=1\n");
    assert_eq!(launch::log_reason(&ctx, None), None, "ERROR inside a word is no error");
}

#[test]
fn only_what_follows_the_mark_counts() {
    let (_h, ctx) = with_log("level=error msg=\"before the restart\"\n");
    let mark = launch::log_mark(&ctx);
    assert_eq!(launch::log_reason(&ctx, Some(mark)), None);
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(ctx.log()).unwrap();
    writeln!(f, "thread 'main' panicked at src/main.rs:9:5").unwrap();
    assert_eq!(launch::log_reason(&ctx, Some(mark)).as_deref(), Some("thread 'main' panicked at src/main.rs:9:5"));
}

#[test]
fn no_log_no_reason() {
    let home = Home::new();
    let ctx = home.ctx(&[]);
    assert_eq!(launch::log_reason(&ctx, None), None);
    assert_eq!(launch::describe(&ctx, None, None), "terminumd does not answer");
}

#[test]
fn a_loaded_agent_whose_plist_changes_beyond_the_path_is_loaded_again() {
    let home = Home::new();
    let (ctx, dir) = stubbed(&home, &[]);
    launch::start(&ctx).unwrap();
    std::fs::remove_file(dir.join("calls")).unwrap();
    std::fs::write(ctx.root.join("settings.json"), r#"{"port": 47912}"#).unwrap();
    launch::start(&ctx).unwrap();
    assert_eq!(verbs(&dir), ["bootout", "bootstrap"], "a new port needs launchd to bind it");
    assert_eq!(launch::plist_port(&ctx), Some(47912));
}

#[test]
fn restart_without_an_install_says_so() {
    let home = Home::new();
    let (ctx, dir) = stubbed(&home, &[]);
    assert!(crate::cli::install::restart(&ctx).unwrap_err().contains("not installed"));
    assert!(!dir.join("calls").exists(), "launchd was not asked");
}
