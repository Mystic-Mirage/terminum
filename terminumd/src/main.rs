// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! terminumd — terminum's server. The control client of a private tmux server (and of each
//! remote host's over ssh), it serves one page on 127.0.0.1, signed in with a password, to the
//! Mac's browser and, through `tailscale serve`, to other devices: the session list, screens, typing, sessions, the Files
//! panel and its API. `--agent` runs it on a remote host as the files agent (AC-82). Run by
//! the name `terminum`, or with a command (`terminumd install`), it is the CLI (cli/).

// Handlers answer early exits as `Err(Response)` (http::ApiResult), as fbd did: a large Err
// variant on purpose.
#![allow(clippy::result_large_err)]

mod activation;
mod agent;
mod agentapi;
mod agentmarks;
mod api_fs;
mod app;
mod attach;
mod auth;
mod cli;
mod config;
mod dragged;
mod events;
mod files;
mod folders;
mod hosts;
mod hotkeys;
mod http;
mod listing;
mod lists;
mod listing_sort;
mod looks;
mod manager;
mod midi;
mod ops;
mod page;
mod release;
mod panel_api;
mod panel_ws;
mod plans;
mod raw_body;
mod remote;
mod lastlines;
mod links;
mod restore;
mod restore_srv;
mod restore_lines;
mod routes;
mod runlink;
mod screen;
mod sent;
mod settings;
mod sock;
mod dispatcher;
mod tasks;
mod termtext;
#[cfg(test)]
mod testutil;
mod theme;
mod tmux;
mod umask;
mod update;
mod watch_web;
mod watcher;
mod webaccess;
mod workspace;

use std::sync::Arc;
use std::time::Instant;

use parking_lot::{Mutex, RwLock};

use app::{App, Config, Shared};
use config::Env;
use events::Bus;
use files::Roots;
use listing::Cache;
use tmux::server::{Link, Server};
use watcher::Watcher;
use workspace::Store;

fn build_app(cfg: Config, env: &Env, looks: looks::Looks, link: Link, socket: &str) -> Shared {
    let bus = Bus::new();
    let cache = Cache::new(env.list_cache_mb << 20);
    let watcher = Watcher::start(cache.clone(), bus.clone()).inspect_err(|e| tracing::warn!(event = "watch.unavailable", error = %e)).ok();
    let changed = Arc::new(tokio::sync::Notify::new());
    let local = Server::new("local", socket, link, changed.clone());
    let dir = cfg.dir.clone();
    // an Agent keeps no tasks: the Mac's terminumd holds the one store
    if !cfg.agent {
        tasks::files::move_old_folder(&dir).unwrap_or_else(|e| tracing::warn!(event = "tasks.attachments", error = %e));
    }
    let tasks = Arc::new(if cfg.agent { tasks::store::Store::absent() } else { tasks::store::Store::open(dir.join("kanban.sqlite")) });
    let projects = if cfg.agent { Default::default() } else { page::projects::Projects::load(tasks.clone(), &dir.join("projects.json")) };
    let task_marks = tasks::marks::Marks::default();
    task_marks.refresh(&tasks);
    Arc::new(App {
        store: Store::load(dir.join("workspaces.json"), env.ttl_days, bus.clone()),
        roots: Roots::parse(&env.roots),
        bus,
        cache,
        remotes: Default::default(),
        watcher,
        web_panes: Default::default(),
        started: Instant::now(),
        denied: Default::default(),
        tmux: tmux::Tmux::new(local),
        pages: Default::default(),
        layout: RwLock::new(serde_json::json!([])),
        offers: Default::default(),
        tmux_problem: Mutex::new(None),
        dirty: Default::default(),
        changed,
        refreshing: tokio::sync::Mutex::new(()),
        inventory_at: Mutex::new(None),
        agent_states: Mutex::new(tmux::agentstate::States::new()),
        hooks: Default::default(),
        sent: sent::Sent::new(cfg.home.join("sessions")),
        sizes: Default::default(),
        looks,
        signins: auth::signin::Signins::load(&dir),
        web: Default::default(),
        hosts: Default::default(),
        tunnels: Default::default(),
        agent_turns: Default::default(),
        links: Default::default(),
        restore: Default::default(),
        recorder: page::trace::Recorder::new(cfg.home.join("logs")),
        bookmarks: Mutex::new(page::bookmarks::Store::load(dir.join("web-bookmarks.json"))),
        projects: Mutex::new(projects),
        tasks,
        task_lines: Default::default(),
        dispatcher: Default::default(),
        task_marks,
        unread: Mutex::new(page::unread::Marks::load(&dir)),
        turns: Default::default(),
        plans: plans::Plans::load(&dir),
        cfg,
    })
}

