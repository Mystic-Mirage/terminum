// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! RUN (`terminum-dispatcher.md` AC-121, AC-122): opens the window with the fixed wrapper of §9,
//! marks its pane, and watches the agent's box until the prompt is taken.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::runs::{self, Run};
use super::screen::Screen;
use super::{gate, hooks, move_task, say, store};
use crate::app::Shared;
use crate::tasks::ops;
use crate::tasks::store::DISPATCHER;
use crate::tmux::server::{Link, HOST_SESSION};
use crate::tmux::{args, Server};

const BOX_WITHIN: Duration = Duration::from_secs(60);
const TAKEN_WITHIN: Duration = Duration::from_secs(30);
/// How long an empty box stands before a first prompt goes in with no start heard.
const STANDS: Duration = Duration::from_secs(2);
/// A test's clock runs these slower than the rest: a program needs its real moment to start.
fn start_scaled(d: Duration) -> Duration {
    d / super::scale().min(10) as u32
}
/// The wrapper's words are its arguments: no quote or `#` in them can break it. What the tty still
/// holds when the agent has gone is dropped, so nothing meant for it reaches the shell it leaves.
const WRAPPER: &str = r#"d=$1; c=$2; t=$3; case $d in "~"|"~/"*) d=$HOME${d#"~"};; esac; s=${SHELL:-/bin/sh}; if cd -- "$d"; then "$s" -lic "$c"; e=$?; else e=-1; fi; "$t" set -p -t "$TMUX_PANE" @terminum-exit "$e"; stty -icanon min 0 time 0 2>/dev/null; cat >/dev/null 2>&1; stty sane 2>/dev/null; exec "$s" -l"#;
/// What the wrapper records when it could not enter the folder: no process exits with it.
const NO_FOLDER: i32 = -1;

/// The words of a shell-command's arguments for tmux: each quoted, tmux runs no shell of its own.
fn window_command(folder: &str, command: &str, tmux: &str) -> String {
    ["/bin/sh", "-c", WRAPPER, "sh", folder, command, tmux].iter().map(|w| args::quote(w)).collect::<Vec<_>>().join(" ")
}

fn tmux_of(server: &Server) -> String {
    match &server.link {
        Link::Local { tmux, .. } => tmux.display().to_string(),
        Link::Ssh { .. } => "tmux".to_string(),
    }
}

