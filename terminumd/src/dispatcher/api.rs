// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! RUN's routes (`terminum-dispatcher.md` AC-121, AC-122): what its dialog offers, and the press.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use super::gate::{self, Miss};
use super::runs::{self, Run};
use super::press::{also_of, budget_told, prompt_of, rev_of, shown_of, way, ways, Asked, Way};
use super::pool::{self, Full};
use super::screen::Screen;
use super::{acts, move_one, move_task, procs, say, start, store, walk};
use crate::app::Shared;
use crate::tasks::agents::{self, Agent};
use crate::tasks::api::{answer, told};
use crate::tasks::files;
use crate::tasks::ops::{self, Task};
use crate::tasks::store::{DEFAULT_PROJECT, HUMAN};
use crate::termtext::has_control;
use crate::tmux::Server;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/tasks/{id}/run", get(offer).post(press))
        .route("/api/tasks/{id}/give-up", post(give_up))
        .route("/api/task-projects/{id}/session", post(ad_hoc))
        .route("/api/runs/{id}/let-go", post(let_go))
}

/// What RUN's dialog shows (AC-121, AC-138): the pool, each saying what keeps it busy; an agent named that cannot
/// take them is refused here, so the window never offers another under its name. `no_folder`: nothing can start (AC-140).
async fn offer(State(app): State<Shared>, Path(id): Path<String>, axum::extract::Query(asked): axum::extract::Query<Asked>) -> Response {
    answer(store(&app, move |tx| {
        let (tasks, w) = ways(tx, &id, &also_of(&asked.also), asked.agent.as_deref(), false)?;
        let w = w?;
        let t = &tasks[0];
        let (name, host, dir) = ops::project_place(tx, &t.project)?;
        let all: Vec<Agent> = agents::list(tx)?;
        let lacks = no_folder(&name, &dir).err().filter(|_| matches!(w, Way::First));
        let slots = pool::Slots::read(tx)?;
        let mut offered = Vec::new();
        for a in &all {
            let full = slots.check(tx, Some(a), &t.project, None)?;
            if let (Some(Full { words, close: None }), Some(named), Way::First) = (&full, &asked.agent, &w)
                && named == &a.id {
                return Err(words.clone());
            }
            let mut v = json!(a);
            v["full"] = json!(full);
            offered.push(v);
        }
        if asked.agent.as_ref().is_some_and(|named| !all.iter().any(|a| &a.id == named)) && matches!(w, Way::First) {
            return Err("That agent is gone.".to_string());
        }
        let last = runs::last_agent(tx, &t.project)?.filter(|l| all.iter().any(|a| a.id == *l));
        let shown = shown_of(tx, &tasks)?;
        Ok(json!({"first": matches!(w, Way::First), "agents": offered, "last": last, "dir": dir, "host": host, "project": t.project, "no_folder": lacks,
                  "prompt_rev": rev_of(&shown), "prompt": shown, "by_agent": tasks.iter().any(|t| t.by_agent), "ids": tasks.iter().map(|t| &t.id).collect::<Vec<_>>()}))
    }).await)
}

/// A project with no folder starts nothing: no one knows where (AC-140).
fn no_folder(project: &str, dir: &str) -> Result<(), String> {
    if dir.is_empty() { Err(format!("{project} has no folder: set it in its window.")) } else { Ok(()) }
}

/// Whether terminum's hooks are in place for `kind` on this Mac (AC-121).
fn hooks_there(kind: &str) -> bool {
    if crate::config::test_store() {
        return true;
    }
    crate::cli::Ctx::from_env("terminumd").is_ok_and(|ctx| crate::cli::agentsetup::installed(&ctx, kind))
}

/// Whether `folder` stands on this Mac: off the async workers, as a stale mount can hang.
async fn folder_there(folder: &str) -> bool {
    let path = std::path::PathBuf::from(crate::tasks::folder::folder_in(&crate::config::home().display().to_string(), folder));
    crate::http::off(move || Ok(path.is_dir())).await.unwrap_or(false)
}

fn server_of(app: &Shared, host: Option<&str>) -> Result<Arc<Server>, String> {
    match host {
        None => Ok(app.tmux.local.clone()),
        Some(h) => app.tmux.host(h).filter(|s| s.connected()).ok_or_else(|| format!("The host {h} is out of reach.")),
    }
}

#[derive(Deserialize)]
struct Press {
    agent: Option<String>,
    comment: Option<String>,
    prompt_rev: Option<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    fresh: bool,
    /// the idle run whose window this press closes to make room (AC-141)
    close: Option<String>,
    /// the other tasks of this press (AC-138)
    #[serde(default)]
    also: Vec<String>,
}

