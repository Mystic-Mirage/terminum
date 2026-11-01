// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The layout of every server terminum shows, kept and made again (AC-15): besides terminum's
//! own (restore.rs), a host's `terminum` session and the sessions of the user's own tmux
//! attached by name (AC-89), on this Mac and on hosts. Each server's sessions (windows: name,
//! layout; panes: folder, kind) are written into layout.json's `servers` under the server's id
//! while it is read; a server that cannot be read keeps what was written before. They are made
//! again only where the whole tmux is gone: a server of the user's own that does not run, a
//! host whose tmux server started after its checkpoint (a reboot, a killed server). A session
//! or window the user closed while its server runs stays closed, and so does the last pane
//! whose closing ended its tmux (tmux's own `%exit` while one pane was shown).

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::app::Shared;
use crate::tmux::args;
use crate::tmux::server::{plain, Link, Server, HOST_SESSION};

pub const WINDOWS: &str = "#{session_name}\t#{window_id}\t#{@tm-quick}\t#{window_name}\t#{window_layout}\t#{automatic-rename}";
pub const PANES: &str = "#{session_name}\t#{window_id}\t#{pane_id}\t#{pane_current_command}\t#{pane_tty}\t#{pane_current_path}\t#{@tm-title}\t#{pane_title}";
/// The server's identity (`@terminum-id`) for the checkpoint: whose files hold its panes' last lines (AC-96).
pub const IDENTITY: &str = "show -sqv @terminum-id";

/// How tmux commands reach a server: its own process (`tmux -L <socket>`, a config file for
/// terminum's), or a control client of it (a host's).
pub enum Exec<'a> {
    Local { tmux: &'a Path, socket: &'a str, conf: Option<&'a Path> },
    Ctl(&'a Server),
}

impl Exec<'_> {
    pub async fn run(&self, words: &[&str]) -> Result<String, String> {
        match self {
            Exec::Local { tmux, socket, conf } => {
                let mut c = plain(tmux);
                c.args(["-L", socket]);
                if let Some(f) = conf {
                    c.arg("-f").arg(f);
                }
                let out = c.args(words).output().await.map_err(|e| e.to_string())?;
                if out.status.success() {
                    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
                } else {
                    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
                }
            }
            Exec::Ctl(s) => s.run(words.iter().map(|w| args::word(w)).collect::<Vec<_>>().join(" ")).await.map(|l| l.join("\n")),
        }
    }

    /// The server's identity, given as Server::identity gives it (set only where none is).
    pub async fn identity(&self) -> Option<String> {
        let mine = crate::auth::crypto::random_hex::<8>();
        self.run(&["set", "-sqo", "@terminum-id", &mine]).await.ok()?;
        self.run(&["show", "-sv", "@terminum-id"]).await.ok().map(|i| i.trim().to_string()).filter(|i| !i.is_empty())
    }
}

/// The panes running on `exec`'s server before a restore makes any, each paired with itself:
/// put after the panes made again, they keep their projects and bookmarks (a restore again
/// passes over the sessions still running).
pub async fn kept(exec: &Exec<'_>) -> Vec<(String, String)> {
    let listed = exec.run(&["list-panes", "-a", "-F", "#{pane_id}"]).await.unwrap_or_default();
    listed.lines().map(str::trim).filter(|p| p.starts_with('%')).map(|p| (p.to_string(), p.to_string())).collect()
}

/// The kind of a pane's program for the checkpoint (a shell, else another program: never its
/// command line). terminum's own server says more (restore.rs: ssh recipes).
pub fn simple_kind(command: &str) -> Value {
    if crate::tmux::titles::SHELLS.contains(&command.trim()) { json!({"kind": "shell"}) } else { json!({"kind": "other", "program": command.trim()}) }
}

