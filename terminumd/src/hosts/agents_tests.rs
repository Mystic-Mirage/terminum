// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;
use parking_lot::Mutex;
use serde_json::json;

/// The host's answers given, every step recorded; no ssh.
struct Fake {
    hosts: Mutex<Map<String, Value>>,
    install: Result<Got, String>,
    remove: Result<Got, String>,
    steps: Mutex<Vec<&'static str>>,
    recipe: Vec<String>,
    /// Another edit gives the host a new recipe while the install runs.
    moved: bool,
}

fn got(v: &[(&str, &str)]) -> Got {
    v.iter().map(|(a, r)| (a.to_string(), r.to_string())).collect()
}

fn fake(files: Value, agents: Value, install: Result<Got, String>, remove: Result<Got, String>) -> Fake {
    let hosts = json!({"devbox": {"ssh": ["alex@devbox.example"], "files": files, "agents": agents},
                       "other": {"ssh": ["alex@other.example"], "agents": "enabled"}});
    Fake { hosts: Mutex::new(hosts.as_object().unwrap().clone()), install, remove, steps: Mutex::new(vec![]),
           recipe: vec!["alex@devbox.example".into()], moved: false }
}

impl Ops for Fake {
    async fn install(&self) -> Result<Got, String> {
        self.steps.lock().push("install");
        if self.moved {
            self.hosts.lock()["devbox"]["ssh"] = json!(["alex@devbox2.example"]);
        }
        self.install.clone()
    }
    async fn remove(&self) -> Result<Got, String> {
        self.steps.lock().push("remove");
        self.remove.clone()
    }
    async fn remove_helper(&self) -> Result<(), String> {
        self.steps.lock().push("remove_helper");
        Ok(())
    }
    fn save(&self, f: &mut dyn FnMut(&mut Map<String, Value>)) -> Result<(), String> {
        self.steps.lock().push("save");
        f(&mut self.hosts.lock());
        Ok(())
    }
    fn changed(&self) {
        self.steps.lock().push("changed");
    }
    fn recipe(&self) -> &[String] {
        &self.recipe
    }
}

fn run<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}

#[test]
fn on_stores_enabled_when_an_agent_took_the_hooks_and_shows_each_result() {
    let f = fake(Value::Null, Value::Null, Ok(got(&[("claude", "installed"), ("codex", "needs-trust")])), Ok(vec![]));
    let lines = run(set_with(&f, "devbox", true)).unwrap();
    assert_eq!(lines, ["claude: installed", "codex: installed; trust terminum's hooks in Codex's /hooks once"]);
    assert_eq!(f.hosts.lock()["devbox"]["agents"], "enabled");
    assert_eq!(f.hosts.lock()["other"]["agents"], "enabled", "another host's record is kept");
    assert_eq!(*f.steps.lock(), ["install", "save", "changed"]);
    let own = fake(Value::Null, Value::Null, Ok(got(&[("claude", "skipped-own"), ("codex", "not-found")])), Ok(vec![]));
    assert_eq!(run(set_with(&own, "devbox", true)).unwrap(), ["claude: uses devbox's own terminum", "codex: not found"]);
    assert_eq!(own.hosts.lock()["devbox"]["agents"], "enabled");
}

#[test]
fn on_stores_nothing_when_no_agent_took_them_or_the_host_failed() {
    let f = fake(Value::Null, Value::Null, Ok(got(&[("claude", "not-found"), ("codex", "failed: no helper")])), Ok(vec![]));
    let e = run(set_with(&f, "devbox", true)).unwrap_err();
    assert!(e.contains("No coding agent on devbox") && e.contains("claude: not found; codex: failed: no helper"), "{e}");
    assert!(f.hosts.lock()["devbox"]["agents"].is_null());
    assert_eq!(*f.steps.lock(), ["install"], "neither saved nor the link changed");
    let f = fake(Value::Null, Value::Null, Err("ssh: Connection refused".into()), Ok(vec![]));
    assert!(run(set_with(&f, "devbox", true)).unwrap_err().contains("Connection refused"));
    assert_eq!(*f.steps.lock(), ["install"]);
}

#[test]
fn off_clears_the_record_and_removes_the_helper_only_when_files_are_off() {
    let f = fake(Value::Null, json!("enabled"), Ok(vec![]), Ok(got(&[("claude", "removed"), ("codex", "off")])));
    assert_eq!(run(set_with(&f, "devbox", false)).unwrap(), ["claude: removed", "codex: off", "helper removed from devbox"]);
    assert!(f.hosts.lock()["devbox"]["agents"].is_null());
    assert_eq!(f.hosts.lock()["other"]["agents"], "enabled", "another host's record is kept");
    assert_eq!(*f.steps.lock(), ["save", "changed", "remove", "remove_helper"], "off here and the link closed before the host is reached");
    for files in ["enabled", "wanted"] {
        let f = fake(json!(files), json!("enabled"), Ok(vec![]), Ok(got(&[("claude", "removed")])));
        assert_eq!(run(set_with(&f, "devbox", false)).unwrap(), ["claude: removed"], "{files}");
        assert_eq!(*f.steps.lock(), ["save", "changed", "remove"], "Files keeps the helper ({files})");
        assert_eq!(f.hosts.lock()["devbox"]["files"], files);
    }
}

#[test]
fn off_turns_the_link_off_even_when_the_host_cannot_be_reached_and_says_so() {
    let f = fake(Value::Null, json!("enabled"), Ok(vec![]), Err("ssh: Connection timed out".into()));
    let e = run(set_with(&f, "devbox", false)).unwrap_err();
    assert!(e.contains("Turned off here") && e.contains("timed out"), "{e}");
    assert!(f.hosts.lock()["devbox"]["agents"].is_null());
    assert_eq!(*f.steps.lock(), ["save", "changed", "remove"], "the helper is not touched on a host not reached");
}

