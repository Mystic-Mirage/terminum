// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What the user configures in `~/.terminum` and the pages wear (AC-12, AC-18, AC-19):
//! settings.json, the themes folder, hotkeys.json and custom.css, read again within a
//! second of a change and told to every page; each problem is told once as a toast.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use parking_lot::{Mutex, RwLock};
use serde_json::{json, Value};

use crate::app::Shared;
use crate::page::Page;
use crate::settings::Settings;
use crate::theme::Themes;
use crate::tmux::Server;

pub struct Looks {
    home: PathBuf,
    pub settings: RwLock<Settings>,
    themes: Mutex<Themes>,
    hotkeys: RwLock<Value>,
    dark: AtomicBool,
    /// custom.css's modification time (the page reloads its link when it changes)
    css: Mutex<Option<SystemTime>>,
    stamps: Mutex<HashMap<PathBuf, Option<SystemTime>>>,
    /// messages already told (each once)
    told: Mutex<HashSet<String>>,
    pending: Mutex<Vec<String>>,
}

fn stamp(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// The newest modification time in a folder (a theme file added, changed or removed).
fn folder_stamp(dir: &Path) -> Option<SystemTime> {
    let mut newest = stamp(dir);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        newest = newest.max(stamp(&e.path()));
    }
    newest
}

impl Looks {
    pub fn load(home: &Path) -> Looks {
        let _ = crate::theme::ensure_defaults(&home.join("themes"));
        let (settings, mut msgs) = crate::settings::load(&home.join("settings.json"));
        let (themes, more) = Themes::load(&home.join("themes"));
        msgs.extend(more);
        let looks = Looks {
            home: home.to_path_buf(),
            settings: RwLock::new(settings),
            themes: Mutex::new(themes),
            hotkeys: RwLock::new(crate::hotkeys::load(&home.join("hotkeys.json"))),
            dark: AtomicBool::new(crate::theme::system_dark()),
            css: Mutex::new(stamp(&home.join("custom.css"))),
            stamps: Mutex::default(),
            told: Mutex::default(),
            pending: Mutex::default(),
        };
        looks.note(msgs);
        looks.changed();
        looks
    }

    fn note(&self, msgs: Vec<String>) {
        let mut told = self.told.lock();
        for m in msgs {
            if told.insert(m.clone()) {
                tracing::warn!(event = "config", msg = %m);
                self.pending.lock().push(m);
            }
        }
    }

    /// Which watched files changed since the last call.
    fn changed(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut stamps = self.stamps.lock();
        for (what, path, folder) in [("settings", "settings.json", false), ("themes", "themes", true), ("hotkeys", "hotkeys.json", false), ("css", "custom.css", false)] {
            let p = self.home.join(path);
            let now = if folder { folder_stamp(&p) } else { stamp(&p) };
            if stamps.insert(p, now) != Some(now) {
                out.push(what);
            }
        }
        out
    }

    pub fn dark(&self) -> bool {
        self.dark.load(Ordering::Relaxed)
    }

    /// The theme a pane of `session` on `host` wears: (name, theme for the page).
    pub fn theme(&self, host: Option<&str>, session: &str) -> (String, Value) {
        let r = self.themes.lock().resolve(&self.settings.read(), host, session, self.dark());
        if let Some(m) = r.message {
            self.note(vec![m]);
        }
        (r.name, r.theme)
    }

    pub fn theme_name(&self, host: Option<&str>, session: &str) -> String {
        self.themes.lock().name_of(&self.settings.read(), host, session)
    }

    /// The `settings` message: page defaults, hot keys, custom.css's stamp, new messages.
    pub fn message(&self, take_messages: bool) -> Value {
        let css = self.css.lock().map(|t| t.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64));
        let messages: Vec<String> = if take_messages { std::mem::take(&mut *self.pending.lock()) } else { Vec::new() };
        json!({"t": "settings", "page": self.settings.read().page_json(), "hotkeys": self.hotkeys.read().clone(), "css": css, "messages": messages})
    }
}

/// The session of `pane` on `server`, as tmux says now.
async fn session_of(server: &Server, pane: &str) -> String {
    server.run(format!("display -p -t {pane} '#{{session_name}}'")).await.ok().and_then(|l| l.into_iter().next()).unwrap_or_default()
}

/// What a shown pane wears (the stream sends it first).
pub async fn theme_of(app: &Shared, server: &Server, pane: &str) -> (String, Value) {
    let session = session_of(server, pane).await;
    let host = (!server.is_local()).then_some(server.name.as_str());
    app.looks.theme(host, &session)
}

/// What a page learns about settings when it connects.
pub fn hello(app: &Shared, page: &Page) {
    page.send_json(&app.looks.message(false));
}

