// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The pane's size follows the active page that shows it (AC-13, AC-06): a page sizes the
//! window on opening the pane (before its first frame, at the grid sent with `sub`: AC-04), on
//! becoming active (in sight and focused: `resume`) and on typing; a change of its view resizes only a window it sizes or nobody does; a hidden page
//! sizes nothing and hands its windows over. The user's own tmux too (AC-89), marked. A
//! window of one pane, or whose shown pane the user's own tmux zoomed, is resized with
//! `window-size manual` + `resize-window`; a window of several panes in sight never (AC-13).
//! A window no page shows goes back to tmux's own policy. At most one resize a second a page.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::client::Shown;
use super::Page;
use crate::app::Shared;
use crate::screen::frame::Fields;
use crate::tmux::{mouse, Server};

const EVERY: Duration = Duration::from_secs(1);
/// The grids a pane is sized to, whatever the screen (AC-13): the smallest fits an iPhone in
/// portrait at the default 13 px (a smaller view re-flows it, AC-04), the largest a
/// big desktop window, about 1920×1200 (a larger view leaves the rest blank).
const MIN: (u16, u16) = (45, 14);
const MAX: (u16, u16) = (240, 70);

/// A page's grid within MIN and MAX.
fn bounded(cols: u16, rows: u16) -> (u16, u16) {
    (cols.clamp(MIN.0, MAX.0), rows.clamp(MIN.1, MAX.1))
}

/// Who sized which window: (server id, @window) -> its record.
#[derive(Default)]
pub struct Sizes(Mutex<HashMap<(String, String), Win>>);

#[derive(Default, Clone)]
struct Win {
    /// the page whose grid the window has
    by: Option<u64>,
    /// window-size manual was set by terminumd
    manual: bool,
}

/// Why a page claims its pane's window.
#[derive(Clone, Copy, PartialEq)]
enum Why {
    /// its view changed size: only a window it sizes or nobody does
    View,
    /// it became the active page
    Active,
    /// it opened the pane: before its first frame, never kept waiting for the window's second
    Open,
    /// it typed
    Typed,
}

/// The shown pane's window and its last fields.
fn shown_window(page: &Page) -> Option<(Arc<Server>, String, Fields)> {
    let st = page.state.lock();
    let s = st.shown.as_ref()?;
    let f = s.ctl.fields.lock().clone()?;
    Some((s.server.clone(), s.pane.clone(), f))
}

/// Whether `pane` is in a session of the user's own tmux, attached by name (AC-89).
fn users_pane(server: &Server, pane: &str) -> bool {
    server.is_users() || server.session_of(pane).is_some_and(|s| server.users(&s))
}

/// Pages showing `window` of `server` now.
fn viewers(app: &Shared, server: &Server, window: &str) -> Vec<Arc<Page>> {
    app.pages.all().into_iter().filter(|p| {
        let st = p.state.lock();
        st.shown.as_ref().is_some_and(|s| s.server.id() == server.id() && s.ctl.fields.lock().as_ref().is_some_and(|f| f.window == window))
    }).collect()
}