/// The press on RUN: a comment is kept whatever follows (AC-122).
async fn press(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Press>) -> Response {
    answer(pressed(&app, &id, body).await)
}

/// Where a run's pane is.
fn pane_of(app: &Shared, r: &Run) -> Result<(Arc<Server>, String), String> {
    r.place(app).ok_or_else(|| "Its session is gone.".to_string())
}

async fn pressed(app: &Shared, id: &str, body: Press) -> Result<Value, String> {
    let comment = body.comment.as_deref().map(str::trim).filter(|c| !c.is_empty()).map(str::to_string);
    if comment.as_deref().is_some_and(has_stray) {
        return Err("The message has control or invisible characters.".to_string());
    }
    let (wanted, said, attached, fresh, also, agent) = (id.to_string(), comment.clone(), body.files.clone(), body.fresh, body.also.clone(), body.agent.clone());
    let (tasks, w, who) = store(app, move |tx| {
        let (tasks, w) = ways(tx, &wanted, &also, agent.as_deref(), fresh)?;
        // a run that starts keeps its comment with it, in the transaction that adds it; going on, or refused, it stays, once
        if tasks.len() == 1 && !matches!(w, Ok(Way::First)) && let Some(c) = said.as_ref().filter(|c| !ops::last_comment_is(tx, &wanted, c).unwrap_or(false)) {
            let event = ops::comment(tx, HUMAN, &wanted, c, ops::now())?;
            files::attach(tx, &attached, &wanted, Some(&event))?;
        }
        Ok((tasks, w, ops::user_name(tx, HUMAN)?))
    }).await?;
    let t = tasks[0].clone();
    let going_on = !matches!(w, Ok(Way::First));
    if comment.is_some() && going_on {
        told(app, id);
    }
    let again = comment.as_ref().map_or_else(|| format!("{}: go on.", t.id), |c| format!("{}: a new comment by {who} follows.\n\n{c}", t.id));
    match w? {
        Way::First => first_run(app, &tasks, body, comment).await,
        Way::Live(r) => {
            let (server, pane) = pane_of(app, &r)?;
            // the walk may not have read yet that its agent ended
            if start::exit_of(&server, &pane).await.is_some() {
                return from_shell(app, &r, &server, &pane, again, &t).await;
            }
            // its turn is taken for the words about to be typed, so nothing reads it idle meanwhile; no limit stops it:
            // it starts no process (AC-141)
            let id = r.id.clone();
            store(app, move |tx| runs::change(tx, &id, |now| now.turn_ended = false).map(drop)).await?;
            if let Err(miss) = gate::deliver(app, &r, &server, &pane, &again, false).await {
                let (id, was) = (r.id.clone(), r.turn_ended);
                let _ = store(app, move |tx| runs::change(tx, &id, |now| now.turn_ended = was).map(drop)).await;
                return Err(match miss {
                    Miss::NotEmpty(_) => "The session is not at an empty input box: answer there first".to_string(),
                    Miss::Other(why) => format!("Not delivered: {why}"),
                });
            }
            went_on(app, &r, &t).await
        }
        Way::Shell(r) => {
            let (server, pane) = pane_of(app, &r)?;
            from_shell(app, &r, &server, &pane, again, &t).await
        }
        Way::Resume(r) => {
            let Ok(line) = start::resume_command(&r) else {
                return could_not_continue(app, &r, "no conversation is known").await;
            };
            if r.host.is_none() && !folder_there(&r.folder).await {
                return could_not_continue(app, &r, "its first folder is gone").await;
            }
            let server = server_of(app, r.host.as_deref())?;
            let new = next_run(app, &r, &t, again).await?;
            let project = project_named(app, &t.project);
            start::open(app, start::Opening { server: &server, project: project.as_ref().map(|(i, n)| (i.as_str(), n.as_str())), folder: &r.folder, command: &line, run: &new, tasks: std::slice::from_ref(&t.id), program: &r.kind, agent: &r.agent_id }).await?;
            start::watch(app, &new);
            Ok(json!({"run": new}))
        }
    }
}

/// What a press types may hold line ends, nothing else unseen: tmux would refuse it after the run was written.
pub(super) fn has_stray(text: &str) -> bool {
    text.split(['\n', '\t']).any(has_control)
}

