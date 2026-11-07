// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Dispatcher's walk (`terminum-dispatcher.md` AC-131): every beat it reads tmux's panes and one `ps`,
//! then in one transaction adds the time it saw pass to each run's clocks and does what `plan` says,
//! so what came meanwhile (a human's move, a hook) stands. Typing and closing come after it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use super::plan::{self, Act, Facts};
use super::runs::{self, Run};
use super::{gate, move_task, say, scale, start, store, BEAT, GAP};
use crate::app::Shared;
use crate::tasks::{files, ops};
use crate::tmux::Server;

/// CPU seconds in a beat over which an agent's tree counts as active (§9 Activity, AS-127).
const CPU_ACTIVE: f64 = 3.0;
/// How long one server's list is waited for: a slow host delays only itself.
const LIST_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default)]
struct Pane {
    mark: String,
    exit: Option<i32>,
    pid: String,
    command: String,
}

async fn panes_of(server: &Server) -> Option<HashMap<String, Pane>> {
    let lines = tokio::time::timeout(LIST_WAIT, server.run("list-panes -a -F '#{pane_id}\t#{@terminum-run}\t#{@terminum-exit}\t#{pane_pid}\t#{pane_current_command}'")).await.ok()?.ok()?;
    Some(lines.iter().filter_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        (f.len() == 5).then(|| (f[0].to_string(), Pane { mark: f[1].to_string(), exit: f[2].parse().ok(), pid: f[3].to_string(), command: f[4].to_string() }))
    }).collect())
}

/// This Mac's processes: children by parent, each with its CPU seconds; arguments are never read (§9).
pub(super) type Tree = HashMap<String, Vec<(String, f64)>>;

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

pub(super) async fn local_tree() -> Option<Tree> {
    let out = tokio::process::Command::new("ps").args(["-Ao", "pid=,ppid=,time="]).output().await.ok()?;
    out.status.success().then(|| read_tree(&String::from_utf8_lossy(&out.stdout)))
}