/// Resizes the shown pane's window to `page`'s grid, as `page`; tells the pages showing it.
/// A window at most once a second a page: a later resize of it waits for the next second, and
/// while one waits the others leave it to that one, which reads the window and the page's grid
/// anew; a dragged window's passing sizes never reach tmux (each narrowing reflows the pane and
/// leaves a redrawing program's old lines in history). Another window at once, and a pane's
/// opening at once: switching panes waits for nothing (AC-04).
async fn resize(app: &Shared, page: &Arc<Page>, opening: bool) -> bool {
    let Some(window) = shown_window(page).map(|(s, _, f)| (s.id(), f.window)) else { return false };
    let wait = {
        let mut st = page.state.lock();
        let now = Instant::now();
        let last = st.resized_at.as_ref().filter(|(w, _)| !opening && *w == window).map(|(_, t)| *t);
        if last.is_some_and(|t| t > now) {
            return false; // one is waiting: it will do what this one would
        }
        let wait = last.map(|t| EVERY.saturating_sub(t.elapsed())).filter(|w| !w.is_zero());
        st.resized_at = Some((window, now + wait.unwrap_or_default()));
        wait
    };
    if let Some(w) = wait {
        tokio::time::sleep(w).await;
    }
    let (view, inactive) = { let st = page.state.lock(); (st.view, !st.active()) };
    if inactive {
        return false; // hidden while it waited: a hidden page sizes nothing
    }
    let (Some((server, pane, f)), Some(grid)) = (shown_window(page), view) else { return false };
    let (cols, rows) = (grid.0 as usize, grid.1 as usize);
    let key = (server.id(), f.window.clone());
    if !f.whole() {
        return false; // a window of several panes keeps its layout; the page re-flows its pane (AC-04, AC-13)
    }
    let mut cmds = Vec::new();
    let mut win = app.sizes.0.lock().get(&key).cloned().unwrap_or_default();
    if (f.width, f.height) != (cols, rows) || !win.manual {
        // also at its size already: held by tmux's own policy, another client would take it
        cmds.push(format!("set-option -w -t {} window-size manual", f.window));
        cmds.push(format!("resize-window -t {} -x {cols} -y {rows}", f.window));
        if users_pane(&server, &pane) {
            // marked: a crash or a dropped link still gives it its size back (`reclaim`)
            cmds.push(format!("set-option -w -t {} {SIZED} 1", f.window));
        }
        win.manual = true;
    }
    win.by = Some(page.id);
    {
        let mut g = app.sizes.0.lock();
        if !page.state.lock().active() {
            return false; // hidden meanwhile (pause is set before handover): its handover saw nothing to give
        }
        g.insert(key, win);
    }
    if cmds.is_empty() {
        return false;
    }
    let resized = server.key(&pane);   // the user may show another pane by the time tmux answers
    for a in server.run_all_on(&pane, cmds).await {
        if let Err(e) = a {
            tracing::warn!(event = "tmux.error", cmd = "resize-window", msg = e);
            page.send_json(&json!({"t": "sized", "sid": resized, "cols": f.width, "rows": f.height, "mine": false, "error": e}));
            return false;
        }
    }
    for p in viewers(app, &server, &f.window) {
        p.send_json(&json!({"t": "sized", "sid": shown_key(&p), "cols": cols, "rows": rows, "mine": p.id == page.id}));
        if let Some(s) = p.state.lock().shown.as_ref() {
            s.ctl.wake.notify_one();
        }
    }
    if !page.state.lock().active() {
        Box::pin(handover(app, page)).await;   // hidden while it resized: its handover came first and saw nothing to give
    }
    true
}

/// The window option marking a window of the user's that terminumd sized (AC-89).
const SIZED: &str = "@tm-sized";

fn shown_key(page: &Page) -> String {
    page.state.lock().shown.as_ref().map(|s| s.key.clone()).unwrap_or_default()
}

/// The pane's window gets `page`'s grid when `why` allows (AC-13): a hidden page claims
/// nothing; a change of the view only a window this page sizes or nobody (looking) does; the
/// user's own tmux as any (AC-89).
async fn claim(app: &Shared, page: &Arc<Page>, why: Why) -> bool {
    let Some((server, _, f)) = shown_window(page) else { return false };
    let (has_view, inactive) = {
        let st = page.state.lock();
        (st.view.is_some(), !st.active())
    };
    if !has_view || inactive {
        return false;
    }
    let key = (server.id(), f.window.clone());
    let by = app.sizes.0.lock().get(&key).and_then(|w| w.by);
    let mine = by == Some(page.id);
    // free: nobody sizes it, or the page that does no longer shows it or is hidden (a locked phone, a tab in the background)
    let free = by.is_none_or(|id| !viewers(app, &server, &f.window).iter().any(|v| v.id == id && v.state.lock().active()));
    let view = page.state.lock().view.map(|(c, r)| (c as usize, r as usize));
    if mine && view == Some((f.width, f.height)) {
        return false;                                         // it has this page's grid already: the once-a-second slot stays free
    }
    (why != Why::View || free || mine) && resize(app, page, why == Why::Open).await
}

/// A claim on its own task: a resize that waits for its second never holds up the page's
/// next messages (its keys).
fn later(app: &Shared, page: &Arc<Page>, why: Why) {
    let (app, page) = (app.clone(), page.clone());
    tokio::spawn(async move { claim(&app, &page, why).await });
}