/// Closes an idle holder for the press that asked: read again here, its screen too: a draft or a question stands in the way (AC-141).
async fn close_idle(app: &Shared, run: &str, for_tasks: &str) -> Result<(), String> {
    let wanted = run.to_string();
    let r = store(app, move |tx| runs::by_id(tx, &wanted)).await?;
    let (server, pane) = pane_of(app, &r)?;
    let wanted = r.id.clone();
    let closable = store(app, move |tx| pool::closable(tx, &runs::by_id(tx, &wanted)?)).await?;
    let quiet = closable && app.dispatcher.watch.lock().get(&server.key(&pane)).is_none_or(|w| w.subs.is_empty());
    if !quiet || !matches!(gate::read(&server, &pane, &r.kind).await, Ok(Screen::Empty)) {
        return Err(format!("{}'s session is no longer idle: it stays.", r.names()));
    }
    acts::close(app, &r, &server, &pane, &format!("Session ended: closed for {for_tasks}."), true, pool::closable).await?;
    super::told_all(app, &r);
    Ok(())
}

/// What runs in `pane` other than its shell: in front, or stopped or left behind it (AC-122).
async fn what_runs(app: &Shared, server: &Server, pane: &str) -> Result<Option<String>, String> {
    let said = server.run(format!("display -p -t {pane} '#{{pane_current_command}}\t#{{pane_pid}}'")).await.map_err(|e| format!("tmux does not answer: {e}"))?;
    let (name, pid) = said.first().and_then(|l| l.split_once('\t')).unwrap_or(("", ""));
    if !walk::is_shell(name) {
        return Ok(Some(name.to_string()));
    }
    // the moment after an agent's end its wrapper and a prompt's helpers still run: what stays is a job
    for look in 0..3 {
        let tree = procs::read(app, server).await.ok_or("ps did not answer")?;
        if !procs::under(&tree, pid).any {
            return Ok(None);
        }
        if look < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
    }
    Ok(Some("a job of its shell".to_string()))
}

/// Only its shell remains: the agent starts there anew, by the resume when a conversation is known;
/// the pane's queue held from the look to the start, so no key starts anything between (AC-122).
async fn from_shell(app: &Shared, r: &Run, server: &Arc<Server>, pane: &str, again: String, t: &Task) -> Result<Value, String> {
    let _held = server.typing(pane).lock_owned().await;
    if let Some(program) = what_runs(app, server, pane).await? {
        return Err(format!("{program} runs in its pane."));
    }
    let line = start::resume_command(r).unwrap_or_else(|_| r.command.clone());
    let new = next_run(app, r, t, again).await?;
    let done = start::respawn(app, server, pane, &r.id, &new, &r.folder, &line).await;
    crate::inputs::wrote(app, crate::inputs::Who::dispatcher("dispatcher.start", &new, &r.ids, &r.agent_id), "launch", server, pane, &line, done.as_ref().err().cloned(), None);
    done?;
    let (a, key, n) = (app.clone(), server.key(pane), new.clone());
    app.dispatcher.panes.lock().insert(key.clone());    // its agent's first hooks
    store(app, move |tx| a.dispatcher.pane_opened(tx, &n, &key).map(drop)).await?;
    start::watch(app, &new);
    Ok(json!({"run": new}))
}

/// The conversation cannot go on: FAILED, its comment says why, and Run from scratch stays (AC-122).
async fn could_not_continue(app: &Shared, r: &Run, why: &str) -> Result<Value, String> {
    let (run, words) = (r.clone(), format!("Conversation {} could not be continued: {why}", r.agent_session.as_deref().unwrap_or("unknown")));
    store(app, move |tx| if super::plan::finished(&run.status) { say(tx, &run, &words) } else { move_task(tx, &run, "FAILED", &words) }).await?;
    super::told_all(app, r);
    Err(format!("{why}: use Run from scratch."))
}

/// A project's id and name for its sessions' place in the list; Default stands for none.
fn project_named(app: &Shared, project: &str) -> Option<(String, String)> {
    if project == DEFAULT_PROJECT {
        return None;
    }
    app.projects.lock().list().iter().find(|p| p.id == project).map(|p| (p.id.clone(), p.name.clone()))
}

/// RUN goes on in its live session: every clock starts anew; a finished task reopens alone, an open one
/// with the others waiting with it.
async fn went_on(app: &Shared, r: &Run, t: &Task) -> Result<Value, String> {
    let (id, task, key, status, words) = (r.id.clone(), t.id.clone(), t.key.clone(), t.status.clone(), "The human ran it again.");
    let done = store(app, move |tx| {
        let now = runs::change(tx, &id, |now| { now.state = "working".into(); now.new_work(); })?;
        bring_back(tx, &task)?;
        if super::plan::finished(&status) {
            move_one(tx, &task, "IN PROGRESS", words)?;
            runs::reopened(tx, &key)?;
        } else {
            move_task(tx, &now, "IN PROGRESS", words)?;
        }
        Ok(now.id)
    }).await?;
    super::told_all(app, r);
    Ok(json!({"run": done}))
}

