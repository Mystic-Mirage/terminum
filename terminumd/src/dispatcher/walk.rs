// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Dispatcher's walk (`terminum-dispatcher.md` AC-131): every beat it reads tmux's panes and one `ps`,
//! then in one transaction adds the time it saw pass to each run's clocks and does what `plan` says,
//! so what came meanwhile (a human's move, a hook) stands. Typing and closing come after it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use super::plan::{self, Act, Facts};
use super::runs::{self, Run};
use super::{gate, move_task, procs, say, scale, start, store, BEAT, GAP};
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
    used: i64,
}

/// What the transaction left to do outside it.
struct Later {
    run: Run,
    act: Act,
}

/// What a beat's transaction leaves: acts, tasks changed, tasks whose minutes a card shows.
type Walked = (Vec<Later>, Vec<String>, Vec<String>);

/// In one transaction: every run as it stands now, its clocks, what `plan` says. Moves and words are
/// done here; what is not written is not done (AC-131).
fn walk_store(tx: &rusqlite::Transaction, reads: Vec<Read>, now: i64) -> Result<Walked, String> {
    let (mut later, mut changed, mut minutes) = (Vec::new(), Vec::new(), Vec::new());
    let live: HashMap<String, Run> = runs::live(tx)?.into_iter().map(|r| (r.id.clone(), r)).collect();
    for read in reads {
        let Some(mut r) = live.get(&read.id).cloned() else { continue };
        if r.is_starting() {
            continue;
        }
        let seen = now - r.last_walk;
        if seen > GAP {
            r.idle_for = 0;                            // it reads the agent's state first: no ending act for one idle period (AC-130, AC-131)
        }
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
        // a card shows the time used: every page hears once a minute
        if r.budget.is_some() && (r.budget_used / 60) != (read.used / 60) {
            minutes.push(r.task_id.clone());
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
    Ok((later, changed, minutes))
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

/// The agent and what runs under it: the pane's mark checked just before (AC-126).
async fn end_agent(app: &Shared, run: &Run, server: &Arc<Server>, pane: &str) {
    if start::is_marked(server, pane, &run.id).await.is_err() {
        return;
    }
    let Ok(said) = server.run(format!("display -p -t {pane} '#{{pane_pid}}'")).await else { return };
    let Some(root) = said.first().cloned() else { return };
    procs::end(app, server, &root, (Duration::from_secs(10) / scale() as u32).max(Duration::from_millis(500))).await;
}

/// Words for the agent: on its next tool call while it works, through the gate while it idles.
async fn tell(app: &Shared, run: &Run, server: &Arc<Server>, pane: &str, text: String, what: &str) {
    let key = server.key(pane);
    let words = if app.hooks.working(&key) || run.open_tools > 0 {
        app.task_lines.push(&key, &run.task_id, text);
        format!("{what} waits for the agent's next tool call.")
    } else {
        match gate::deliver(app, run, server, pane, &text, false).await {
            Ok(()) => format!("{what} was given."),
            Err(why) => {
                app.task_lines.push(&key, &run.task_id, text);
                format!("{what} waits for the agent's next tool call: {why}.")
            }
        }
    };
    let r = run.clone();
    let _ = store(app, move |tx| say(tx, &r, &words)).await;
}

async fn do_later(app: &Shared, l: Later) {
    let Some((server, pane)) = l.run.pane_key.as_deref().and_then(|k| app.tmux.resolve(k)) else { return };
    let task = l.run.task_id.clone();
    let done = match l.act {
        Act::Warn(left) => {
            let text = format!("[terminum Dispatcher] {task}: {} minutes of the {}-minute budget are left. Write the result with finish_task and end your subagents now. Then the agent is ended and unsaved work is lost.", (left.max(0) + 59) / 60, l.run.budget.unwrap_or(0) / 60);
            tell(app, &l.run, &server, &pane, text, "The last warning").await;
            Ok(())
        }
        Act::MemoryWarn(gib) => {
            let text = format!("[terminum Dispatcher] {task}: its session uses {gib:.1} GiB of memory. End what it does not need.");
            tell(app, &l.run, &server, &pane, text, "The memory warning").await;
            app.pages.tell(&serde_json::json!({"t": "note", "msg": format!("{task}'s session uses {gib:.1} GiB of memory")}).to_string());
            Ok(())
        }
        Act::End => {
            let key = server.key(&pane);
            if !app.task_lines.take(&key).is_empty() {
                let r = l.run.clone();
                let _ = store(app, move |tx| say(tx, &r, "The last warning could not reach the agent before the limit.")).await;
            }
            let (a, r, s, p) = (app.clone(), l.run.clone(), server.clone(), pane.clone());
            tokio::spawn(async move { end_agent(&a, &r, &s, &p).await });
            Ok(())
        }
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

/// A subagent silent and idle: one request to the main agent, then it is said that it cannot be
/// ended alone: it runs inside the agent (AC-127, OQ-122). A busy tree runs neither clock.
async fn watch_subagents(app: &Shared, run: &Run, server: &Arc<Server>, pane: &str, key: &str, busy: bool) {
    let due: Vec<(String, plan::SubStep)> = {
        let mut subs = app.dispatcher.subs.lock();
        let Some(of_pane) = subs.get_mut(key) else { return };
        of_pane.iter_mut().filter_map(|(id, sub)| {
            if busy || run.open_tools > 0 {
                sub.last = std::time::Instant::now();
                return None;
            }
            let step = plan::subagent_step(sub.last.elapsed().as_secs() as i64 * scale(), sub.told);
            match step {
                plan::SubStep::Nothing => None,
                plan::SubStep::Tell => { sub.told = 1; Some((id.clone(), step)) }
                plan::SubStep::CannotEnd => { sub.told = 2; Some((id.clone(), step)) }
            }
        }).collect()
    };
    for (id, step) in due {
        if step == plan::SubStep::Tell {
            let text = format!("[terminum Dispatcher] A subagent of {} has been silent for 10 minutes. End it if it is stuck.", run.task_id);
            tell(app, run, server, pane, text, "The request about a silent subagent").await;
        } else {
            let (r, words) = (run.clone(), format!("Subagent {} cannot be ended alone: it runs inside the agent", crate::plans::rules::clean(&id)));
            app.pages.tell(&serde_json::json!({"t": "note", "msg": format!("{}: {words}", run.task_id)}).to_string());
            let _ = store(app, move |tx| say(tx, &r, &words)).await;
        }
        crate::tasks::api::told(app, &run.task_id);
    }
}

/// What a project's session may use: its project's limits where set, else the global ones (AC-128).
fn limits_of(tx: &rusqlite::Transaction) -> Result<HashMap<String, (f64, f64)>, String> {
    Ok(ops::rows(tx, "SELECT project, warn_gib, end_gib FROM project_limits", [], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?.into_iter().collect())
}

/// A line of `taskusage`: what a card shows of its session (AC-128, AC-129).
fn usage_line(app: &Shared, key: &str, seen: Option<(f64, procs::Under)>, far: bool) -> serde_json::Value {
    let tokens = app.dispatcher.tokens.lock().get(key).cloned();
    let idle: Vec<serde_json::Value> = app.dispatcher.subs.lock().get(key).map(|m| m.iter().filter(|(_, s)| s.told > 0).map(|(id, s)| serde_json::json!({"id": crate::plans::rules::clean(id), "kind": crate::plans::rules::clean(&s.kind)})).collect()).unwrap_or_default();
    let mut v = serde_json::json!({"idle_subs": idle});
    if let Some(t) = tokens {
        v["tokens"] = t;
    }
    match seen {
        Some((cpu, u)) => {
            v["cpu"] = serde_json::json!(cpu.round());
            v["gib"] = serde_json::json!((u.rss_kib as f64 / 1_048_576.0 * 10.0).round() / 10.0);
            v["minutes"] = serde_json::json!(u.age / 60);
        }
        None => v["not_measured"] = serde_json::json!(if far { "Host out of reach" } else { "Not measured: ps did not answer" }),
    }
    v
}

/// One beat.
async fn walk(app: &Shared) {
    let Ok((all, linked, limits)) = store(app, |tx| Ok((
        runs::live(tx)?,
        ops::rows(tx, "SELECT 'T-' || number, session_key FROM tasks WHERE session_key IS NOT NULL AND archived_at IS NULL", [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?,
        limits_of(tx)?,
    ))).await else { return };
    *app.dispatcher.panes.lock() = all.iter().filter_map(|r| r.pane_key.clone()).collect();
    if all.is_empty() && linked.is_empty() {
        return;
    }
    let (global_warn, global_end) = { let s = app.looks.settings.read(); (s.memory_warn_gib, s.memory_end_gib) };
    let keys = all.iter().filter_map(|r| r.pane_key.clone()).chain(linked.iter().map(|(_, k)| k.clone()));
    let servers: HashMap<String, Arc<Server>> = keys.filter_map(|k| app.tmux.resolve(&k)).map(|(s, _)| (s.id(), s)).collect();
    let read = futures_util::future::join_all(servers.values().map(|s| async move { (s.id(), panes_of(s).await, procs::read(app, s).await) })).await;
    let (mut lists, mut trees) = (HashMap::new(), HashMap::new());
    for (id, list, tree) in read {
        lists.insert(id.clone(), list);
        trees.insert(id, tree);
    }
    let now = ops::now();
    let beat_seconds = (BEAT.as_secs() as f64 / scale() as f64).max(0.2);
    let (mut reads, mut subagents) = (Vec::new(), Vec::new());
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
        let measured = trees.get(&server.id()).and_then(|t| t.as_ref()).zip(p).map(|(t, p)| procs::under(t, &p.pid));
        // ps did not answer: the agent stands while tmux says no exit, and no limit fires on missing numbers
        let under = measured.as_ref().map_or(p.is_some_and(|p| p.exit.is_none()), |u| u.any);
        let busy = measured.as_ref().is_some_and(|u| {
            let mut last = app.dispatcher.cpu.lock();
            let was = last.insert(r.id.clone(), u.cpu).unwrap_or(u.cpu);
            u.cpu - was >= CPU_ACTIVE / scale() as f64
        });
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
        let memory = measured.as_ref().map(|u| {
            let gib = u.rss_kib as f64 / 1_048_576.0;
            let (warn_gib, end_gib) = limits.get(&r.project).copied().unwrap_or((global_warn, global_end));
            let mut over = app.dispatcher.over.lock();
            let count = over.entry(r.id.clone()).or_default();
            count.0 = if warn_gib > 0.0 && gib > warn_gib { count.0 + 1 } else { 0 };
            count.1 = if end_gib > 0.0 && gib > end_gib { count.1 + 1 } else { 0 };
            plan::Memory { gib, warn_gib, end_gib, over_warn: count.0, over_end: count.1 }
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
            memory,
            ..Default::default()
        };
        if facts.alive {
            subagents.push((r.clone(), server.clone(), pane.clone(), key, busy));
        }
        reads.push(Read { id: r.id.clone(), facts, ended, used: r.budget_used });
    }
    let (later, changed, minutes) = match store(app, move |tx| walk_store(tx, reads, now)).await {
        Ok(done) => done,
        Err(e) => return tracing::warn!(event = "dispatcher.walk", error = %e),
    };
    for task in changed.into_iter().collect::<HashSet<_>>() {
        crate::tasks::api::told(app, &task);
    }
    for task in minutes {
        crate::tasks::api::change_heard(app, &task);
    }
    for l in later {
        do_later(app, l).await;
    }
    for (r, server, pane, key, busy) in subagents {
        watch_subagents(app, &r, &server, &pane, &key, busy).await;
    }
    // what a card shows of every linked live session; for others' sessions only the numbers (AC-128)
    let mut items = serde_json::Map::new();
    for (task, key) in linked {
        let Some((server, pane)) = app.tmux.resolve(&key) else { continue };
        let Some(list) = lists.get(&server.id()) else { continue };
        let far = list.is_none();
        if list.as_ref().is_some_and(|l| !l.contains_key(&pane)) {
            continue;
        }
        let seen = list.as_ref().and_then(|l| l.get(&pane)).zip(trees.get(&server.id()).and_then(|t| t.as_ref())).map(|(p, t)| {
            let u = procs::under(t, &p.pid);
            let mut last = app.dispatcher.usage_cpu.lock();
            let was = last.insert(key.clone(), u.cpu).unwrap_or(u.cpu);
            ((u.cpu - was).max(0.0) / beat_seconds * 100.0, u)
        });
        items.insert(task, usage_line(app, &key, seen, far));
    }
    let message = serde_json::json!({"t": "taskusage", "items": items}).to_string();
    let mut told = app.dispatcher.usage.lock();
    if *told != message {
        app.pages.tell(&message);
        *told = message;
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
