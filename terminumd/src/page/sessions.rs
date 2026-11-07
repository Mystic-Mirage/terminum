// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Sessions, windows and panes from the page (AC-07): new window after the shown one in its
//! folder, new session, split, rename, reorder, move to another session, close. Names and
//! folders reach tmux as single quoted arguments with `#` doubled where tmux expands formats
//! (AS-11); a folder with a control character is not used. tmux's refusals are said in words.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::client::error;
use super::Page;
use crate::app::Shared;
use crate::tmux::{args, Server};

/// One new window, session or split a second per page (AC-07).
const NEW_EVERY: Duration = Duration::from_secs(1);

/// The server and session a group id names (`Server::group_prefix`; a bare name is a session
/// of the private server).
pub(super) fn group(app: &Shared, wid: &str) -> Option<(Arc<Server>, String)> {
    match wid.split_once('/') {
        Some(("local", s)) => Some((app.tmux.local.clone(), s.to_string())),
        Some((h, s)) => match h.strip_prefix("local:") {
            Some(socket) => app.tmux.attached(socket),
            None => app.tmux.host(h),
        }.map(|srv| (srv, s.to_string())),
        None if !wid.is_empty() => Some((app.tmux.local.clone(), wid.to_string())),
        None => None,
    }
}

fn home_of(server: &Server) -> String {
    if server.is_local() { crate::config::home().display().to_string() } else { "~".to_string() }
}

/// A pane's folder for a new window there: "~" (with a note) when its name cannot be used.
pub(super) async fn folder_of(server: &Server, pane: &str) -> (String, Option<String>) {
    let cwd = server.run(format!("display -p -t {pane} '#{{pane_current_path}}'")).await.ok().and_then(|l| l.into_iter().next()).unwrap_or_default();
    if cwd.is_empty() {
        return (home_of(server), None);
    }
    match args::check_text(&cwd) {
        Ok(()) => (cwd, None),
        Err(_) => (home_of(server), Some("folder name not usable: opened in ~".to_string())),
    }
}

/// tmux's answer as one line of words.
pub(super) fn said(e: &str) -> String {
    let e = e.lines().next().unwrap_or("").trim();
    if e.is_empty() { "tmux refused it".into() } else { e.to_string() }
}

pub(super) fn allowed_now(page: &Page) -> bool {
    let mut st = page.state.lock();
    if st.last_new.is_some_and(|t| t.elapsed() < NEW_EVERY) {
        return false;
    }
    st.last_new = Some(Instant::now());
    true
}

/// Something new was made: the list has it before the page is told to show it.
async fn created(app: &Shared, page: &Page, asked_on: Option<&str>, server: &Arc<Server>, pane_id: &str, note: Option<String>) {
    crate::lists::refresh_server(app, server).await;
    let listed = super::marks::find_item(&app.layout.read(), &server.key(pane_id)).is_some();
    page.send_json(&json!({"t": "layout", "groups": app.layout.read().clone()}));
    // the user went on meanwhile, or the list does not have it yet: they stay where they are (AC-132)
    let moved = page.state.lock().shown.as_ref().map(|s| s.key.as_str()) != asked_on;
    page.send_json(&json!({"t": "created", "id": server.key(pane_id), "note": note, "stay": moved || !listed}));
}

