// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Dispatcher's walk (`terminum-dispatcher.md` AC-131): every beat it reads tmux's panes and one `ps`,
//! then in one transaction adds the time it saw pass to each run's clocks and does what `plan` says,
//! so what came meanwhile (a human's move, a hook) stands. Typing and closing come after it.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::acts::{self, Later};
use super::plan::{self, Act, Facts};
use super::runs::{self, Run};
use super::{move_task, procs, say, scale, start, store, BEAT, GAP};
use crate::app::Shared;
use crate::tasks::ops;
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

/// What the list says of a pane: waiting, failed, and its turn ended: an interrupt fires no Stop, its screen says so (AC-123).
fn listed(app: &Shared, key: &str) -> (Option<String>, Option<String>, bool) {
    let layout = app.layout.read();
    let item = crate::page::marks::find_item(&layout, key);
    let why = || item.and_then(|it| it["why"].as_str().or(it["tool"].as_str())).unwrap_or("").to_string();
    match item.and_then(|it| it["state"].as_str()) {
        Some("waiting") => (Some(why()), None, false),
        Some("failed") => (None, Some(why()), false),
        Some("done") => (None, None, true),
        _ => (None, None, false),
    }
}

/// What is read of one run's pane before the transaction.
struct Read {
    id: String,
    facts: Facts,
    ended: bool,
}

/// What a beat's transaction leaves: acts, tasks changed, the budget used and whole by task.
type Walked = (Vec<Later>, Vec<String>, HashMap<String, (i64, i64)>);

/// In one transaction: every run as it stands now, its clocks, what `plan` says. Moves and words are
/// done here; what is not written is not done (AC-131). A run that cannot be written costs only itself, and says so.
fn walk_store(tx: &rusqlite::Transaction, reads: Vec<Read>, now: i64) -> Result<Walked, String> {
    let (mut later, mut changed, mut used) = (Vec::new(), Vec::new(), HashMap::new());
    let live: HashMap<String, Run> = runs::live(tx)?.into_iter().map(|r| (r.id.clone(), r)).collect();
    for read in reads {
        let Some(r) = live.get(&read.id).cloned() else { continue };
        tx.execute_batch("SAVEPOINT one_run").map_err(ops::sql)?;
        let walked = walk_one(tx, r.clone(), read, now);
        tx.execute_batch(if walked.is_ok() { "RELEASE one_run" } else { "ROLLBACK TO one_run; RELEASE one_run" }).map_err(ops::sql)?;
        match walked {
            Ok((l, c, u)) => {
                later.extend(l);
                changed.extend(c);
                used.extend(u);
            }
            Err(e) => {
                tracing::warn!(event = "dispatcher.walk", error = %e);
                if said_once(tx, &r, &format!("Dispatcher could not go on with this run: {e}"))? {
                    changed.extend(r.ids.iter().cloned());
                }
            }
        }
    }
    Ok((later, changed, used))
}

/// Says `words` unless the last thing said on its first task says them.
fn said_once(tx: &rusqlite::Transaction, r: &Run, words: &str) -> Result<bool, String> {
    let last: Option<String> = tx.query_row("SELECT text FROM events WHERE task = ?1 ORDER BY at DESC, rowid DESC LIMIT 1", [&r.task], |x| x.get(0)).ok();
    if last.as_deref() == Some(words) {
        return Ok(false);
    }
    say(tx, r, words).map(|()| true)
}

fn walk_one(tx: &rusqlite::Transaction, mut r: Run, read: Read, now: i64) -> Result<Walked, String> {
    let (mut later, mut changed, mut used) = (Vec::new(), Vec::new(), HashMap::new());
    if r.is_starting() {
        return Ok((later, changed, used));
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
        Some(key) => runs::others_open(tx, key, &r.task)?,
        None => false,
    };
    r.last_walk = now;
    let mut acts = plan::step(&mut r, &facts);
    if seen > GAP {
        acts.push(Act::Say(format!("Clocks paused {} minutes: no walk saw them pass", seen / 60)));
    }
    if let Some(total) = r.budget {
        used.extend(r.ids.iter().map(|id| (id.clone(), (r.budget_used, total))));
    }
    runs::save(tx, &r)?;
    if facts.pane && !facts.marked {
        for id in &r.ids {
            ops::link(tx, super::DISPATCHER, id, None, now)?;   // a stale link drops (AC-125)
        }
    }
    for act in acts {
        match act {
            Act::Move { to, words } => {
                move_task(tx, &r, to, &words)?;
                r.status = to.to_string();
                changed.extend(r.ids.iter().cloned());
            }
            Act::Say(words) => {
                say(tx, &r, &words)?;
                changed.extend(r.ids.iter().cloned());
            }
            act => later.push(Later { run: r.clone(), act }),
        }
    }
    Ok((later, changed, used))
}

/// What one walk read of a server: its panes, and what runs under them, if asked and answered.
struct Seen {
    panes: Option<HashMap<String, Pane>>,
    tree: Option<procs::Tree>,
    /// its `ps` was not asked this beat: a host with no run is read less often
    skipped: bool,
}

