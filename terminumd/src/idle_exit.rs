// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The idle agents' exit (`terminum-agents.md` AC-153): Claude Code or Codex in a pane no page in
//! sight has shown for `agentExitDays` is asked to exit: Ctrl+C twice, then `/exit` and Enter while
//! it still runs. Its shell, window and session stay. When a page shows a pane is kept in tmux
//! (`@terminum-seen`, unix seconds by terminumd's clock), so it outlives terminumd. Every key goes
//! only while the agent is idle with its box shown empty, read again before each one: no draft, no
//! question, no answer to a permission is ever typed over; what is sent is in the input log (AC-147).

use std::sync::Arc;
use std::time::Duration;

use crate::app::Shared;
use crate::auth::store::unix_now as now;
use crate::dispatcher::screen::Screen;
use crate::page::marks::{find_item, layout_rows};
use crate::tmux::titles::is_shell;
use crate::tmux::Server;

/// How often panes are looked at.
const EVERY: Duration = Duration::from_secs(600);
/// Nothing is ended this soon after terminumd starts or the Mac wakes: the user's page comes back
/// and shows what it showed first.
const GRACE: Duration = Duration::from_secs(3600);
/// How long an agent is given to end after the second Ctrl+C, and after `/exit`.
#[cfg(not(test))]
const ENDS_WITHIN: Duration = Duration::from_secs(10);
#[cfg(test)]
const ENDS_WITHIN: Duration = Duration::from_secs(3);
const DAY: u64 = 86_400;
const FIELDS: &str = "#{@terminum-seen}\t#{@terminum-run}\t#{pane_in_mode}\t#{pane_current_command}\t#{pane_start_command}";
/// Between the two Ctrl+C: inside the agents' window for a second press (Claude Code's is under a second).
const PRESS_GAP: Duration = Duration::from_millis(300);
/// How long the look waits for a tmux answer: a host that does not answer delays only its own panes.
const WAIT: Duration = Duration::from_secs(5);

/// What tmux says of a pane (FIELDS).
#[derive(Debug, Default, PartialEq)]
struct Pane {
    seen: Option<u64>,
    run: String,
    in_mode: bool,
    command: String,
    start: String,
}

impl Pane {
    fn parse(line: &str) -> Pane {
        let f: Vec<&str> = line.split('\t').collect();
        let at = |i: usize| f.get(i).copied().unwrap_or("").to_string();
        Pane { seen: at(0).trim().parse().ok(), run: at(1), in_mode: at(2) != "0", command: at(3), start: at(4) }
    }
}

