// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Keeps the tmux side going (AC-01, AC-14): starts the private local server when none runs
//! (or restores the layout after a reboot, AC-15), keeps a control client per session,
//! asks each server's list (lists.rs) on its notices (coalesced 100 ms) and every 2 s while a
//! page looks, tells the pages what changed, says when tmux stops answering, and expires
//! workspaces of panes gone for good (AC-71).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::app::Shared;
use crate::lists::{self, Got};
use crate::tmux::control::STALL;
use crate::tmux::layout;
use crate::tmux::server::{Server, Status};

/// The list is read this often while a page looks (AC-01).
const EVERY: Duration = Duration::from_secs(2);
/// tmux missing: looked for again this often (AC-01 failure).
const TMUX_AGAIN: Duration = Duration::from_secs(10);
/// Workspaces of panes gone this long are removed at an inventory this often (AC-71).
const INVENTORY: Duration = Duration::from_secs(300);
/// Turns due to settle ask the list at most this often: one on a host that is down stays due (AC-132).
const SETTLE_POLL: Duration = Duration::from_millis(500);

/// Starts the local server when none runs, and keeps a control client per session.
async fn ensure_local(app: &Shared) {
    let server = &app.tmux.local;
    // through the bulk client while it lives; a process of its own only when none does
    if server.connected()
        && let Ok(list) = server.run("list-sessions -F '#{session_name}'").await
            && !list.is_empty() {
                if let Err(e) = server.sync_clients(&list) {
                    set_problem(app, Some(e));
                }
                return;
            }
    let sessions = match server.local_sessions().await {
        Ok(s) => s,
        Err(e) => {
            set_problem(app, Some(e));
            return;
        }
    };
    let sessions = match sessions {
        Some(s) if !s.is_empty() => s,
        _ => {
            // the user closed the last pane and tmux ended with it: a fresh "main", nothing restored
            if !server.took_closed_by_user() && crate::restore::at_start(app).await {
                tracing::info!(event = "tmux.restored");
            } else if let Err(e) = server.start_local("main", &crate::config::home().display().to_string()).await {
                set_problem(app, Some(format!("tmux could not start: {e}")));
                return;
            } else {
                tracing::info!(event = "tmux.started", socket = %server.socket);
            }
            // settings.json's history lines over tmux.conf's (written once): panes made from now on
            let history = app.looks.settings.read().tmux_history;
            if let Err(e) = server.local(&["set-option", "-g", "history-limit", &history.to_string()]).await {
                tracing::warn!(event = "tmux.option", option = "history-limit", error = %e);
            }
            // macOS judges every program in its terminals by the program that started it (AC-08)
            if let Some(exe) = std::env::current_exe().ok().and_then(|e| std::fs::canonicalize(e).ok())
                && let Err(e) = server.local(&["set-option", "-g", "@terminum-started-by", &exe.display().to_string()]).await {
                tracing::warn!(event = "tmux.option", option = "@terminum-started-by", error = %e);
            }
            match server.local_sessions().await {
                Ok(Some(s)) => s,
                _ => return,
            }
        }
    };
    set_problem(app, None);
    if let Err(e) = server.sync_clients(&sessions) {
        set_problem(app, Some(e));
    } else {
        *server.status.lock() = Status::Attached;
    }
}

fn set_problem(app: &Shared, problem: Option<String>) {
    let mut p = app.tmux_problem.lock();
    if *p != problem {
        *p = problem.clone();
        if let Some(m) = problem {
            tracing::warn!(event = "tmux.problem", msg = %m);
            app.pages.tell(&json!({"t": "error", "msg": m}).to_string());
        }
    }
}

/// One read of a server, on its own task (lists.rs): nothing here waits for another server.
pub async fn read_one(app: &Shared, server: &Arc<Server>) -> Got {
    let mut absent = Vec::new();
    let mut sure = true;
    if server.is_users() {
        (absent, sure) = crate::attach::ensure(app, server).await;
    } else if server.is_local() {
        ensure_local(app).await;
    }
    let home = crate::config::home().display().to_string();
    match read_server(app, server.clone(), &home).await {
        Got::Read { groups, keys, ok, .. } => Got::Read { groups, absent, keys, ok: ok && sure },
        other => other,
    }
}