/// Whether anything runs under `pid`, and the CPU seconds of what does.
pub(super) fn below(tree: &Tree, pid: &str) -> (bool, f64) {
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

pub(super) fn is_shell(name: &str) -> bool {
    let name = crate::termtext::shell_name(name);
    name == "login" || crate::tmux::titles::SHELLS.contains(&name)
}

/// What the list says of a pane.
enum Listed {
    Waiting(String),
    Failed(String),
    /// its turn ended: an interrupt fires no Stop, its screen says so (AC-123)
    TurnEnded,
    Other,
}

fn listed(app: &Shared, key: &str) -> Listed {
    let item = crate::page::marks::find_item(&app.layout.read(), key);
    let why = || item.as_ref().and_then(|it| it["why"].as_str().or(it["tool"].as_str())).unwrap_or("").to_string();
    match item.as_ref().and_then(|it| it["state"].as_str()) {
        Some("waiting") => Listed::Waiting(why()),
        Some("failed") => Listed::Failed(why()),
        Some("done") => Listed::TurnEnded,
        _ => Listed::Other,
    }
}

/// What is read of one run's pane before the transaction.
struct Read {
    id: String,
    facts: Facts,
    ended: bool,
}

/// What the transaction left to do outside it.
struct Later {
    run: Run,
    act: Act,
}

/// In one transaction: every run as it stands now, its clocks, what `plan` says. Moves and words are
/// done here; what is not written is not done (AC-131).
fn walk_store(tx: &rusqlite::Transaction, reads: Vec<Read>, now: i64) -> Result<(Vec<Later>, Vec<String>), String> {
    let (mut later, mut changed) = (Vec::new(), Vec::new());
    let live: HashMap<String, Run> = runs::live(tx)?.into_iter().map(|r| (r.id.clone(), r)).collect();
    for read in reads {
        let Some(mut r) = live.get(&read.id).cloned() else { continue };
        if r.is_starting() {
            continue;
        }
        let seen = now - r.last_walk;
        let dt = if seen > (GAP / scale()).max(1) { 0 } else { seen.max(0) * scale() };
        let mut facts = read.facts;
        facts.dt = dt;
        facts.active |= r.open_tools > 0;
        r.turn_ended |= read.ended;
        facts.others_open = match r.pane_key.as_deref().filter(|_| plan::finished(&r.status) && r.closing_for + dt >= plan::CLOSE) {
            Some(key) => tx.query_row("SELECT EXISTS (SELECT 1 FROM tasks WHERE session_key = ?1 AND id != ?2 AND status NOT IN ('DONE', 'FAILED') AND archived_at IS NULL)", (key, &r.task), |x| x.get(0)).map_err(ops::sql)?,
            None => false,
        };
        r.last_walk = now;
        let mut acts = plan::step(&mut r, &facts);
        if seen > GAP {
            acts.push(Act::Say(format!("Clocks paused {} minutes: no walk saw them pass", seen / 60)));
        }
        runs::save(tx, &r)?;
        if facts.pane && !facts.marked {
            ops::link(tx, super::DISPATCHER, &r.task_id, None, now)?;   // a stale link drops (AC-125)
        }
        for act in acts {
            match act {
                Act::Move { to, words } => {
                    move_task(tx, &r, to, &words)?;
                    r.status = to.to_string();
                    changed.push(r.task_id.clone());
                }
                Act::Say(words) => {
                    say(tx, &r, &words)?;
                    changed.push(r.task_id.clone());
                }
                act => later.push(Later { run: r.clone(), act }),
            }
        }
    }
    Ok((later, changed))
}

/// Closes the pane: its mark checked just before; with no result the screen's last 200 rows stay on
/// the task. tmux refusing: one more try at the next beat, then it stays and says so (AC-125).
async fn close(app: &Shared, run: &Run, server: &Arc<Server>, pane: &str, words: &str, keep: bool) -> Result<(), String> {
    start::is_marked(server, pane, &run.id).await?;
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
    let (id, words) = (run.id.clone(), words.to_string());
    match server.run(format!("kill-pane -t {pane}")).await {
        Ok(_) => store(app, move |tx| {
            let r = runs::change(tx, &id, |r| r.state = "closed".into())?;
            say(tx, &r, &words)
        }).await,
        Err(e) => store(app, move |tx| {
            let r = runs::change(tx, &id, |r| if r.close_tried { r.state = "left".into(); } else { r.close_tried = true; })?;
            if r.state == "left" { say(tx, &r, &format!("Session not closed: {e}. It stays open.")) } else { Ok(()) }
        }).await,
    }
}

async fn do_later(app: &Shared, l: Later) {
    let Some((server, pane)) = l.run.pane_key.as_deref().and_then(|k| app.tmux.resolve(k)) else { return };
    let task = l.run.task_id.clone();
    let done = match l.act {
        Act::Nudge => {
            let text = format!("[terminum Dispatcher] {task} is still open and you stopped. Call finish_task with the result, or ask what you need.");
            match gate::deliver(app, &l.run, &server, &pane, &text, false).await {
                Ok(()) => Ok(()),
                Err(why) => {
                    let r = l.run.clone();
                    store(app, move |tx| say(tx, &r, &format!("Not delivered: {why}"))).await
                }
            }
        }
        Act::Close { words, keep } => close(app, &l.run, &server, &pane, &words, keep).await,
        _ => Ok(()),
    };
    if let Err(e) = done {
        tracing::warn!(event = "dispatcher.act", task = %task, error = %e);
    }
    crate::tasks::api::told(app, &task);
}

/// One beat.
async fn walk(app: &Shared) {
    let Ok(all) = store(app, runs::live).await else { return };
    *app.dispatcher.panes.lock() = all.iter().filter_map(|r| r.pane_key.clone()).collect();
    if all.is_empty() {
        return;
    }
    let tree = local_tree().await;
    let servers: HashMap<String, Arc<Server>> = all.iter().filter_map(|r| r.pane_key.as_deref().and_then(|k| app.tmux.resolve(k))).map(|(s, _)| (s.id(), s)).collect();
    let lists = futures_util::future::join_all(servers.values().map(|s| async move { (s.id(), panes_of(s).await) })).await;
    let lists: HashMap<String, Option<HashMap<String, Pane>>> = lists.into_iter().collect();
    let mut reads = Vec::new();
    for r in &all {
        if r.is_starting() {
            start::watch(app, &r.id);
            continue;
        }
        // no server for its key, or out of reach: nothing is known of its pane, and no clock runs (AC-130)
        let Some((server, pane)) = r.pane_key.as_deref().and_then(|k| app.tmux.resolve(k)) else { continue };
        let Some(Some(list)) = lists.get(&server.id()) else { continue };
        let key = server.key(&pane);
        let p = list.get(&pane);
        let (under, cpu) = match (&tree, p) {
            (Some(t), Some(p)) if server.is_local() => below(t, &p.pid),
            _ => (p.is_some_and(|p| p.exit.is_none()), 0.0),
        };
        let busy = {
            let mut last = app.dispatcher.cpu.lock();
            let was = last.insert(r.id.clone(), cpu).unwrap_or(cpu);
            cpu - was >= CPU_ACTIVE / scale() as f64
        };
        let (waiting, failed, ended) = match listed(app, &key) {
            Listed::Waiting(why) => (Some(why), None, false),
            Listed::Failed(why) => (None, Some(why), false),
            Listed::TurnEnded => (None, None, true),
            Listed::Other => (None, None, false),
        };
        // after its agent, anything running there is a human's: in front, or stopped behind the shell
        let program = p.filter(|p| p.exit.is_some()).and_then(|p| {
            if !is_shell(&p.command) { Some(p.command.clone()) } else if under { Some("a job of its shell".to_string()) } else { None }
        });
        let facts = Facts {
            pane: p.is_some(),
            marked: p.is_some_and(|p| p.mark == r.id),
            exit: p.and_then(|p| p.exit),
            alive: under && p.is_some_and(|p| p.exit.is_none()),
            waiting,
            failed,
            active: app.dispatcher.take_active(&key) || busy,
            program,
            ..Default::default()
        };
        reads.push(Read { id: r.id.clone(), facts, ended });
    }
    let now = ops::now();
    let (later, changed) = match store(app, move |tx| walk_store(tx, reads, now)).await {
        Ok(done) => done,
        Err(e) => return tracing::warn!(event = "dispatcher.walk", error = %e),
    };
    for task in changed.into_iter().collect::<HashSet<_>>() {
        crate::tasks::api::told(app, &task);
    }
    for l in later {
        do_later(app, l).await;
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
            () = tokio::time::sleep((BEAT / scale() as u32).max(Duration::from_millis(200))) => {}
            () = app.dispatcher.soon.notified() => {}
        }
    }
}
