// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The control client against a private tmux server (`-L tm-test-ctl-<pid>`, never the
//! user's): answers in order, errors, notices, and the end of the process.

use super::*;
use crate::testutil::Tmux;

/// The control client of `s`'s session "main".
fn attach(s: &Tmux) -> (Control, mpsc::UnboundedReceiver<Notice>, oneshot::Receiver<String>) {
    let mut cmd = Command::new("tmux");
    cmd.args(["-C", "-u", "-L", &s.socket, "attach", "-f", "ignore-size", "-t", "main"]);
    let (ntx, nrx) = mpsc::unbounded_channel();
    let (dtx, drx) = oneshot::channel();
    (Control::spawn(cmd, ntx, dtx).unwrap(), nrx, drx)
}

#[tokio::test]
async fn answers_come_in_order_and_errors_say_why() {
    let Some(s) = Tmux::start("ctl-order", (40, 10), "/bin/sh") else { return };
    let (c, _n, _d) = attach(&s);
    let answers = c.run_all(vec!["display -p one".into(), "bogus-command".into(), "display -p three".into()]).await;
    assert_eq!(answers[0], Ok(vec!["one".to_string()]));
    assert!(answers[1].as_ref().is_err_and(|e| e.contains("unknown command")), "{:?}", answers[1]);
    assert_eq!(answers[2], Ok(vec!["three".to_string()]));
    assert!(c.attached.load(Ordering::Relaxed));
}

#[tokio::test]
async fn the_identity_is_set_once_with_one_answer_so_later_answers_stay_in_order() {
    let Some(s) = Tmux::start("ctl-identity", (40, 10), "/bin/sh") else { return };
    let (c, _n, _d) = attach(&s);
    let set = |id: &str| format!("set -sqo @terminum-id {id}");
    let answers = c.run_all(vec![set("aaa"), "show -sv @terminum-id".into(), "display -p next".into()]).await;
    assert_eq!(answers[1], Ok(vec!["aaa".to_string()]));
    assert_eq!(answers[2], Ok(vec!["next".to_string()]), "an `if -F` that sets answers twice: this was the identity");
    let again = c.run_all(vec![set("bbb"), "show -sv @terminum-id".into()]).await;
    assert_eq!(again[1], Ok(vec!["aaa".to_string()]), "a second connection keeps the first identity");
}

#[tokio::test]
async fn output_of_a_pane_is_a_notice() {
    let Some(s) = Tmux::start("ctl-notice", (40, 10), "/bin/sh") else { return };
    let (c, mut n, _d) = attach(&s);
    let pane = c.run("display -p '#{pane_id}'").await.unwrap().remove(0);
    c.run(format!("send-keys -t {pane} -l 'echo hi'")).await.unwrap();
    c.run(format!("send-keys -t {pane} Enter")).await.unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(x) = n.recv().await {
            if x == Notice::Output(pane.clone()) {
                return true;
            }
        }
        false
    })
    .await;
    assert_eq!(got, Ok(true));
    c.run("new-window -d").await.unwrap();
    let changed = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(x) = n.recv().await {
            if x == Notice::Changed {
                return true;
            }
        }
        false
    })
    .await;
    assert_eq!(changed, Ok(true), "a new window is a change");
}

#[tokio::test]
async fn a_killed_server_ends_the_client_and_its_waiters() {
    let Some(s) = Tmux::start("ctl-end", (40, 10), "/bin/sh") else { return };
    let (c, _n, done) = attach(&s);
    c.run("display -p x").await.unwrap();
    let said = c.run("display -p -t %999 x").await.err().unwrap_or_default();
    assert!(!c.no_answer(&said), "AC-132: tmux answered, with an error: {said}");
    let _ = s.tmux(&["kill-server"]);
    let why = tokio::time::timeout(Duration::from_secs(5), done).await.expect("ended").unwrap();
    assert!(!why.is_empty());
    assert!(!c.alive());
    assert!(c.no_answer(&why), "AC-132: the client's end is no answer, whatever its reason says: {why}");
    assert!(c.clean.load(Ordering::Relaxed), "kill-server is an exit on purpose");
    assert!(c.run("display -p y").await.is_err(), "no answer from a dead client");
}

/// AC-132: a slow link is not a hung client: only silence, or a command a minute old, ends it.
#[test]
fn a_client_is_hung_when_silent_or_a_command_is_very_old() {
    let s = Duration::from_secs;
    assert!(!hung(s(9), s(30)), "the command has not waited long");
    assert!(!hung(s(30), s(1)), "bytes still arrive: a slow link");
    assert!(hung(s(11), s(11)), "AC-14: nothing arrives");
    assert!(hung(s(61), s(0)), "bytes arrive, this answer never does");
}