/// Saves the workspaces and exits (WebSockets never end, so there is no graceful drain).
fn exit_with(store: &Store, reason: &str) -> ! {
    store.flush();
    agent::cleanup();
    tracing::info!(event = "stop", reason);
    std::process::exit(0);
}

fn logging() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("TM_LOG").unwrap_or_else(|_| "info".into()))
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();
}

async fn run_agent(socket: std::path::PathBuf) {
    let setup = agent::start(&socket);
    logging();
    let env = Env::read(0, "terminum");
    let home = config::home();
    let cfg = Config { port: 0, agent: true, service: false, text_max: env.text_max, history: env.history as i64,
        dir: setup.dir.clone(), home: home.join(".terminum"), agent_token: setup.token.clone() };
    let looks = looks::Looks::load(&setup.dir);
    let link = Link::Local { tmux: "tmux".into(), socket: "unused".into(), conf: "/dev/null".into() };
    let app = build_app(cfg, &Env { list_cache_mb: 32, ..env }, looks, link, "unused");
    let store = app.store.clone();
    let heard = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let line = heard.clone();
    agent::exit_with_stdin(move || line.store(true, std::sync::atomic::Ordering::Relaxed), move || exit_with(&store, "ssh connection closed"));
    let (store, bus) = (app.store.clone(), app.bus.clone());
    let used = move || heard.swap(false, std::sync::atomic::Ordering::Relaxed) || bus.subscribers() > 0;
    agent::exit_when_unused(used, move || exit_with(&store, "no Mac connected for 90 s"));
    ops::NO_FINDER.store(true, std::sync::atomic::Ordering::Relaxed);
    tokio::spawn(page::upload::sweep_daily(home.join(page::upload::REMOTE_DIR)));   // this host's pastes (AC-36)
    let store = app.store.clone();
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let (mut term, mut hup) = (signal(SignalKind::terminate()).unwrap(), signal(SignalKind::hangup()).unwrap());
        tokio::select! { _ = term.recv() => {}, _ = hup.recv() => {} }
        exit_with(&store, "signal");
    });
    tracing::info!(event = "start", mode = "agent", agent_id = agent::AGENT_ID, pid = std::process::id());
    axum::serve(setup.listener, routes::agent(app)).await.unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // a coding agent's hook (AC-39): nothing else first, it runs on every tool call
    if args.get(1).map(String::as_str) == Some("hook") {
        std::process::exit(cli::hook::main(args.get(2).map_or("", String::as_str)));
    }
    if cli::wanted(&args) {
        std::process::exit(cli::main(&args));
    }
    if args.iter().any(|a| a == "--version") {
        println!("{}", config::build_id());
        return;
    }
    if args.iter().any(|a| a == "--agent-id") {
        println!("{}", agent::AGENT_ID);
        return;
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    rt.block_on(serve());
}