pub async fn handle(app: &Shared, page: &Arc<Page>, msg: &Value, at: super::client::At) {
    let t = msg["t"].as_str().unwrap_or("");
    let asked_on = at.as_ref().map(|(key, ..)| key.as_str());
    let s = |k: &str| msg[k].as_str().unwrap_or("").to_string();
    if matches!(t, "new" | "newsession" | "split" | "clone" | "projectwindow") && !allowed_now(page) {
        error(page, "Wait a second: one new window, session or split a second.", None);
        return;
    }
    let result = match t {
        "new" => new_window(app, page, at.as_ref(), &s("group")).await,
        "newsession" => new_session(app, page, asked_on, &s("name")).await,
        "split" => split(app, page, asked_on, &s("id"), &s("how")).await,
        "clone" => clone_window(app, page, asked_on, &s("id"), msg["project"].as_str()).await,
        "projectwindow" => project_window(app, page, asked_on, &s("project"), &s("dir")).await,
        "rename" => rename(app, &s("id"), msg["name"].as_str().unwrap_or("")).await,
        "renamegroup" => rename_group(app, &s("group"), &s("name")).await,
        "reorder" => reorder(app, &s("group"), msg["ids"].as_array().cloned().unwrap_or_default()).await,
        "move" => move_window(app, &s("id"), &s("session")).await,
        "kill" => kill(app, &s("id")).await,
        _ => Ok(()),
    };
    match result {
        Ok(()) if matches!(t, "rename" | "renamegroup" | "reorder" | "move" | "kill") => {
            // its server's list alone: nothing else changed
            let server = msg["id"].as_str().and_then(|id| app.tmux.resolve(id)).or_else(|| msg["group"].as_str().and_then(|g| group(app, g)));
            match server {
                Some((server, _)) => { crate::lists::request(app, &server); }
                None => crate::lists::request_all(app),
            }
        }
        Ok(()) => {}
        Err(e) => error(page, e, None),
    }
}

async fn new_window(app: &Shared, page: &Page, at: Option<&(String, Arc<Server>, String)>, wid: &str) -> Result<(), String> {
    let (server, session) = group(app, wid).ok_or("That session is gone.")?;
    // after the window of the pane shown when it was asked, in its folder, when it is in this session (AC-132)
    let shown = at.filter(|(_, s, _)| s.id() == server.id()).map(|(_, _, pane)| pane.clone());
    let place = match &shown {
        Some(pane) => server.run(format!("display -p -t {pane} '#{{session_name}}\t#{{window_id}}'")).await.ok().and_then(|l| l.into_iter().next())
            .and_then(|l| l.split_once('\t').map(|(s, w)| (s.to_string(), w.to_string()))).filter(|(s, _)| *s == session),
        None => None,
    };
    let (dir, note) = match (&place, &shown) {
        (Some(_), Some(pane)) => folder_of(&server, pane).await,
        _ => (home_of(&server), None),
    };
    let target = match &place {
        Some((_, w)) => format!("-a -t {w}"),
        None => format!("-t {}", args::quote(&format!("={session}:"))),
    };
    let pane = open_window(&server, &target, &dir).await?;
    created(app, page, at.map(|(key, ..)| key.as_str()), &server, &pane, note).await;
    Ok(())
}

/// A new window at `target` ("-a -t @2": right after it) in `dir`: its pane's id.
async fn open_window(server: &Server, target: &str, dir: &str) -> Result<String, String> {
    let line = format!("new-window {target} -c {} -P -F '#{{pane_id}}'", args::format_arg(dir));
    let made = server.run(line).await.map_err(|e| format!("No new window: {}", said(&e)))?;
    Ok(made.into_iter().next().unwrap_or_default())
}

