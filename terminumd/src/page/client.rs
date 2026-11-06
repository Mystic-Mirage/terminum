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
    /// how the page is looked at (AC-13, AC-37)
    pub look: Look,
    /// its terminal is hidden behind Files or View (a phone): no screens captured or sent for it
    pub screens_off: bool,
    /// the pane shown when it last asked for a window (AC-132)
    pub asked_on: Option<String>,
    /// when this page last made a window, session or split (one a second, AC-07)
    pub last_new: Option<Instant>,
    /// typed, not yet handed to tmux
    pub queue: super::typing::Queue,
    /// the window this page last resized and when (a window at most once a second, AC-13)
    pub resized_at: Option<((String, String), Instant)>,
}

/// How a page is looked at: `On` is the active page (in sight and, on a computer, focused), which
/// sizes its pane and reads its turns; `Away` is in sight out of focus (its stream and the list go
/// on); `Hidden` has neither (AC-13, AC-37, AC-93).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    #[default]
    On,
    Away,
    Hidden,
}

impl State {
    pub fn active(&self) -> bool {
        self.look == Look::On
    }
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
            subscribe(app, page, &s("id"))
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
        "size" | "mouse" => super::sizing::handle(app, page, t, &msg),
        "new" | "newsession" | "split" | "clone" | "rename" | "renamegroup" | "reorder" | "move" | "kill" => {
            page.state.lock().asked_on = shown_key(page);    // a window that comes late takes nobody away (AC-132)
            let (a, p, t) = (app.clone(), page.clone(), t.to_string());
            action(app, page, &msg.clone(), async move { super::sessions::handle(&a, &p, &t, &msg).await });
        }
        "plan" => super::plans::handle(app, page, &msg).await,
        "project" => super::projects::handle(app, page, &msg).await,
        "quick" | "quickside" => {
            let (a, p, t) = (app.clone(), page.clone(), t.to_string());
            action(app, page, &msg.clone(), async move { super::quick::handle(&a, &p, &t, &msg).await });
        }
        // a host's ssh, helper copy and install take up to minutes: the page's loop goes on (it waits for the answer)
        "addhost" | "hostaction" | "hostagents" => {
            let (app, page) = (app.clone(), page.clone());
            tokio::spawn(async move { super::sessions::admin(&app, &page, msg["t"].as_str().unwrap_or(""), &msg).await });
        }
        "restore" | "addtmux" | "detachtmux" | "tmuxsessions" => {
            let (a, p, t) = (app.clone(), page.clone(), t.to_string());
            action(app, page, &msg.clone(), async move { super::sessions::admin(&a, &p, &t, &msg).await });
        }
        "updatechoice" if !page.web => {
            if let Err(e) = crate::release::choose(app, &s("action"), msg["tag"].as_str()) {
                error(page, e, None);
            }
        }
        "sent" | "sentget" | "sentmarks" | "askno" | "asked" | "askedmarks" => {
            let (a, p, t) = (app.clone(), page.clone(), t.to_string());
            action(app, page, &msg.clone(), async move { super::sent::handle(&a, &p, &t, &msg).await });
        }
        "history" => crate::looks::set_history(app, page, &msg),
        "scheme" => crate::looks::set_scheme(app, page, &msg),
        "restoreseen" => crate::restore::seen(app),
        "marksfullseen" => super::marksfull::seen(app, &s("scope"), msg["remind"] == true),
        "unread" => super::unread::set(app, &s("id"), msg["on"].as_bool().unwrap_or(true)),
        "lastlines" => {
            let id = s("id");
            read(app, page, "lastlines", move |a, p| async move { crate::lastlines::show(&a, &p, &id).await });
        }
        "diag" => {
            let id = s("id");
            read(app, page, "diag", move |a, p| async move { super::diag::show(&a, &p, &id).await });
        }
        "trace" => super::timing::handle(app, page, &msg),
        "pause" => {
            // a page before 0.24.32 sends no `hidden`: it was hidden
            look(page, if msg["hidden"] == false { Look::Away } else { Look::Hidden });
            super::sizing::handover(app, page);
        }
        "resume" => resume(app, page),
        "screens" => screens(page, msg["on"] != false),
        _ => tracing::debug!(event = "page.unknown", t),
    }
}

/// The shown pane's server, what its lanes go by (AC-132).
fn shown_lane(page: &Page) -> String {
    page.state.lock().shown.as_ref().map_or_else(|| "none".to_string(), |s| s.server.id())
}

fn shown_key(page: &Page) -> Option<String> {
    page.state.lock().shown.as_ref().map(|s| s.key.clone())
}

/// The server a message is about: its pane's, its group's or its place, else the shown pane's.
fn lane_of(app: &Shared, page: &Page, msg: &Value) -> String {
    if let Some((server, _)) = msg["id"].as_str().and_then(|id| app.tmux.resolve(id)) {
        return server.id();
    }
    if let Some((prefix, _)) = msg["group"].as_str().and_then(|g| g.rsplit_once('/')) {
        return prefix.to_string();
    }
    msg["server"].as_str().map_or_else(|| shown_lane(page), str::to_string)
}

