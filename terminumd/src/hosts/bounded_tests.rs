// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use std::process::Stdio;
use std::time::Duration;

use tokio::io::BufReader;
use tokio::process::Command;

use super::*;

fn sh(script: &str) -> tokio::process::Child {
    Command::new("/bin/sh").args(["-c", script]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).spawn().unwrap()
}

#[tokio::test]
async fn output_keeps_a_small_answer_and_the_stderr_tail() {
    let mut c = sh("echo answer; head -c 20000 /dev/zero | tr '\\0' e >&2; echo last-words >&2; exit 3");
    let o = output(&mut c, ANSWER_MAX, Duration::from_secs(10)).await.unwrap();
    assert_eq!(o.stdout, b"answer\n");
    assert_eq!(o.stderr.len(), ERR_TAIL, "only the tail of stderr is kept");
    assert!(o.stderr.ends_with(b"last-words\n"));
    assert_eq!(o.status.code(), Some(3));
}

#[tokio::test]
async fn output_over_the_cap_fails_and_ends_the_child() {
    // a host printing forever: the cap, not the timeout, ends it
    let mut c = sh("yes flood");
    let t = std::time::Instant::now();
    let e = output(&mut c, 64 * 1024, Duration::from_secs(30)).await.err().unwrap();
    assert!(e.contains("larger than 64 KiB"), "{e}");
    assert!(t.elapsed() < Duration::from_secs(10));
    assert!(c.try_wait().unwrap().is_some(), "the child was ended");
}

#[tokio::test]
async fn output_past_the_time_fails_and_ends_the_child() {
    let mut c = sh("sleep 30");
    let e = output(&mut c, ANSWER_MAX, Duration::from_millis(300)).await.err().unwrap();
    assert!(e.contains("timed out"), "{e}");
    assert!(c.try_wait().unwrap().is_some(), "the child was ended");
}

#[tokio::test]
async fn line_reads_lines_and_refuses_one_without_end() {
    let mut r = BufReader::new(&b"one\r\ntwo\nlast"[..]);
    assert_eq!(line(&mut r, 16).await.unwrap().as_deref(), Some("one"));
    assert_eq!(line(&mut r, 16).await.unwrap().as_deref(), Some("two"));
    assert_eq!(line(&mut r, 16).await.unwrap().as_deref(), Some("last"));
    assert_eq!(line(&mut r, 16).await.unwrap(), None);
    // exactly the cap passes, one byte more does not
    let mut r = BufReader::new(&b"0123456789abcdef\n0123456789abcdefX\n"[..]);
    assert_eq!(line(&mut r, 16).await.unwrap().as_deref(), Some("0123456789abcdef"));
    assert!(line(&mut r, 16).await.is_err());
    // a stream with no newline at all stops at the cap, not at its end
    let mut c = sh("yes | tr -d '\\n'");
    let mut out = BufReader::new(c.stdout.take().unwrap());
    let e = line(&mut out, LINE_MAX).await.err().unwrap();
    assert!(e.contains("longer than 4 KiB"), "{e}");
}