/// Clone (AC-07): a new window right after pane `id`'s, in its folder, with a shell (what runs
/// there is not copied). A session of a project (or its [+], `project`) gives the project's
/// window: in the project's folder while that is usable, and in the project after it (AC-92).
async fn clone_window(app: &Shared, page: &Page, asked_on: Option<&str>, id: &str, project: Option<&str>) -> Result<(), String> {
    let (server, pane) = app.tmux.resolve(id).ok_or(super::CLOSED)?;
    let window = window_of(&server, &pane).await?;
    let joins = {
        let ps = app.projects.lock();
        project.and_then(|p| ps.list().iter().find(|x| x.id == p)).or_else(|| ps.of(id)).map(|p| (p.id.clone(), p.dir.clone()))
    };
    let usable = |d: &str| !d.is_empty() && args::check_text(d).is_ok() && (!server.is_local() || std::path::Path::new(d).is_dir());
    let (dir, note) = match &joins {
        Some((_, d)) if usable(d) => (d.clone(), None),
        _ => folder_of(&server, &pane).await,
    };
    let new = open_window(&server, &format!("-a -t {window}"), &dir).await?;
    if let Some((pid, d)) = joins {
        let (key, after, a) = (server.key(&new), id.to_string(), app.clone());
        let host = (!server.is_local()).then(|| server.name.clone());
        let r = tokio::task::spawn_blocking(move || a.projects.lock().add(&key, Some(&pid), Some(&after), host, d, None)).await;
        if let Ok(Err(e)) = r {
            error(page, e, None);
        }
        super::projects::tell(app);
    }
    created(app, page, asked_on, &server, &new, note).await;
    Ok(())
}

/// The [+] of a project with no session (AC-110): a window in its folder, on its host, in the tmux
/// session named after it. `dir`, when given, is its folder from now on.
async fn project_window(app: &Shared, page: &Page, asked_on: Option<&str>, project: &str, dir: &str) -> Result<(), String> {
    let (name, host, kept) = {
        let ps = app.projects.lock();
        let p = ps.list().iter().find(|p| p.id == project).ok_or("That project is gone.")?;
        (p.name.clone(), p.host.clone(), p.dir.clone())
    };
    let dir = if dir.is_empty() { kept } else { dir.to_string() };
    let server = match &host {
        Some(h) => app.tmux.host(h).ok_or_else(|| format!("{h} is not connected."))?,
        None => app.tmux.local.clone(),
    };
    if dir.is_empty() {
        return Err("This project has no folder yet.".into());
    }
    args::check_text(&dir)?;
    if server.is_local() && !std::path::Path::new(&dir).is_dir() {
        return Err(format!("The folder {dir} does not exist."));
    }
    let session = args::check_session_name(&name.replace([':', '.'], "-"))?;
    let exists = server.run(format!("has-session -t {}", args::quote(&format!("={session}")))).await.is_ok();
    let line = if exists {
        format!("new-window -t {} -c {} -P -F '#{{pane_id}}'", args::quote(&format!("={session}:")), args::format_arg(&dir))
    } else {
        format!("new-session -d -s {} -c {} -P -F '#{{pane_id}}'", args::format_arg(&session), args::format_arg(&dir))
    };
    let made = server.run(line).await.map_err(|e| format!("No new window: {}", said(&e)))?;
    let pane = made.into_iter().next().unwrap_or_default();
    let (key, pid, a) = (server.key(&pane), project.to_string(), app.clone());
    let folder = dir.clone();
    let joined = tokio::task::spawn_blocking(move || {
        let mut ps = a.projects.lock();
        ps.set_dir(&pid, folder.clone())?;
        ps.add(&key, Some(&pid), None, host, folder, None).map(|_| ())
    }).await.map_err(|e| e.to_string()).and_then(|r| r);
    joined?;
    super::projects::tell(app);
    created(app, page, asked_on, &server, &pane, None).await;
    Ok(())
}

async fn new_session(app: &Shared, page: &Page, asked_on: Option<&str>, name: &str) -> Result<(), String> {
    let name = args::check_session_name(name)?;
    let server = app.tmux.local.clone();
    let home = home_of(&server);
    let line = format!("new-session -d -s {} -c {} -P -F '#{{pane_id}}'", args::format_arg(&name), args::format_arg(&home));
    let pane = server.run(line).await.map_err(|e| format!("No new session: {}", said(&e)))?;
    created(app, page, asked_on, &server, pane.first().map_or("", |s| s.as_str()), None).await;
    Ok(())
}

