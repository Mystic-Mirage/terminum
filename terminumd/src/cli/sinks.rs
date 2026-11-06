// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Where `terminum hook` and `terminum mcp` post, on any machine: the local terminumd's private
//! socket first, then every Mac's link in `~/.terminum/run` (a host's `terminum-host`, whose
//! Macs forward their hook socket there; runlink.rs checks the folder and the sockets).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::sockhttp::{self, Down};
use crate::plans::rules::NOT_IN_PANE;

/// One answer of a sink: (status, body) or Down.
pub type Answer = Result<(u16, Option<Value>), Down>;

/// The local terminumd's socket, found without creating a folder.
fn local() -> PathBuf {
    let dir = std::env::var_os("TM_APP_DIR").map(PathBuf::from)
        .unwrap_or_else(|| crate::runlink::dir().parent().map_or_else(|| PathBuf::from("/"), Path::to_path_buf).join("state"));
    crate::sock::path(&dir)
}

/// This machine's sinks: the local socket, then the run folder's links in name order.
pub fn all() -> Vec<PathBuf> {
    list(local(), &crate::runlink::dir())
}

/// `local`, then the links of `run`; one socket reached two ways is posted to once.
pub fn list(local: PathBuf, run: &Path) -> Vec<PathBuf> {
    let mut out = vec![local];
    let mut seen: Vec<PathBuf> = out.iter().map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())).collect();
    for l in crate::runlink::links(run) {
        let c = std::fs::canonicalize(&l).unwrap_or_else(|_| l.clone());
        if !seen.contains(&c) {
            seen.push(c);
            out.push(l);
        }
    }
    out
}

/// `body` POSTed to `path` of every sink at once, each within `budget`: the answers, in
/// the sinks' order.
pub fn post_all(sinks: &[PathBuf], path: &str, body: &Value, budget: Duration) -> Vec<Answer> {
    std::thread::scope(|s| {
        let calls: Vec<_> = sinks.iter().map(|p| s.spawn(move || sockhttp::call(p, "POST", path, Some(body), budget))).collect();
        calls.into_iter().map(|c| c.join().unwrap_or(Err(Down))).collect()
    })
}

/// An answer that says the sink does not know the pane.
pub fn not_in_pane(a: &(u16, Option<Value>)) -> bool {
    a.1.as_ref().and_then(|v| v["text"].as_str()) == Some(NOT_IN_PANE)
}

/// The sinks asked in order, `ask` answering for one: the first answer of a sink that is up
/// and knows the pane; else the last "not in a pane", else Down.
pub fn first_knowing(sinks: &[PathBuf], ask: impl Fn(&Path) -> Answer) -> Answer {
    let mut last = Err(Down);
    for p in sinks {
        match ask(p) {
            Err(Down) => {}
            Ok(a) if not_in_pane(&a) => last = Ok(a),
            Ok(a) => return Ok(a),
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::os::unix::net::UnixListener;

    use serde_json::json;

    use super::*;

    #[test]
    fn local_first_then_links_by_name_each_once() {
        let base = crate::testutil::temp_root().join(format!("tmsk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let run = base.join("run");
        crate::runlink::check_dir(&run, true).unwrap();
        let (a, b) = (run.join("m-a-1.sock"), run.join("m-b-1.sock"));
        let (_la, _lb) = (UnixListener::bind(&a).unwrap(), UnixListener::bind(&b).unwrap());
        crate::runlink::link(&run, "b", &b).unwrap();
        crate::runlink::link(&run, "a", &a).unwrap();
        let local = base.join("terminumd.sock");
        assert_eq!(list(local.clone(), &run), vec![local.clone(), a.clone(), b.clone()]);
        assert_eq!(list(a.clone(), &run), vec![a.clone(), b.clone()], "the local socket reached through a link: once");
        assert_eq!(list(local.clone(), &base.join("none")), vec![local.clone()], "no run folder: the local socket only");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_first_sink_that_knows_the_pane_answers() {
        let sinks: Vec<PathBuf> = ["l", "m1", "m2", "m3"].iter().map(PathBuf::from).collect();
        let asked = RefCell::new(Vec::new());
        let r = first_knowing(&sinks, |p| {
            asked.borrow_mut().push(p.to_path_buf());
            match p.to_str() {
                Some("l") => Err(Down),
                Some("m1") => Ok((200, Some(json!({"text": NOT_IN_PANE, "error": true})))),
                _ => Ok((200, Some(json!({"text": format!("from {}", p.display())})))),
            }
        });
        assert_eq!(r.unwrap().1.unwrap()["text"], "from m2");
        assert_eq!(asked.into_inner(), sinks[..3].to_vec(), "m3 never asked");
        let none = first_knowing(&sinks, |_| Ok((200, Some(json!({"text": NOT_IN_PANE, "error": true})))));
        assert!(not_in_pane(&none.unwrap()), "nobody knows it: not in a pane");
        assert!(first_knowing(&sinks, |_| Err(Down)).is_err(), "all down: down");
    }

    #[test]
    fn nothing_listening_answers_down_at_once() {
        let sinks = vec![PathBuf::from("/nonexistent/a.sock"), PathBuf::from("/nonexistent/b.sock")];
        let t = std::time::Instant::now();
        let r = post_all(&sinks, "/hook", &json!({}), Duration::from_millis(150));
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(Result::is_err) && t.elapsed() < Duration::from_millis(150));
    }
}