/// The server (no argument, or `serve` as launchd's plist says), or `--agent` on a host.
async fn serve() {
    // what terminumd writes is the user's alone; terminals and Files' new entries keep the started umask (AC-09)
    umask::private();
    if let Some(socket) = agent::socket_arg() {
        return run_agent(socket).await;
    }
    logging();
    let home = config::tm_home();
    let dir = config::app_dir();
    let looks = looks::Looks::load(&home);
    let (port, socket, history) = {
        let s = looks.settings.read();
        (s.port, s.tmux_socket.clone(), s.tmux_history as usize)
    };
    let mut env = Env::read(port, &socket);
    let mut service = false;
    let listener = match activation::listener() {
        activation::Activation::Held(l) => { service = true; tokio::net::TcpListener::from_std(l).expect("launchd's socket") }
        activation::Activation::Unbound => sock::unbound(activation::unbound_message(env.port)),
        activation::Activation::NotLaunchd => sock::bind_port(&dir, env.port).await,
    };
    // launchd's port is the one served (and in /health) even when settings.json says another
    let held = listener.local_addr().map(|a| a.port()).unwrap_or(env.port);
    if held != env.port {
        tracing::warn!(event = "port.differs", settings = env.port, held, "launchd holds another port than settings.json: run terminum restart");
        env.port = held;
    }
    let cli = sock::bind(&dir);
    panel_ws::raise_open_files();
    let conf = home.join("tmux.conf");
    if let Err(e) = tmux::conf::ensure(&conf, if history > 0 { history } else { tmux::conf::HISTORY_LIMIT }) {
        tracing::warn!(event = "tmux.conf", error = %e);
    }
    let tmux_bin = config::find_program("tmux", "TM_TMUX").unwrap_or_else(|| "tmux".into());
    let link = Link::Local { tmux: tmux_bin, socket: env.tmux_socket.clone(), conf };
    let cfg = Config { port: env.port, agent: false, service, text_max: env.text_max,
        history: env.history as i64, dir: dir.clone(), home: home.clone(), agent_token: String::new() };
    let app = build_app(cfg, &env, looks, link, &env.tmux_socket);
    agentmarks::import_notes(&mut app.bookmarks.lock(), &dir);   // 0.23's notes are agents' bookmarks now (AC-98)
    page::bookmarks::adopt(&app);   // a file before version 3: its bookmarks take their projects
    tracing::info!(event = "start", port = env.port, pid = std::process::id(), build = config::build_id(), socket = %env.tmux_socket);
    tokio::spawn(axum::serve(cli, sock::routes(app.clone())).into_future());
    let checked = app.clone();
    tokio::task::spawn_blocking(move || {
        checked.tasks.quick_check();
        if let Err(e) = tasks::files::sweep(&checked.tasks, &checked.cfg.dir, tasks::ops::now()) {
            tracing::warn!(event = "tasks.sweep", error = %e);
        }
    });
    hosts::sync(&app);
    attach::sync(&app);
    webaccess::apply(&app);
    tokio::spawn(manager::run(app.clone()));
    tokio::spawn(dispatcher::walking(app.clone()));
    tokio::spawn(dispatcher::hearing(app.clone()));
    tokio::spawn(looks::follow(app.clone()));
    tokio::spawn(restore::follow(app.clone()));
    tokio::spawn(lastlines::follow(app.clone()));
    tokio::spawn(release::run(app.clone()));
    tokio::spawn(page::upload::sweep_daily(page::upload::local_dir()));   // pastes older than 7 days (AC-36)
    tokio::spawn(page::bookmarks::purge_daily(app.clone()));   // reminders resolved over 30 days ago (AC-91)
    let stopping = app.clone();
    tokio::spawn(async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
        // the user's own tmux windows get their size back, here and on hosts, all at once (AC-89);
        // the tmux control clients end with terminumd
        let releases = stopping.tmux.all().into_iter().flat_map(|server| {
            let only = server.only.lock().clone();
            let sessions: Vec<Option<String>> = match only {
                Some(_) if server.is_users() => vec![None],
                Some(list) => list.into_iter().map(Some).collect(),
                None => Vec::new(),
            };
            let app = stopping.clone();
            sessions.into_iter().map(move |s| {
                let (app, server) = (app.clone(), server.clone());
                async move { page::sizing::release(&app, &server, s.as_deref()).await }
            })
        }).collect::<Vec<_>>();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), futures_util::future::join_all(releases)).await;
        for server in stopping.tmux.all() {
            server.kill_clients();
        }
        page::unread::flush(&stopping);                // a mark shown is kept (AC-93)
        plans::flush(&stopping);                       // a plan is kept (AC-97)
        exit_with(&stopping.store, "signal");
    });
    let router = routes::pages(app);
    axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
}