async fn split(app: &Shared, page: &Page, asked_on: Option<&str>, id: &str, how: &str) -> Result<(), String> {
    let (server, pane) = app.tmux.resolve(id).ok_or(super::CLOSED)?;
    let (dir, note) = folder_of(&server, &pane).await;
    let flag = if how == "below" { "-v" } else { "-h" };
    let line = format!("split-window {flag} -t {pane} -c {} -P -F '#{{pane_id}}'", args::format_arg(&dir));
    let new = server.run(line).await.map_err(|e| format!("No split: {}", said(&e)))?;
    created(app, page, asked_on, &server, new.first().map_or("", |s| s.as_str()), note).await;
    Ok(())
}

pub(crate) async fn window_of(server: &Server, pane: &str) -> Result<String, String> {
    server.run(format!("display -p -t {pane} '#{{window_id}}'")).await.ok().and_then(|l| l.into_iter().next()).filter(|w| w.starts_with('@')).ok_or_else(|| super::CLOSED.to_string())
}

/// The pencil: names the window and the pane; an empty name gives tmux the naming back.
async fn rename(app: &Shared, id: &str, name: &str) -> Result<(), String> {
    let (server, pane) = app.tmux.resolve(id).ok_or(super::CLOSED)?;
    let window = window_of(&server, &pane).await?;
    let name = name.trim();
    let lines = if name.is_empty() {
        vec![format!("set-option -w -t {window} automatic-rename on"), format!("select-pane -t {pane} -T ''")]
    } else {
        let name = args::check_name(name)?;
        // `--`: a name that starts with "-" is the name, not a flag (-T's value is safe as it is)
        vec![format!("rename-window -t {window} -- {}", args::format_arg(&name)), format!("select-pane -t {pane} -T {}", args::format_arg(&name))]
    };
    for a in server.run_all_on(&pane, lines).await {
        a.map_err(|e| format!("Not renamed: {}", said(&e)))?;
    }
    Ok(())
}

/// A tmux session's group renamed (its menu's Rename…): its session, on terminum's own tmux only
/// (the user's own sessions and a host's keep the names they have there, AC-89).
async fn rename_group(app: &Shared, wid: &str, name: &str) -> Result<(), String> {
    let (server, session) = group(app, wid).ok_or("That session is gone.")?;
    if !server.is_local() || server.is_users() {
        return Err("Only a session of terminum's own tmux is renamed here.".into());
    }
    let name = args::check_session_name(name.trim())?;
    if name == session {
        return Ok(());
    }
    // `--`: a name that starts with "-" is the name, not a flag
    let line = format!("rename-session -t {} -- {}", args::quote(&format!("={session}")), args::format_arg(&name));
    server.run(line).await.map(drop).map_err(|e| format!("Not renamed: {}", said(&e)))
}

/// Windows of a session in the order the page dragged them to (rows of another session
/// are never moved).
async fn reorder(app: &Shared, wid: &str, ids: Vec<Value>) -> Result<(), String> {
    let (server, session) = group(app, wid).ok_or("That session is gone.")?;
    let target = args::quote(&format!("={session}"));
    let lines = server.run(format!("list-windows -t {target} -F '#{{window_id}}'")).await.map_err(|e| said(&e))?;
    let mut current: Vec<String> = lines.into_iter().filter(|w| w.starts_with('@')).collect();
    let mut want: Vec<String> = Vec::new();
    for id in ids.iter().filter_map(Value::as_str) {
        let Some((s, pane)) = app.tmux.resolve(id) else { continue };
        if s.id() != server.id() {
            continue;
        }
        if let Ok(w) = window_of(&server, &pane).await
            && current.contains(&w) && !want.contains(&w) {
                want.push(w);
            }
    }
    // the windows named take, in their order, the places they hold; the others keep theirs
    // (a project's, listed elsewhere, AC-92)
    let mut order = want.iter();
    let want: Vec<String> = current.iter().map(|w| if want.contains(w) { order.next().cloned().unwrap_or_else(|| w.clone()) } else { w.clone() }).collect();
    for i in 0..current.len() {
        if current[i] != want[i] {
            let j = current.iter().position(|w| *w == want[i]).ok_or("The windows changed meanwhile.")?;
            server.run(format!("swap-window -d -s {} -t {}", current[j], current[i])).await.map_err(|e| format!("Not moved: {}", said(&e)))?;
            current.swap(i, j);
        }
    }
    Ok(())
}

