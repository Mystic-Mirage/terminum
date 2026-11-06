// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Last lines (AC-96): each shown pane's last `lastLines` lines (300) are kept on disk within
//! 5 s of its output (a host's within 15 s), so after its tmux restarted the pane made again in
//! its place (AC-15) shows how it ended. Files: `state/lastlines/<server>/<identity>_<N>.json`,
//! named by the tmux server's `@terminum-id`, so a new server's `%0` never writes over the old
//! `%0`; a restore renames the old pane's file to `<new identity>_<new N>.before.json`, by the
//! identity the checkpoint saved and restore's own pairs. A pane's files go once it is absent
//! from two reads of its server 30 s apart (a reboot ends programs before tmux: their panes
//! close while the server answers, then it dies with their lines kept); the old server's that a
//! restore did not take go at once; those of a server no restore took, 7 days after it was last
//! read (an hourly sweep, the first an hour after start: a restore comes first). Off (`lastLines` 0, or restore off: nothing
//! would take them) deletes them all. Plain text from `capture-pane`, never sent but to a
//! signed-in page asking for one pane's; excluded from Time Machine. A full-screen program's own
//! screen is not kept: the terminal's lines under it are.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::app::Shared;
use crate::tmux::Server;

const EVERY: Duration = Duration::from_secs(5);
/// a host's panes are read every third round: captures share the ssh link with typing
const HOST_ROUNDS: u64 = 3;
/// a pane absent this long, in reads of its live server, was closed
const GONE_FOR: Duration = Duration::from_secs(30);
const KEEP_OLD: Duration = Duration::from_secs(7 * 86400);
const SWEEP_EVERY: Duration = Duration::from_secs(3600);
const DIR: &str = "lastlines";
/// the identity, from the same answer as the panes: never another server's (a restart between reads)
const FIELDS: &str = "#{@terminum-id}\t#{pane_id}\t#{window_activity}\t#{alternate_on}\t#{history_size}\t#{@tm-quick}\t#{session_name}\t#{window_name}\t#{pane_current_path}";

/// One write, rename or delete at a time: a round in flight never writes after "off".
static WRITING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
/// Identities a restore took the lines of: a round read before the restart writes none of theirs.
static RETIRED: parking_lot::Mutex<Vec<String>> = parking_lot::Mutex::new(Vec::new());
/// A pane printed nothing since a restore but its prompt: its lines from before stay its own.
pub const ONLY_A_PROMPT: usize = 2;

/// The lines kept, or 0: off, also while restore is (nothing would take them after a restart).
pub fn wanted(app: &Shared) -> usize {
    let st = app.looks.settings.read();
    if st.restore { st.last_lines as usize } else { 0 }
}

fn root(app: &Shared) -> PathBuf {
    app.cfg.dir.join(DIR)
}

