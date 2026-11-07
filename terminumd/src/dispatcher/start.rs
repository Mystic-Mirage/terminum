// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! RUN (`terminum-dispatcher.md` AC-121, AC-122): opens the window with the fixed wrapper of §9,
//! marks its pane, and watches the agent's box until the prompt is taken.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::runs::{self, Run};
use super::screen::Screen;
use super::{gate, hooks, move_task, say, write};
use crate::app::Shared;
use crate::tasks::ops;
use crate::tasks::store::DISPATCHER;
use crate::tmux::{args, Server};

const BOX_WITHIN: Duration = Duration::from_secs(60);
const TAKEN_WITHIN: Duration = Duration::from_secs(30);
/// A test's clock runs these slower than the rest: a program needs its real moment to start.
fn start_scaled(d: Duration) -> Duration {
    d / super::scale().clamp(1, 10) as u32
}
/// How long an empty box stands before a first prompt goes in with no start heard.
const STANDS: Duration = Duration::from_secs(2);
const WRAPPER: &str = r#"d=$1; c=$2; t=$3; case $d in "~"|"~/"*) d=$HOME${d#"~"};; esac; s=${SHELL:-/bin/sh}; if cd "$d"; then "$s" -lic "$c"; e=$?; else e=126; fi; "$t" set -p -t "$TMUX_PANE" @terminum-exit "$e"; exec "$s" -l"#;
/// What the wrapper records when it could not enter the folder.
const NO_FOLDER: i32 = 126;

/// The words of a shell-command's arguments for tmux: each quoted, tmux runs no shell of its own.
fn window_command(folder: &str, command: &str, tmux: &str) -> String {
    ["/bin/sh", "-c", WRAPPER, "sh", folder, command, tmux].iter().map(|w| args::quote(w)).collect::<Vec<_>>().join(" ")
}

fn tmux_of(server: &Server) -> String {
    if server.is_local() {
        crate::config::find_program("tmux", "TM_TMUX").map_or_else(|| "tmux".to_string(), |p| p.display().to_string())
    } else {
        "tmux".to_string()
    }
}

/// A tmux session's name for a project: what tmux takes.
fn session_name(project: &str) -> String {
    let name: String = project.chars().map(|c| if c == '.' || c == ':' || c.is_whitespace() { '-' } else { c }).collect();
    if name.is_empty() { "tasks".to_string() } else { name }
}

/// A new window for `project`: after its last session on this server, in a session of its name
/// (made when there is none); its pane and where it stands.
async fn open_window(app: &Shared, server: &Arc<Server>, project: Option<(&str, &str)>, folder: &str, command: &str) -> Result<(String, String), String> {
    let cmd = window_command(folder, command, &tmux_of(server));
    let fmt = "-P -F '#{pane_id} #{session_name}:#{window_index}'";
    let last = project.and_then(|(id, _)| {
        let ps = app.projects.lock();
        ps.list().iter().find(|p| p.id == id).and_then(|p| p.members.iter().rev().filter_map(|k| app.tmux.resolve(k)).find(|(s, _)| Arc::ptr_eq(s, server)).map(|(_, pane)| pane))
    });
    let window = match &last {
        Some(pane) => server.run(format!("display -p -t {pane} '#{{window_id}}'")).await.ok().and_then(|l| l.into_iter().next()),
        None => None,
    };
    let line = if let Some(w) = window {
        format!("new-window -d -a -t {w} {fmt} {cmd}")
    } else {
        let wanted = project.map_or("tasks".to_string(), |(_, name)| session_name(name));
        let sessions = server.run("list-sessions -F '#{session_name}'").await.unwrap_or_default();
        let target = if sessions.contains(&wanted) { Some(wanted.clone()) } else if project.is_none() { sessions.first().cloned() } else { None };
        match target {
            Some(s) => format!("new-window -d -t {} {fmt} {cmd}", args::quote(&format!("={s}:"))),
            None => format!("new-session -d -s {} {fmt} {cmd}", args::quote(&wanted)),
        }
    };
    let made = server.run(line).await.map_err(|e| format!("No new window: {e}"))?;
    let first = made.into_iter().next().unwrap_or_default();
    first.split_once(' ').map(|(p, w)| (p.to_string(), w.to_string())).ok_or_else(|| "No new window: tmux named no pane".to_string())
}