/// A host with no run of Dispatcher's is read one beat in this many, and only with a page open.
const IDLE_HOST_BEATS: u64 = 4;

/// Every wanted pane's server: its list first, and only where one stands its `ps`, so a stale link
/// or a dead host costs no ssh; each on its own time, a slow host delaying only itself.
async fn read_servers(app: &Shared, wanted: &[String], with_run: &HashSet<String>) -> HashMap<String, Seen> {
    let mut servers: HashMap<String, (Arc<Server>, Vec<String>)> = HashMap::new();
    for (server, pane) in wanted.iter().filter_map(|k| app.tmux.resolve(k)) {
        servers.entry(server.id()).or_insert_with(|| (server, Vec::new())).1.push(pane);
    }
    let beat = app.dispatcher.beat.fetch_add(1, Ordering::Relaxed);
    let read = servers.into_values().map(|(server, mine)| async move {
        let panes = panes_of(&server).await;
        let wants = panes.as_ref().is_some_and(|list| mine.iter().any(|p| list.contains_key(p)));
        let due = server.is_local() || with_run.contains(&server.id()) || (beat.is_multiple_of(IDLE_HOST_BEATS) && app.pages.count() > 0);
        let tree = if wants && due { procs::read(app, &server).await } else { None };
        (server.id(), Seen { panes, tree, skipped: wants && !due })
    });
    futures_util::future::join_all(read).await.into_iter().collect()
}

/// What a card shows of its session (AC-128, AC-129): as of this walk, with its budget's time.
fn usage_line(app: &Shared, key: &str, seen: &Seen, pane: &str) -> serde_json::Value {
    let mut watch = app.dispatcher.watch.lock();
    let w = watch.entry(key.to_string()).or_default();
    let idle: Vec<serde_json::Value> = w.subs.iter().filter(|(_, s)| s.told != plan::SubStep::Nothing).map(|(id, s)| serde_json::json!({"id": id, "kind": s.kind})).collect();
    let mut v = serde_json::json!({"idle_subs": idle});
    if let Some(t) = &w.tokens {
        v["tokens"] = t.clone();
    }
    if seen.skipped {
        if let Some(last) = &w.line {
            for k in ["cpu", "gib", "minutes", "not_measured"] {
                if !last[k].is_null() {
                    v[k] = last[k].clone();
                }
            }
        }
        return v;
    }
    let under = seen.panes.as_ref().and_then(|l| l.get(pane)).zip(seen.tree.as_ref()).map(|(p, t)| procs::under(t, &p.pid));
    match under {
        Some(u) => {
            let now = Instant::now();
            let cpu = w.cpu.replace((u.cpu, now)).map_or(0.0, |(was, at)| (u.cpu - was).max(0.0) / at.elapsed().as_secs_f64().max(0.1) * 100.0);
            v["cpu"] = serde_json::json!(cpu.round());
            v["gib"] = serde_json::json!((u.gib() * 10.0).round() / 10.0);
            v["minutes"] = serde_json::json!(u.age / 60);
        }
        None => v["not_measured"] = serde_json::json!(if seen.panes.is_none() { "Host out of reach" } else { "Not measured: ps did not answer" }),
    }
    w.line = Some(v.clone());
    v
}