/// A new window: after the project's last session on this server, else in a session of its name,
/// made when there is none; its pane and where it stands.
async fn open_window(app: &Shared, server: &Arc<Server>, project: Option<(&str, &str)>, folder: &str, command: &str) -> Result<(String, String), String> {
    let cmd = window_command(folder, command, &tmux_of(server));
    let fmt = "-P -F '#{pane_id} #{session_name}:#{window_index}'";
    let last = project.and_then(|(id, _)| {
        let ps = app.projects.lock();
        ps.by_id(id).and_then(|p| p.members.iter().rev().filter_map(|k| app.tmux.resolve(k)).find(|(s, _)| Arc::ptr_eq(s, server)).map(|(_, pane)| pane))
    });
    let window = match &last {
        Some(pane) => crate::page::sessions::window_of(server, pane).await.ok(),
        None => None,
    };
    let line = if let Some(w) = window {
        format!("new-window -d -a -t {w} {fmt} {cmd}")
    } else if !server.is_local() {
        // a host shows only the session terminumd holds there (terminum-remote.md OQ-81)
        format!("new-window -d -t {} {fmt} {cmd}", args::quote(&format!("={HOST_SESSION}:")))
    } else {
        let wanted = args::project_session(project.map_or("tasks", |(_, name)| name))?;
        let sessions = server.run("list-sessions -F '#{session_name}'").await.unwrap_or_default();
        let target = if sessions.contains(&wanted) { Some(wanted.clone()) } else if project.is_none() { sessions.first().cloned() } else { None };
        match target {
            Some(s) => format!("new-window -d -t {} {fmt} {cmd}", args::quote(&format!("={s}:"))),
            None => format!("new-session -d -s {} {fmt} {cmd}", args::format_arg(&wanted)),
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

/// Whether `pane` still carries `run`'s mark: read just before any act on it.
pub async fn is_marked(server: &Server, pane: &str, run: &str) -> Result<(), String> {
    let mine = server.run(format!("display -p -t {pane} '#{{@terminum-run}}'")).await.map_err(|e| format!("tmux does not answer: {e}"))?;
    if mine.first().map(String::as_str) == Some(run) { Ok(()) } else { Err("its pane is not this run's".to_string()) }
}

/// What a window is for.
pub struct Opening<'a> {
    pub server: &'a Arc<Server>,
    pub project: Option<(&'a str, &'a str)>,
    pub folder: &'a str,
    pub command: &'a str,
    pub run: &'a str,
    pub task: &'a str,
    pub program: &'a str,
}

/// Opens the window, links it and says so.
pub async fn open(app: &Shared, o: Opening<'_>) -> Result<(), String> {
    let (pane, place) = open_window(app, o.server, o.project, o.folder, o.command).await?;
    mark(o.server, &pane, o.run).await?;
    if let Some((session, _)) = place.rsplit_once(':') {
        o.server.note_pane(&pane, session);            // its agent's first hook may come before the next list
    }
    let key = o.server.key(&pane);
    app.dispatcher.panes.lock().insert(key.clone());
    if let Some((project, _)) = o.project {
        let (a, k, p, host) = (app.clone(), key.clone(), project.to_string(), (!o.server.is_local()).then(|| o.server.name.clone()));
        let _ = crate::http::off(move || a.projects.lock().add(&k, Some(&p), None, host, String::new(), None)).await;
        crate::page::projects::tell(app);
    }
    let on = if o.server.is_local() { "on this Mac".to_string() } else { format!("on {}", o.server.name) };
    let (run, task, words) = (o.run.to_string(), o.task.to_string(), format!("Session started: {} in window {place}, {on}.", o.program));
    store(app, move |tx| {
        ops::link(tx, DISPATCHER, &task, Some(&key), ops::now())?;
        let r = runs::change(tx, &run, |r| r.pane_key = Some(key.clone()))?;
        say(tx, &r, &words)
    }).await
}

/// A run's start goes wrong: FAILED, with the screen's last rows, redacted; the window stays (AC-121).
async fn did_not_start(app: &Shared, r: &Run, at: Option<(&Server, &str)>, why: &str) {
    let rows = match at {
        Some((s, pane)) => s.run(format!("capture-pane -p -t {pane}")).await.unwrap_or_default(),
        None => Vec::new(),
    };
    let tail: Vec<&str> = rows.iter().map(String::as_str).rev().skip_while(|l| l.trim().is_empty()).take(20).collect::<Vec<_>>().into_iter().rev().collect();
    let shown = crate::sent::redact(&tail.join("\n"));
    let words = if shown.is_empty() { format!("Session did not start: {why}") } else { format!("Session did not start: {why}\n\n```\n{shown}\n```") };
    let id = r.id.clone();
    let done = store(app, move |tx| {
        let now = runs::change(tx, &id, |r| { r.state = "gone".into(); r.prompt = None; })?;
        if now.held || super::plan::finished(&now.status) { say(tx, &now, &words) } else { move_task(tx, &now, "FAILED", &words) }
    }).await;
    if let Err(e) = done {
        tracing::warn!(event = "dispatcher.start", error = %e);
    }
    crate::tasks::api::told(app, &r.task_id);
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

/// Its own question stands before the box: nothing is typed, a human is asked (AC-121).
async fn now_asking(app: &Shared, r: &Run) {
    let id = r.id.clone();
    let moved = store(app, move |tx| {
        let now = runs::change(tx, &id, |r| r.state = "asking".into())?;
        if now.held { Ok(false) } else { move_task(tx, &now, "WAIT FOR HUMAN", "Answer in its window").map(|()| true) }
    }).await;
    if moved.unwrap_or(false) {
        crate::tasks::api::told(app, &r.task_id);
    }
}

/// The prompt goes in: `typed` first, so no restart between can type it twice; a miss puts it back.
async fn type_prompt(app: &Shared, r: &Run, server: &Server, pane: &str, prompt: &str) -> Result<(), gate::Miss> {
    let (id, was, session) = (r.id.clone(), r.state.clone(), app.hooks.session(&server.key(pane)).map(|(_, s)| s).filter(|s| hooks::good_id(s)));
    let wrote = store(app, move |tx| runs::change(tx, &id, |now| {
        if now.is_starting() {
            now.state = "typed".into();
        }
        if let Some(s) = session {
            now.agent_session.get_or_insert(s);
        }
    })).await;
    if let Err(e) = wrote {
        return Err(gate::Miss::Other(e));                  // what it could not write it does not do (AC-131)
    }
    let sent = gate::deliver(app, r, server, pane, prompt, true).await;
    if sent.is_err() {
        let id = r.id.clone();
        let _ = store(app, move |tx| runs::change(tx, &id, |now| if now.state == "typed" { now.state = was; })).await;
    }
    sent
}

async fn watching(app: &Shared, id: &str) {
    let began = Instant::now();
    let (mut typed_at, mut empty_since, mut miss): (Option<Instant>, Option<Instant>, Option<String>) = (None, None, None);
    let box_within = |miss: &Option<String>| match miss { Some(m) => format!("its input box did not come in 60 s: {m}"), None => "its input box did not come in 60 s".to_string() };
    loop {
        let wanted = id.to_string();
        let Ok(r) = store(app, move |tx| runs::by_id(tx, &wanted)).await else { return };
        if !r.is_starting() {
            return;
        }
        let Some((server, pane)) = r.pane_key.as_deref().and_then(|k| app.tmux.resolve(k)) else {
            return did_not_start(app, &r, None, "its window is gone").await;
        };
        if let Some(code) = exit_of(&server, &pane).await {
            let why = if code == NO_FOLDER { format!("The folder {} does not exist there", r.folder) } else { format!("the program ended with exit {code}") };
            return did_not_start(app, &r, Some((&server, &pane)), &why).await;
        }
        if r.state == "typed" {
            let at = *typed_at.get_or_insert_with(Instant::now);
            if at.elapsed() >= start_scaled(TAKEN_WITHIN) {
                let why = if r.kind == "codex" { "the prompt was not taken in 30 s; hooks not trusted: open /hooks in Codex" } else { "the prompt was not taken in 30 s" };
                return did_not_start(app, &r, Some((&server, &pane)), why).await;
            }
        } else {
            match gate::read(&server, &pane, &r.kind).await.unwrap_or(Screen::Unknown) {
                Screen::Asks if r.state != "asking" => now_asking(app, &r).await,
                Screen::Asks => {}
                Screen::Empty => {
                    // Claude Code's SessionStart comes after its box; one that stands this long has
                    // started, should the hook have gone unheard: its prompt's hook is what proves them (AS-127)
                    let since = *empty_since.get_or_insert_with(Instant::now);
                    let heard = app.hooks.session(&server.key(&pane)).is_some();
                    if (heard || since.elapsed() >= STANDS) && let Some(prompt) = &r.prompt
                        && let Err(m) = type_prompt(app, &r, &server, &pane, prompt).await {
                        miss = Some(m.to_string());
                        if r.state == "starting" && began.elapsed() >= start_scaled(BOX_WITHIN) {
                            return did_not_start(app, &r, Some((&server, &pane)), &box_within(&miss)).await;
                        }
                    }
                }
                _ => {
                    empty_since = None;
                    if r.state == "starting" && began.elapsed() >= start_scaled(BOX_WITHIN) {
                        return did_not_start(app, &r, Some((&server, &pane)), &box_within(&miss)).await;
                    }
                }
            }
        }
        // a question nobody answers may stand all night: no need to look five times a second
        tokio::time::sleep(Duration::from_millis(if r.state == "asking" { 1000 } else { 200 })).await;
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

/// The agent starts anew in a pane where only its shell remains (AC-122): the old run's mark checked,
/// the new one's set, the exit cleared.
pub async fn respawn(app: &Shared, server: &Arc<Server>, pane: &str, old: &str, new: &str, folder: &str, command: &str) -> Result<(), String> {
    is_marked(server, pane, old).await?;
    mark(server, pane, new).await?;
    if let Some(w) = app.dispatcher.watch.lock().get_mut(&server.key(pane)) {
        w.subs.clear();                                // a new agent: none of the last one's
    }
    server.run(format!("respawn-pane -k -t {pane} {}", window_command(folder, command, &tmux_of(server)))).await.map(drop).map_err(|e| format!("tmux did not start it: {e}"))
}