/// What a look decides for a pane.
#[derive(Debug, PartialEq)]
enum Due {
    /// never stamped: its days start now
    Stamp,
    /// opened within the days
    Wait,
    Skip(&'static str),
    Go,
}

/// Whether the pane's own program is a bare shell (none named: tmux's default, a shell; flags such
/// as `-l` only, no command): an agent that is its own program, or a shell's `-c` command, takes its pane, and
/// maybe its session, with it when it ends.
fn shell_root(start: &str) -> bool {
    let mut words = start.trim().trim_matches('"').split_whitespace();
    words.next().is_none_or(|first| is_shell(first) && words.all(|w| w.starts_with('-')))   // a `-c`'s command is a word
}

fn due(p: &Pane, now: u64, days: u32) -> Due {
    let Some(seen) = p.seen else { return Due::Stamp };
    if days == 0 || now.saturating_sub(seen) < u64::from(days) * DAY {
        return Due::Wait;
    }
    if !p.run.is_empty() {
        return Due::Skip("Dispatcher's pane: its run ends it");
    }
    if !shell_root(&p.start) {
        return Due::Skip("the agent is the pane's own program: its exit would close the pane");
    }
    if p.in_mode {
        return Due::Skip("the pane is in a tmux mode");
    }
    if is_shell(&p.command) {
        return Due::Skip("only a shell runs there");
    }
    Due::Go
}

/// tmux's answer to `line`, within WAIT.
async fn ask(server: &Server, line: String) -> Result<Vec<String>, String> {
    tokio::time::timeout(WAIT, server.run(line)).await.unwrap_or_else(|_| Err("tmux does not answer".into()))
}

async fn read(server: &Server, pane: &str) -> Option<Pane> {
    let said = ask(server, format!("display -p -t {pane} '{FIELDS}'")).await.ok()?;
    said.first().map(|l| Pane::parse(l))
}

async fn stamp(server: &Server, pane: &str, at: u64) {
    if let Err(e) = ask(server, format!("set-option -p -t {pane} @terminum-seen {at}")).await {
        tracing::debug!(event = "idleexit.unstamped", pane, error = %e);
    }
}

/// A page in sight shows pane `key`: its days start again.
pub fn opened(app: &Shared, key: &str) {
    let Some((server, pane)) = app.tmux.resolve(key) else { return };
    tokio::spawn(async move { stamp(&server, &pane, now()).await });
}

/// A list row's coding agent, idle (not working, not waiting mid-turn, no subagents running).
fn idle_agent(it: &serde_json::Value) -> Option<&str> {
    it["agent"].as_str().filter(|a| matches!(*a, "claude" | "codex") && !crate::page::turns::busy(it))
}

/// The list's idle coding agents: (key, claude or codex).
fn idle_agents(app: &Shared) -> Vec<(String, String)> {
    layout_rows(&app.layout.read()).filter_map(|it| Some((it["id"].as_str()?.to_string(), idle_agent(it)?.to_string()))).collect()
}

fn busy(app: &Shared, key: &str) -> bool {
    app.hooks.working(key) || find_item(&app.layout.read(), key).and_then(idle_agent).is_none()
}

/// Why the next key may not go, read just before it: None when the agent is still `first`'s
/// program, idle, unopened since then, its box shown empty.
async fn holds(app: &Shared, server: &Server, pane: &str, key: &str, kind: &str, first: &Pane) -> Option<String> {
    let Some(p) = read(server, pane).await else { return Some("tmux does not answer".into()) };
    if p.command != first.command {
        return Some(format!("{} no longer runs in front", first.command));
    }
    if p.seen != first.seen {
        return Some("a page showed it meanwhile".into());
    }
    if p.in_mode || !p.run.is_empty() || busy(app, key) {
        return Some("it is at work, or the pane is in a tmux mode".into());
    }
    match crate::dispatcher::read_screen(server, pane, kind).await {
        Err(e) => Some(e),
        Ok(Screen::Empty) => None,
        Ok(Screen::Holds(_)) => Some("its box holds a draft".into()),
        Ok(Screen::Asks) => Some("it asks a question".into()),
        Ok(Screen::Unknown) => Some(format!("its screen is not known ({kind})")),
    }
}

/// Whether `program` gave the pane back to its shell within ENDS_WITHIN.
async fn ended(server: &Server, pane: &str) -> bool {
    let until = tokio::time::Instant::now() + ENDS_WITHIN;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        match read(server, pane).await {
            Some(p) if is_shell(&p.command) => return true,
            None => return true,                              // the pane is gone
            _ if tokio::time::Instant::now() >= until => return false,
            _ => {}
        }
    }
}

/// `keys` into `pane`, in order.
async fn send(server: &Server, pane: &str, keys: &[&str]) -> Result<(), String> {
    for k in keys {
        ask(server, format!("send-keys -t {pane} {k}")).await.map_err(|e| format!("tmux did not take it: {e}"))?;
    }
    Ok(())
}