/// New work on an archived task returns it to the board (AC-122).
fn bring_back(tx: &rusqlite::Transaction, id: &str) -> Result<(), String> {
    if ops::get(tx, id)?.archived_at.is_some() {
        ops::archive(tx, HUMAN, id, false, ops::now())?;
    }
    Ok(())
}

/// A new run of `t` going on from `r`, which is over: its agent, unless another run took it meanwhile. What
/// else was in `r` lets go of its session: it stays open to its own Run, and no agent there holds it.
async fn next_run(app: &Shared, r: &Run, t: &Task, prompt: String) -> Result<String, String> {
    let (old, key, id) = (r.clone(), t.key.clone(), t.id.clone());
    store(app, move |tx| {
        // an agent deleted since has no slot to take: its run's own settings start
        let agent = agents::by_id(tx, &old.agent_id)?;
        if let Some(full) = pool::check(tx, agent.as_ref(), &old.project, Some(&old.id))? {
            return Err(full.words);
        }
        if agent.is_none() {
            say(tx, &old, &format!("{} is no longer in the pool: this goes on with the settings it started with.", old.agent_name))?;
        }
        runs::change(tx, &old.id, |was| { was.state = "closed".into(); was.pane_key = None; })?;
        for other in old.ids.iter().filter(|other| **other != id) {
            ops::link(tx, super::DISPATCHER, other, None, ops::now())?;
        }
        bring_back(tx, &id)?;
        let (from, id) = (old.id.clone(), runs::add(tx, &Run { prompt: Some(prompt), ..old }, &[key], ops::now())?);
        super::usage::carry(tx, &from, &id)?;
        Ok(id)
    }).await
}

/// Where `agent` would start in `project`, and what refuses it before any slot is asked (AC-140): no folder, no such
/// agent, a host's link down, a folder or hooks not there.
async fn place(app: &Shared, project: &str, agent: Option<String>) -> Result<(Agent, Option<String>, String, Arc<Server>), String> {
    let project = project.to_string();
    let (a, host, dir) = store(app, move |tx| {
        let (name, host, dir) = ops::project_place(tx, &project)?;
        no_folder(&name, &dir)?;
        let a = agent.as_deref().map(|id| agents::by_id(tx, id)).transpose()?.flatten().ok_or("No agents yet: add one first.")?;
        Ok((a, host, dir))
    }).await?;
    let server = server_of(app, host.as_deref())?;
    if let Some(host) = &host {
        let state = crate::hosts::link::agents_state(app, host);
        if state == "off" {
            return Err(format!("Run terminum hooks install on {host} first"));
        }
        if !state.starts_with("up") {
            return Err(format!("The link to {host}'s agents is not up: {state}"));
        }
    } else {
        if !folder_there(&dir).await {
            return Err(format!("The folder {dir} does not exist on this Mac"));
        }
        let kind = a.kind.clone();
        if !crate::http::off(move || Ok(hooks_there(&kind))).await.unwrap_or(false) {
            return Err("Run terminum hooks install first: Dispatcher cannot follow the agent without them.".to_string());
        }
    }
    Ok((a, host, dir, server))
}

#[derive(Deserialize)]
struct AdHoc {
    agent: String,
}

/// + Session (AC-142): a project's agent with no task, nothing typed: the human's from its first box.
async fn ad_hoc(State(app): State<Shared>, Path(project): Path<String>, Json(body): Json<AdHoc>) -> Response {
    answer(session(&app, &project, body.agent).await)
}

async fn session(app: &Shared, project: &str, agent: String) -> Result<Value, String> {
    let (a, host, dir, server) = place(app, project, Some(agent)).await?;
    let (ag, p, f) = (a.clone(), project.to_string(), dir.clone());
    let run = store(app, move |tx| {
        if let Some(full) = pool::check(tx, Some(&ag), &p, None)? {
            return Err(full.words);
        }
        runs::add(tx, &Run::of(&ag, host, f, p, None), &[], ops::now())
    }).await?;
    let key = launch(app, &run, &server, project, &dir, &a, &[]).await?;
    Ok(json!({"run": run, "key": key}))
}