/// Follows the files and the system's appearance: a change reaches every page within a
/// second, without reload (AC-12, AC-19).
pub async fn follow(app: Shared) {
    let mut ticks = 0u64;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        ticks += 1;
        // the folder's stats and, every 10 s, `defaults` for the appearance: off the async workers
        let (a, appearance) = (app.clone(), ticks.is_multiple_of(10));
        let Ok((changed, dark)) = tokio::task::spawn_blocking(move || (a.looks.changed(), appearance.then(crate::theme::system_dark))).await else { continue };
        let mut retheme = false;
        if changed.contains(&"settings") {
            let (s, msgs) = crate::settings::load(&app.looks.home.join("settings.json"));
            let history = (s.tmux_history != app.looks.settings.read().tmux_history).then_some(s.tmux_history);
            *app.looks.settings.write() = s;
            if let Some(n) = history {
                // panes made from now on keep this many lines (tmux applies it to new panes only)
                let _ = app.tmux.local.run(format!("set-option -g history-limit {n}")).await;
            }
            app.looks.note(msgs);
            crate::hosts::sync(&app);
            crate::attach::sync(&app);
            retheme = true;
        }
        if changed.contains(&"themes") {
            let msgs = app.looks.themes.lock().reload();
            app.looks.note(msgs);
            retheme = true;
        }
        if changed.contains(&"hotkeys") {
            *app.looks.hotkeys.write() = crate::hotkeys::load(&app.looks.home.join("hotkeys.json"));
        }
        if changed.contains(&"css") {
            *app.looks.css.lock() = stamp(&app.looks.home.join("custom.css"));
        }
        if let Some(dark) = dark {
            retheme |= app.looks.dark.swap(dark, Ordering::Relaxed) != dark;
        }
        let news = !app.looks.pending.lock().is_empty();
        if changed.iter().any(|c| *c != "themes") || news {
            app.pages.tell(&app.looks.message(true).to_string());
        }
        if retheme {
            for page in app.pages.all() {
                let shown = page.state.lock().shown.as_ref().map(|s| (s.key.clone(), s.server.clone(), s.pane.clone()));
                if let Some((key, server, pane)) = shown {
                    let (name, theme) = theme_of(&app, &server, &pane).await;
                    page.send_json(&json!({"t": "theme", "sid": key, "theme": theme, "name": name}));
                }
            }
            app.dirty.notify_one(); // the list's dots follow the theme names
            // the board shows no pane: it wears the local default, as /api/tasks/meta gives it
            app.pages.tell(&json!({"t": "boardtheme", "theme": app.looks.theme(None, "").1}).to_string());
        }
    }
}

/// `history {lines}` from the page's menu: kept in settings.json as tmux.history, which `follow`
/// applies to the panes made from then on (AC-19).
pub fn set_history(app: &Shared, page: &Page, msg: &Value) {
    let Some(n) = msg["lines"].as_u64().filter(|n| (1000..=1_000_000).contains(n)) else {
        return crate::page::client::error(page, "History lines must be 1000–1000000", None);
    };
    let path = app.looks.home.join("settings.json");
    if let Err(e) = crate::settings::save_object(&path, "tmux", |t| { t.insert("history".into(), json!(n)); }) {
        crate::page::client::error(page, e, None);
    }
}

/// `agentexit {days}` from the page's menu: kept in settings.json as agentExitDays, which the
/// idle agents' exit reads at its next look (AC-153); under 1: off.
pub fn set_agent_exit(app: &Shared, page: &Page, msg: &Value) {
    let max = crate::settings::MAX_EXIT_DAYS;
    let Some(n) = msg["days"].as_i64().filter(|n| *n <= max) else {
        return crate::page::client::error(page, format!("Days must be a whole number up to {max}, 0 for off"), None);
    };
    let path = app.looks.home.join("settings.json");
    if let Err(e) = crate::settings::save_value(&path, "agentExitDays", json!(n.max(0))) {
        crate::page::client::error(page, e, None);
    }
}

/// `scheme {name}` from the page's Colors: one color scheme for every browser, kept in
/// settings.json as page.colorScheme (none: the themes' own colors), told to every page (AC-12).
pub fn set_scheme(app: &Shared, page: &Page, msg: &Value) {
    let name = msg["name"].as_str().map(str::to_string);
    if name.as_deref().is_some_and(|n| !crate::settings::is_short_name(n)) {
        return crate::page::client::error(page, "Not a color scheme", None);
    }
    let path = app.looks.home.join("settings.json");
    let saved = crate::settings::save_object(&path, "page", |p| match name {
        Some(n) => { p.insert("colorScheme".into(), json!(n)); }
        None => { p.remove("colorScheme"); }
    });
    if let Err(e) = saved {
        crate::page::client::error(page, e, None);
    }
}
