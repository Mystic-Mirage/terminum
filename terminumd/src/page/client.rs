// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! One page's state and its messages (spec §9 WebSocket table): which pane it shows, its
//! view's grid, and the dispatch of what it sends. Errors are said in words on the page
//! and in the log (fail loud).

use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};
use tokio::task::JoinHandle;

use super::stream::Ctl;
use super::{Page, CLOSED};
use crate::app::Shared;
use crate::tmux::Server;

/// The pane a page shows.
pub struct Shown {
    pub key: String,
    pub server: Arc<Server>,
    pub pane: String,
    pub ctl: Arc<Ctl>,
    pub task: JoinHandle<()>,
}

impl Drop for Shown {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
pub struct State {
    pub shown: Option<Shown>,
    /// the view's grid at the chosen text size (`size`)
    pub view: Option<(u16, u16)>,
    /// the page is hidden: no stream, no list refresh for it
    pub paused: bool,
    /// its terminal is hidden behind Files or View (a phone): no screens captured or sent for it
    pub screens_off: bool,
    /// when this page last made a window, session or split (one a second, AC-07)
    pub last_new: Option<Instant>,
    /// typed, not yet handed to tmux
    pub queue: super::typing::Queue,
    /// the window this page last resized and when (a window at most once a second, AC-13)
    pub resized_at: Option<((String, String), Instant)>,
}

/// What the page sent cannot be done: say why, on the page and in the log.
pub fn error(page: &Page, msg: impl Into<String>, sid: Option<&str>) {
    let msg = msg.into();
    tracing::warn!(event = "page.error", page = page.id, msg);
    let mut v = json!({"t": "error", "msg": msg});
    if let Some(s) = sid {
        v["sid"] = json!(s);
    }
    page.send_json(&v);
}

/// Handles one message from a page.
pub async fn handle(app: &Shared, page: &Arc<Page>, msg: Value) {
    let t = msg["t"].as_str().unwrap_or("");
    let s = |k: &str| msg[k].as_str().unwrap_or("").to_string();
    match t {
        "sub" => {
            super::sizing::view_of(page, &msg);       // its grid with it: the first frame comes at it (AC-04)
            subscribe(app, page, &s("id")).await
        }
        "in" => super::typing::typed(app, page, s("data"), msg["k"].as_u64()),
        "paste" => super::typing::paste(app, page, s("data"), msg["bracket"].as_bool().unwrap_or(true)),
        // reads wait on tmux or ssh: off the loop, so keys sent meanwhile are typed at once
        "more" => {
            let (before, to) = (msg["before"].as_i64().unwrap_or(0), msg["to"].as_i64());
            read(app, page, "more", move |a, p| async move { super::hist::older(&a, &p, before, to).await });
        }
        "unfold" => {
            let (n, to) = (msg["n"].as_i64().unwrap_or(0), msg["to"].as_i64().unwrap_or(-1));
            read(app, page, "unfold", move |a, p| async move { super::hist::unfold(&a, &p, n, to).await });
        }
        "find" => super::find::start(app, page, &msg),
        "mark" | "unmark" | "locate" | "remind" => super::marks::handle(app, page, t, &msg).await,
        "files" => read(app, page, "files", |a, p| async move { files(&a, &p).await }),
        "size" | "mouse" => super::sizing::handle(app, page, t, &msg).await,
        "new" | "newsession" | "split" | "clone" | "rename" | "renamegroup" | "reorder" | "move" | "kill" => super::sessions::handle(app, page, t, &msg).await,
        "plan" => super::plans::handle(app, page, &msg).await,
        "project" => super::projects::handle(app, page, &msg).await,
        "quick" | "quickside" => super::quick::handle(app, page, t, &msg).await,
        // a host's ssh, helper copy and install take up to minutes: the page's loop goes on (it waits for the answer)
        "addhost" | "hostaction" | "hostagents" => {
            let (app, page) = (app.clone(), page.clone());
            tokio::spawn(async move { super::sessions::admin(&app, &page, msg["t"].as_str().unwrap_or(""), &msg).await });
        }
        "restore" | "addtmux" | "detachtmux" | "tmuxsessions" => super::sessions::admin(app, page, t, &msg).await,
        "updatechoice" if !page.web => {
            if let Err(e) = crate::release::choose(app, &s("action"), msg["tag"].as_str()) {
                error(page, e, None);
            }
        }
        "sent" | "sentget" | "sentmarks" | "askno" | "asked" | "askedmarks" => super::sent::handle(app, page, t, &msg).await,
        "history" => crate::looks::set_history(app, page, &msg),
        "scheme" => crate::looks::set_scheme(app, page, &msg),
        "restoreseen" => crate::restore::seen(app),
        "marksfullseen" => super::marksfull::seen(app, &s("scope"), msg["remind"] == true),
        "unread" => super::unread::set(app, &s("id"), msg["on"].as_bool().unwrap_or(true)),
        "lastlines" => crate::lastlines::show(app, page, &s("id")).await,
        "diag" => super::diag::show(app, page, &s("id")).await,
        "trace" => super::timing::handle(app, page, &msg),
        "pause" => {
            pause(page, true);
            super::sizing::handover(app, page).await;
        }
        "resume" => resume(app, page),
        "screens" => screens(page, msg["on"] != false),
        _ => tracing::debug!(event = "page.unknown", t),
    }
}

/// Runs a read in its lane (`reads`); a full lane is said on the page.
fn read<F, Fut>(app: &Shared, page: &Arc<Page>, kind: &'static str, work: F)
where
    F: FnOnce(Shared, Arc<Page>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    if !super::reads::spawn(page, kind, work(app.clone(), page.clone())) {
        error(page, "Too many reads of the history or folder are waiting; try again in a moment.", None);
    }
}

/// Shows pane `key`: its theme, history and screen, then every change (AC-02).
pub async fn subscribe(app: &Shared, page: &Arc<Page>, key: &str) {
    let old = page.state.lock().shown.take();
    if let Some(old) = old {
        super::sizing::left(app, page, &old).await;
        drop(old);
    }
    let Some((server, pane)) = app.tmux.resolve(key) else {
        error(page, CLOSED, Some(key));
        return;
    };
    let ctl = Arc::new(Ctl::default());
    let task = tokio::spawn(super::stream::run(app.clone(), page.clone(), server.clone(), pane.clone(), key.to_string(), ctl.clone()));
    page.state.lock().shown = Some(Shown { key: key.to_string(), server, pane, ctl, task });
    if !page.state.lock().paused {
        super::unread::read(app, key);
    }
}

fn pause(page: &Page, on: bool) {
    let mut st = page.state.lock();
    st.paused = on;
    if on
        && let Some(s) = &st.shown {
            s.ctl.pause(true);
        }
}

/// The terminal hidden behind Files or View, or shown again: unlike `pause`, Files and the pane's
/// sizing go on; shown again, the stream sends a whole screen and history (AC-02).
fn screens(page: &Page, on: bool) {
    let mut st = page.state.lock();
    st.screens_off = !on;
    if on
        && let Some(s) = &st.shown {
            s.ctl.wake.notify_one();
        }
}

/// Back from hidden: the stream goes on, sending only what changed meanwhile.
fn resume(app: &Shared, page: &Arc<Page>) {
    pause(page, false);
    let st = page.state.lock();
    if let Some(s) = &st.shown {
        s.ctl.pause(false);
    }
    let shown = st.shown.as_ref().map(|s| s.key.clone());
    drop(st);
    if let Some(k) = shown {
        super::unread::read(app, &k);
    }
    page.send_json(&json!({"t": "layout", "groups": app.layout.read().clone()}));
    super::sizing::activated(app, page);           // the active page sizes its pane (AC-13)
}

/// The shown pane's folder, home and host, for the Files view and printed paths (AC-51).
async fn files(app: &Shared, page: &Arc<Page>) {
    match shown_cwd(page).await {
        Ok((key, server, cwd)) => page.send_json(&files_message(app, &server, &key, &cwd).await),
        Err((key, e)) => page.send_json(&json!({"t": "files", "sid": key, "error": e})),
    };
}

/// The shown pane's key, server and folder; Err(its key if any, why).
pub async fn shown_cwd(page: &Page) -> Result<(String, Arc<Server>, String), (Option<String>, String)> {
    let (key, server, pane) = {
        let st = page.state.lock();
        let s = st.shown.as_ref().ok_or((None, "No pane is shown.".to_string()))?;
        (s.key.clone(), s.server.clone(), s.pane.clone())
    };
    match server.run(format!("display -p -t {pane} '#{{pane_current_path}}'")).await {
        Ok(lines) => Ok((key, server, lines.into_iter().next().unwrap_or_default())),
        Err(e) => Err((Some(key), format!("The pane's folder is not known: {e}"))),
    }
}

/// The `files` message: the shown pane's key, folder, home and host (for the Files panel), and
/// the folder its coding agent works in when its hooks said one (printed paths, AC-35).
pub async fn files_message(app: &Shared, server: &crate::tmux::Server, key: &str, cwd: &str) -> Value {
    let host = (!server.is_local()).then(|| server.name.clone());
    let home = if server.is_local() { crate::config::home().display().to_string() } else { server.home().await.unwrap_or_default() };
    let agent = app.hooks.cwd(key).filter(|a| a != cwd);
    json!({"t": "files", "sid": key, "key": key, "cwd": cwd, "home": home, "host": host, "agent": agent})
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::page::{trace, Pages};

    #[tokio::test]
    async fn a_hidden_terminal_stops_screens_only_and_showing_it_wakes_the_stream() {
        let dir = std::env::temp_dir().join(format!("tm-screens-{}", std::process::id()));
        let (page, _rx) = Pages::default().add(true, trace::Recorder::new(dir)).unwrap();
        screens(&page, true);                                // nothing shown yet: only the state
        assert!(!page.state.lock().screens_off);
        let link = crate::tmux::server::Link::Local { tmux: "tmux".into(), socket: "tm-test-screens".into(), conf: "/dev/null".into() };
        let server = Server::new("local", "tm-test-screens", link, Arc::new(tokio::sync::Notify::new()));
        let ctl = Arc::new(Ctl::default());
        let task = tokio::spawn(async {});
        page.state.lock().shown = Some(Shown { key: "k".into(), server, pane: "%1".into(), ctl: ctl.clone(), task });
        let woken = |ctl: Arc<Ctl>| async move { tokio::time::timeout(Duration::from_millis(50), ctl.wake.notified()).await.is_ok() };

        screens(&page, false);
        assert!(page.state.lock().screens_off && !page.state.lock().paused, "hidden behind Files: screens off, not paused");
        assert!(!woken(ctl.clone()).await, "hiding wakes nothing");
        screens(&page, true);
        assert!(!page.state.lock().screens_off);
        assert!(woken(ctl.clone()).await, "shown again: the stream is woken to send all anew");
    }
}