/// The window of a run just added: opened at its place and watched; one that does not open closes its run. Its key.
async fn launch(app: &Shared, run: &str, server: &Arc<Server>, project: &str, dir: &str, a: &Agent, tasks: &[String]) -> Result<String, String> {
    let named = project_named(app, project);
    let opening = start::Opening { server, project: named.as_ref().map(|(i, n)| (i.as_str(), n.as_str())), folder: dir, command: &a.line, run, tasks, program: &a.kind, agent: &a.id };
    match start::open(app, opening).await {
        Ok(key) => {
            start::watch(app, run);
            Ok(key)
        }
        Err(e) => {
            let r = run.to_string();
            let _ = store(app, move |tx| runs::change(tx, &r, |now| now.state = "closed".into()).map(drop)).await;
            Err(e)
        }
    }
}

/// The human lets go of a session Dispatcher holds: its open tasks fail, its slot frees, its window stays (AC-142).
async fn let_go(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let_go_of(&app, store(&app, move |tx| runs::let_go(tx, &id)).await)
}

/// What a run let go of says, and every page hears of its tasks.
fn let_go_of(app: &Shared, done: Result<Run, String>) -> Response {
    if let Ok(r) = &done {
        super::told_all(app, r);
    }
    answer(done.map(|r| json!({"ok": true, "ids": r.ids})))
}

async fn first_run(app: &Shared, tasks: &[Task], body: Press, comment: Option<String>) -> Result<Value, String> {
    let t = &tasks[0];
    let (all, seen, said, attached) = (tasks.to_vec(), body.prompt_rev.clone(), comment, body.files.clone());
    let (a, host, dir, server) = place(app, &t.project, body.agent.clone()).await?;
    if let Some(holder) = &body.close {
        close_idle(app, holder, &tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")).await?;
    }
    let (ag, f, project) = (a.clone(), dir.clone(), t.project.clone());
    let run = store(app, move |tx| {
        // the one check that counts: in the transaction that takes the slots, each task as it stands now
        if let Some(full) = pool::check(tx, Some(&ag), &project, None)? {
            return Err(full.words);
        }
        let now: Vec<Task> = all.iter().map(|t| ops::get(tx, &t.id)).collect::<Result<_, _>>()?;
        for t in &now {
            way(t, runs::last_of(tx, &t.key)?, true)?;
        }
        // what the human saw is what goes: an agent may have changed a task meanwhile
        if seen.is_some_and(|rev| rev != rev_of(&shown_of(tx, &now).unwrap_or_default())) {
            return Err("The task's text changed: look at it again.".to_string());
        }
        let prompt = prompt_of(tx, &now, said.as_deref())?;
        if let Some(c) = &said {
            for (i, t) in now.iter().enumerate() {
                let event = ops::comment(tx, HUMAN, &t.id, c, ops::now())?;
                if i == 0 {
                    files::attach(tx, &attached, &t.id, Some(&event))?;
                }
            }
        }
        for t in &now {
            bring_back(tx, &t.id)?;
        }
        // one live run a task: an older one's window stays the human's, untouched (Run from scratch)
        let keys: Vec<String> = now.iter().map(|t| t.key.clone()).collect();
        for t in &now {
            if let Some(old) = runs::last_of(tx, &t.key)?.filter(|r| !r.is_over() && r.pane_key == t.session) {
                runs::change(tx, &old.id, |r| r.state = "left".into())?;
            }
        }
        runs::add(tx, &Run::of(&ag, host, f, project, Some(prompt)), &keys, ops::now())
    }).await?;
    let ids: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
    launch(app, &run, &server, &t.project, &dir, &a, &ids).await?;
    for id in &ids {
        told(app, id);
    }
    Ok(json!({"run": run}))
}

/// The human gives up on a run they cannot reach or no longer want (AC-138).
async fn give_up(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let_go_of(&app, store(&app, move |tx| { let key = ops::key_of(tx, &id)?; runs::give_up(tx, &key) }).await)
}

/// A human changed the budget on a running task: it holds at once, and the agent is told as the last
/// warning is (AC-126).
pub fn budget_changed(app: &Shared, task: &Task) {
    let Some(key) = task.session.clone().filter(|k| app.dispatcher.has_run(k)) else { return };
    let (app, id, own) = (app.clone(), task.id.clone(), task.budget_seconds);
    tokio::spawn(async move {
        let k = key.clone();
        let Ok(Some(r)) = store(&app, move |tx| {
            let Some(r) = runs::in_pane(tx, &k)? else { return Ok(None) };
            runs::change(tx, &r.id, |now| now.warned = false).map(Some)
        }).await else { return };
        if let Some((server, pane)) = app.tmux.resolve(&key) {
            let words = budget_told(&id, own, &r);
            acts::tell(&app, &r, &server, &pane, words, "The new budget").await;
        }
    });
}