/// A server's folder: its id (`<host>:<socket>`) with what a file name cannot hold replaced.
pub fn folder(server_id: &str) -> String {
    server_id.chars().map(|c| if c == ':' { '@' } else if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect()
}

/// `@terminum-id` as terminumd sets it (hex): anything else is no file name.
pub fn usable_identity(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// A pane's file (`before`: the one kept from before its tmux restarted).
pub fn file(dir: &Path, identity: &str, pane: &str, before: bool) -> PathBuf {
    dir.join(format!("{identity}_{}{}.json", pane.trim_start_matches('%'), if before { ".before" } else { "" }))
}

/// One pane of a read: what decides its capture and what its file says.
#[derive(Debug, PartialEq)]
pub struct Row {
    pub identity: String,
    pub pane: String,
    pub activity: u64,
    pub alt: bool,
    /// lines in its history (none: a capture of it would give the screen's first line)
    pub history: u64,
    pub session: String,
    pub window: String,
    pub dir: String,
}

/// The panes of a FIELDS read the list shows (`shows(session)`), quick shells left out (never
/// made again, AC-88).
pub fn rows(lines: &[String], shows: impl Fn(&str) -> bool) -> Vec<Row> {
    lines.iter().filter_map(|l| {
        let f: Vec<&str> = l.splitn(9, '\t').collect();
        (f.len() == 9 && f[1].starts_with('%') && f[5].is_empty() && shows(f[6])).then(|| Row {
            identity: f[0].trim().to_string(), pane: f[1].to_string(), activity: f[2].parse().unwrap_or(u64::MAX),
            alt: f[3] == "1", history: f[4].parse().unwrap_or(0), session: f[6].to_string(), window: f[7].to_string(), dir: f[8].to_string(),
        })
    }).collect()
}

/// The last `n` lines of `parts` (each a capture), in order: lines' trailing blanks and each
/// part's trailing blank lines dropped (a screen's empty bottom).
pub fn tail(parts: &[Vec<String>], n: usize) -> String {
    let mut all: Vec<&str> = Vec::new();
    for p in parts {
        let lines: Vec<&str> = p.iter().map(|l| l.trim_end()).collect();
        let end = lines.iter().rposition(|l| !l.is_empty()).map_or(0, |i| i + 1);
        all.extend(&lines[..end]);
    }
    all[all.len().saturating_sub(n)..].join("\n")
}

/// The captures of a pane: its history's end and its screen; under a full-screen program the
/// history and the normal screen it covers (`-a`), never the program's own (nothing a restart
/// could bring back). Twice `n` rows: joined (`-J`), wrapped rows give fewer lines.
pub fn captures(r: &Row, n: usize) -> Vec<String> {
    let pane = &r.pane;
    if !r.alt {
        return vec![format!("capture-pane -p -J -t {pane} -S -{}", 2 * n)];
    }
    let history = (r.history > 0).then(|| format!("capture-pane -p -J -t {pane} -S -{} -E -1", 2 * n));
    history.into_iter().chain([format!("capture-pane -p -J -a -q -t {pane}")]).collect()
}

/// What a round knows of a pane: when it was read, its text's hash, since when it is absent.
#[derive(Default)]
struct Pane {
    read_at: u64,
    hash: u64,
    gone: Option<Instant>,
}

/// One server's panes as the rounds saw them, for one identity.
#[derive(Default)]
pub struct Seen {
    identity: String,
    panes: HashMap<String, Pane>,
}

impl Seen {
    /// A server read anew under `identity`: its panes with files already (a terminumd before this
    /// one kept them) are known, so a pane closed meanwhile goes as one closed now.
    pub fn new(identity: &str, dir: &Path) -> Seen {
        let mut panes = HashMap::new();
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(n) = name.strip_prefix(&format!("{identity}_")).and_then(|r| r.split('.').next()).filter(|n| n.parse::<u64>().is_ok()) {
                panes.insert(format!("%{n}"), Pane::default());
            }
        }
        Seen { identity: identity.to_string(), panes }
    }

    /// The panes of this read to capture: never read, or their window printed in or after the
    /// second they were last read (tmux's `window_activity` is in seconds).
    pub fn due<'a>(&self, rows: &'a [Row]) -> Vec<&'a Row> {
        rows.iter().filter(|r| self.panes.get(&r.pane).is_none_or(|p| r.activity >= p.read_at)).collect()
    }

    /// A pane's text was read at `at`: true when it differs from what was written.
    pub fn read(&mut self, pane: &str, at: u64, text: &str) -> bool {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        let p = self.panes.entry(pane.to_string()).or_default();
        p.read_at = at;
        std::mem::replace(&mut p.hash, h.finish()) != p.hash
    }

    /// The panes closed: absent from this read and from reads since `GONE_FOR` ago. They are
    /// forgotten; a pane listed again is no longer absent.
    pub fn closed(&mut self, rows: &[Row], now: Instant) -> Vec<String> {
        let mut out = Vec::new();
        for (pane, p) in self.panes.iter_mut() {
            if rows.iter().any(|r| &r.pane == pane) {
                p.gone = None;
            } else if now.duration_since(*p.gone.get_or_insert(now)) >= GONE_FOR {
                out.push(pane.clone());
            }
        }
        for pane in &out {
            self.panes.remove(pane);
        }
        out
    }
}

