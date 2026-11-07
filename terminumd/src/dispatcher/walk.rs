// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Dispatcher's walk (`terminum-dispatcher.md` AC-131): every beat it reads its runs, tmux's panes
//! and one `ps`, adds what time it saw pass to each run's clocks, and does what `plan` says.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::plan::{self, Act, Facts};
use super::runs::{self, Run};
use super::{gate, hooks, move_task, say, scale, scaled, start, write, BEAT, GAP};
use crate::app::Shared;
use crate::tasks::{files, ops};
use crate::tmux::{args, Server};

/// CPU seconds in a beat over which an agent's tree counts as active (§9 Activity, AS-127).
const CPU_ACTIVE: f64 = 3.0;

#[derive(Clone, Debug, Default)]
struct Pane {
    mark: String,
    exit: Option<i32>,
    pid: String,
    command: String,
}

async fn panes_of(server: &Server) -> Option<HashMap<String, Pane>> {
    let lines = server.run("list-panes -a -F '#{pane_id}\t#{@terminum-run}\t#{@terminum-exit}\t#{pane_pid}\t#{pane_current_command}'").await.ok()?;
    Some(lines.iter().filter_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        (f.len() == 5).then(|| (f[0].to_string(), Pane { mark: f[1].to_string(), exit: f[2].parse().ok(), pid: f[3].to_string(), command: f[4].to_string() }))
    }).collect())
}

/// This Mac's processes: children by parent, each with its CPU seconds; arguments are never read (§9).
type Tree = HashMap<String, Vec<(String, f64)>>;

fn parse_time(t: &str) -> f64 {
    t.split([':', '-']).fold(0.0, |sum, part| sum * 60.0 + part.parse::<f64>().unwrap_or(0.0))
}

fn read_tree(out: &str) -> Tree {
    let mut tree = Tree::new();
    for line in out.lines() {
        let mut f = line.split_whitespace();
        if let (Some(pid), Some(ppid), Some(time)) = (f.next(), f.next(), f.next()) {
            tree.entry(ppid.to_string()).or_default().push((pid.to_string(), parse_time(time)));
        }
    }
    tree
}

async fn local_tree() -> Option<Tree> {
    let out = tokio::process::Command::new("ps").args(["-Ao", "pid=,ppid=,time="]).output().await.ok()?;
    out.status.success().then(|| read_tree(&String::from_utf8_lossy(&out.stdout)))
}

/// Whether anything runs under `pid`, and the CPU seconds of what does.
fn below(tree: &Tree, pid: &str) -> (bool, f64) {
    let (mut any, mut sum, mut stack) = (false, 0.0, vec![pid.to_string()]);
    while let Some(p) = stack.pop() {
        for (kid, cpu) in tree.get(&p).into_iter().flatten() {
            any = true;
            sum += cpu;
            stack.push(kid.clone());
        }
    }
    (any, sum)
}

fn is_shell(name: &str) -> bool {
    matches!(name.trim_start_matches('-').rsplit('/').next().unwrap_or(""), "sh" | "bash" | "zsh" | "fish" | "dash" | "ksh" | "tcsh" | "csh" | "nu" | "login")
}

/// The list's row of `key`: what it waits for, why it failed, and whether its turn ended: an
/// interrupt fires no Stop, its screen says so (AC-123).
fn listed(app: &Shared, key: &str) -> (Option<String>, Option<String>, bool) {
    let item = crate::page::marks::find_item(&app.layout.read(), key);
    let why = item.as_ref().and_then(|it| it["why"].as_str().or(it["tool"].as_str())).unwrap_or("").to_string();
    match item.as_ref().and_then(|it| it["state"].as_str()) {
        Some("waiting") => (Some(why), None, false),
        Some("failed") => (None, Some(why), false),
        Some("done") => (None, None, true),
        _ => (None, None, false),
    }
}

async fn do_acts(app: &Shared, run: &Run, acts: Vec<Act>, server: &Arc<Server>, pane: &str) {
    let changed = !acts.is_empty();
    for act in acts {
        let r = run.clone();
        let done = match act {
            Act::Move { to, words } => write(app, move |tx| move_task(tx, &r, to, &words)).await,
            Act::Say(words) => write(app, move |tx| say(tx, &r, &words)).await,
            Act::Nudge => {
                let text = format!("[terminum Dispatcher] {} is still open and you stopped. Call finish_task with the result, or ask what you need.", run.task_id);
                match gate::deliver(app, run, server, pane, &text, false).await {
                    Ok(()) => Ok(()),
                    Err(why) => write(app, move |tx| hooks::not_delivered(tx, &r, &why)).await,
                }
            }
            Act::Close { words, keep } => close(app, run, server, pane, &words, keep).await,
        };
        if let Err(e) = done {
            tracing::warn!(event = "dispatcher.act", task = %run.task_id, error = %e);
        }
    }
    if changed {
        super::heard_change(app, &run.task_id);
    }
}

