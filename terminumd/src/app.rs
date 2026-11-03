// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The shared state of one terminumd: configuration, the Files backend (listings, ops,
//! workspaces, watcher, remote agents), the tmux servers, the pages, sign-ins and settings.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

use parking_lot::{Mutex, RwLock};

use crate::events::Bus;
use crate::files::Roots;
use crate::listing::Cache;
use crate::watcher::Watcher;
use crate::workspace::Store;

pub struct Config {
    pub port: u16,
    /// running as a remote host's agent (AC-82): served on a Unix socket
    pub agent: bool,
    /// started by the service manager (its socket came from launchd or systemd): on macOS,
    /// macOS asks for terminumd itself (AC-08)
    pub service: bool,
    pub text_max: u64,
    /// lines a page may load (TM_HISTORY)
    pub history: i64,
    /// the private state folder (TM_APP_DIR)
    pub dir: PathBuf,
    /// ~/.terminum (TM_HOME)
    pub home: PathBuf,
    /// the agent's token (agent mode only)
    pub agent_token: String,
}

pub struct App {
    pub cfg: Config,
    pub bus: Bus,
    pub cache: Arc<Cache>,
    pub store: Arc<Store>,
    pub roots: Roots,
    pub remotes: crate::remote::Remotes,
    pub watcher: Option<Arc<Watcher>>,
    pub web_panes: crate::watch_web::WebPanes,
    pub started: Instant,
    pub denied: AtomicU64,
    pub tmux: crate::tmux::Tmux,
    pub pages: crate::page::Pages,
    /// the list as last sent (`layout` groups), shared by every page
    pub layout: RwLock<serde_json::Value>,
    /// each host's files offer as last announced (`Event::Host` when it changes)
    pub offers: Mutex<std::collections::HashMap<String, serde_json::Value>>,
    /// why the local server cannot be used (tmux missing), shown instead of the list
    pub tmux_problem: Mutex<Option<String>>,
    /// the list must be read again (shared by every tmux server)
    pub dirty: Arc<tokio::sync::Notify>,
    /// one list refresh at a time
    pub refreshing: tokio::sync::Mutex<()>,
    /// the last workspace inventory (AC-71)
    pub inventory_at: Mutex<Option<Instant>>,
    /// coding agents' states by pane key (AC-39)
    pub agent_states: Mutex<crate::tmux::agentstate::States>,
    /// what coding agents' hooks said about their panes (AC-39)
    pub hooks: crate::tmux::hookstate::Hooks,
    /// the messages and commands sent, per project (AC-90)
    pub sent: crate::sent::Sent,
    /// who sized which window (AC-13)
    pub sizes: crate::page::sizing::Sizes,
    /// settings, themes, hot keys, custom.css (AC-12, AC-18, AC-19)
    pub looks: crate::looks::Looks,
    /// sign-ins and the password (AC-09)
    pub signins: crate::auth::signin::Signins,
    pub web: crate::webaccess::Web,
    /// remote hosts and their files agents (AC-81–AC-87)
    pub hosts: crate::hosts::Hosts,
    pub tunnels: crate::hosts::tunnel::Tunnels,
    /// each host's agents on/off, one at a time, the last asked winning (AC-99)
    pub agent_turns: crate::hosts::agents::Turns,
    /// each host's agents link (links.rs)
    #[allow(dead_code)] // started and stopped by the host tunnel (the next round)
    pub links: crate::links::Links,
    /// reboot restore (AC-15)
    pub restore: crate::restore::Restore,
    /// the latency trace's file (AC-38)
    pub recorder: Arc<crate::page::trace::Recorder>,
    /// bookmarks (AC-33)
    pub bookmarks: Mutex<crate::page::bookmarks::Store>,
    /// agents' records that went for room, said over the terminal at the turn's end (AC-98)
    pub marks_full: crate::page::marksfull::Full,
    /// named groups of sessions in the list (AC-92)
    pub projects: Mutex<crate::page::projects::Projects>,
    /// panes whose agent ended a turn nobody has looked at since (AC-93)
    pub unread: Mutex<crate::page::unread::Marks>,
    /// turn ends read from the screen, waiting to stay so (AC-91, AC-93)
    pub turns: crate::page::turns::Turns,
    /// the panes' session plans (AC-97)
    pub plans: crate::plans::Plans,
}

pub type Shared = Arc<App>;

impl App {
    /// Watch what the pages show: the Files panels' roots, expanded folders and open tabs,
    /// here or through a host's agent.
    pub fn refresh_watch(&self) {
        let panes = self.web_panes.keys().into_iter().map(|k| (crate::watch_web::host_of(&k), self.store.get(&k, None))).collect();
        let (local, remote) = crate::watch_web::plan(panes);
        for (h, r) in remote {
            crate::remote::watch(&self.remotes, &h, r.root, r.expanded, r.files);
        }
        if let Some(w) = &self.watcher {
            w.set(local);
        }
    }

    /// A change made through a panel: tell every panel now, without waiting for FSEvents.
    pub fn changed(&self, dirs: &[PathBuf], files: &[PathBuf], moved: &[(PathBuf, PathBuf)]) {
        crate::events::publish_change(&self.cache, &self.bus, dirs, files, moved);
    }
}
