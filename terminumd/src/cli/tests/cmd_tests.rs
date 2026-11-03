// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! AC-08, AC-09, AC-87: the `terminum` command against a terminumd socket served in the
//! test (status, open, password, signout, hosts), when terminumd is down, and its command line.

use super::*;
use crate::cli::{commands, parse, run, wanted, Cmd};

fn health() -> Value {
    json!({"build": "v0.3.0", "pid": 4242, "uptime_s": 7380, "port": 48080,
           "password": {"set": true, "error": null},
           "servers": [{"name": "local", "socket": "terminum", "sessions": ["main", "notes"], "panes": 5,
                        "state": {"state": "attached"}, "connected": true}],
           "hosts": [{"name": "devbox", "state": "reconnecting", "files": "enabled", "why": "ssh exited 255"}],
           "pages": 1, "tmux": {"ok": true, "message": ""}})
}

struct T {
    _home: Home,
    ctx: Ctx,
    opened: PathBuf,
}

impl T {
    fn new() -> T {
        let home = Home::new();
        let mut ctx = home.ctx(&[("TM_NO_LAUNCHD", "1"), ("DISPLAY", ":0")]); // a Linux desktop too
        let opened = home.path("opened");
        ctx.opener = home.path("open");
        write_exec(&ctx.opener, &format!("#!/bin/sh\necho \"$*\" >> '{}'\n", opened.display()));
        T { _home: home, ctx, opened }
    }
    fn serve(&self, mut routes: HashMap<String, Route>) -> Served {
        routes.entry("GET /health".into()).or_insert_with(|| fixed(200, health()));
        serve(&self.ctx.sock(), routes)
    }
    /// (Err message or None, output) of `terminum args`.
    fn cli(&self, args: &[&str]) -> (Option<String>, Vec<String>) {
        if let Some(c) = &self.ctx.captured {
            c.lock().clear();
        }
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let r = parse(&args).and_then(|cmd| run(&self.ctx, cmd));
        (r.err(), said(&self.ctx))
    }
    fn opened(&self) -> Vec<String> {
        std::fs::read_to_string(&self.opened).unwrap_or_default().lines().map(str::to_string).collect()
    }
}

