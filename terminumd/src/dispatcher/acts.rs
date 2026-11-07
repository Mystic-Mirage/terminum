// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What a walk leaves to do outside its transaction (`terminum-dispatcher.md` AC-131): typing, ending
//! an agent, closing a pane, and what is said of them.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::plan::{self, Act};
use super::runs::{self, Run};
use super::{gate, procs, say, scale, start, store, BEAT};
use crate::app::Shared;
use crate::tasks::{files, ops};
use crate::tmux::Server;

/// What the transaction left to do outside it.
pub(super) struct Later {
    pub(super) run: Run,
    pub(super) act: Act,
}

/// Closes the pane: its mark checked just before; with no result the screen's last 200 rows stay on
/// the task. tmux refusing: one more try at the next beat, then it stays and says so (AC-125).
async fn close(app: &Shared, run: &Run, server: &Arc<Server>, pane: &str, words: &str, keep: bool) -> Result<(), String> {
    start::is_marked(server, pane, &run.id).await?;
    if keep {
        let rows = server.run(format!("capture-pane -p -S -200 -t {pane}")).await.unwrap_or_default();
        let text = crate::sent::redact(&rows.join("\n"));
        if !text.trim().is_empty() {
            let (a, task, name, at) = (app.clone(), run.ids.first().cloned().unwrap_or_default(), format!("{}-screen.txt", run.names().replace(", ", "-")), ops::now());
            let kept = crate::http::off(move || {
                let f = files::keep(&a.tasks, &a.cfg.dir, &name, text.as_bytes(), at)?;
                a.tasks.with(|tx| files::attach(tx, std::slice::from_ref(&f.id), &task, None))
            }).await;
            if let Err(e) = kept {
                tracing::warn!(event = "dispatcher.close", task = %run.names(), error = %e);
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

/// The agent and what runs under it, tmux's word taken just before: its mark, and no exit recorded,
/// as a program a human then starts at the shell it leaves is none of this (AC-126). A `ps` that does
/// not answer is tried again; what could not be ended is said.
async fn end_agent(app: &Shared, run: &Run, server: &Arc<Server>, pane: &str) {
    let grace = (Duration::from_secs(10) / scale() as u32).max(Duration::from_millis(500));
    let mut why = String::new();
    for _ in 0..3 {
        let said = server.run(format!("display -p -t {pane} '#{{@terminum-run}}\t#{{@terminum-exit}}\t#{{pane_pid}}'")).await.unwrap_or_default();
        let fields: Vec<&str> = said.first().map_or(Vec::new(), |l| l.split('\t').collect());
        let [mark, exit, root] = fields[..] else { return };
        if mark != run.id || !exit.is_empty() {
            return;                                    // it has gone by itself, or the pane is no more this run's
        }
        match procs::end(app, server, root, grace).await {
            Ok(n) => {
                tracing::info!(event = "dispatcher.end", task = %run.names(), signalled = n);
                return;
            }
            Err(e) => why = e,
        }
        tokio::time::sleep((BEAT / scale() as u32).max(Duration::from_millis(500))).await;
    }
    let (r, words) = (run.clone(), format!("The agent could not be ended: {why}. It may still run."));
    tracing::warn!(event = "dispatcher.end", task = %run.names(), error = %why);
    let _ = store(app, move |tx| say(tx, &r, &words)).await;
    super::told_all(app, run);
}

/// Words for the agent: on its next tool call while it works, through the gate while it idles.
pub(super) async fn tell(app: &Shared, run: &Run, server: &Arc<Server>, pane: &str, text: String, what: &str) {
    let key = server.key(pane);
    let sent = if app.hooks.working(&key) || run.open_tools > 0 { Err(None) } else { gate::deliver(app, run, server, pane, &text, false).await.map_err(Some) };
    let words = match sent {
        Ok(()) => format!("{what} was given."),
        Err(why) => {
            app.task_lines.push(&key, run.ids.first().map_or("", String::as_str), text);
            match why {
                Some(why) => format!("{what} waits for the agent's next tool call: {why}."),
                None => format!("{what} waits for the agent's next tool call."),
            }
        }
    };
    let r = run.clone();
    let _ = store(app, move |tx| say(tx, &r, &words)).await;
}

/// A line every open page shows for a moment.
pub(super) fn note(app: &Shared, msg: String) {
    app.pages.tell(&serde_json::json!({"t": "note", "msg": msg}).to_string());
}

pub(super) async fn do_later(app: &Shared, l: Later) {
    let Some((server, pane)) = l.run.place(app) else { return };
    let task = l.run.names();
    let done = match l.act {
        Act::Warn(left) => {
            let text = format!("[terminum Dispatcher] {task}: {} minutes of the {}-minute budget are left. Write the result with finish_task and end your subagents now. Then the agent is ended and unsaved work is lost.", (left.max(0) + 59) / 60, l.run.budget.unwrap_or(0) / 60);
            tell(app, &l.run, &server, &pane, text, "The last warning").await;
            Ok(())
        }
        Act::MemoryWarn(gib) => {
            let text = format!("[terminum Dispatcher] {task}: its session uses {gib:.1} GiB of memory. End what it does not need.");
            tell(app, &l.run, &server, &pane, text, "The memory warning").await;
            note(app, format!("{task}'s session uses {gib:.1} GiB of memory"));
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
    super::told_all(app, &l.run);
}

/// A subagent silent and idle: one request to the main agent, then it is said that it cannot be
/// ended alone: it runs inside the agent (AC-127, OQ-122). A busy tree runs neither clock.
pub(super) async fn watch_subagents(app: &Shared, run: &Run, server: &Arc<Server>, pane: &str, key: &str, busy: bool) {
    let due: Vec<(String, plan::SubStep)> = {
        let mut watch = app.dispatcher.watch.lock();
        let Some(w) = watch.get_mut(key) else { return };
        if busy || run.open_tools > 0 {
            for sub in w.subs.values_mut() {
                sub.last = Instant::now();
            }
            return;
        }
        w.subs.iter_mut().filter_map(|(id, sub)| {
            let step = plan::subagent_step(sub.last.elapsed().as_secs() as i64 * scale(), sub.told)?;
            sub.told = step;
            Some((id.clone(), step))
        }).collect()
    };
    for (id, step) in due {
        if step == plan::SubStep::Tell {
            let text = format!("[terminum Dispatcher] A subagent of {} has been silent for 10 minutes. End it if it is stuck.", run.names());
            tell(app, run, server, pane, text, "The request about a silent subagent").await;
        } else {
            let (r, words) = (run.clone(), format!("Subagent {id} cannot be ended alone: it runs inside the agent"));
            note(app, format!("{}: {words}", run.names()));
            let _ = store(app, move |tx| say(tx, &r, &words)).await;
        }
        super::told_all(app, run);
    }
}