/// One beat.
async fn walk(app: &Shared) {
    let a = app.clone();
    let Ok((all, linked, limits)) = store(app, move |tx| Ok((
        a.dispatcher.read_runs(tx)?,
        ops::rows(tx, "SELECT slug, session_key FROM tasks WHERE session_key IS NOT NULL AND archived_at IS NULL", [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?,
        crate::tasks::limits::limits(tx)?,
    ))).await else { return };
    if all.is_empty() && linked.is_empty() {
        return;
    }
    let limits: HashMap<String, (f64, f64)> = limits.into_iter().map(|(project, warn, end)| (project, (warn, end))).collect();
    let (global_warn, global_end) = { let s = app.looks.settings.read(); (s.memory_warn_gib, s.memory_end_gib) };
    let with_run: HashSet<String> = all.iter().filter_map(|r| r.place(app)).map(|(s, _)| s.id()).collect();
    let wanted: Vec<String> = all.iter().filter_map(|r| r.pane_key.clone()).chain(linked.iter().map(|(_, k)| k.clone())).collect();
    let seen = read_servers(app, &wanted, &with_run).await;
    let now = ops::now();
    let (mut reads, mut subagents) = (Vec::new(), Vec::new());
    for r in &all {
        if r.is_starting() {
            start::watch(app, &r.id);
            continue;
        }
        // no server for its key, or out of reach: nothing is known of its pane, and no clock runs (AC-130)
        let Some((server, pane)) = r.place(app) else { continue };
        let Some(s) = seen.get(&server.id()) else { continue };
        let Some(list) = &s.panes else { continue };
        let key = server.key(&pane);
        let p = list.get(&pane);
        let measured = s.tree.as_ref().zip(p).map(|(t, p)| procs::under(t, &p.pid));
        // ps did not answer: the agent stands while tmux says no exit, and no limit fires on missing numbers
        let under = measured.as_ref().map_or(p.is_some_and(|p| p.exit.is_none()), |u| u.any);
        let (busy, memory, under) = {
            let mut watch = app.dispatcher.watch.lock();
            let w = watch.entry(key.clone()).or_default();
            // gone with no exit recorded: only a second reading says so, the wrapper records it a moment after
            let unsure = !under && p.is_some_and(|p| p.exit.is_none()) && !std::mem::replace(&mut w.agent_missed, !under);
            if unsure {
                app.dispatcher.soon.notify_one();
            }
            let under = under || unsure;
            let busy = measured.as_ref().is_some_and(|u| w.cpu.is_some_and(|(was, _)| u.cpu - was >= CPU_ACTIVE / scale() as f64));
            let memory = measured.as_ref().map(|u| {
                let (warn, end) = limits.get(&r.project).copied().unwrap_or((global_warn, global_end));
                plan::Memory::read(u.gib(), warn, end, &mut w.over)
            });
            (busy, memory, under)
        };
        let (waiting, failed, ended) = listed(app, &key);
        // after its agent, what runs in front is a human's: it stays theirs. A job behind the shell keeps
        // the 2 hours from running: a prompt's own helpers come and go, and must not leave it open for good
        let program = p.filter(|p| p.exit.is_some() && !is_shell(&p.command)).map(|p| p.command.clone());
        let job = under && p.is_some_and(|p| p.exit.is_some());
        let facts = Facts {
            pane: p.is_some(),
            marked: p.is_some_and(|p| p.mark == r.id),
            exit: p.and_then(|p| p.exit),
            alive: under && p.is_some_and(|p| p.exit.is_none()),
            waiting,
            failed,
            active: app.dispatcher.take_active(&key) || busy || job,
            program,
            memory,
            ..Default::default()
        };
        // + Session's is the human's: nothing is typed in it, about its subagents either (AC-142)
        if facts.alive && !r.ad_hoc() {
            subagents.push((r.clone(), server.clone(), pane.clone(), key, busy));
        }
        reads.push(Read { id: r.id.clone(), facts, ended });
    }
    let (later, changed, used) = match store(app, move |tx| walk_store(tx, reads, now)).await {
        Ok(done) => done,
        Err(e) => return tracing::warn!(event = "dispatcher.walk", error = %e),
    };
    for task in changed.into_iter().collect::<HashSet<_>>() {
        crate::tasks::api::told(app, &task);
    }
    for l in later {
        acts::do_later(app, l).await;
    }
    for (r, server, pane, key, busy) in subagents {
        acts::watch_subagents(app, &r, &server, &pane, &key, busy).await;
    }
    // what a card shows of every linked live session; for others' sessions only the numbers (AC-128)
    let mut items = serde_json::Map::new();
    let mut lines: HashMap<String, serde_json::Value> = HashMap::new();   // one reading a session, its tasks sharing it
    for (task, key) in linked {
        let Some((server, pane)) = app.tmux.resolve(&key) else { continue };
        let Some(s) = seen.get(&server.id()) else { continue };
        if s.panes.as_ref().is_some_and(|l| !l.contains_key(&pane)) {
            continue;
        }
        let mut line = lines.entry(key.clone()).or_insert_with(|| usage_line(app, &key, s, &pane)).clone();
        if let Some((spent, total)) = used.get(&task) {
            line["budget_used"] = serde_json::json!(spent);
            line["budget_total"] = serde_json::json!(total);
        }
        items.insert(task, line);
    }
    // what no walk watches any more goes with it
    let kept: HashSet<&str> = wanted.iter().map(String::as_str).collect();
    app.dispatcher.watch.lock().retain(|k, _| kept.contains(k.as_str()));
    let slots = store(app, super::pool::board).await.unwrap_or(serde_json::Value::Null);
    let message = serde_json::json!({"t": "taskusage", "items": items, "slots": slots}).to_string();
    let mut told = app.dispatcher.usage.lock();
    if *told != message {
        app.pages.tell(&message);
        *told = message;
    }
}

/// What has been DONE for 2 days leaves the board (terminum-kanban.md AC-118): it is tried again at the next beat.
async fn archive_due(app: &Shared) {
    match store(app, |tx| crate::tasks::archiving::due(tx, ops::now())).await {
        Ok(gone) => gone.iter().for_each(|task| crate::tasks::api::told(app, task)),
        Err(e) => tracing::warn!(event = "dispatcher.archive", error = %e),
    }
}

/// The supervised walk: a walk still running makes the next wait; a panic is said and takes nothing down.
pub async fn run(app: Shared) {
    if app.cfg.agent {
        return;
    }
    loop {
        let a = app.clone();
        if let Err(e) = tokio::spawn(async move { archive_due(&a).await; walk(&a).await }).await {
            tracing::error!(event = "dispatcher.panic", error = %e);
            acts::note(&app, "Dispatcher's walk failed: see terminumd's log. It goes on at the next beat.".to_string());
        }
        tokio::select! {
            () = tokio::time::sleep((BEAT / scale() as u32).max(Duration::from_millis(200))) => {}
            () = app.dispatcher.soon.notified() => {}
        }
    }
}