/// Ctrl+C, Ctrl+C, then `/exit` and Enter while the agent stays; each key only while `holds` says
/// none. Whether it ended, or why not, and how many steps of keys went.
async fn end(app: &Shared, server: &Arc<Server>, pane: &str, key: &str, kind: &str, first: &Pane) -> (usize, Result<(), String>) {
    let _held = server.typing(pane).lock_owned().await;
    // (as logged, tmux's keys, whether the agent may have ended after them)
    let steps: [(&str, &[&str], bool); 3] = [("<C-c>", &["C-c"], false), ("<C-c>", &["C-c"], true), ("/exit<Enter>", &["-l /exit", "Enter"], true)];
    for (i, (words, keys, may_end)) in steps.into_iter().enumerate() {
        if let Some(why) = holds(app, server, pane, key, kind, first).await {
            return (i, Err(if i == 0 { why } else { format!("after {i} key(s): {why}") }));
        }
        let sent = send(server, pane, keys).await;
        let who = crate::inputs::Who { sender: crate::tasks::store::DISPATCHER, component: "idleexit", ..Default::default() };
        crate::inputs::wrote(app, who, "keys", server, pane, words, sent.clone().err(), None);
        if let Err(e) = sent {
            return (i, Err(e));
        }
        if !may_end {
            tokio::time::sleep(PRESS_GAP).await;
        } else if ended(server, pane).await {
            return (i + 1, Ok(()));
        }
    }
    (steps.len(), Err(format!("{} still runs after Ctrl+C twice and /exit", first.command)))
}

/// One look at every listed agent: those due are asked to exit, one after another.
async fn look(app: &Shared, days: u32, may_end: bool) {
    let at = now();
    for (key, kind) in idle_agents(app) {
        let Some((server, pane)) = app.tmux.resolve(&key) else { continue };
        let Some(p) = read(&server, &pane).await else { continue };
        match due(&p, at, days) {
            Due::Stamp => stamp(&server, &pane, at).await,
            Due::Wait => {}
            Due::Skip(why) => tracing::debug!(event = "idleexit.skipped", key = %key, why),
            Due::Go if may_end => {
                let (keys, got) = end(app, &server, &pane, &key, &kind, &p).await;
                match got {
                    Ok(()) => tracing::info!(event = "idleexit.ended", key = %key, kind = %kind, days, keys),
                    Err(why) => tracing::warn!(event = "idleexit.stayed", key = %key, kind = %kind, why = %why, keys),
                }
                if keys > 0 {
                    stamp(&server, &pane, now()).await;   // keys went: the next try is days away; none: the next look
                }
            }
            Due::Go => {}
        }
    }
}

/// The panes pages in sight show (a page on another screen too; not a hidden tab), and those a tmux
/// client of the user's own shows (`tmux attach` in a terminal; terminumd's are control clients):
/// their days start again.
async fn stamp_shown(app: &Shared) {
    let shown: Vec<String> = app.pages.all().iter().filter_map(|p| {
        let st = p.state.lock();
        st.shown.as_ref().filter(|_| st.look != crate::page::client::Look::Hidden).map(|s| s.key.clone())
    }).collect();
    for key in shown {
        if let Some((server, pane)) = app.tmux.resolve(&key) {
            stamp(&server, &pane, now()).await;
        }
    }
    for server in app.tmux.all() {
        for pane in attached_panes(&server).await {
            stamp(&server, &pane, now()).await;
        }
    }
}

/// The panes the tmux clients that are not control clients show.
async fn attached_panes(server: &Server) -> Vec<String> {
    let said = ask(server, "list-clients -F '#{client_control_mode}\t#{pane_id}'".into()).await.unwrap_or_default();
    said.iter().filter_map(|l| l.strip_prefix("0\t")).filter(|p| p.starts_with('%')).map(str::to_string).collect()
}

/// Looks every EVERY; ends nothing within GRACE of the start or of a gap in its beat (a Mac asleep).
pub async fn run(app: Shared) {
    let mut free_at = tokio::time::Instant::now() + GRACE;
    let mut last = now();
    loop {
        tokio::time::sleep(EVERY).await;
        let at = now();
        if at.saturating_sub(last) > 2 * EVERY.as_secs() {
            free_at = tokio::time::Instant::now() + GRACE;
        }
        last = at;
        stamp_shown(&app).await;
        let days = app.looks.settings.read().agent_exit_days;
        look(&app, days, tokio::time::Instant::now() >= free_at).await;
    }
}

#[cfg(test)]
#[path = "idle_exit_tests.rs"]
mod tests;