/// "Move to session…": the window goes to the end of another session of its server.
async fn move_window(app: &Shared, id: &str, to: &str) -> Result<(), String> {
    let (server, pane) = app.tmux.resolve(id).ok_or(super::CLOSED)?;
    let session = match group(app, to) {
        Some((srv, s)) if srv.id() == server.id() => s,
        _ => return Err("A window moves only between sessions of its own tmux server.".into()),
    };
    let window = window_of(&server, &pane).await?;
    server.run(format!("move-window -s {window} -t {}", args::quote(&format!("={session}:")))).await.map_err(|e| format!("Not moved: {}", said(&e)))?;
    Ok(())
}

/// "Close pane" (the page asked first): the program in it ends.
async fn kill(app: &Shared, id: &str) -> Result<(), String> {
    let (server, pane) = app.tmux.resolve(id).ok_or(super::CLOSED)?;
    server.run(format!("kill-pane -t {pane}")).await.map_err(|e| format!("Not closed: {}", said(&e)))?;
    Ok(())
}

/// Host, attach and restore actions, from either listener's page (§9 "What each listener allows").
pub async fn admin(app: &Shared, page: &Arc<Page>, t: &str, msg: &Value) {
    let s = |k: &str| msg[k].as_str().unwrap_or("").to_string();
    let result = match t {
        "addhost" => crate::hosts::add(app, &s("name"), &s("recipe"), msg["files"].as_bool().unwrap_or(false)).await.map(|name| {
            page.send_json(&json!({"t": "hostadded", "name": name}));
        }),
        "hostagents" => {
            let on = msg["on"].as_bool().unwrap_or(false);
            let got = crate::hosts::agents::set_agents(app, &s("name"), on).await;
            let (ok, lines) = match got { Ok(l) => (true, l), Err(e) => (false, vec![e]) };
            page.send_json(&json!({"t": "hostagents", "name": s("name"), "on": on, "ok": ok, "lines": lines}));
            crate::lists::request_all(app);
            return;
        }
        "hostaction" => crate::hosts::action(app, &s("name"), &s("action")).await,
        // AC-89: a place is its server's id (`local:<socket>`, `<host>:default`)
        "addtmux" => {
            let names: Vec<String> = msg["sessions"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
            crate::attach::Place::parse(app, &s("server")).and_then(|place| crate::attach::add(app, &place, &names).map(|()| {
                page.send_json(&json!({"t": "tmuxadded", "server": place.id(), "sessions": names}));
            }))
        }
        "detachtmux" => match crate::attach::Place::parse(app, &s("server")) {
            Ok(place) => crate::attach::detach(app, &place, &s("session")).await,
            Err(e) => Err(e),
        },
        "tmuxsessions" => {
            let (server, running) = match crate::attach::Place::parse(app, &s("server")) {
                Ok(place) => (place.id(), crate::attach::running(app, &place).await),
                Err(_) => (s("server"), Vec::new()),
            };
            // `ends`: those tmux destroys once terminum is their last client (destroy-unattached)
            let names: Vec<&str> = running.iter().map(|r| r.name.as_str()).collect();
            let ends: Vec<&str> = running.iter().filter(|r| r.ends).map(|r| r.name.as_str()).collect();
            page.send_json(&json!({"t": "tmuxsessions", "server": server, "names": names, "ends": ends}));
            return;
        }
        "restore" => crate::restore::again(app).await,
        _ => Ok(()),
    };
    match result {
        Ok(()) => crate::lists::request_all(app),
        Err(e) => error(page, e, None),
    }
}