#[test]
fn on_saves_nothing_when_the_record_changed_while_it_ran() {
    let mut f = fake(Value::Null, Value::Null, Ok(got(&[("claude", "installed")])), Ok(vec![]));
    f.moved = true;
    let e = run(set_with(&f, "devbox", true)).unwrap_err();
    assert!(e.contains("Could not set up coding agents' progress on devbox") && e.contains("changed meanwhile"), "{e}");
    assert!(f.hosts.lock()["devbox"]["agents"].is_null(), "the newer record is not turned on");
    assert_eq!(f.hosts.lock()["devbox"]["ssh"], json!(["alex@devbox2.example"]), "the newer recipe is kept");
    assert_eq!(*f.steps.lock(), ["install", "save"], "the link is not changed");
    // the host removed meanwhile: nothing added back
    let f = fake(Value::Null, Value::Null, Ok(got(&[("claude", "installed")])), Ok(vec![]));
    f.hosts.lock().remove("devbox");
    assert!(run(set_with(&f, "devbox", true)).unwrap_err().contains("changed meanwhile"));
    assert!(!f.hosts.lock().contains_key("devbox"));
}

/// Records its start and its end, or that it was stopped (dropped) before ending.
struct Step<'a>(&'a Mutex<Vec<String>>, &'static str, bool);

impl Drop for Step<'_> {
    fn drop(&mut self) {
        if !self.2 {
            self.0.lock().push(format!("{} stopped", self.1));
        }
    }
}

async fn after_ms(n: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(n)).await;
}

async fn step(log: &Mutex<Vec<String>>, name: &'static str, n: u64) -> Result<&'static str, String> {
    let mut s = Step(log, name, false);
    log.lock().push(format!("{name} began"));
    after_ms(n).await;
    log.lock().push(format!("{name} ended"));
    s.2 = true;
    Ok(name)
}

#[tokio::test]
async fn an_off_during_a_long_on_stops_it_before_it_runs() {
    let (turns, log) = (Turns::default(), Mutex::new(Vec::new()));
    let on = turns.run("devbox", step(&log, "on", 5_000));
    let off = async { after_ms(50).await; turns.run("devbox", step(&log, "off", 10)).await };
    let (on, off) = tokio::join!(on, off);
    assert_eq!((on, off), (Err(SUPERSEDED.to_string()), Ok("off")));
    assert_eq!(*log.lock(), ["on began", "on stopped", "off began", "off ended"], "the on never saves after the off");
}

#[tokio::test]
async fn the_last_asked_wins_and_one_waiting_meanwhile_never_runs() {
    let (turns, log) = (Turns::default(), Mutex::new(Vec::new()));
    let later = |ms, name, work| { let (turns, log) = (&turns, &log); async move { after_ms(ms).await; turns.run("devbox", step(log, name, work)).await } };
    // each later one stops the one running
    let (a, b, c) = tokio::join!(later(0, "on", 5_000), later(30, "off", 5_000), later(60, "on again", 10));
    assert_eq!((a.is_err(), b.is_err(), c), (true, true, Ok("on again")));
    assert_eq!(*log.lock(), ["on began", "on stopped", "off began", "off stopped", "on again began", "on again ended"]);
}

#[tokio::test]
async fn one_waiting_when_a_later_one_is_asked_never_begins() {
    let (turns, log) = (Arc::new(Turns::default()), Arc::new(Mutex::new(Vec::new())));
    let turn = turns.0.lock().entry("devbox".into()).or_default().clone();
    let ending = turn.running.lock().await;                     // a turn still ending
    let ask = |name| { let (turns, log) = (turns.clone(), log.clone()); tokio::spawn(async move { turns.run("devbox", step(&log, name, 10)).await }) };
    let off = ask("off");
    after_ms(20).await;
    let on = ask("on again");
    after_ms(20).await;
    drop(ending);
    assert_eq!((off.await.unwrap(), on.await.unwrap()), (Err(SUPERSEDED.to_string()), Ok("on again")));
    assert_eq!(*log.lock(), ["on again began", "on again ended"]);
}

#[tokio::test]
async fn another_hosts_turn_runs_beside_it() {
    let (turns, log) = (Turns::default(), Mutex::new(Vec::new()));
    let (a, b) = tokio::join!(turns.run("devbox", step(&log, "devbox on", 100)), turns.run("other", step(&log, "other off", 10)));
    assert_eq!((a, b), (Ok("devbox on"), Ok("other off")));
}

#[tokio::test]
async fn a_refresh_waits_for_the_one_running_and_a_later_on_or_off_stops_it() {
    let (turns, log) = (Turns::default(), Mutex::new(Vec::new()));
    // after an on, left to end: the refresh runs next
    let on = turns.run("devbox", step(&log, "on", 100));
    let refresh = async { after_ms(20).await; assert!(turns.busy("devbox")); turns.after("devbox", step(&log, "refresh", 10)).await };
    assert_eq!(tokio::join!(on, refresh), (Ok("on"), Ok("refresh")));
    assert_eq!(*log.lock(), ["on began", "on ended", "refresh began", "refresh ended"]);
    assert!(!turns.busy("devbox"));
    // a refresh running, an off asked: the refresh stops
    log.lock().clear();
    let refresh = turns.after("devbox", step(&log, "refresh", 5_000));
    let off = async { after_ms(20).await; turns.run("devbox", step(&log, "off", 10)).await };
    let (refresh, off) = tokio::join!(refresh, off);
    assert_eq!((refresh.is_err(), off), (true, Ok("off")));
    assert_eq!(*log.lock(), ["refresh began", "refresh stopped", "off began", "off ended"]);
}
