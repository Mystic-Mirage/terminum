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
    let _ = s.tmux(&["kill-server"]);
    let why = tokio::time::timeout(Duration::from_secs(5), done).await.expect("ended").unwrap();
    assert!(!why.is_empty());
    assert!(!c.alive());
    assert!(c.clean.load(Ordering::Relaxed), "kill-server is an exit on purpose");
    assert!(c.run("display -p y").await.is_err(), "no answer from a dead client");
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
    let dir = std::env::temp_dir().join(format!("tm-ctl-ssh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _gone = Gone(dir.clone());
    for (name, body, want) in [
        ("ssh", "exit 255", "ssh: no answer from the host"),
        ("my-ssh", "echo 'Connection timed out during banner exchange' >&2; exit 255", "Connection timed out during banner exchange"),
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