/// When each server last answered, under which identity: what the hourly sweep keeps.
type Answered = Arc<parking_lot::Mutex<HashMap<String, (String, Instant)>>>;

/// Keeps the panes' last lines while `lastLines` is on, every 5 s: each server on a task of its own,
/// so one that hangs delays only its own lines (AC-132).
pub async fn follow(app: Shared) {
    let answered: Answered = Arc::default();
    // by the server itself: a host replaced by a new record is another server, whoever holds the old one
    let mut servers: HashMap<usize, tokio::task::JoinHandle<()>> = HashMap::new();
    let mut swept = Instant::now();
    loop {
        tokio::time::sleep(EVERY).await;
        if wanted(&app) == 0 {
            let dir = root(&app);
            if dir.exists() {
                let _ = tokio::task::spawn_blocking(move || off(&dir)).await;
            }
            continue;
        }
        servers.retain(|_, task| !task.is_finished());
        for s in app.tmux.all() {
            servers.entry(Arc::as_ptr(&s) as usize).or_insert_with(|| tokio::spawn(keep_server(app.clone(), Arc::downgrade(&s), answered.clone())));
        }
        if swept.elapsed() >= SWEEP_EVERY {
            swept = Instant::now();
            // the servers that answered since the last sweep: a host gone or removed ages
            let current: HashMap<String, String> = answered.lock().iter().filter(|(_, (_, t))| t.elapsed() < SWEEP_EVERY)
                .map(|(id, (identity, _))| (folder(id), identity.clone())).collect();
            let dir = root(&app);
            let _ = tokio::task::spawn_blocking(move || sweep(&dir, &current, KEEP_OLD)).await;
        }
    }
}

/// One server's rounds, until it goes.
async fn keep_server(app: Shared, server: std::sync::Weak<Server>, answered: Answered) {
    let (mut seen, mut round) = (Seen::default(), 0u64);
    loop {
        tokio::time::sleep(EVERY).await;
        round += 1;
        let Some(s) = server.upgrade() else { return };
        let n = wanted(&app);
        if n == 0 {
            seen = Seen::default();
        } else if s.is_local() || round % HOST_ROUNDS == 0 {
            keep(&app, &s, n, &mut seen, &answered).await;
        }
    }
}

