// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The update notice (AC-17), ported from iterm-enhancer's updates.py. Once a day by the wall
//! clock (a sleeping Mac does not stretch it; a failed try repeats at the next hourly tick)
//! terminumd asks GitHub which release is the latest: one HEAD request for
//! `<releases>/latest`, whose redirect names the tag; nothing of the user's is sent. Only a
//! `vMAJOR.MINOR.PATCH` tag is kept, so no other text from the network reaches a page. A build
//! that is not a release never asks; `state/no-update-check` stops the check and
//! `state/update-skip` hides one version (the chip's menu writes both).
//!
//! The request runs `/usr/bin/curl`: the tree has no TLS crate (hyper here is plain HTTP/1)
//! and one HEAD request a day does not justify adding rustls and its certificate handling;
//! curl uses the system's trust store.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

use serde_json::{Value, json};

pub const RELEASES: &str = "https://github.com/extractumio/terminum/releases";
/// Seconds after start before the first check, between ticks, and between successful checks.
pub const FIRST_S: u64 = 30;
pub const TICK_S: u64 = 3600;
pub const EVERY_S: f64 = 24.0 * 3600.0;
const OFF: &str = "no-update-check";
const SKIP: &str = "update-skip";
const CHECKED: &str = "update-checked";

/// `vMAJOR.MINOR.PATCH` with 1–6 digits each, as numbers; anything else is None.
pub fn version(tag: &str) -> Option<(u32, u32, u32)> {
    let mut parts = tag.strip_prefix('v')?.split('.');
    let mut n = || parts.next().filter(|p| (1..=6).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))?.parse().ok();
    let v = (n()?, n()?, n()?);
    parts.next().is_none().then_some(v)
}

/// Whether `latest` is a release newer than `current` (a checkout's build is never older).
pub fn newer(latest: Option<&str>, current: &str) -> bool {
    matches!((latest.and_then(version), version(current)), (Some(a), Some(b)) if a > b)
}

/// The tag the `Location` of a HEAD answer names, from curl's `-sI` output. Err when the
/// answer is not a redirect (a missing page, rate limited); Ok(None) when the tag is no release.
pub fn tag_from_headers(head: &str) -> Result<Option<String>, String> {
    let mut blocks = head.split("\r\n\r\n").filter(|b| !b.trim().is_empty());
    let block = blocks.next().ok_or("no answer")?;
    let mut lines = block.lines();
    let status = lines.next().and_then(|l| l.split_whitespace().nth(1)).ok_or("no status line")?;
    if !status.starts_with('3') {
        return Err(format!("status {}", status.chars().take(3).collect::<String>()));
    }
    let location = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("location"))
        .map(|(_, v)| v.trim())
        .ok_or("no location")?;
    let tag = location.split(['?', '#']).next().unwrap_or("").trim_end_matches('/').rsplit('/').next().unwrap_or("");
    Ok(version(tag).map(|_| tag.to_string()))
}

/// The latest release tag, by one HEAD request (no redirect followed, 10 s at most).
pub fn latest_tag() -> Result<Option<String>, String> {
    let out = Command::new("/usr/bin/curl")
        .args(["-sI", "--max-time", "10", "--proto", "=https", "-A", "terminum-update-check", &format!("{RELEASES}/latest")])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl exited {}", out.status.code().unwrap_or(-1)));
    }
    tag_from_headers(&String::from_utf8_lossy(&out.stdout))
}

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// The check's state: the newest release seen and when, kept across restarts.
#[derive(Debug)]
pub struct Updates {
    dir: PathBuf,
    current: String,
    pub latest: Option<String>,
    pub checked: f64,
}

impl Updates {
    /// `dir` is the state folder; `current` this build's id ("v0.3.0", or "dev" from a checkout).
    pub fn new(dir: &Path, current: &str) -> Updates {
        let mut u = Updates { dir: dir.to_path_buf(), current: current.to_string(), latest: None, checked: 0.0 };
        if let Some(seen) = fs::read_to_string(dir.join(CHECKED)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
            u.latest = seen["latest"].as_str().filter(|t| version(t).is_some()).map(str::to_string);
            u.checked = seen["at"].as_f64().unwrap_or(0.0);
        }
        u
    }

    fn off(&self) -> bool {
        self.dir.join(OFF).exists()
    }

    /// What pages show: the newer release, unless skipped or the check is off.
    pub fn notice(&self) -> Option<String> {
        let skipped = fs::read_to_string(self.dir.join(SKIP)).unwrap_or_default();
        let latest = self.latest.as_deref()?;
        (!self.off() && newer(Some(latest), &self.current) && skipped.trim() != latest).then(|| latest.to_string())
    }

    /// Whether a tick at `now` should ask: a release build, checking on, a day since the last answer.
    pub fn due(&self, now: f64) -> bool {
        version(&self.current).is_some() && !self.off() && now - self.checked >= EVERY_S
    }

