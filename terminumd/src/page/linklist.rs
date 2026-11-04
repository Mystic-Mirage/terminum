// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The shown pane's links (AC-102): every http or https address its Claude Code session's
//! transcript names (agentlinks.rs: a full-screen agent leaves tmux no history), each once, the
//! newest first; sentence punctuation after an address is not part of it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::Page;
use crate::app::Shared;

/// Distinct addresses told, the newest.
pub const MAX_LINKS: usize = 500;
/// Distinct addresses of a transcript kept, the last named; more than a quarter over, the others are
/// left out.
pub const KEPT_LINKS: usize = 5000;
/// An address's length at most.
const LINK_MAX: usize = 2048;
/// What ends an address, and what is taken off its end (Markdown's bold too); a backslash is in no
/// address: a command's "\n" written out in a transcript follows one.
const STOP: &[char] = &['<', '>', '"', '\'', '`', '\\', '|'];
const TRAIL: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '>', '\'', '"', '`', '*'];

/// One address: the times it was named and when it was last (its entry's time).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub url: String,
    pub count: usize,
    pub when: Option<String>,
    last: usize,
}

/// The addresses seen.
#[derive(Default)]
pub struct Found {
    links: Vec<Link>,
    at: HashMap<String, usize>,
    seq: usize,
    /// The time of what is read now: given to the addresses seen in it.
    pub when: Option<String>,
}

impl Found {
    fn seen(&mut self, url: String) {
        if url.len() > LINK_MAX || url.chars().any(crate::termtext::control) {
            return;
        }
        self.seq += 1;
        match self.at.get(&url) {
            Some(&i) => {
                let l = &mut self.links[i];
                (l.count, l.last) = (l.count + 1, self.seq);
                if self.when.is_some() {
                    l.when.clone_from(&self.when);
                }
            }
            None => {
                self.at.insert(url.clone(), self.links.len());
                self.links.push(Link { url, count: 1, when: self.when.clone(), last: self.seq });
                if self.links.len() > KEPT_LINKS + KEPT_LINKS / 4 {
                    self.links.sort_unstable_by_key(|l| std::cmp::Reverse(l.last));
                    self.links.truncate(KEPT_LINKS);
                    self.at = self.links.iter().enumerate().map(|(i, l)| (l.url.clone(), i)).collect();
                }
            }
        }
    }

    /// Every address of `text`.
    pub fn text(&mut self, text: &str) {
        for url in addresses(text) {
            self.seen(url);
        }
    }

    /// The newest first (`by_count`: the most named first, then the newest), at most MAX_LINKS,
    /// and how many distinct ones there are.
    pub fn told(&self, by_count: bool) -> (Vec<Link>, usize) {
        let mut links: Vec<&Link> = self.links.iter().collect();
        links.sort_unstable_by_key(|l| std::cmp::Reverse((if by_count { l.count } else { 0 }, l.last)));
        (links.into_iter().take(MAX_LINKS).cloned().collect(), self.links.len())
    }
}

/// Each http or https address of `text`.
fn addresses(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let scheme = if starts(b, i, b"https://") { 8 } else if starts(b, i, b"http://") { 7 } else { 0 };
        // i is a character's start when a scheme starts there: its "h" is ASCII
        if scheme == 0 || text[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            i += 1;
            continue;
        }
        let rest = &text[i + scheme..];
        let ipv6 = rest.starts_with('[');      // "http://[::1]:8080/": else "]" ends it, as Markdown's "[url](url)" has
        let end = rest.find(|c: char| c.is_whitespace() || STOP.contains(&c) || (!ipv6 && c == ']')).unwrap_or(rest.len());
        let url = text[i..i + scheme + end].trim_end_matches(TRAIL);
        if url.len() > scheme && plausible(&url[scheme..]) {
            out.push(url.to_string());
        }
        i += scheme + end;
    }
    out
}

/// Whether what follows "https://" can be an address, not a template, a shell variable or text
/// cut short: its host is letters, digits, "-", "." and a port (or "[" an IPv6 address "]"); it
/// holds no "{", "}", "…", "$NAME" or a value "=$x" (OData's "?$filter=" is kept), and no "[" past
/// the host (a route's "/[slug]/").
fn plausible(rest: &str) -> bool {
    let (host, path) = match rest.strip_prefix('[') {
        Some(v6) => match v6.split_once(']') {
            Some((h, p)) if h.contains(':') => ("v6", p),
            _ => return false,
        },
        None => rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len())),
    };
    let named = host == "v6" || (host.chars().next().is_some_and(char::is_alphanumeric)
        && host.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '.' | ':' | '@' | '_' | '%')));
    let b = rest.as_bytes();
    let var = b.windows(2).any(|w| w[0] == b'$' && (w[1].is_ascii_uppercase() || matches!(w[1], b'_' | b'{' | b'(')))
        || b.windows(3).any(|w| w[0] == b'=' && w[1] == b'$' && w[2].is_ascii_alphanumeric());
    named && !var && !rest.contains(['{', '}', '…']) && !path.contains(['[', ']'])
}

