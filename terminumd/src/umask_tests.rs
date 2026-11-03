// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! AC-09: terminumd's umask 077 stays its own; the tmux it starts and Files' new entries get the
//! umask it was started with (027 here: neither a test process's 022 nor terminumd's 077).

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::ops;
use crate::files::Roots;

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn files_new_entries_keep_the_started_umask() {
    super::set_started(0o027);
    let dir = crate::testutil::fresh_dir("umask-ops");
    let roots = Roots::parse(&dir.display().to_string());
    assert_eq!(mode(&ops::touch(&roots, &dir, "new.txt").unwrap()), 0o640);
    ops::mkdir(&roots, &dir, "a/b").unwrap();
    assert_eq!((mode(&dir.join("a")), mode(&dir.join("a/b"))), (0o750, 0o750), "every folder made");
    std::fs::create_dir(dir.join("private")).unwrap();
    std::fs::set_permissions(dir.join("private"), std::fs::Permissions::from_mode(0o700)).unwrap();
    ops::mkdir(&roots, &dir, "private/c/d").unwrap();
    assert_eq!([mode(&dir.join("private")), mode(&dir.join("private/c")), mode(&dir.join("private/c/d"))], [0o700, 0o750, 0o750],
        "a folder that was there keeps its mode");
    assert_eq!(mode(&ops::upload(&roots, &dir, "dropped.txt", b"x", false).unwrap()), 0o640);
    ops::save(&roots, &dir.join("saved.txt"), "x", "*").unwrap();
    assert_eq!(mode(&dir.join("saved.txt")), 0o640, "a new file saved");
    let kept = dir.join("private.txt");
    std::fs::write(&kept, "x").unwrap();
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o600)).unwrap();
    let etag = crate::files::etag(&std::fs::metadata(&kept).unwrap());
    ops::save(&roots, &kept, "y", &etag).unwrap();
    assert_eq!(mode(&kept), 0o600, "a file saved over keeps its own mode");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn the_tmux_it_starts_gives_its_terminals_the_started_umask() {
    if std::process::Command::new("tmux").arg("-V").output().is_err() {
        return eprintln!("skipped: tmux is not on PATH");
    }
    super::set_started(0o027);
    let socket = format!("tm-test-umask-{}", std::process::id());
    let dir = std::fs::canonicalize(crate::testutil::fresh_dir("umask-tmux")).unwrap();
    // ends the server and removes the folder, also when an assertion fails
    let _server = crate::testutil::Tmux { socket: socket.clone(), dir: dir.clone() };
    let said = dir.join("umask");
    let shell = format!("umask > {}; sleep 30", said.display());
    let out = crate::tmux::server::plain(Path::new("tmux")).args(["-u", "-L", &socket, "-f", "/dev/null", "new-session", "-d", "-s", "main", &shell])
        .output().await.unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    for _ in 0..100 {
        if std::fs::read_to_string(&said).is_ok_and(|s| !s.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    // sh says 0027, zsh 027
    let said = std::fs::read_to_string(&said).unwrap_or_default();
    assert_eq!(u32::from_str_radix(said.trim(), 8), Ok(0o027), "{said}");
}
