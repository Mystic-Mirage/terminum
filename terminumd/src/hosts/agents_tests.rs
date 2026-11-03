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