/// Marks `pane` as `run`'s: only that is ever typed in, signalled or closed (AC-125).
async fn mark(server: &Server, pane: &str, run: &str) -> Result<(), String> {
    server.run(format!("set-option -p -t {pane} @terminum-run {run}")).await.map(drop).map_err(|e| format!("tmux did not take the mark: {e}"))?;
    let _ = server.run(format!("set-option -p -u -t {pane} @terminum-exit")).await;
    Ok(())
}

/// What is known of the run's start, for a window: its server and folder.
pub struct Opening<'a> {
    pub server: &'a Arc<Server>,
    pub project: Option<(&'a str, &'a str)>,
    pub folder: &'a str,
    pub command: &'a str,
    pub run: &'a str,
}

/// Opens the window, links it and says so. Its pane's key.
pub async fn open(app: &Shared, o: Opening<'_>, task: &str, program: &str) -> Result<String, String> {
    let (pane, place) = open_window(app, o.server, o.project, o.folder, o.command).await?;
    mark(o.server, &pane, o.run).await?;
    let key = o.server.key(&pane);
    if let Some((project, _)) = o.project {
        let (a, k, p, host) = (app.clone(), key.clone(), project.to_string(), (!o.server.is_local()).then(|| o.server.name.clone()));
        let _ = crate::http::off(move || a.projects.lock().add(&k, Some(&p), None, host, String::new(), None)).await;
        crate::page::projects::tell(app);
    }
    let (run, task, k, on) = (o.run.to_string(), task.to_string(), key.clone(), if o.server.is_local() { "on this Mac".to_string() } else { format!("on {}", o.server.name) });
    let words = format!("Session started: {program} in window {place}, {on}.");
    write(app, move |tx| {
        ops::link(tx, DISPATCHER, &task, Some(&k), ops::now())?;
        let mut r = runs::by_id(tx, &run)?.ok_or("That run is gone.")?;
        r.pane_key = Some(k.clone());
        runs::save(tx, &r)?;
        say(tx, &r, &words)
    }).await?;
    Ok(key)
}

/// A run's start goes wrong: FAILED, with the screen's last rows, redacted; the window stays (AC-121).
async fn did_not_start(app: &Shared, r: &Run, server: Option<&Server>, pane: &str, why: &str) {
    let rows = match server {
        Some(s) => s.run(format!("capture-pane -p -t {pane}")).await.unwrap_or_default(),
        None => Vec::new(),
    };
    let tail: Vec<&str> = rows.iter().map(String::as_str).rev().skip_while(|l| l.trim().is_empty()).take(20).collect::<Vec<_>>().into_iter().rev().collect();
    let shown = crate::sent::redact(&tail.join("\n"));
    let words = if shown.is_empty() { format!("Session did not start: {why}") } else { format!("Session did not start: {why}\n\n```\n{shown}\n```") };
    let run = r.clone();
    let done = write(app, move |tx| {
        let mut r = runs::by_id(tx, &run.id)?.ok_or("That run is gone.")?;
        r.state = "gone".into();
        r.prompt = None;
        runs::save(tx, &r)?;
        if r.held || super::plan::finished(&r.status) { say(tx, &r, &words) } else { move_task(tx, &r, "FAILED", &words) }
    }).await;
    if let Err(e) = done {
        tracing::warn!(event = "dispatcher.start", error = %e);
    }
    super::heard_change(app, &r.task_id);
}

/// The window command's record for `pane`, if the wrapper got there.
pub async fn exit_of(server: &Server, pane: &str) -> Option<i32> {
    server.run(format!("display -p -t {pane} '#{{@terminum-exit}}'")).await.ok()?.first()?.parse().ok()
}

/// Watches a run until its prompt is taken: the box, the gate, the hook. Started once per run; a
/// restart's walk starts it anew and reads every fact from the store.
pub fn watch(app: &Shared, id: &str) {
    if !app.dispatcher.starting.lock().insert(id.to_string()) {
        return;
    }
    let (app, id) = (app.clone(), id.to_string());
    tokio::spawn(async move {
        watching(&app, &id).await;
        app.dispatcher.starting.lock().remove(&id);
    });
}

