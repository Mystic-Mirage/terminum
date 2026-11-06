// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;

#[test]
fn platforms_with_a_helper() {
    assert_eq!(platform("Darwin", "arm64"), Some("macos-aarch64"));
    assert_eq!(platform("Linux", "aarch64"), Some("linux-aarch64"));
    assert_eq!(platform("FreeBSD", "amd64"), None);
}

#[test]
fn only_this_builds_helper_is_checked_through_the_forward() {
    // AC-84: an older helper is replaced, never held to the forward's check (it failed
    // there and was retried with a backoff instead of updated)
    assert!(current(crate::agent::AGENT_ID));
    assert!(!current("0123456789ab"), "another build");
    assert!(!current(""), "no helper on the host");
}

#[test]
fn the_helper_answer_is_the_last_non_empty_line() {
    assert_eq!(answer("Welcome to devbox\nlast login: today\n0123456789ab\n"), "0123456789ab", "rc noise before");
    assert_eq!(answer("noise\n0123456789ab\n\n  \n"), "0123456789ab", "a trailing blank line");
    assert_eq!(answer("noise\r\n0123456789ab\r\n\r\n"), "0123456789ab", "CRLF");
    assert_eq!(answer(""), "");
}

#[tokio::test]
async fn a_forward_counts_only_when_the_helper_answers() {
    let dir = crate::testutil::temp_root().join(format!("tm-fw-{}", crate::auth::crypto::random_hex::<4>()));
    std::fs::create_dir_all(&dir).unwrap();
    let (up, refused) = (dir.join("up.sock"), dir.join("no.sock"));
    let (l1, l2) = (tokio::net::UnixListener::bind(&up).unwrap(), tokio::net::UnixListener::bind(&refused).unwrap());
    tokio::spawn(async move {
        let (mut s, _) = l1.accept().await.unwrap();
        let mut b = [0u8; 64];
        let _ = s.read(&mut b).await;
        let _ = s.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
    });
    // an sshd refusing the channel: ssh accepted on the Mac, then closes it
    tokio::spawn(async move { drop(l2.accept().await.unwrap()) });
    assert_eq!(answers(tokio::net::UnixStream::connect(&up).await.unwrap()).await, Ok(()));
    assert!(answers(tokio::net::UnixStream::connect(&refused).await.unwrap()).await.is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Issue #8 (AC-83): what reaches the host's login shell, run by zsh as sshd does (`$SHELL -c`):
/// the helper's removal with no old helper to match, which zsh's nomatch aborted.
#[test]
fn a_zsh_login_shell_runs_the_helpers_scripts_as_sh() {
    if std::process::Command::new("zsh").arg("-c").arg("true").output().is_err() {
        return eprintln!("skipped: zsh is not on PATH");
    }
    let home = crate::testutil::fresh_dir("zsh-host");
    std::fs::create_dir_all(home.join(".terminum/bin")).unwrap();
    std::fs::write(home.join(".terminum/bin/terminumd-agent-0123abcd"), "x").unwrap();
    let mut c = Command::new("ssh");
    with_command(&mut c, &["alex@devbox.example".to_string()], &remove_script());
    let remote = c.as_std().get_args().last().unwrap().to_str().unwrap().to_string();
    assert!(remote.starts_with("exec /bin/sh -c '"), "{remote}");
    let out = std::process::Command::new("zsh").arg("-c").arg(&remote).env("HOME", &home).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!home.join(".terminum").exists(), "the helper and its folders went");
    // as it was: the script itself in zsh stops at the first glob that matches nothing
    std::fs::create_dir_all(home.join(".terminum/bin")).unwrap();
    let old = std::process::Command::new("zsh").arg("-c").arg(remove_script()).env("HOME", &home).output().unwrap();
    assert!(String::from_utf8_lossy(&old.stderr).contains("no matches found"), "{old:?}");
    let _ = std::fs::remove_dir_all(&home);
}