/// Off: every kept line goes.
fn off(dir: &Path) {
    let _one = WRITING.lock();
    match std::fs::remove_dir_all(dir) {
        Ok(()) => tracing::info!(event = "lastlines.off", "the kept last lines were deleted"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(event = "lastlines.undeleted", error = %e),
    }
}

/// One round on a server: one read of its panes and the captures of those due, on one control
/// client (a server that died fails them, never answers for another), then the files written.
async fn keep(app: &Shared, s: &Server, n: usize, mine: &mut Seen, answered: &Answered) {
    let Some(c) = s.bulk().filter(|_| !s.backed_up()) else { return };
    let Some(Ok(lines)) = c.run_all(vec![format!("list-panes -a -F '{FIELDS}'")]).await.pop() else { return };
    // the identity of every pane listed, shown or not: a server none of whose sessions is shown
    // any more (the user detached them) still loses their panes' lines
    let ids: Vec<&str> = lines.iter().filter_map(|l| l.split('\t').next()).map(str::trim).collect();
    let identity = match ids.first() {
        Some(i) if usable_identity(i) && ids.iter().all(|x| x == i) => i.to_string(),
        _ => return,                                    // no pane, or no identity yet
    };
    let rows = rows(&lines, |session| s.shows(session));
    let dir = root(app).join(folder(&s.id()));
    if mine.identity != identity {
        *mine = Seen::new(&identity, &dir);
    }
    answered.lock().insert(s.id(), (identity.clone(), Instant::now()));
    let due = mine.due(&rows);
    let cmds: Vec<String> = due.iter().flat_map(|r| captures(r, n)).collect();
    let at = crate::auth::store::unix_now();          // before: output during the captures is read again
    let mut answers = c.run_all(cmds).await.into_iter();
    let mut writes = Vec::new();
    for r in due {
        let parts: Option<Vec<Vec<String>>> = captures(r, n).iter().map(|_| answers.next().and_then(Result::ok)).collect();
        let Some(parts) = parts else { continue };     // the pane went meanwhile: read next round
        let text = tail(&parts, n);
        if mine.read(&r.pane, at, &text) {
            let v = json!({"key": s.key(&r.pane), "session": r.session, "window": r.window, "dir": r.dir, "at": at, "text": text});
            writes.push((file(&dir, &identity, &r.pane, false), v));
        }
    }
    let gone: Vec<PathBuf> = mine.closed(&rows, Instant::now()).iter().flat_map(|p| [false, true].map(|b| file(&dir, &identity, p, b))).collect();
    if writes.is_empty() && gone.is_empty() {
        return;
    }
    let a = app.clone();
    let _ = tokio::task::spawn_blocking(move || save(&a, &dir, &identity, &writes, &gone)).await;
}

/// The round's files written and the closed panes' deleted, while lastLines is still on and no
/// restore took `identity`'s lines meanwhile (the round read the server before it restarted).
fn save(app: &Shared, dir: &Path, identity: &str, writes: &[(PathBuf, Value)], gone: &[PathBuf]) {
    let _one = WRITING.lock();
    if wanted(app) == 0 || RETIRED.lock().iter().any(|r| r == identity) {
        return;
    }
    if let Err(e) = private_dir(&root(app)).and_then(|_| private_dir(dir)) {
        tracing::warn!(event = "lastlines.unsaved", error = %e);
        return;
    }
    for (path, v) in writes {
        if let Err(e) = crate::auth::store::write_private(path, v) {
            tracing::warn!(event = "lastlines.unsaved", file = %path.display(), error = %e);
        }
    }
    for path in gone {
        let _ = std::fs::remove_file(path);
    }
}

/// A 0700 folder; the top one also kept out of Time Machine (macOS) when made, by a thread of
/// its own (`tmutil` takes seconds: no write waits for it).
fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    if cfg!(target_os = "macos") && dir.file_name().is_some_and(|n| n == DIR) {
        let dir = dir.to_path_buf();
        std::thread::spawn(move || match std::process::Command::new("/usr/bin/tmutil").arg("addexclusion").arg(&dir).output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => tracing::warn!(event = "lastlines.backed_up", msg = %String::from_utf8_lossy(&o.stderr).trim()),
            Err(e) => tracing::warn!(event = "lastlines.backed_up", error = %e),
        });
    }
    Ok(())
}

/// Files of a server's other identities (a tmux that no restore took) and leftover temporary
/// files go once `keep` old; the current identity's are touched, so their age is the time since
/// their server was last seen (`current`: identity by folder).
pub fn sweep(dir: &Path, current: &HashMap<String, String>, keep: Duration) {
    let _one = WRITING.lock();
    let now = std::time::SystemTime::now();
    for server in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = server.file_name().to_string_lossy().to_string();
        let mine = current.get(&name).map(|i| format!("{i}_"));
        for f in std::fs::read_dir(server.path()).into_iter().flatten().flatten() {
            let fname = f.file_name().to_string_lossy().to_string();
            if mine.as_ref().is_some_and(|m| fname.starts_with(m.as_str())) {
                let _ = std::fs::File::options().append(true).open(f.path()).and_then(|h| h.set_modified(now));
            } else if f.metadata().and_then(|m| m.modified()).is_ok_and(|t| now.duration_since(t).unwrap_or_default() >= keep) {
                let _ = std::fs::remove_file(f.path());
            }
        }
        let _ = std::fs::remove_dir(server.path());     // only when empty
    }
}