/// The sessions of `wins` and `panes` (the lists of WINDOWS and PANES) that `keep` takes, in
/// the checkpoint's shape; `kind(command, tty)` describes a pane's program.
pub fn sessions_of(wins: &[String], panes: &[String], keep: impl Fn(&str) -> bool, kind: impl Fn(&str, &str) -> Value) -> Vec<Value> {
    let fields: Vec<Vec<&str>> = panes.iter().map(|l| l.splitn(8, '\t').collect::<Vec<_>>()).filter(|g| g.len() == 8).collect();
    let mut sessions: Vec<Value> = Vec::new();
    for w in wins {
        let f: Vec<&str> = w.splitn(6, '\t').collect();
        if f.len() < 5 || !f[2].is_empty() || !keep(f[0]) {
            continue;                                   // a quick shell (AC-88) is not kept
        }
        let ps: Vec<Value> = fields.iter().filter(|g| g[1] == f[1]).map(|p| {
            let mut k = kind(p[3], p[4]);
            k["dir"] = json!(p[5]);
            k["id"] = json!(p[2]);                      // its pane's id, for what keeps pane keys (projects, AC-92)
            if let Some(t) = agent_title(p[3], p[7], p[6]) {
                k["title"] = json!(t);                  // its row's name (AC-15)
            }
            k
        }).collect();
        let mut win = json!({"name": f[3], "layout": f[4], "panes": ps});
        // tmux names it after its program: not the user's name; a version (Claude Code's process,
        // "2.1.310") is one a restore before 0.24.11 fixed with `-n`, never the user's
        if f.get(5) == Some(&"1") || crate::tmux::titles::is_version(f[3]) {
            win["auto"] = json!(true);
        }
        match sessions.iter_mut().find(|s| s["name"] == f[0]) {
            Some(s) => s["windows"].as_array_mut().expect("array").push(win),
            None => sessions.push(json!({"name": f[0], "windows": [win]})),
        }
    }
    sessions
}

/// The title a coding agent gave its pane (Claude Code's conversation), which its row shows,
/// or the one a restore gave it (`restored`, `@tm-title`) while a shell runs there: none for
/// another program or a title that names nothing but the program.
fn agent_title(command: &str, title: &str, restored: &str) -> Option<String> {
    use crate::tmux::titles;
    let shell = titles::SHELLS.contains(&command.trim_start_matches('-'));
    let title = if titles::agent_of(command).is_some() || crate::tmux::agentstate::claude_marked(title) {
        title
    } else if shell && !restored.is_empty() {
        restored
    } else {
        return None;
    };
    let t: String = crate::termtext::clean(&titles::bare(title), false).chars().take(TITLE_MAX).collect();
    let t = t.trim();
    (!t.is_empty() && t != "Claude Code" && !titles::program_name(t) && !titles::is_version(t)).then(|| t.to_string())
}

/// The pane made again keeps the title its row showed (`@tm-title`, shown while a shell runs
/// there, titles.rs) when its checkpoint has one.
async fn titled(exec: &Exec<'_>, pane: &str, saved: Option<&Value>) {
    if let Some(t) = saved.and_then(|p| p["title"].as_str()).filter(|t| !t.is_empty() && pane.starts_with('%')) {
        let _ = exec.run(&["set", "-pq", "-t", pane, "@tm-title", t]).await;
    }
}

/// A window's name made of a title: at most this many characters.
const TITLE_MAX: usize = 100;

/// What one making-again did, for the report.
#[derive(Default)]
pub struct Made {
    pub sessions: usize,
    pub windows: usize,
    pub programs: Vec<String>,
    pub errors: Vec<String>,
    pub notes: Vec<String>,
    /// the panes made again: (the checkpoint's pane id, the new one)
    pub panes: Vec<(String, String)>,
    /// how many of them got their last lines from before the restart (AC-96)
    pub lines: usize,
}