/// Runs what may wait on a server's tmux in that server's lane (`reads`), in order, never cut
/// part-way; a full lane is said on the page.
fn action<Fut>(app: &Shared, page: &Arc<Page>, msg: &Value, work: Fut)
where
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    if !super::reads::spawn_action(page, &lane_of(app, page, msg), work) {
        error(page, "Too many actions are waiting for that server; try again in a moment.", None);
    }
}

/// Runs a read in its lane (`reads`), one per kind and server; a full lane is said on the page.
fn read<F, Fut>(app: &Shared, page: &Arc<Page>, kind: &'static str, work: F)
where
    F: FnOnce(Shared, Arc<Page>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    if !super::reads::spawn(page, &format!("{kind} {}", shown_lane(page)), work(app.clone(), page.clone())) {
        error(page, "Too many reads of the history or folder are waiting; try again in a moment.", None);
    }
}

/// Shows pane `key`: its theme, history and screen, then every change (AC-02).
pub fn subscribe(app: &Shared, page: &Arc<Page>, key: &str) {
    let old = page.state.lock().shown.take();
    if let Some(old) = old {
        super::sizing::left(app, page, &old);
        drop(old);
    }
    let Some((server, pane)) = app.tmux.resolve(key) else {
        error(page, CLOSED, Some(key));
        return;
    };
    let ctl = Arc::new(Ctl::default());
    let mut st = page.state.lock();
    ctl.pause(st.look == Look::Hidden);            // a hidden page showing a pane (its reconnect) streams once in sight
    let task = tokio::spawn(super::stream::run(app.clone(), page.clone(), server.clone(), pane.clone(), key.to_string(), ctl.clone()));
    st.shown = Some(Shown { key: key.to_string(), server, pane, ctl, task });
    let active = st.active();
    drop(st);
    if active {
        super::unread::read(app, key);
    }
}

/// The page's new look: only a hidden page's stream pauses (AC-37).
fn look(page: &Page, look: Look) {
    let mut st = page.state.lock();
    st.look = look;
    if let Some(s) = &st.shown {
        s.ctl.pause(look == Look::Hidden);
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

/// The active page again: a stream it paused goes on, sending only what changed meanwhile.
fn resume(app: &Shared, page: &Arc<Page>) {
    look(page, Look::On);
    let shown = page.state.lock().shown.as_ref().map(|s| s.key.clone());
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

    /// A page that shows pane "k" of a server on `socket` (never started), and its stream's control.
    fn shown(page: &Page, socket: &str) -> Arc<Ctl> {
        let link = crate::tmux::server::Link::Local { tmux: "tmux".into(), socket: socket.into(), conf: "/dev/null".into() };
        let server = Server::new("local", socket, link, Arc::new(tokio::sync::Notify::new()));
        let ctl = Arc::new(Ctl::default());
        page.state.lock().shown = Some(Shown { key: "k".into(), server, pane: "%1".into(), ctl: ctl.clone(), task: tokio::spawn(async {}) });
        ctl
    }

    #[tokio::test]
    async fn a_hidden_terminal_stops_screens_only_and_showing_it_wakes_the_stream() {
        let dir = crate::testutil::temp_root().join(format!("tm-screens-{}", std::process::id()));
        let (page, _rx) = Pages::default().add(true, trace::Recorder::new(dir)).unwrap();
        screens(&page, true);                                // nothing shown yet: only the state
        assert!(!page.state.lock().screens_off);
        let ctl = shown(&page, "tm-test-screens");
        let woken = |ctl: Arc<Ctl>| async move { tokio::time::timeout(Duration::from_millis(50), ctl.wake.notified()).await.is_ok() };

        screens(&page, false);
        assert!(page.state.lock().screens_off && page.state.lock().active(), "hidden behind Files: screens off, not paused");
        assert!(!woken(ctl.clone()).await, "hiding wakes nothing");
        screens(&page, true);
        assert!(!page.state.lock().screens_off);
        assert!(woken(ctl.clone()).await, "shown again: the stream is woken to send all anew");
    }

    #[tokio::test]
    async fn a_page_out_of_focus_keeps_its_stream_and_only_a_hidden_one_pauses_it() {
        let dir = crate::testutil::temp_root().join(format!("tm-pause-{}", std::process::id()));
        let pages = Pages::default();
        let (page, _rx) = pages.add(true, trace::Recorder::new(dir)).unwrap();
        let ctl = shown(&page, "tm-test-pause");
        let state = || {
            let active = page.state.lock().active();
            (active, ctl.is_paused(), pages.any_in_sight())
        };

        look(&page, Look::Away);
        assert_eq!(state(), (false, false, true), "out of focus: not the active page, the stream and the list go on");
        look(&page, Look::Hidden);
        assert_eq!(state(), (false, true, false), "hidden: the stream pauses, the list slows");
        look(&page, Look::Away);
        assert_eq!(state(), (false, false, true), "in sight again, still out of focus: the stream is back");
        look(&page, Look::On);
        assert_eq!(state(), (true, false, true));
    }
}