/// A restore made panes again (AC-15) on server `server_id`: each pair's (old -> new) lines of
/// the identity its checkpoint saved become the new pane's from before the restart: the old
/// pane's own, or those it had from an earlier restart when it printed nothing since but its
/// prompt. The old identity's other files go (panes not made again). How many panes have them.
pub async fn restarted(app: &Shared, server_id: &str, saved: Option<&str>, now: Option<String>, pairs: &[(String, String)]) -> usize {
    let (Some(old), Some(new)) = (saved.filter(|i| usable_identity(i)), now.filter(|i| usable_identity(i))) else { return 0 };
    if old == new || pairs.is_empty() || wanted(app) == 0 {
        return 0;                                       // nothing made again: the sweep decides
    }
    let (dir, old, pairs) = (root(app).join(folder(server_id)), old.to_string(), pairs.to_vec());
    tokio::task::spawn_blocking(move || {
        let _one = WRITING.lock();
        RETIRED.lock().push(old.clone());
        let handed = pairs.iter().filter(|(o, n)| hand_on(&file(&dir, &old, o, false), &file(&dir, &old, o, true), &file(&dir, &new, n, true))).count();
        for f in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if f.file_name().to_string_lossy().starts_with(&format!("{old}_")) {
                let _ = std::fs::remove_file(f.path());
            }
        }
        tracing::info!(event = "lastlines.restarted", server = %dir.display(), old, new, handed);
        handed
    }).await.unwrap_or(0)
}

/// Renames a pane's lines to `to`: `live`, or `before` while `live` holds no more than a prompt;
/// `to` is touched (the sweep ages a file from when it was last current).
pub fn hand_on(live: &Path, before: &Path, to: &Path) -> bool {
    let printed = |p: &Path| std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .map(|v| v["text"].as_str().unwrap_or("").lines().filter(|l| !l.trim().is_empty()).count());
    let from = match printed(live) {
        Some(n) if n <= ONLY_A_PROMPT && before.exists() => before,
        Some(_) => live,
        None => before,
    };
    let moved = std::fs::rename(from, to).is_ok();
    if moved {
        let _ = std::fs::File::options().append(true).open(to).and_then(|h| h.set_modified(std::time::SystemTime::now()));
    }
    moved
}

/// A pane's kept lines for a page (`lastlines`): those from before its tmux restarted and the
/// latest, each null when there are none, or `{"error"}` when its file cannot be read; a pane
/// gone or a server that does not answer is the answer's `error`.
pub async fn show(app: &Shared, page: &crate::page::Page, key: &str) {
    let refuse = |msg: String| { page.send_json(&json!({"t": "lastlines", "id": key, "error": msg})); };   // said in its window
    let Some((server, pane)) = app.tmux.resolve(key) else {
        return refuse(crate::page::CLOSED.into());
    };
    // read now, not the identity kept for the connection: just after a restart that is the old one
    let read = match server.bulk() {
        Some(c) => c.run_all(vec![crate::restore_srv::IDENTITY.into()]).await.pop().and_then(Result::ok),
        None => None,
    };
    let Some(identity) = read.and_then(|l| l.into_iter().next()).map(|i| i.trim().to_string()).filter(|i| usable_identity(i)) else {
        return refuse(format!("{} does not answer.", server.label()));
    };
    let dir = root(app).join(folder(&server.id()));
    let read = move |before: bool| match std::fs::read(file(&dir, &identity, &pane, before)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(e) => json!({"error": format!("The kept lines cannot be read: {e}")}),
        Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| json!({"error": format!("The kept lines cannot be read: {e}")})),
    };
    let (before, latest) = tokio::task::spawn_blocking(move || (read(true), read(false))).await.unwrap_or_default();
    page.send_json(&json!({"t": "lastlines", "id": key, "lines": wanted(app), "before": before, "latest": latest}));
}

#[cfg(test)]
#[path = "lastlines_tests.rs"]
mod tests;