/// Makes `sessions` on the server `exec` reaches, but those in `existing`; `into` names a
/// session that exists already and takes the windows instead (a host's fresh `terminum`).
/// `dir_ok(dir)`: whether a folder can be used (else the home folder, with a note); `print`: the
/// command an old pane's replacement starts with (its last lines, restore_lines.rs).
#[allow(clippy::too_many_arguments)]
pub async fn make(exec: &Exec<'_>, sessions: &[Value], existing: &[String], into: Option<&str>, dir_ok: impl Fn(&str) -> bool, home: &str, print: &HashMap<String, String>, out: &mut Made) {
    let print_of = |p: Option<&Value>| p.and_then(|p| p["id"].as_str()).and_then(|o| print.get(o)).map(String::as_str);
    for s in sessions {
        let name = s["name"].as_str().unwrap_or("");
        let fill = into == Some(name);
        if name.is_empty() || args::check_session_name(name).is_err() || (existing.iter().any(|e| e == name) && !fill) {
            continue;
        }
        let mut first = !fill;
        for w in s["windows"].as_array().into_iter().flatten() {
            let panes = w["panes"].as_array().cloned().unwrap_or_default();
            let dir_of = |p: &Value| {
                let d = p["dir"].as_str().unwrap_or("");
                if !d.is_empty() && dir_ok(d) { d.to_string() } else { home.to_string() }
            };
            for p in &panes {
                let d = p["dir"].as_str().unwrap_or("");
                if !d.is_empty() && !dir_ok(d) {
                    out.notes.push(format!("{d} is gone: opened in ~"));
                }
                match p["kind"].as_str() {
                    Some("ssh") => out.programs.push(format!("ssh {}", p["recipe"].as_array().map(|r| r.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default())),
                    Some("other") => out.programs.push(p["program"].as_str().unwrap_or("a program").to_string()),
                    _ => {}
                }
            }
            let d0 = args::literal_format(&panes.first().map_or(home.to_string(), dir_of));
            let window_name = args::literal_format(w["name"].as_str().unwrap_or(""));
            let target = format!("={name}:");
            let mut cmd = if first { vec!["new-session", "-d", "-s", name] } else { vec!["new-window", "-d", "-t", target.as_str()] };
            // `-n` turns automatic-rename off: a window tmux named after its program is left to tmux
            if w["auto"] != true {
                cmd.extend(["-n", &window_name]);
            }
            cmd.extend(["-c", &d0, "-P", "-F", "#{window_id} #{pane_id}"]);
            cmd.extend(print_of(panes.first()));
            let made = exec.run(&cmd).await;
            let old = |p: Option<&Value>| p.and_then(|p| p["id"].as_str()).map(str::to_string);
            let wid = match made {
                Ok(ids) => {
                    let (wid, pane) = ids.split_once(' ').unwrap_or((ids.as_str(), ""));
                    titled(exec, pane, panes.first()).await;
                    if let Some(o) = old(panes.first()) {
                        out.panes.push((o, pane.to_string()));
                    }
                    wid.to_string()
                }
                Err(e) => {
                    out.errors.push(format!("{name}: {e}"));
                    if first { break } else { continue }
                }
            };
            if first {
                out.sessions += 1;
            }
            first = false;
            out.windows += 1;
            for p in panes.iter().skip(1) {
                let d = args::literal_format(&dir_of(p));
                let mut split = vec!["split-window", "-d", "-t", &wid, "-c", &d, "-P", "-F", "#{pane_id}"];
                split.extend(print_of(Some(p)));
                match exec.run(&split).await {
                    Ok(new) => {
                        titled(exec, &new, Some(p)).await;
                        out.panes.extend(old(Some(p)).map(|o| (o, new)));
                    }
                    Err(e) => out.errors.push(format!("{name}: {e}")),
                }
            }
            if let Some(layout) = w["layout"].as_str().filter(|l| !l.is_empty()) {
                let _ = exec.run(&["select-layout", "-t", &wid, layout]).await;
            }
        }
    }
}

/// What the old panes had goes to the panes a restore made in their place (`m.panes`; `kept`,
/// the panes running before it, paired with themselves): projects and bookmarks, Unread
/// (AC-93) and the last lines (AC-96), by the identity the checkpoint `saved` names.
pub async fn hand_on(app: &Shared, exec: &Exec<'_>, server: &Server, saved: &Value, kept: Vec<(String, String)>, m: &mut Made) {
    let prefix = server.key("");
    crate::page::projects::restarted(app, &prefix, &[m.panes.clone(), kept].concat()).await;
    let (old, now) = (saved["identity"].as_str(), exec.identity().await);
    crate::page::unread::restarted(app, &prefix, old, now.as_deref(), &m.panes);
    m.lines = crate::lastlines::restarted(app, &server.id(), old, now, &m.panes).await;
}

/// The checkpoint of every other server that can be read now (a host's `terminum`, the
/// sessions of the user's own tmux attached by name), over the ones `prev` kept.
pub async fn checkpoint(app: &Shared, prev: &Value) -> Value {
    let mut servers = prev.as_object().cloned().unwrap_or_default();
    for s in app.tmux.all().into_iter().filter(|s| !std::ptr::eq(s.as_ref(), app.tmux.local.as_ref())) {
        let Some(bulk) = s.bulk() else { continue };
        let mut answers = bulk.run_all(vec![format!("list-windows -a -F '{WINDOWS}'"), format!("list-panes -a -F '{PANES}'"), IDENTITY.into()]).await.into_iter();
        let (Some(Ok(wins)), Some(Ok(panes))) = (answers.next(), answers.next()) else { continue };
        let identity = answers.next().and_then(Result::ok).and_then(|l| l.into_iter().next()).unwrap_or_default();
        // a host's tmux started anew is kept as it was until its layout was made again (host())
        if !s.is_local() && let Some(started) = started(&s).await {
            let at = servers.get(&s.id()).and_then(|v| v["at"].as_i64()).unwrap_or(i64::MAX);
            if started > at && !app.restore.was_tried(&format!("{}@{started}", s.id())) {
                continue;
            }
        }
        let sessions = sessions_of(&wins, &panes, |name| s.shows(name), |c, _| simple_kind(c));
        if sessions.is_empty() {
            continue;                                   // nothing of it shown now: what was kept stays
        }
        servers.insert(s.id(), json!({"host": (!s.is_local()).then(|| s.name.clone()), "at": crate::auth::store::unix_now(), "identity": identity.trim(), "sessions": sessions}));
    }
    Value::Object(servers)
}

/// When the server started (Unix seconds): a session closed and made again by terminumd while it
/// runs (the user closed its last window) is no new server.
async fn started(server: &Server) -> Option<i64> {
    let lines = server.run("list-sessions -F '#{start_time}'").await.ok()?;
    lines.first()?.trim().parse().ok()
}

fn saved(app: &Shared, id: &str) -> Option<Value> {
    let text = std::fs::read_to_string(app.cfg.dir.join("layout.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v["servers"].get(id).cloned()
}

fn report(app: &Shared, place: &str, m: Made) {
    if m.sessions + m.windows == 0 && m.errors.is_empty() {
        return;
    }
    let text = format!("Restored {place}: {} session{}, {} window{}.", m.sessions, if m.sessions == 1 { "" } else { "s" },
        m.windows, if m.windows == 1 { "" } else { "s" });
    tracing::info!(event = "restore", place, sessions = m.sessions, windows = m.windows, errors = m.errors.len());
    crate::restore::add_report(app, &text, m);
}

/// The user's own tmux on this Mac does not run (a reboot, its server killed): its attached
/// sessions are made again from their checkpoint, once while terminumd runs.
pub async fn users_local(app: &Shared, server: &Server) {
    let by_user = server.took_closed_by_user();
    let Link::Local { tmux, socket, .. } = &server.link else { return };
    if !app.looks.settings.read().restore {
        return;
    }
    let Some(saved) = saved(app, &server.id()) else { return };
    if !app.restore.first_try(&format!("{}@{}", server.id(), saved["at"])) || by_user {
        return;                                         // made again already, or the user closed its last pane
    }
    let only = server.only.lock().clone().unwrap_or_default();
    let wanted: Vec<Value> = saved["sessions"].as_array().into_iter().flatten().filter(|s| only.iter().any(|o| s["name"] == o.as_str())).cloned().collect();
    if wanted.is_empty() {
        return;
    }
    let mut m = Made::default();
    let home = crate::config::home().display().to_string();
    let exec = Exec::Local { tmux, socket, conf: None };
    let kept = kept(&exec).await;
    let print = crate::restore_lines::commands(app, &exec, &server.id(), saved["identity"].as_str(), &wanted).await;
    make(&exec, &wanted, &[], None, crate::restore::usable, &home, &print, &mut m).await;
    hand_on(app, &exec, server, &saved, kept, &mut m).await;
    report(app, &format!("your tmux{}", if socket == "default" { String::new() } else { format!(" -L {socket}") }), m);
}

/// A host attached again: when its tmux server started after its checkpoint (a reboot, a killed
/// server), the `terminum` session's windows and the user's attached sessions there are made
/// again. `seen`: the sessions running there.
pub async fn host(app: &Shared, server: &Server, seen: &[(String, String)]) {
    let by_user = server.took_closed_by_user();
    if server.is_local() || !app.looks.settings.read().restore {
        return;
    }
    let Some(saved) = saved(app, &server.id()) else { return };
    let at = saved["at"].as_i64().unwrap_or(i64::MAX);
    let started = started(server).await.unwrap_or(0);
    // the user closed its last pane and its tmux ended with it: nothing to make again
    if started <= at || !app.restore.first_try(&format!("{}@{started}", server.id())) || by_user {
        return;
    }
    let existing: Vec<String> = seen.iter().map(|(n, _)| n.clone()).collect();
    let fresh = server.run(format!("list-windows -t ={HOST_SESSION} -F '#{{window_id}}'")).await.unwrap_or_default();
    let sessions: Vec<Value> = saved["sessions"].as_array().cloned().unwrap_or_default();
    let mut m = Made::default();
    let home = server.home().await.unwrap_or_default();
    let kept = kept(&Exec::Ctl(server)).await;
    make(&Exec::Ctl(server), &sessions, &existing, Some(HOST_SESSION), |_| true, if home.is_empty() { "~" } else { &home }, &HashMap::new(), &mut m).await;
    hand_on(app, &Exec::Ctl(server), server, &saved, kept, &mut m).await;
    // the fresh session's own first window goes once the kept ones are there
    if m.windows > 0 && fresh.len() == 1 {
        let _ = server.run(format!("kill-window -t {}", args::word(&fresh[0]))).await;
    }
    report(app, &format!("host {}", server.name), m);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn restore_keeps_literal_hash_names_and_folders_on_both_transports() {
        let Some(tm) = crate::testutil::Tmux::start("restore-formats", (80, 30), "/bin/sh") else { panic!("tmux required") };
        let dirs: Vec<_> = ["#S", "#{session_name}"].iter().map(|name| tm.dir.join(name)).collect();
        for dir in &dirs { std::fs::create_dir(dir).unwrap(); }
        let tmux = crate::config::find_program("tmux", "TM_TMUX").expect("tmux on PATH");
        let server = Server::new("local", &tm.socket, Link::Local {
            tmux: tmux.clone(), socket: tm.socket.clone(), conf: "/dev/null".into(),
        }, std::sync::Arc::new(tokio::sync::Notify::new()));
        for (name, exec) in [
            ("restoredlocal", Exec::Local { tmux: &tmux, socket: &tm.socket, conf: None }),
            ("restoredctl", Exec::Ctl(&server)),
        ] {
            if matches!(exec, Exec::Ctl(_)) {
                server.sync_clients(&["main".to_string()]).unwrap();
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
                while !server.attached() && tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                assert!(server.attached(), "control client did not attach: {:?}", server.last_words());
            }
            let sessions = json!([{"name": name, "windows": [
                {"name": "#S #{session_name}", "panes": [{"id": "%70", "dir": dirs[0]}, {"id": "%71", "dir": dirs[1]}]},
                {"name": "second #S", "panes": [{"id": "%72", "dir": dirs[1]}]}
            ]}]);
            let mut out = Made::default();
            make(&exec, sessions.as_array().unwrap(), &[], None, |_| true, tm.dir.to_str().unwrap(), &HashMap::new(), &mut out).await;
            assert!(out.errors.is_empty(), "{:?}", out.errors);
            assert_eq!((out.sessions, out.windows, out.panes.len()), (1, 2, 3));
            let names = tm.read(&["list-windows", "-t", name, "-F", "#{window_name}"]);
            assert_eq!(names.lines().collect::<Vec<_>>(), ["#S #{session_name}", "second #S"]);
            for ((_, pane), dir) in out.panes.iter().zip([&dirs[0], &dirs[1], &dirs[1]]) {
                assert_eq!(tm.read(&["display-message", "-p", "-t", pane, "#{pane_current_path}"]).trim(), dir.to_str().unwrap());
            }
        }
        server.disconnect();
    }

    #[test]
    fn sessions_are_kept_with_their_windows_and_folders_but_quick_shells() {
        let wins = ["main\t@1\t\tapi\tlayout1\t0", "main\t@2\t%9\tquick\tlayout2\t0", "work\t@3\t\t2.1.310\tlayout3\t1", "work\t@5\t\t2.1.312\tlayout5\t0", "work\t@6\t\t2.1.312\tlayout6\t1",
                    "work\t@7\t\tbash\tlayout7\t1", "other\t@4\t\tx\tl\t0"].map(String::from);
        let panes = ["main\t@1\t%1\tzsh\t/dev/ttys1\t/Users/alex/api\t\tdevbox", "main\t@1\t%2\tnode\t/dev/ttys2\t/Users/alex/api/web\t\t",
                     "main\t@2\t%9\tzsh\t/dev/ttys3\t/tmp\t\t", "work\t@3\t%4\tzsh\t/dev/ttys4\t/var/log\t\t\u{2733} Claude Code",
                     "work\t@5\t%5\tbash\t/dev/ttys5\t/var/log\tRestored talk\tdevbox", "work\t@6\t%6\t2.1.312\t/dev/ttys6\t/var/log\tRestored talk\t\u{2733} Fix the login\u{7} form",
                     "work\t@7\t%7\tbash\t/dev/ttys7\t/var/log\t\t\u{2733} Old talk"].map(String::from);
        let s = sessions_of(&wins, &panes, |n| n != "other", |c, _| simple_kind(c));
        assert_eq!(s.len(), 2, "a session the server does not show is not kept");
        assert_eq!(s[0]["windows"].as_array().unwrap().len(), 1, "the quick shell's window is not kept");
        assert_eq!(s[0]["windows"][0]["panes"][1], json!({"kind": "other", "program": "node", "dir": "/Users/alex/api/web", "id": "%2"}));
        assert_eq!(s[1]["windows"][0]["panes"][0], json!({"kind": "shell", "dir": "/var/log", "id": "%4"}));
        assert_eq!(s[0]["windows"][0].get("auto"), None, "a window the user named keeps its name");
        assert_eq!(s[1]["windows"][0]["auto"], true, "a window tmux names after its program is marked so");
        assert_eq!(s[1]["windows"][1]["auto"], true, "a version's name an earlier restore fixed is tmux's again");
        let title = |w: usize| s[1]["windows"][w]["panes"][0].get("title").cloned();
        assert_eq!(title(0), None, "Claude Code's own title names nothing");
        assert_eq!(title(1), Some(json!("Restored talk")), "a shell keeps the title a restore gave it");
        assert_eq!(title(2), Some(json!("Fix the login form")), "a coding agent's title wins, its mark and control characters dropped");
        assert_eq!(title(3), Some(json!("Old talk")), "a shell under a marked title (the agent just ended) too");
        assert_eq!(s[1]["windows"][2]["auto"], true, "and the window stays tmux's to name");
        assert_eq!(s[0]["windows"][0]["panes"][1].get("title"), None, "another program's title is not kept");
    }
}