fn starts(text: &[u8], i: usize, s: &[u8]) -> bool {
    text.get(i..i + s.len()).is_some_and(|w| w.eq_ignore_ascii_case(s))
}

pub fn answer(r: &str, key: &str, links: &[Link], total: usize) -> Value {
    let items: Vec<Value> = links.iter().map(|l| json!({"u": l.url, "c": l.count, "w": l.when})).collect();
    json!({"t": "linklist", "ref": r, "sid": key, "items": items, "total": total})
}

pub fn refused(r: &str, why: &str) -> Value {
    json!({"t": "linklist", "ref": r, "error": why})
}

/// `links {ref, by}` → `linklist`; reading takes a while, so it runs beside typing.
pub fn start(app: &Shared, page: &Arc<Page>, msg: &Value) {
    let r = super::search::ref_of(msg);
    let by_count = msg["by"].as_str() == Some("count");
    let Some((key, server, pane, _)) = super::find::shown(page) else {
        page.send_json(&refused(&r, "No session is shown."));
        return;
    };
    // a Claude Code session of this machine: its transcript is here (a host's is on the host)
    let local = key.starts_with("tmux:local:");
    let hooked = app.hooks.session(&key).filter(|(a, _)| *a == "claude" && local).map(|(_, s)| s);
    let page = page.clone();
    tokio::spawn(async move {
        let t0 = std::time::Instant::now();
        let dir = super::agentlinks::claude_dir();
        let session = match (hooked, local) {
            (Some(s), _) => Ok(Some(s)),
            (None, true) => process_session(&key, &server, &pane, &dir).await,
            (None, false) => Ok(None),
        };
        let read = match session {
            Err(e) => Err(e),
            Ok(None) => Err("No Claude Code session runs in this pane.".to_string()),
            Ok(Some(session)) => tokio::task::spawn_blocking(move || {
                let path = super::agentlinks::transcript(&dir, &session)
                    .ok_or_else(|| "Claude Code's transcript could not be read: not found.".to_string())?;
                super::agentlinks::read(&path, by_count).map_err(|e| format!("Claude Code's transcript could not be read: {e}."))
            }).await.unwrap_or_else(|e| Err(e.to_string())),
        };
        match read {
            Ok((found, total)) => {
                tracing::info!(event = "links", sid = %key, links = total, ms = t0.elapsed().as_millis() as u64);
                page.send_json(&answer(&r, &key, &found, total));
            }
            Err(e) => {
                page.send_json(&refused(&r, &e));
            }
        }
    });
}

/// A pane's Claude Code session: the pane's process, the process with the record, the session.
type Ran = (u32, u32, String);
/// The session each pane's Claude Code process ran, by key.
static RAN: Mutex<Option<HashMap<String, Ran>>> = Mutex::new(None);
/// Panes remembered, at most.
const RAN_MOST: usize = 256;

/// The session the pane's Claude Code process runs, or why the pane's process is not known: the
/// hooks name a session only after its next event (none since terminumd started). The process
/// found last for the same pane process (a new pane with an old one's id has another) is asked
/// first; only when it no longer runs that session are the pane's processes looked through again.
async fn process_session(key: &str, server: &Arc<crate::tmux::server::Server>, pane: &str, dir: &std::path::Path) -> Result<Option<String>, String> {
    let lines = server.run(format!("display-message -p -t {pane} '#{{pane_pid}}'")).await.map_err(|e| format!("tmux did not say the pane's process: {e}"))?;
    let pid = lines.first().and_then(|l| l.trim().parse::<u32>().ok()).ok_or("tmux did not say the pane's process.")?;
    let last = RAN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().get(key).cloned();
    let d = dir.to_path_buf();
    let found = tokio::task::spawn_blocking(move || match still(&d, last, pid) {
        Some(s) => Some((None, s)),
        None => super::agentlinks::session_of(&d, pid).map(|(rec, s)| (Some(rec), s)),
    }).await.ok().flatten();
    let mut g = RAN.lock().unwrap_or_else(|e| e.into_inner());
    let ran = g.get_or_insert_default();
    match &found {
        Some((Some(rec), session)) => {
            if ran.len() >= RAN_MOST {
                ran.clear();
            }
            ran.insert(key.to_string(), (pid, *rec, session.clone()));
        }
        Some((None, _)) => {}
        None => {
            ran.remove(key);
        }
    }
    Ok(found.map(|(_, s)| s))
}

/// The session found last (`last`) when the pane's process is still `pid` and the process with
/// the record still runs that session.
fn still(dir: &std::path::Path, last: Option<Ran>, pid: u32) -> Option<String> {
    let (pane, rec, session) = last?;
    (pane == pid && super::agentlinks::record(dir, rec).as_deref() == Some(session.as_str())).then_some(session)
}

#[cfg(test)]
#[path = "linklist_tests.rs"]
mod tests;