/// One server's groups and pane keys.
async fn read_server(app: &Shared, server: Arc<Server>, home: &str) -> Got {
    let local = server.is_local();
    if server.is_users() && !server.connected() {
        return Got::Read { groups: Vec::new(), absent: Vec::new(), keys: Vec::new(), ok: true };
    }
    if !local && !server.attached() {
        // judged by its `terminum` client; the user's sessions there wait with the host's state
        let mut g = layout::host_group(&server, crate::hosts::files_state(app, &server.name).as_deref());
        g["agents"] = json!(crate::hosts::link::agents_state(app, &server.name));   // AC-99
        let why = format!("{}: {}", server.name, g["why"].as_str().unwrap_or_default());
        let mut gs = vec![g];
        gs.extend(crate::attach::host_waiting(&server, &why));
        return Got::Down(gs);
    }
    if server.backed_up() {
        return Got::Unread;                      // what was asked is still on its way: no more on top (AC-132)
    }
    let host = (!local).then(|| server.name.clone());
    let home = if local { home.to_string() } else { server.home().await.unwrap_or_default() };
    let looks = &app.looks;
    let out = match layout::read(&server, &app.agent_states, &app.hooks, &app.turns.hooked(Instant::now()), &home, |s| looks.theme_name(host.as_deref(), s)).await {
        Ok(read) => {
            let mut gs = read.groups;
            if !local {
                let files = crate::hosts::files_state(app, &server.name);
                if !gs.iter().any(|g| g["attached"].is_null()) {
                    gs.insert(0, layout::host_group(&server, files.as_deref()));
                }
                crate::restore_srv::host(app, &server, &read.sessions).await;   // its tmux started anew: its layout again (AC-15)
                let (absent, others) = crate::attach::ensure_host(&server, &read.sessions);
                let held = server.held.lock().clone();
                for g in gs.iter_mut().filter(|g| g["state"].is_null()) {
                    g["state"] = json!("attached");
                    g["files"] = json!(files);
                    if let Some(why) = held.as_ref().filter(|_| g["attached"].is_null()) {
                        g["why"] = json!(why);   // its record changed and is not used yet (AC-87)
                    }
                }
                // the host's own group offers the other sessions running there (AC-89)
                if let Some(g) = gs.iter_mut().find(|g| g["attached"].is_null()) {
                    g["others"] = json!(others);
                    g["agents"] = json!(crate::hosts::link::agents_state(app, &server.name));
                }
                gs.extend(absent);
            }
            let live = read.keys.iter().map(|k| format!("web:{}", json!([host.clone().unwrap_or_default(), k]))).collect();
            crate::page::projects::seen(app, &server, &read.keys, &read.every).await;   // closed panes leave their projects (AC-92)
            crate::page::unread::seen(app, &server).await;                 // a tmux started anew takes its marks (AC-93)
            crate::plans::seen(app, &server).await;                        // and its plans (AC-97)
            // a gone pane's plan, or one whose program changed, is archived (AC-97)
            let changed: Vec<String> = app.hooks.take_changed().into_iter().filter(|k| app.plans.session(k, None)).collect();
            if app.plans.retain(&server.key(""), &read.keys) | !changed.is_empty() {
                crate::plans::save(app);
                crate::page::plans::tell(&app.pages, &changed);
            }
            server.set_panes(read.panes);
            Got::Read { groups: gs, absent: Vec::new(), keys: live, ok: true }
        }
        Err(e) => {
            tracing::warn!(event = "tmux.error", server = %server.name, cmd = "list-panes", msg = e);
            if server.connected() {
                Got::Unread                          // a timeout on a link up: its rows stay as last read
            } else {
                Got::Gone(if local { Vec::new() } else { vec![layout::host_group(&server, None)] })
            }
        }
    };
    server.prune_wakes();
    out
}

/// What a server not read yet shows: a host says it is on its way, so a page showing its pane stays
/// there (AC-14, AC-132); a local server's list is told first of all, so it has none.
fn unread_groups(app: &Shared, server: &Arc<Server>) -> Vec<Value> {
    let reading = |mut g: Value| {
        if g["state"] == "attached" || g["state"] == "absent" {
            g["state"] = json!("reconnecting");
            g["why"] = json!("reading its list");
        }
        g
    };
    let mut gs = Vec::new();
    if !server.is_local() {
        gs.push(reading(layout::host_group(server, crate::hosts::files_state(app, &server.name).as_deref())));
    }
    gs.extend(crate::attach::host_waiting(server, "reading its list").into_iter().map(reading));
    gs
}