/// Closes the pane: its mark checked just before; with no result the screen's last 200 rows stay on the task.
async fn close(app: &Shared, run: &Run, server: &Arc<Server>, pane: &str, words: &str, keep: bool) -> Result<(), String> {
    let mine = server.run(format!("display -p -t {pane} '#{{@terminum-run}}'")).await.map_err(|e| e.to_string())?;
    if mine.first().map(String::as_str) != Some(run.id.as_str()) {
        return Err("its pane is not this run's".into());
    }
    if keep {
        let rows = server.run(format!("capture-pane -p -S -200 -t {pane}")).await.unwrap_or_default();
        let text = crate::sent::redact(&rows.join("\n"));
        if !text.trim().is_empty() {
            let (a, task, name, at) = (app.clone(), run.task_id.clone(), format!("{}-screen.txt", run.task_id), ops::now());
            let kept = crate::http::off(move || {
                let f = files::keep(&a.tasks, &a.cfg.dir, &name, text.as_bytes(), at)?;
                a.tasks.with(|tx| files::attach(tx, std::slice::from_ref(&f.id), &task, None))
            }).await;
            if let Err(e) = kept {
                tracing::warn!(event = "dispatcher.close", task = %run.task_id, error = %e);
            }
        }
    }
    let r = run.clone();
    let closed = server.run(format!("kill-pane -t {}", args::word(pane))).await;
    let words = match closed {
        Ok(_) => words.to_string(),
        Err(e) => format!("Session not closed: {e}"),
    };
    write(app, move |tx| say(tx, &r, &words)).await
}

/// One beat.
async fn walk(app: &Shared) {
    let Ok(all) = write(app, runs::live).await else { return };
    if all.is_empty() {
        return;
    }
    let now = ops::now();
    let tree = local_tree().await;
    let mut panes: HashMap<String, Option<HashMap<String, Pane>>> = HashMap::new();
    for mut r in all {
        if matches!(r.state.as_str(), "starting" | "asking" | "typed") {
            start::watch(app, &r.id);
            continue;
        }
        let Some((server, pane)) = r.pane_key.as_deref().and_then(|k| app.tmux.resolve(k)) else {
            // no server for its key: it says nothing of the pane, and no clock runs (AC-130)
            r.last_walk = now;
            let saved = r.clone();
            let _ = write(app, move |tx| runs::save(tx, &saved)).await;
            continue;
        };
        if let std::collections::hash_map::Entry::Vacant(place) = panes.entry(server.id()) {
            place.insert(panes_of(&server).await);
        }
        let Some(read) = panes.get(&server.id()).and_then(|p| p.as_ref()) else { continue };   // out of reach: nothing is known (AC-130)
        let key = server.key(&pane);
        let p = read.get(&pane).cloned();
        let seen = now - r.last_walk;
        let dt = if seen > (GAP / scale()).max(1) { 0 } else { seen.max(0) * scale() };
        let (alive, cpu) = match (&tree, &p) {
            (Some(t), Some(p)) if server.is_local() => below(t, &p.pid),
            _ => (p.as_ref().is_some_and(|p| p.exit.is_none()), 0.0),
        };
        let busy = {
            let mut last = app.dispatcher.cpu.lock();
            let was = last.insert(r.id.clone(), cpu).unwrap_or(cpu);
            cpu - was >= CPU_ACTIVE / scale() as f64
        };
        let (waiting, failed, ended) = listed(app, &key);
        r.turn_ended |= ended;
        let (task, k) = (r.task.clone(), key.clone());
        let others_open: bool = write(app, move |tx| {
            tx.query_row("SELECT EXISTS (SELECT 1 FROM tasks WHERE session_key = ?1 AND id != ?2 AND status NOT IN ('DONE', 'FAILED') AND archived_at IS NULL)", (&k, &task), |x| x.get(0)).map_err(ops::sql)
        }).await.unwrap_or(true);
        let human_program = p.as_ref().filter(|p| p.exit.is_some() && !is_shell(&p.command)).map(|p| p.command.clone());
        let facts = Facts {
            pane: p.is_some(),
            marked: p.as_ref().is_some_and(|p| p.mark == r.id),
            exit: p.as_ref().and_then(|p| p.exit),
            alive: alive && p.as_ref().is_some_and(|p| p.exit.is_none()),
            waiting,
            failed,
            active: app.dispatcher.take_active(&key) || busy || r.open_tools > 0,
            program: human_program,
            others_open,
            dt,
        };
        r.last_walk = now;
        let mut acts = plan::step(&mut r, &facts);
        if seen > GAP {
            acts.push(Act::Say(format!("Clocks paused {} minutes: no walk saw them pass", seen / 60)));
        }
        let saved = r.clone();
        if let Err(e) = write(app, move |tx| runs::save(tx, &saved)).await {
            // what it could not write it does not do (AC-131)
            tracing::warn!(event = "dispatcher.walk", task = %r.task_id, error = %e);
            continue;
        }
        do_acts(app, &r, acts, &server, &pane).await;
    }
}

/// The supervised walk: a walk still running makes the next wait; a panic is said and takes nothing down.
pub async fn run(app: Shared) {
    if app.cfg.agent {
        return;
    }
    loop {
        let a = app.clone();
        if let Err(e) = tokio::spawn(async move { walk(&a).await }).await {
            tracing::error!(event = "dispatcher.panic", error = %e);
            app.pages.tell(&serde_json::json!({"t": "note", "msg": "Dispatcher's walk failed: see terminumd's log. It goes on at the next beat."}).to_string());
        }
        tokio::select! {
            () = tokio::time::sleep(scaled(BEAT).max(Duration::from_millis(200))) => {}
            () = app.dispatcher.soon.notified() => {}
        }
    }
}