/// AC-132: a client that keeps printing while a command waits past ANSWER_WAIT lives on; one that
/// goes quiet is killed (AC-14).
#[tokio::test]
async fn output_arriving_keeps_a_slow_client_alive_past_the_answer_wait() {
    let start = |script: &str| {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", script]);
        let (ntx, nrx) = mpsc::unbounded_channel();
        let (dtx, _drx) = oneshot::channel();
        (Control::spawn(cmd, ntx, dtx).unwrap(), nrx)
    };
    let (slow, _n1) = start("while :; do echo '%output %1 x'; sleep 1; done");
    let (quiet, _n2) = start("exec sleep 60");
    let (slow_rx, quiet_rx) = (slow.send("display -p x".into()), quiet.send("display -p x".into()));
    tokio::time::sleep(ANSWER_WAIT + Duration::from_secs(3)).await;
    assert!(slow.alive(), "bytes still arrive: not hung");
    assert!(!quiet.alive(), "nothing arrives: killed by the watchdog");
    drop((slow_rx, quiet_rx));
}

/// AC-81: what tells "ssh works, tmux silent" apart stays when ssh printed a warning first.
#[tokio::test]
async fn a_silent_link_says_so_whatever_ssh_warned() {
    let dir = crate::testutil::temp_root().join(format!("tm-silent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ssh = dir.join("my-ssh");
    std::fs::write(&ssh, "#!/bin/sh\necho 'Warning: Permanently added devbox' >&2\nexec sleep 60\n").unwrap();
    std::fs::set_permissions(&ssh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let (ntx, _nrx) = mpsc::unbounded_channel();
    let (dtx, drx) = oneshot::channel();
    let c = Control::spawn(Command::new(&ssh), ntx, dtx).unwrap();
    drop(c.send("display -p x".into()));
    let why = tokio::time::timeout(ANSWER_WAIT + Duration::from_secs(5), drx).await;
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(why.expect("the watchdog ended it").unwrap(), SSH_NO_ANSWER);
}

#[test]
fn notices_are_read_from_the_head_only() {
    assert_eq!(notice(b"%output %3 \\033[1mhi"), Some(Notice::Output("%3".into())));
    assert_eq!(notice(b"%extended-output %7 12 : x"), Some(Notice::Output("%7".into())));
    assert_eq!(notice(&[b"%output %4 ".as_slice(), &[0xe4, 0xb8]].concat()), Some(Notice::Output("%4".into())));
    assert_eq!(notice(b"%layout-change @1 a,b,c *"), Some(Notice::Changed));
    assert_eq!(notice(b"%session-renamed $1 x"), Some(Notice::Changed));
    assert_eq!(notice(b"%pause %2"), Some(Notice::Pause("%2".into())));
    assert_eq!(notice(b"%paste-buffer-changed x"), None);
}

#[test]
fn only_the_matching_end_closes_a_block() {
    assert_eq!(end_of(b"%end 1 2 1", "1 2 1"), Some(true));
    assert_eq!(end_of(b"%error 1 2 1", "1 2 1"), Some(false));
    assert_eq!(end_of(b"%end 1 3 1", "1 2 1"), None, "a captured line that looks like an end");
    assert_eq!(end_of(b"%end", "1 2 1"), None);
}

/// A link that ends before tmux answered says ssh's own last word, else that the host gave none.
#[tokio::test]
async fn an_ssh_link_that_never_reached_tmux_says_so() {
    struct Gone(std::path::PathBuf);
    impl Drop for Gone {
        fn drop(&mut self) { std::fs::remove_dir_all(&self.0).ok(); }   // also when an assert fails
    }
    let dir = crate::testutil::temp_root().join(format!("tm-ctl-ssh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _gone = Gone(dir.clone());
    for (name, body, want) in [
        ("ssh", "exit 255", "ssh: no answer from the host"),
        ("my-ssh", "echo 'Connection timed out during banner exchange' >&2; exit 255", "Connection timed out during banner exchange"),
        // stdout ends before the words reach stderr (as they raced on a loaded Linux runner)
        ("late-ssh", "exec >&-; sleep 0.3; echo 'Permission denied (publickey).' >&2; exit 255", "Permission denied (publickey)."),
        ("tmux-ish", "exit 1", "tmux exited"),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let mut tries = 0;
        let (_c, _nrx, drx) = loop {
            let (ntx, nrx) = mpsc::unbounded_channel();
            let (dtx, drx) = oneshot::channel();
            match Control::spawn(Command::new(&path), ntx, dtx) {
                // Linux refuses to run a file another test's fork still holds open for writing (ETXTBSY): again
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 100 => {
                    tries += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                r => break (r.unwrap(), nrx, drx),
            }
        };
        assert_eq!(drx.await.unwrap(), want, "{name}");
    }
}

#[tokio::test]
async fn commands_past_the_bound_fail_at_once_while_stdin_is_full() {
    // a client that never reads its stdin: the writer blocks once the pipe is full
    let mut cmd = Command::new("/bin/sleep");
    cmd.arg("30");
    let (ntx, _nrx) = mpsc::unbounded_channel();
    let (dtx, _drx) = oneshot::channel();
    let c = Control::spawn(cmd, ntx, dtx).unwrap();
    let line = format!("display -p {}", "x".repeat(64 << 10));
    let waiting: Vec<_> = (0..(LINES_MAX / line.len()) + 64).map(|_| c.send(line.clone())).collect();
    let refused = c.send(line.clone()).await.unwrap();
    assert_eq!(refused, Err("tmux is not keeping up: too many commands wait for it".into()));
    assert!(c.queued.load(Ordering::Relaxed) <= LINES_MAX, "queued bytes stay bounded");
    drop(waiting);
}