/// Tells the pages the list when it changed, each server's part as last read. `server` was just
/// read; `fresh`: its own list came, so its turns settle by it, and only by it (AC-132).
pub async fn publish(app: &Shared, server: &Arc<Server>, fresh: bool) {
    let _one = app.refreshing.lock().await;
    if app.tmux.local.list.told().is_none() {
        return;                                  // a page reconnecting keeps its list until its own Mac's is read
    }
    let home_due = app.inventory_at.lock().is_none_or(|t| t.elapsed() > INVENTORY);
    let (mut groups, mut absent, mut live, mut complete) = (Vec::new(), Vec::new(), HashSet::new(), true);
    let all = app.tmux.all();
    for s in &all {
        match s.list.told() {
            Some(t) => {
                groups.extend(t.groups);
                absent.extend(t.absent);
                live.extend(t.keys);
                complete &= t.ok;
            }
            None => {
                groups.extend(unread_groups(app, s));
                complete = false;
            }
        }
    }
    crate::page::sizing::reclaim_once(app, server);
    crate::tmux::orphans::sweep_once(server);
    groups.extend(absent);
    // a tmux reached twice lists each pane once (AC-89)
    // (each server's identity was read with its list: projects::seen)
    let ids = all.iter().filter_map(|s| s.identity_known().map(|i| (s.id(), i))).collect();
    layout::once_each(&mut groups, &ids);
    crate::page::unread::mark(app, &mut groups, complete);
    crate::page::plans::mark(app, &mut groups);                      // plans on the rows (AC-97)
    let ended = {
        let mut last = app.layout.write();
        if last.as_array() != Some(&groups) {
            let told = json!({"t": "layout", "groups": groups}).to_string();
            let old = std::mem::replace(&mut *last, Value::Array(groups));
            drop(last);
            app.pages.tell(&told);
            // a pane whose hooks speak ends its turns by their Stop (sock.rs): exact, also between two layouts
            let mut ended = ended_turns(&old, app.layout.read().as_array().map_or(&[][..], Vec::as_slice));
            ended.retain(|k| !app.hooks.end_counted(k));   // no Stop counted for it: its screen's end stands (AC-39)
            ended
        } else {
            vec![]
        }
    };
    crate::page::projects::tell_read(app);                           // after the list without the closed panes (AC-92)
    for k in app.hooks.take_all_ended() {                            // subagents the screen no longer shows (AC-39)
        hook_turn_ended(app, &k, "screen");
    }
    // a turn ends when its pane stayed done a while, its subagents too (turns.rs)
    let ended = if fresh {
        let layout = app.layout.read();
        let busy = |k: &str| crate::page::marks::find_item(&layout, k).map(|it| crate::page::turns::busy(&it));
        app.turns.settle(&server.key(""), ended, busy, Instant::now())
    } else {
        Vec::new()
    };
    if !ended.is_empty() {
        remind(app, ended).await;
    }
    if complete && home_due {
        *app.inventory_at.lock() = Some(Instant::now());
        app.store.maintain(&live, false);
    }
    announce_offers(app);
}

/// A turn its hooks ended (`by` an event, or the screen that shows no subagent): the next list read
/// settles it, and where its answer ended is kept: the screen is final (AC-39, AC-90).
pub fn hook_turn_ended(app: &Shared, key: &str, by: &str) {
    tracing::info!(event = "hook.turn_end", key, by);
    app.turns.hook_ended(key, Instant::now());
    match app.tmux.resolve(key) {
        Some((server, _)) => { lists::request(app, &server); }   // its server's list alone
        None => app.dirty.notify_one(),
    }
    tokio::spawn(crate::page::answers::keep(app.clone(), key.to_string()));
}

/// The panes whose coding agent ended a turn between layouts `old` and `new`: it worked,
/// waited or had subagents running, and now is done, failed or shows no agent (AC-91).
pub fn ended_turns(old: &Value, new: &[Value]) -> Vec<String> {
    let busy = |it: &&Value| crate::page::turns::busy(it);
    let old = old.as_array().map_or(&[][..], Vec::as_slice);
    use crate::page::marks::layout_items as items;
    let was: HashSet<&str> = items(old).filter(busy).filter_map(|it| it["id"].as_str()).collect();
    // a row that cannot be read says nothing of its turn (AC-81)
    items(new).filter(|it| !busy(it) && it["offline"] != true).filter_map(|it| it["id"].as_str()).filter(|id| was.contains(id)).map(str::to_string).collect()
}