    /// One check with `fetch` (the network); a failure only logs and leaves `checked`, so the
    /// next tick tries again. Returns whether it asked.
    pub fn check(&mut self, now: f64, fetch: impl FnOnce() -> Result<Option<String>, String>) -> bool {
        if version(&self.current).is_none() || self.off() {
            return false;
        }
        match fetch() {
            Err(e) => {
                tracing::info!(event = "update.failed", msg = %e.chars().take(200).collect::<String>());
            }
            Ok(tag) => {
                self.latest = tag.filter(|t| version(t).is_some());
                self.checked = now;
                let body = json!({"at": now, "latest": self.latest}).to_string();
                if let Err(e) = crate::settings::write_atomic(&self.dir.join(CHECKED), body.as_bytes()) {
                    tracing::warn!(event = "update.unsaved", msg = %e);
                }
            }
        }
        true
    }

    /// The chip's menu: "skip" a version, "off", or "on" (the caller checks again after "on").
    pub fn choose(&mut self, action: &str, tag: Option<&str>) -> Result<(), String> {
        let res = match (action, tag) {
            ("skip", Some(t)) if version(t).is_some() => crate::settings::write_atomic(&self.dir.join(SKIP), t.as_bytes()),
            ("skip", _) => return Err("Skip needs a release such as v0.4.0".into()),
            ("off", _) => crate::settings::write_atomic(&self.dir.join(OFF), b""),
            ("on", _) => crate::config::remove_if_there(&self.dir.join(OFF)),
            _ => return Err("skip a version, or switch checking off or on".into()),
        };
        if action == "on" {
            self.checked = 0.0;
        }
        res.map_err(|e| format!("The update choice was not saved: {e}"))
    }
}