async fn watching(app: &Shared, id: &str) {
    let began = Instant::now();
    let mut typed_at: Option<Instant> = None;
    let mut empty_since = None;
    loop {
        let wanted = id.to_string();
        let Ok(Some(r)) = write(app, move |tx| runs::by_id(tx, &wanted)).await else { return };
        if !matches!(r.state.as_str(), "starting" | "asking" | "typed") {
            return;
        }
        let Some((server, pane)) = r.pane_key.as_deref().and_then(|k| app.tmux.resolve(k)) else {
            return did_not_start(app, &r, None, "", "its window is gone").await;
        };
        if let Some(code) = exit_of(&server, &pane).await {
            let why = if code == NO_FOLDER { format!("The folder {} does not exist there", r.folder) } else { format!("the program ended with exit {code}") };
            return did_not_start(app, &r, Some(&server), &pane, &why).await;
        }
        if r.state == "typed" {
            let at = *typed_at.get_or_insert_with(Instant::now);
            if at.elapsed() >= start_scaled(TAKEN_WITHIN) {
                let why = if r.kind == "codex" { "the prompt was not taken in 30 s; hooks not trusted: open /hooks in Codex" } else { "the prompt was not taken in 30 s" };
                return did_not_start(app, &r, Some(&server), &pane, why).await;
            }
        } else {
            match gate::read(&server, &pane, &r.kind).await.unwrap_or(Screen::Unknown) {
                Screen::Asks if r.state != "asking" => {
                    let wanted = r.clone();
                    let moved = write(app, move |tx| {
                        let mut now = runs::by_id(tx, &wanted.id)?.ok_or("That run is gone.")?;
                        now.state = "asking".into();
                        runs::save(tx, &now)?;
                        if now.held { Ok(false) } else { move_task(tx, &now, "WAIT FOR HUMAN", "Answer in its window").map(|()| true) }
                    }).await;
                    if moved.unwrap_or(false) {
                        super::heard_change(app, &r.task_id);
                    }
                }
                Screen::Asks => {}
                Screen::Empty => {
                    // Claude Code's SessionStart comes after its box; a box that stands this long started
                    // too, should the hook have gone unheard: its prompt's hook is what proves them (AS-127)
                    let since = *empty_since.get_or_insert_with(Instant::now);
                    let heard = r.hooked || app.hooks.session(&server.key(&pane)).is_some();
                    let ready = heard || since.elapsed() >= STANDS;
                    if ready && let Some(prompt) = &r.prompt {
                        let sent = gate::deliver(app, &r, &server, &pane, prompt, true).await;
                        if sent.is_ok() {
                            let (wanted, session) = (r.id.clone(), app.hooks.session(&server.key(&pane)).map(|(_, s)| s).filter(|s| hooks::good_id(s)));
                            let _ = write(app, move |tx| {
                                let mut now = runs::by_id(tx, &wanted)?.ok_or("That run is gone.")?;
                                if now.state != "working" {
                                    now.state = "typed".into();
                                    if let Some(id) = &session {
                                        now.agent_session.get_or_insert(id.clone());
                                    }
                                    runs::save(tx, &now)?;
                                }
                                Ok(())
                            }).await;
                        }
                    }
                }
                _ => {
                    empty_since = None;
                    if r.state == "starting" && began.elapsed() >= start_scaled(BOX_WITHIN) {
                        return did_not_start(app, &r, Some(&server), &pane, "its input box did not come in 60 s").await;
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// The resume of §9: the run's own command, then the kind's words; its ID's shape checked.
pub fn resume_command(run: &Run) -> Result<String, String> {
    let id = run.agent_session.as_deref().filter(|id| hooks::good_id(id)).ok_or("no conversation is known")?;
    Ok(match run.kind.as_str() {
        "codex" => format!("{} resume {id}", run.command),
        _ => format!("{} --resume {id}", run.command),
    })
}

/// The agent starts anew in a pane where only its shell remains (AC-122): checks the old run's mark,
/// sets the new one's, clears the exit.
pub async fn respawn(server: &Arc<Server>, pane: &str, old: &str, new: &str, folder: &str, command: &str) -> Result<(), String> {
    let mine = server.run(format!("display -p -t {pane} '#{{@terminum-run}}'")).await.map_err(|e| format!("tmux does not answer: {e}"))?;
    if mine.first().map(String::as_str) != Some(old) {
        return Err("its pane is not this run's".into());
    }
    mark(server, pane, new).await?;
    server.run(format!("respawn-pane -k -t {pane} {}", window_command(folder, command, &tmux_of(server)))).await.map(drop).map_err(|e| format!("tmux did not start it: {e}"))
}