/// `page` was hidden: a window it sized goes to the pages still looking at it (AC-13), so a
/// locked phone never keeps a computer's view at the phone's size; one of the user's own tmux
/// that no page looks at gets tmux's own size back (AC-89).
pub async fn handover(app: &Shared, page: &Arc<Page>) {
    let Some((server, pane, f)) = shown_window(page) else { return };
    let key = (server.id(), f.window.clone());
    if app.sizes.0.lock().get(&key).and_then(|w| w.by) != Some(page.id) {
        return;
    }
    let others: Vec<_> = viewers(app, &server, &f.window).into_iter().filter(|v| v.id != page.id && v.state.lock().active()).collect();
    if others.is_empty() && users_pane(&server, &pane) {
        // awaited: the page's next message (its resume) comes after it
        if app.sizes.0.lock().remove(&key).is_some_and(|w| w.manual) {
            let _ = server.run_all_on(&pane, own_size(&f.window)).await;
        }
        return;
    }
    for v in others {
        later(app, &v, Why::Active);
    }
}

/// `page` became the active page (in sight and focused), or showed the first frame of a pane it
/// opened without sizing it first (no grid with "sub"): it sizes its pane's window (AC-13, the
/// user's own tmux too, AC-89); with the grid it sizes it before that frame (`opening`).
pub fn activated(app: &Shared, page: &Arc<Page>) {
    later(app, page, Why::Active);
}

/// The page opened a pane (its fields just read, no frame sent yet): it sizes the pane's window
/// first, so the first frame comes at its grid (AC-04, AC-13); true when the window was resized.
pub async fn opening(app: &Shared, page: &Arc<Page>) -> bool {
    claim(app, page, Why::Open).await
}

/// The page's grid sent with a message (`size`, `sub`): its view within MIN and MAX.
pub fn view_of(page: &Page, msg: &Value) -> bool {
    let (c, r) = (msg["cols"].as_u64().unwrap_or(0).min(1000) as u16, msg["rows"].as_u64().unwrap_or(0).min(1000) as u16);
    if c == 0 || r == 0 {
        return false;
    }
    page.state.lock().view = Some(bounded(c, r));
    true
}

/// The page typed: its grid wins (AC-13).
pub async fn typed(app: &Shared, page: &Arc<Page>) {
    let Some((server, _, f)) = shown_window(page) else { return };
    let by = app.sizes.0.lock().get(&(server.id(), f.window.clone())).and_then(|w| w.by);
    if by != Some(page.id) {
        later(app, page, Why::Typed);
    }
}

/// The page stopped showing `old` (another pane, or it closed): a window no page in sight shows
/// goes back to tmux's policy (a hidden page showing it sizes it again when back, AC-89).
pub async fn left(app: &Shared, page: &Arc<Page>, old: &Shown) {
    let Some(f) = old.ctl.fields.lock().clone() else { return };
    let key = (old.server.id(), f.window.clone());
    let others = viewers(app, &old.server, &f.window).into_iter().any(|p| p.id != page.id && p.state.lock().active());
    let win = {
        let mut g = app.sizes.0.lock();
        let Some(w) = g.get_mut(&key) else { return };
        if w.by == Some(page.id) {
            w.by = None;
        }
        let w = w.clone();
        if !others {
            g.remove(&key);
        }
        w
    };
    if win.manual && !others {
        let _ = old.server.run_all_on(&old.pane, own_size(&f.window)).await;
    }
}

/// The commands that give `window` tmux's own size back.
fn own_size(window: &str) -> Vec<String> {
    vec![format!("set-option -w -u -t {window} window-size"), format!("set-option -w -u -t {window} {SIZED}")]
}

/// `size` and `mouse` from the page.
pub async fn handle(app: &Shared, page: &Arc<Page>, t: &str, msg: &Value) {
    match t {
        "size" => {
            if view_of(page, msg) {
                later(app, page, if msg["typing"].as_bool().unwrap_or(false) { Why::Typed } else { Why::View });
            }
        }
        _ => mouse_event(app, page, msg),
    }
}

/// terminumd lets go of windows of the user's own tmux (a session detached, the server dropped,
/// terminumd stopping, AC-89): those it gave `window-size manual` go back to tmux's own size,
/// so the user's terminal is never left at a browser's size. `session`: only its windows.
pub async fn release(app: &Shared, server: &Server, session: Option<&str>) {
    let windows: Option<Vec<String>> = match session {
        Some(s) => match server.run(format!("list-windows -t {} -F '#{{window_id}}'", crate::tmux::args::quote(&format!("={s}")))).await {
            Ok(w) => Some(w),
            Err(_) => return,
        },
        None => None,
    };
    let id = server.id();
    let mine: Vec<String> = {
        let mut g = app.sizes.0.lock();
        let keys: Vec<(String, String)> = g.keys().filter(|(s, w)| *s == id && windows.as_ref().is_none_or(|ws| ws.contains(w))).cloned().collect();
        keys.into_iter().filter_map(|k| g.remove(&k).filter(|w| w.manual).map(|_| k.1)).collect()
    };
    for w in mine {
        let _ = server.run_all(own_size(&w)).await;
    }
}