/// The daily loop: after `first` (`FIRST_S`), every `tick` (`TICK_S`) a due check runs `fetch`
/// (`latest_tag`) off the async workers and `notify` hears the notice whenever it changes.
pub async fn follow<F, N>(updates: Arc<Mutex<Updates>>, first: Duration, tick: Duration, fetch: F, notify: N)
where
    F: Fn() -> Result<Option<String>, String> + Send + Sync + Clone + 'static,
    N: Fn(Option<String>) + Send + 'static,
{
    tokio::time::sleep(first).await;
    let mut told: Option<Option<String>> = None;
    loop {
        if updates.lock().due(now()) {
            let fetch = fetch.clone();
            if let Ok(answer) = tokio::task::spawn_blocking(fetch).await {
                updates.lock().check(now(), || answer);
            }
        }
        let notice = updates.lock().notice();
        if told.as_ref() != Some(&notice) {
            notify(notice.clone());
            told = Some(notice);
        }
        tokio::time::sleep(tick).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn dir(name: &str) -> PathBuf {
        crate::testutil::fresh_dir(&format!("update-{name}"))
    }

    #[test]
    fn versions_compare_as_numbers_and_only_releases_count() {
        assert!(newer(Some("v0.19.0"), "v0.18.0"));
        assert!(newer(Some("v0.10.0"), "v0.9.0"), "not as text");
        assert!(newer(Some("v1.0.0"), "v0.99.99"));
        assert!(!newer(Some("v0.18.0"), "v0.18.0"));
        assert!(!newer(Some("v0.17.0"), "v0.18.0"));
        for current in ["dev", "30fa800-a13657b1", ""] {
            assert!(!newer(Some("v9.0.0"), current), "{current}");
        }
        for latest in ["v1.2", "v1.2.3-rc1", "<b>v1.2.3", "v1.2.3\n", "", "v1.2.3.4", "v1..3", "v1234567.0.0", "v-1.0.0", "1.2.3"] {
            assert!(!newer(Some(latest), "v0.1.0"), "{latest:?}");
        }
        assert!(!newer(None, "v0.1.0"));
        assert_eq!(version("v10.2.345"), Some((10, 2, 345)));
    }

    #[test]
    fn the_tag_comes_from_the_redirect_only() {
        let head = |status: &str, loc: &str| format!("HTTP/2 {status}\r\ndate: x\r\nLocation: {loc}\r\ncontent-length: 0\r\n\r\n");
        let base = "https://github.com/extractumio/terminum/releases/tag";
        assert_eq!(tag_from_headers(&head("302", &format!("{base}/v0.19.0"))), Ok(Some("v0.19.0".into())));
        assert_eq!(tag_from_headers(&head("301", &format!("{base}/v0.19.0/"))), Ok(Some("v0.19.0".into())));
        for bad in ["nightly", "v1.2.3-rc1", "v1.2.3%3Cb%3E", "<script>"] {
            assert_eq!(tag_from_headers(&head("302", &format!("{base}/{bad}"))), Ok(None), "{bad}");
        }
        assert_eq!(tag_from_headers("HTTP/1.1 302 Found\r\nlocation: /rel/tag/v1.0.0?x=1\r\n\r\n"), Ok(Some("v1.0.0".into())));
        assert!(tag_from_headers("HTTP/2 404\r\nserver: x\r\n\r\n").unwrap_err().contains("404"));
        assert!(tag_from_headers("HTTP/2 429\r\n\r\n").is_err());
        assert!(tag_from_headers("HTTP/2 302\r\nserver: x\r\n\r\n").is_err(), "no location");
        assert!(tag_from_headers("").is_err());
    }

    #[test]
    fn a_newer_release_is_noticed_and_the_same_or_older_is_not() {
        let d = dir("newer");
        let mut u = Updates::new(&d, "v0.18.0");
        assert!(u.check(1e6, || Ok(Some("v0.19.0".into()))));
        assert_eq!(u.notice().as_deref(), Some("v0.19.0"));
        for latest in ["v0.18.0", "v0.17.0"] {
            u.check(1e6, || Ok(Some(latest.into())));
            assert_eq!(u.notice(), None, "{latest}");
        }
        u.check(1e6, || Ok(Some("<b>v9.0.0".into())));
        assert_eq!((u.latest.clone(), u.notice()), (None, None), "no text but a tag is kept");
    }

    #[test]
    fn a_build_that_is_not_a_release_never_asks() {
        let d = dir("dev");
        for build in ["dev", "30fa800-a13657b1"] {
            let mut u = Updates::new(&d, build);
            let asked = Cell::new(false);
            assert!(!u.due(1e9));
            assert!(!u.check(1e9, || {
                asked.set(true);
                Ok(None)
            }));
            assert!(!asked.get());
        }
    }

    #[test]
    fn skip_hides_that_version_but_not_the_next_and_a_bad_skip_is_refused() {
        let d = dir("skip");
        let mut u = Updates::new(&d, "v0.18.0");
        u.check(1e6, || Ok(Some("v0.19.0".into())));
        u.choose("skip", Some("v0.19.0")).unwrap();
        assert_eq!(u.notice(), None);
        u.latest = Some("v0.20.0".into());
        assert_eq!(u.notice().as_deref(), Some("v0.20.0"));
        assert!(u.choose("skip", Some("../../etc/passwd")).is_err());
        assert_eq!(fs::read_to_string(d.join(SKIP)).unwrap(), "v0.19.0");
        assert!(u.choose("nuke", None).is_err());
    }

    #[test]
    fn off_stops_the_network_and_on_checks_again() {
        let d = dir("off");
        let mut u = Updates::new(&d, "v0.18.0");
        u.check(1e6, || Ok(Some("v0.19.0".into())));
        u.choose("off", None).unwrap();
        assert!(d.join(OFF).exists());
        assert_eq!(u.notice(), None);
        assert!(!u.due(1e9));
        assert!(!u.check(1e9, || panic!("no request while off")));
        u.choose("on", None).unwrap();
        assert!(u.due(1e6 + 1.0), "on: due at once");
        assert_eq!(u.notice().as_deref(), Some("v0.19.0"));
        u.choose("on", None).unwrap();
    }

    #[test]
    fn once_a_day_by_the_wall_clock_a_failure_retries_and_restarts_remember() {
        let d = dir("daily");
        let mut u = Updates::new(&d, "v0.18.0");
        assert!(u.due(1e6));
        u.check(1e6, || Err("offline".into()));
        assert!(u.due(1e6 + 3600.0), "a failed try repeats at the next tick");
        u.check(1e6 + 3600.0, || Ok(Some("v0.19.0".into())));
        assert!(!u.due(1e6 + 7200.0), "not again within the day");
        assert!(u.due(1e6 + 3600.0 + EVERY_S));
        let restarted = Updates::new(&d, "v0.18.0");
        assert_eq!((restarted.latest.as_deref(), restarted.checked), (Some("v0.19.0"), 1e6 + 3600.0), "kept across a restart");
        assert_eq!(restarted.notice().as_deref(), Some("v0.19.0"));
        assert!(!restarted.due(1e6 + 7200.0));
        fs::write(d.join(CHECKED), r#"{"at": 5, "latest": "<script>"}"#).unwrap();
        assert_eq!(Updates::new(&d, "v0.18.0").latest, None);
    }

    #[tokio::test]
    async fn follow_checks_after_the_first_wait_and_tells_changes() {
        let d = dir("follow");
        let u = Arc::new(Mutex::new(Updates::new(&d, "v0.18.0")));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let ms = Duration::from_millis(5);
        let task = tokio::spawn(follow(u.clone(), ms, ms, || Ok(Some("v0.19.0".to_string())), move |n| tx.send(n).unwrap()));
        assert_eq!(rx.recv().await.unwrap().as_deref(), Some("v0.19.0"));
        u.lock().choose("skip", Some("v0.19.0")).unwrap();
        assert_eq!(rx.recv().await.unwrap(), None, "the next tick tells the skip");
        task.abort();
    }
}