/// The reminders of the panes whose agent ended a turn (in panes `ended`) come up, on every
/// page (AC-91).
pub async fn remind(app: &Shared, ended: Vec<String>) {
    crate::page::unread::ended(app, &ended);           // a turn nobody saw end is unread (AC-93)
    crate::page::marksfull::turn_ended(app, &ended);   // agents' records that went for room meanwhile (AC-98)
    // where the answer ended (AC-90): a hooked pane's was kept at its Stop (sock.rs)
    for k in ended.iter().filter(|k| !app.hooks.has(k)) {
        tokio::spawn(crate::page::answers::keep(app.clone(), k.clone()));
    }
    let a = app.clone();
    match tokio::task::spawn_blocking(move || a.bookmarks.lock().due(&ended).map_err(|e| e.to_string())).await.map_err(|e| e.to_string()).and_then(|r| r) {
        Ok(items) => {
            for item in items {
                crate::page::marks::tell_mark(app, &item);
            }
        }
        Err(e) => tracing::warn!(event = "reminder.unsaved", error = %e),
    }
}

/// A host's files offer changed (its helper's state, its record, its link): its panels are
/// told, so they need not ask again and again (AC-83).
fn announce_offers(app: &Shared) {
    let mut last = app.offers.lock();
    last.retain(|h, _| app.tmux.host(h).is_some());
    for server in app.tmux.hosts() {
        let offer = crate::hosts::offer(app, &server.name);
        if last.get(&server.name) != Some(&offer) {
            last.insert(server.name.clone(), offer);
            app.bus.send(crate::events::Event::Host { host: server.name.clone() });
        }
    }
}

/// The loop: every server's list every 2 s while a page looks, and on whatever may concern them
/// all (`dirty`); a server's own notices (`changed`) ask its list alone (AC-132), and when a turn's
/// end has settled (turns.rs).
pub async fn run(app: Shared) {
    tokio::spawn(stalls(app.clone()));
    loop {
        lists::request_all(&app);
        let wait = if app.tmux_problem.lock().is_some() { TMUX_AGAIN } else if app.pages.any_in_sight() { EVERY } else { Duration::from_secs(30) };
        let wait = app.hooks.subs_due_in(Instant::now()).map_or(wait, |d| wait.min(d));   // a Stop's subagents, also out of sight (AC-39)
        let wait = app.turns.due_in(Instant::now()).map_or(wait, |d| wait.min((d + Duration::from_millis(50)).max(SETTLE_POLL)));   // a turn's end, settled
        let until = tokio::time::Instant::now() + wait;
        loop {
            tokio::select! {
                _ = app.dirty.notified() => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    break;
                }
                _ = app.changed.notified() => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    lists::request_changed(&app);
                }
                _ = tokio::time::sleep_until(until) => break,
            }
        }
    }
}

/// Says when a server's control client has not answered for 3 s, and when it answers again
/// (the page's stall line, AC-03); the control client itself is restarted after 10 s.
async fn stalls(app: Shared) {
    let mut stalled: HashSet<String> = HashSet::new();
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        for server in app.tmux.all() {
            let waited = server.bulk().map_or(Duration::ZERO, |c| c.waiting());
            let label = server.label();
            // the page keys its line by the server's id; `what` names it (your tmux has no "on")
            let what = if server.is_users() { label.clone() } else { format!("tmux on {label}") };
            if waited >= STALL && stalled.insert(server.id()) {
                tracing::warn!(event = "tmux.stall", server = %server.id(), ms = waited.as_millis() as u64);
                app.pages.tell(&json!({"t": "stall", "server": label, "key": server.id(), "what": what, "ok": false, "ms": waited.as_millis() as u64}).to_string());
            } else if waited < STALL && stalled.remove(&server.id()) {
                app.pages.tell(&json!({"t": "stall", "server": label, "key": server.id(), "what": what, "ok": true}).to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_ends_when_a_working_or_waiting_pane_is_done_failed_or_no_agent() {
        let layout = |states: &[(&str, Value)]| -> Vec<Value> {
            vec![json!({"items": states.iter().map(|(id, st)| json!({"id": id, "state": st})).collect::<Vec<_>>()})]
        };
        let old = Value::Array(layout(&[("a", json!("working")), ("b", json!("waiting")), ("c", json!("working")), ("d", json!(null)), ("e", json!("working"))]));
        let new = layout(&[("a", json!("done")), ("b", json!("working")), ("c", json!("failed")), ("d", json!("done")), ("e", json!(null))]);
        assert_eq!(ended_turns(&old, &new), ["a", "c", "e"]);
        assert!(ended_turns(&json!([]), &new).is_empty(), "a first layout ends nothing");
    }
}