/// `reclaim` once per connection (generation) of a server holding the user's sessions.
pub fn reclaim_once(app: &Shared, server: &Arc<Server>) {
    let has = server.is_users() || server.only.lock().as_ref().is_some_and(|o| !o.is_empty());
    if !has || !server.connected() || !crate::tmux::first_on_link("reclaim", server) {
        return;
    }
    let (app, server) = (app.clone(), server.clone());
    tokio::spawn(async move { reclaim(&app, &server).await });
}

/// The user's windows terminumd sized (marked `@tm-sized`) that no page sizes now get tmux's
/// own size back: after terminumd or a host's link ended without `release` (AC-89). Once per
/// connection of a server that has the user's sessions.
pub async fn reclaim(app: &Shared, server: &Server) {
    let Ok(lines) = server.run(format!("list-windows -a -F '#{{window_id}}\t#{{session_name}}\t#{{{SIZED}}}'")).await else { return };
    let id = server.id();
    let held: Vec<String> = app.sizes.0.lock().keys().filter(|(s, _)| *s == id).map(|(_, w)| w.clone()).collect();
    let mut cmds = Vec::new();
    for l in lines {
        let f: Vec<&str> = l.split('\t').collect();
        if let [w, session, "1"] = f[..] && (server.is_users() || server.users(session)) && !held.iter().any(|h| h == w) {
            cmds.extend(own_size(w));
        }
    }
    if !cmds.is_empty() {
        tracing::info!(event = "sizing.reclaim", server = %id, windows = cmds.len() / 2);
        let _ = server.run_all(cmds).await;
    }
}

/// A click, drag or wheel for a full-screen program, as the mouse report a terminal would send
/// (AC-06): queued with the keys (typing.rs), never awaited here.
fn mouse_event(app: &Shared, page: &Arc<Page>, msg: &Value) {
    let Some((server, pane, key, f)) = (|| {
        let st = page.state.lock();
        let s = st.shown.as_ref()?;
        Some((s.server.clone(), s.pane.clone(), s.key.clone(), s.ctl.fields.lock().clone()?))
    })() else {
        return;
    };
    if !f.mouse {
        return; // the program did not ask for the mouse: nothing is sent
    }
    let Some(bytes) = from_message(msg, f.mouse_sgr) else { return };
    // a left release to a full-screen program: what it copies of its selection is this page's (AC-06)
    let arm = f.alt && msg["action"] == "release" && msg["button"] == 0;
    super::typing::mouse(app, page, server, pane, key, bytes, arm);
}

/// The page's `mouse` message as the bytes of a report.
fn from_message(msg: &Value, sgr: bool) -> Option<Vec<u8>> {
    let (button, action) = mouse::from_page(msg["button"].as_u64().unwrap_or(0).min(255) as u8, msg["action"].as_str().unwrap_or(""))?;
    let m = &msg["mods"];
    let mods = mouse::Mods { shift: m["shift"].as_bool().unwrap_or(false), meta: m["alt"].as_bool().unwrap_or(false), ctrl: m["ctrl"].as_bool().unwrap_or(false) };
    let coord = |k: &str| msg[k].as_u64().filter(|v| *v < 10_000).map(|v| v as u16);
    let one = mouse::report(coord("x")?, coord("y")?, button, action, mods, sgr)?;
    // the wheel's steps of one frame of the page come as one message with their count
    let n = if msg["action"].as_str().is_some_and(|a| a.starts_with("wheel")) { msg["count"].as_u64().unwrap_or(1).clamp(1, WHEEL_MAX) as usize } else { 1 };
    Some(one.repeat(n))
}

/// Most wheel steps one message carries.
const WHEEL_MAX: u64 = 100;

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-13: a phone narrower than an iPhone in portrait, a split, or a huge display get the bounds.
    #[test]
    fn a_grid_stays_within_its_bounds() {
        assert_eq!(bounded(80, 24), (80, 24));
        assert_eq!(bounded(20, 5), MIN);
        assert_eq!(bounded(400, 120), MAX);
        assert_eq!(bounded(30, 90), (45, 70));
    }
}
