// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The idle agents' exit (`terminum-agents.md` AC-153): Claude Code or Codex in a pane no page in
//! sight has shown for `agentExitDays` is asked to exit: Ctrl+C twice, then `/exit` and Enter while
//! it still runs. Its shell, window and session stay. When a page shows a pane is kept in tmux
//! (`@terminum-seen`, unix seconds by terminumd's clock), so it outlives terminumd. Every key goes
//! only while the agent is idle with its box shown empty, read again before each one: no draft, no
//! question, no answer to a permission is ever typed over; what is sent is in the input log (AC-147).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::app::Shared;
use crate::dispatcher::screen::{self, Screen};
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

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What tmux says of a pane (FIELDS).
#[derive(Clone, Debug, Default, PartialEq)]
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

/// Whether the pane's own program is a shell (none named: tmux's default, a shell): an agent that is
/// its own program would take its pane, and maybe its session, with it.
fn shell_root(start: &str) -> bool {
    let first = start.trim().trim_matches('"').split_whitespace().next().unwrap_or("");
    first.is_empty() || crate::dispatcher::is_shell(first.rsplit('/').next().unwrap_or(first))
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
    if crate::dispatcher::is_shell(&p.command) {
        return Due::Skip("only a shell runs there");
    }
    Due::Go
}

async fn read(server: &Server, pane: &str) -> Option<Pane> {
    let said = server.run(format!("display -p -t {pane} '{FIELDS}'")).await.ok()?;
    said.first().map(|l| Pane::parse(l))
}

async fn stamp(server: &Server, pane: &str, at: u64) {
    if let Err(e) = server.run(format!("set-option -p -t {pane} @terminum-seen {at}")).await {
        tracing::debug!(event = "idleexit.unstamped", pane, error = %e);
    }
}

/// A page in sight shows pane `key`: its days start again.
pub fn opened(app: &Shared, key: &str) {
    let Some((server, pane)) = app.tmux.resolve(key) else { return };
    tokio::spawn(async move { stamp(&server, &pane, now()).await });
}

/// The list's coding agents not at work: (key, claude or codex).
fn idle_agents(app: &Shared) -> Vec<(String, String)> {
    let layout = app.layout.read();
    let items = layout.as_array().into_iter().flatten().flat_map(|g| g["items"].as_array().into_iter().flatten());
    items.filter(|it| !crate::page::turns::busy(it))
        .filter_map(|it| Some((it["id"].as_str()?.to_string(), it["agent"].as_str().filter(|a| matches!(*a, "claude" | "codex"))?.to_string())))
        .collect()
}

fn busy(app: &Shared, key: &str) -> bool {
    app.hooks.working(key) || !idle_agents(app).iter().any(|(k, _)| k == key)
}

/// Why the next key may not go, read just before it: None when the agent is still `program`,
/// idle, unopened since `seen`, its box shown empty.
async fn holds(app: &Shared, server: &Server, pane: &str, key: &str, kind: &str, program: &str, seen: Option<u64>) -> Option<String> {
    let Some(p) = read(server, pane).await else { return Some("tmux does not answer".into()) };
    if p.command != program || crate::dispatcher::is_shell(&p.command) {
        return Some(format!("{program} no longer runs in front"));
    }
    if p.seen != seen {
        return Some("a page showed it meanwhile".into());
    }
    if p.in_mode || !p.run.is_empty() || busy(app, key) {
        return Some("it is at work, or the pane is in a tmux mode".into());
    }
    let rows = match server.run(format!("capture-pane -e -p -t {pane}")).await {
        Ok(rows) => rows,
        Err(e) => return Some(format!("tmux did not give its screen: {e}")),
    };
    match screen::read(kind, &rows) {
        Screen::Empty => None,
        Screen::Holds(_) => Some("its box holds a draft".into()),
        Screen::Asks => Some("it asks a question".into()),
        Screen::Unknown => Some(format!("its screen is not known ({kind})")),
    }
}

/// Whether `program` gave the pane back to its shell within ENDS_WITHIN.
async fn ended(server: &Server, pane: &str) -> bool {
    let until = tokio::time::Instant::now() + ENDS_WITHIN;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        match read(server, pane).await {
            Some(p) if crate::dispatcher::is_shell(&p.command) => return true,
            None => return true,                              // the pane is gone
            _ if tokio::time::Instant::now() >= until => return false,
            _ => {}
        }
    }
}

/// Ctrl+C, Ctrl+C, then `/exit` and Enter while the agent stays; each key only while `holds` says
/// none. Whether it ended, or why not.
async fn end(app: &Shared, server: &Arc<Server>, pane: &str, key: &str, kind: &str, first: &Pane) -> Result<(), String> {
    let _held = server.typing(pane).lock_owned().await;
    let program = first.command.clone();
    let steps: [(&str, &[&str]); 3] = [("<C-c>", &["C-c"]), ("<C-c>", &["C-c"]), ("/exit<Enter>", &["-l /exit", "Enter"])];
    for (i, (words, keys)) in steps.iter().enumerate() {
        if let Some(why) = holds(app, server, pane, key, kind, &program, first.seen).await {
            return Err(if i == 0 { why } else { format!("after {i} key(s): {why}") });
        }
        let mut error = None;
        for k in keys.iter() {
            if let Err(e) = server.run(format!("send-keys -t {pane} {k}")).await {
                error = Some(format!("tmux did not take it: {e}"));
                break;
            }
        }
        let who = crate::inputs::Who { sender: crate::tasks::store::DISPATCHER, component: "idleexit", ..Default::default() };
        crate::inputs::wrote(app, who, "keys", server, pane, words, error.clone(), None);
        if let Some(e) = error {
            return Err(e);
        }
        match i {
            0 => tokio::time::sleep(Duration::from_secs(1)).await,
            _ if ended(server, pane).await => return Ok(()),
            _ => {}
        }
    }
    Err(format!("{program} still runs after Ctrl+C twice and /exit"))
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
                match end(app, &server, &pane, &key, &kind, &p).await {
                    Ok(()) => tracing::info!(event = "idleexit.ended", key = %key, kind = %kind, days),
                    Err(why) => tracing::warn!(event = "idleexit.stayed", key = %key, kind = %kind, why = %why),
                }
                stamp(&server, &pane, now()).await;       // ended or not: the next try is days away
            }
            Due::Go => {}
        }
    }
}

/// The panes pages in sight show (a page on another screen too; not a hidden tab): their days start again.
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
        look(&app, days, days > 0 && tokio::time::Instant::now() >= free_at).await;
    }
}

#[cfg(test)]
#[path = "idle_exit_tests.rs"]
mod tests;