fn routes(list: Vec<(&str, Route)>) -> HashMap<String, Route> {
    list.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

#[test]
fn status_tells_what_runs() {
    let t = T::new();
    let _s = t.serve(HashMap::new());
    let (err, out) = t.cli(&["status"]);
    assert_eq!(err, None);
    for line in ["terminumd:  running, build v0.3.0, pid 4242, up 2h 3m", "page:       http://127.0.0.1:48080/ (other devices: tailscale serve, HTTPS)",
                 "password:   set", "tmux:       local: 2 sessions (main, notes), 5 panes, attached",
                 "host:       devbox: reconnecting, files enabled — ssh exited 255", "pages:      1 connected", "builds:     current -, previous -"] {
        assert!(out.contains(&line.to_string()), "{line} not in {out:?}");
    }
}

#[test]
fn status_names_a_missing_password_and_a_missing_tmux() {
    let t = T::new();
    let mut h = health();
    h["servers"] = json!([]);
    h["password"] = json!({"set": false, "error": null});
    h["tmux"] = json!({"ok": false, "message": "tmux is needed for terminals: brew install tmux"});
    let _s = t.serve(routes(vec![("GET /health", fixed(200, h))]));
    let (_, out) = t.cli(&[]);
    assert!(out.contains(&"password:   not set — no browser signs in: terminum password".to_string()));
    assert!(out.contains(&"tmux:       tmux is needed for terminals: brew install tmux".to_string()));
}

#[test]
fn status_when_terminumd_is_down_says_why() {
    let t = T::new();
    std::fs::create_dir_all(t.ctx.log_dir()).unwrap();
    std::fs::write(t.ctx.log(), format!("ERROR terminumd::sock: msg=\"{}\"\n", crate::activation::unbound_message(48080))).unwrap();
    let (err, out) = t.cli(&["status"]);
    assert_eq!(err, None);
    assert_eq!(out[0], "terminumd:  not running — terminumd does not answer: port 48080 was held by another program when launchd loaded terminum: free it, then run `terminum restart`");
    assert_eq!(out[1], "            log: ~/.terminum/logs/terminumd.log");
}

#[test]
fn status_flags_a_port_launchd_does_not_hold() {
    let home = Home::new();
    let dir = home.path("lc");
    std::fs::create_dir_all(&dir).unwrap();
    let mut ctx = home.ctx(&[]);
    ctx.launchctl = stub_launchctl(&dir);
    ctx.wait = std::time::Duration::from_millis(300);
    crate::cli::launch::start(&ctx).unwrap();
    std::fs::write(ctx.root.join("settings.json"), r#"{"port": 47901}"#).unwrap();
    crate::cli::commands::status(&ctx);
    assert!(said(&ctx).contains(&"port:       settings.json says 47901, launchd holds 48080: run terminum restart".to_string()), "{:?}", said(&ctx));
}

#[test]
fn commands_need_terminumd() {
    let t = T::new();
    assert_eq!(t.cli(&["open"]).0.as_deref(), Some("terminumd does not answer (see: terminum status)"));
}

#[test]
fn open_opens_the_page_once_a_password_is_set() {
    let t = T::new();
    let _s = t.serve(HashMap::new());
    let (err, out) = t.cli(&["open"]);
    assert_eq!(err, None);
    assert_eq!(out, ["http://127.0.0.1:48080/ (sign in with your password)"]);
    assert_eq!(t.opened(), ["http://127.0.0.1:48080/"]);
    let (_, out) = t.cli(&["open", "--print"]);
    assert_eq!(t.opened().len(), 1, "--print only prints");
    assert!(out[0].contains("http://127.0.0.1:48080/"));
}

#[cfg(target_os = "linux")]
#[test]
fn open_on_a_server_without_a_desktop_says_the_ssh_tunnel() {
    let mut t = T::new();
    t.ctx.env.remove("DISPLAY");
    let _s = t.serve(HashMap::new());
    let (err, out) = t.cli(&["open"]);
    assert_eq!(err, None);
    assert_eq!(out, ["http://127.0.0.1:48080/ (sign in with your password)",
                     "From your computer: ssh -L 48080:127.0.0.1:48080 <this server>, then open http://127.0.0.1:48080/"]);
    assert!(t.opened().is_empty(), "no browser to open on a server");
}

#[test]
fn open_without_a_password_says_how_to_set_one() {
    let t = T::new();
    let mut h = health();
    h["password"] = json!({"set": false, "error": null});
    let _s = t.serve(routes(vec![("GET /health", fixed(200, h))]));
    let (err, _) = t.cli(&["open"]);
    assert_eq!(err.as_deref(), Some("No password is set yet, so no browser can sign in: run terminum password, then terminum open"));
    assert!(t.opened().is_empty());
}

#[test]
fn signout() {
    let t = T::new();
    let s = t.serve(routes(vec![("POST /signout", fixed(200, json!({"ok": true})))]));
    let (err, out) = t.cli(&["signout"]);
    assert_eq!(err, None);
    assert_eq!(s.requests.lock().last().unwrap().1, "/signout");
    assert_eq!(out, ["Signed out every browser; each signs in again with the password."]);
}

#[test]
fn the_password_is_at_least_8_and_says_how_other_devices_reach_the_page() {
    let home = Home::new();
    let ctx = home.ctx(&[("TM_WEB_PASSWORD", "short")]);
    let s = serve(&ctx.sock(), routes(vec![("POST /password", fixed(200, json!({"ok": true}))), ("GET /health", fixed(200, health()))]));
    let err = run(&ctx, Cmd::Password).unwrap_err();
    assert!(err.contains("at least 8"), "{err}");
    assert!(s.requests.lock().is_empty(), "refused passwords are never sent");
    let ctx = home.ctx(&[("TM_WEB_PASSWORD", "correct horse")]);
    run(&ctx, Cmd::Password).unwrap();
    assert_eq!(s.requests.lock().iter().find(|r| r.1 == "/password").unwrap().2, Some(json!({"password": "correct horse"})));
    assert_eq!(said(&ctx), ["Password set; browsers signed in before sign in again with it. Open the page: terminum open",
                            "Other devices: serve it over HTTPS on your tailnet: tailscale serve --bg --https=443 http://127.0.0.1:48080 (it takes this machine's name; a name of its own: README \"Over Tailscale\")"]);
}

#[test]
fn a_refused_password_says_why() {
    let home = Home::new();
    let ctx = home.ctx(&[("TM_WEB_PASSWORD", "correct horse")]);
    let _s = serve(&ctx.sock(), routes(vec![("POST /password", fixed(409, json!({"error": "password", "message": "web.json not saved: disk full"})))]));
    assert_eq!(run(&ctx, Cmd::Password).unwrap_err(), "web.json not saved: disk full");
}

#[test]
fn hosts_list() {
    let t = T::new();
    let list = Arc::new(Mutex::new(json!({"hosts": [{"name": "devbox", "ssh": ["-p", "2222", "alex@devbox.example"], "state": "attached", "files": "enabled", "helper": "a1b2c3d4e5f6"}]})));
    let l = list.clone();
    let _s = t.serve(routes(vec![("GET /hosts", Box::new(move |_| (200, l.lock().clone())))]));
    assert_eq!(t.cli(&["hosts"]).1, ["devbox: -p 2222 alex@devbox.example  attached, files enabled, helper a1b2c3d4e5f6, agents off"]);
    *list.lock() = json!({"hosts": [{"name": "devbox", "ssh": ["alex@devbox.example"], "state": "attached", "agents": "refused: the host's sshd forbids socket forwarding"}]});
    assert_eq!(t.cli(&["hosts"]).1, ["devbox: alex@devbox.example  attached, files not enabled, agents refused: the host's sshd forbids socket forwarding"]);
    *list.lock() = json!({"hosts": []});
    assert!(t.cli(&["hosts"]).1[0].contains("No hosts yet"));
}

#[test]
fn hosts_add_sends_the_recipe_and_shows_terminumd_refusals() {
    let t = T::new();
    let s = t.serve(routes(vec![("POST /hosts/add", Box::new(|b: &Option<Value>| {
        let recipe = b.as_ref().and_then(|b| b["recipe"].as_str()).unwrap_or("").to_string();
        if recipe.split_whitespace().any(|w| w == "-o") { (400, json!({"error": "bad_recipe", "message": "option not allowed: -o"})) } else { (200, json!({"ok": true})) }
    }))]));
    let (err, out) = t.cli(&["hosts", "add", "devbox", "-p 2222 alex@devbox.example"]);
    assert_eq!(err, None);
    assert_eq!(s.requests.lock().last().unwrap().2, Some(json!({"name": "devbox", "recipe": "-p 2222 alex@devbox.example", "files": false})));
    assert!(out[0].contains("Added devbox"));
    t.cli(&["hosts", "add", "devbox", "-p", "2222", "alex@devbox.example"]);
    assert_eq!(s.requests.lock().last().unwrap().2.as_ref().unwrap()["recipe"], "-p 2222 alex@devbox.example", "unquoted words are joined");
    assert_eq!(t.cli(&["hosts", "add", "x", "alex@h -o ProxyCommand=nc"]).0.as_deref(), Some("option not allowed: -o"));
    assert!(t.cli(&["hosts", "add"]).0.unwrap().contains("usage"));
    t.cli(&["hosts", "add", "--files", "alex@devbox.example -p 2222"]);
    assert_eq!(s.requests.lock().last().unwrap().2, Some(json!({"name": "", "recipe": "alex@devbox.example -p 2222", "files": true})),
        "one word: the recipe, terminumd names the host");
}

#[test]
fn host_actions() {
    let t = T::new();
    let mut r: Vec<(&str, Route)> = vec![];
    for path in ["POST /hosts/remove", "POST /hosts/enable", "POST /hosts/files-remove", "POST /hosts/on", "POST /hosts/retry"] {
        r.push((path, fixed(200, json!({"ok": true}))));
    }
    r.push(("POST /hosts/off", fixed(404, json!({"error": "no_host", "message": "no host named nope"}))));
    let s = t.serve(routes(r));
    for action in ["remove", "enable", "files-remove", "on", "retry"] {
        assert_eq!(t.cli(&["hosts", action, "devbox"]).0, None, "{action}");
        let last = s.requests.lock().last().cloned().unwrap();
        assert_eq!((last.1, last.2), (format!("/hosts/{action}"), Some(json!({"name": "devbox"}))));
    }
    assert_eq!(t.cli(&["hosts", "off", "nope"]).0.as_deref(), Some("no host named nope"));
    assert!(t.cli(&["hosts", "remove"]).0.unwrap().contains("usage"));
    assert!(t.cli(&["hosts", "remove", "a", "b"]).0.unwrap().contains("usage"));
}

#[test]
fn hosts_agents_on_off_post_their_action_and_print_the_result_lines() {
    let t = T::new();
    let s = t.serve(routes(vec![
        ("POST /hosts/agents-on", fixed(200, json!({"ok": true, "message": "claude: installed\ncodex: not found"}))),
        ("POST /hosts/agents-off", fixed(409, json!({"error": "host", "message": "Turned off here; could not take terminum's hooks out of devbox: ssh: timed out"}))),
    ]));
    let (err, out) = t.cli(&["hosts", "agents", "devbox", "on"]);
    assert_eq!(err, None);
    assert!(out.join("\n").contains("claude: installed\ncodex: not found"), "{out:?}");
    let last = s.requests.lock().last().cloned().unwrap();
    assert_eq!((last.1, last.2), ("/hosts/agents-on".to_string(), Some(json!({"name": "devbox"}))));
    assert!(t.cli(&["hosts", "agents", "devbox", "off"]).0.unwrap().contains("Turned off here"));
    for bad in [&["hosts", "agents"][..], &["hosts", "agents", "devbox"], &["hosts", "agents", "devbox", "yes"], &["hosts", "agents-on", "devbox"]] {
        assert!(t.cli(bad).0.unwrap().contains("usage"), "{bad:?}");
    }
}

#[test]
fn hosts_agents_on_waits_for_the_whole_enable() {
    // the install on the host outlasts every other answer's 5 s: its first run prints the result (AC-99)
    let b = |a| commands::host_budget(a).as_secs();
    assert_eq!((b("agents-on"), b("agents-off"), b("on"), b("off")), (635, 155, 5, 5));
    assert_eq!((b("enable"), b("files-remove")), (365, 65), "the sum of the server's stages, not their maximum");
    let m = commands::timed_out("/hosts/enable", commands::host_budget("enable"));
    assert!(m.contains("/hosts/enable within 365 s") && !m.contains("answers"), "{m}");
    let t = T::new();
    let _s = t.serve(routes(vec![("POST /hosts/agents-on", Box::new(|_: &Option<Value>| {
        std::thread::sleep(std::time::Duration::from_secs(6));
        (200, json!({"ok": true, "message": "claude: installed"}))
    }))]));
    let (err, out) = t.cli(&["hosts", "agents", "devbox", "on"]);
    assert_eq!(err, None);
    assert_eq!(out, ["Enabling coding agents' progress on devbox… (copying the helper and installing)", "claude: installed"]);
}

#[test]
fn the_name_or_a_command_selects_the_cli() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(wanted(&a(&["/Users/alex/.terminum/bin/terminum"])));
    assert!(wanted(&a(&["terminum", "status"])));
    assert!(wanted(&a(&["/tmp/pkg/terminum/agents/macos-aarch64/terminumd", "install"])));
    assert!(!wanted(&a(&["terminumd"])), "no argument: the server");
    assert!(!wanted(&a(&["terminumd", "serve"])), "launchd's plist: the server");
    assert!(!wanted(&a(&["terminumd", "--agent", "--socket", "/tmp/x"])));
}

#[test]
fn the_command_line() {
    let p = |v: &[&str]| parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(p(&[]), Ok(Cmd::Status));
    assert_eq!(p(&["open", "--print"]), Ok(Cmd::Open { print: true }));
    assert_eq!(p(&["upgrade", "--to", "v0.3.0"]), Ok(Cmd::Upgrade(Some("v0.3.0".into()))));
    assert_eq!(p(&["logs", "-n", "5", "-f"]), Ok(Cmd::Logs { lines: 5, follow: true }));
    assert_eq!(p(&["logs"]), Ok(Cmd::Logs { lines: 50, follow: false }));
    assert_eq!(p(&["restart"]), Ok(Cmd::Restart));
    for bad in [&["frobnicate"][..], &["status", "x"], &["password", "x"], &["logs", "-n", "x"], &["hosts", "frob", "x"], &["upgrade", "v1"]] {
        assert!(p(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn root_is_refused() {
    assert!(crate::cli::refuse_root(0).unwrap_err().contains("not root"));
    assert!(crate::cli::refuse_root(501).is_ok());
}

#[test]
fn words_are_quoted_as_a_shell_reads_them() {
    assert_eq!(commands::quote("alex@devbox.example"), "alex@devbox.example");
    assert_eq!(commands::quote("it's"), "'it'\"'\"'s'");
    assert_eq!(commands::quote(""), "''");
    assert_eq!(commands::quote("a b"), "'a b'");
    assert_eq!(commands::uptime(59), "59s");
    assert_eq!(commands::uptime(90061), "1d 1h 1m");
}
