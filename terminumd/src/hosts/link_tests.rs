// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;
use std::path::PathBuf;

#[test]
fn ssh_args_forward_a_socket_without_mux_or_exit_on_failure() {
    assert_eq!(ssh_args(None), ["-o", "ExitOnForwardFailure=yes"], "agents off: files' ssh as before");
    let a = ssh_args(Some(("/home/alex/.terminum/run/m-abc-12345678.sock", &PathBuf::from("/Users/alex/.terminum/links/h.sock"))));
    assert_eq!(a, ["-o", "ControlPath=none", "-o", "ExitOnForwardFailure=no", "-R", "/home/alex/.terminum/run/m-abc-12345678.sock:/Users/alex/.terminum/links/h.sock"]);
    assert!(!a.iter().any(|s| s.contains("ClearAllForwardings")));
    assert_eq!(helper_args("/r/m.sock", "abc"), " --link '/r/m.sock' --mac 'abc'", "single-quoted words");
    assert_eq!(helper_args("/r/it's", "abc"), " --link '/r/it'\\''s' --mac 'abc'");
}

#[test]
fn remote_and_local_paths_are_checked() {
    assert_eq!(remote_path("/home/alex/", "abc", "12345678").unwrap(), "/home/alex/.terminum/run/m-abc-12345678.sock");
    let long = format!("/{}", "h".repeat(70));
    assert!(remote_path(&long, "abc", "12345678").unwrap_err().contains("longer than 100"));
    assert!(remote_path("relative", "abc", "12345678").is_err());
    assert!(remote_path("/a:b", "abc", "12345678").is_err());
    for home in ["/home/alex smith", "/home/$USER", "/home/a;b", "/home/a'b", "/home/a\nb"] {
        let e = remote_path(home, "abc", "12345678").unwrap_err();
        assert!(e.contains("characters terminum cannot pass"), "{home:?}: {e}");
    }
    assert!(remote_path("/home/alex.b_c-1", "abc", "12345678").is_ok());
    assert!(local_ok(&PathBuf::from("/Users/alex/.terminum/links/x.sock")).is_ok());
    assert!(local_ok(&PathBuf::from(format!("/{}", "x".repeat(103)))).unwrap_err().contains("103"));
    assert!(local_ok(&PathBuf::from("x.sock")).is_err());
}

#[test]
fn the_probe_reads_after_its_marker_whatever_rc_files_print() {
    let m = "tm-probe-0123456789ab";
    let out = format!("Welcome to devbox!\nLinux is great\n{m}\nLinux x86_64\n/home/alex\n");
    assert_eq!(parse_probe(&out, m), Some(("Linux".into(), "x86_64".into(), "/home/alex".into())));
    assert_eq!(parse_probe(&format!("{m}\r\nDarwin arm64\r\n/Users/alex\r\n"), m), Some(("Darwin".into(), "arm64".into(), "/Users/alex".into())));
    assert_eq!(parse_probe("Linux x86_64\n/home/alex\n", m), None, "no marker: no answer");
    assert_eq!(parse_probe(&format!("{m}\nLinux\n"), m), None);
    assert!(probe_command(m).starts_with(&format!("echo {m};")));
}

#[test]
fn link_lines_and_retry_timing() {
    assert_eq!(said("terminumd-agent link: up"), Some(Said::Up));
    assert_eq!(said("terminumd-agent link: refused"), Some(Said::Refused));
    assert_eq!(said("terminumd-agent ready x"), None);
    let now = Instant::now();
    assert!(try_forward(None, 0, now));
    assert!(!try_forward(Some(now), 1, now + Duration::from_secs(599)));
    assert!(try_forward(Some(now), 1, now + RETRY));
    // refused again and again: 10, 20, 40, 80, then 160 minutes for good
    assert!(!try_forward(Some(now), 2, now + RETRY));
    assert!(try_forward(Some(now), 2, now + RETRY * 2));
    assert_eq!((1..=7).map(|n| backoff(n).as_secs() / 60).collect::<Vec<_>>(), [10, 20, 40, 80, 160, 160, 160]);
    assert!(refused_why("devbox.example").contains("devbox.example's sshd"));
}

#[test]
fn the_mac_id_is_made_once_and_kept() {
    let d = crate::testutil::temp_root().join(format!("tm-macid-{}", crate::auth::crypto::random_hex::<4>()));
    std::fs::create_dir_all(&d).unwrap();
    let a = mac_id(&d).unwrap();
    assert_eq!(a.len(), 12);
    assert_eq!(mac_id(&d).unwrap(), a);
    std::fs::remove_dir_all(&d).unwrap();
    // an id that cannot be kept is refused, never a new one per connection
    let e = mac_id(&d.join("missing")).unwrap_err();
    assert!(e.contains("could not be kept") && e.contains("mac-id"), "{e}");
}

#[test]
fn retry_is_scheduled_only_for_a_refused_wanted_link() {
    let now = Instant::now();
    let t = now - Duration::from_secs(100);
    assert_eq!(retry_in(true, "refused: no -R", Some(t), 1, now), Some(RETRY - Duration::from_secs(100)));
    assert_eq!(retry_in(true, "refused: no -R", Some(t), 3, now), Some(RETRY * 4 - Duration::from_secs(100)), "backed off");
    assert_eq!(retry_in(true, "refused: no -R", Some(now - RETRY - RETRY), 1, now), Some(Duration::ZERO));
    assert_eq!(retry_in(true, "up", Some(t), 1, now), None, "a link that came up is not cut");
    assert_eq!(retry_in(false, "refused: no -R", Some(t), 1, now), None, "agents off: Files is not cut");
    assert_eq!(retry_in(true, "refused: no -R", None, 0, now), None);
}

#[tokio::test(start_paused = true)]
async fn the_scheduler_ends_a_healthy_connection_at_the_retry_time() {
    // a Files connection that never closes on its own: the retry time ends the wait
    let start = tokio::time::Instant::now();
    assert!(closed_or_retry(std::future::pending::<()>(), Some(RETRY)).await);
    assert_eq!(start.elapsed(), RETRY);
    // a connection that closes first is not a retry
    assert!(!closed_or_retry(async {}, Some(RETRY)).await);
    assert!(!closed_or_retry(tokio::time::sleep(Duration::from_secs(5)), None).await);
}
