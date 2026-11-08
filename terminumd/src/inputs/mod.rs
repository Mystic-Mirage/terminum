// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The input log (terminum.md AC-147): everything terminumd writes into a pane, a row each in the
//! tasks' store's `inputs` table, with who sent it (the user from a page or the Files panel,
//! Dispatcher), from where (page, browser, address) and into what (machine, session, window,
//! program, folder, project). A page's typed keys gather in one row until Enter, a paste, another
//! pane or IDLE without a key; a row whose text is a secret's keeps only why it is not kept. The
//! log never holds up or refuses input: a full channel or a store that cannot be used drops rows,
//! counted and logged.

pub mod text;
mod writer;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::tmux::Server;

/// A row of typed keys ends after this long without a key.
pub const IDLE: Duration = Duration::from_secs(2);
/// Rows waiting for the writer at most; more are dropped.
const BACKLOG: usize = 10_000;
/// The most characters of a typed row's text, and of a paste's (`bytes` says the whole).
const TYPED_TEXT: usize = 20_000;
const PASTED_TEXT: usize = 4_000;
/// How long a row waits for its pane's description (a host's tmux over ssh).
const DESCRIBE_WAIT: Duration = Duration::from_secs(3);

/// Who wrote: the sender (`users.id`: `human`, `dispatcher`), terminumd's component that wrote,
/// and what the sender's side tells: a page's socket and browser, Dispatcher's run.
#[derive(Clone, Debug, Default)]
pub struct Who {
    pub sender: &'static str,
    pub component: &'static str,
    pub page: Option<u64>,
    /// `mac` (the Mac's own browser) or `web` (through the tailnet)
    pub device: Option<&'static str>,
    pub ip: Option<String>,
    pub browser: Option<String>,
    pub tailnet_user: Option<String>,
    pub run: Option<String>,
    pub tasks: Option<String>,
    pub agent: Option<String>,
}

/// What a page's socket or a Panel's request said about its browser (ws.rs, panel_api.rs).
#[derive(Clone, Debug, Default)]
pub struct Client {
    pub web: bool,
    pub ip: String,
    pub browser: String,
    pub tailnet_user: Option<String>,
}

impl Who {
    /// The user, from `component` (`page`, `panel`) of a browser `c`.
    pub fn user(component: &'static str, page: Option<u64>, c: &Client) -> Who {
        Who { sender: crate::tasks::store::HUMAN, component, page, device: Some(if c.web { "web" } else { "mac" }), ip: Some(c.ip.clone()),
              browser: Some(c.browser.clone()).filter(|b| !b.is_empty()), tailnet_user: c.tailnet_user.clone(), ..Default::default() }
    }

    /// Dispatcher, from `component`, for run `run` of `tasks` and its agent.
    pub fn dispatcher(component: &'static str, run: &str, tasks: &[String], agent: &str) -> Who {
        Who { sender: crate::tasks::store::DISPATCHER, component, run: Some(run.to_string()), tasks: Some(tasks.join(",")).filter(|t| !t.is_empty()),
              agent: Some(agent.to_string()).filter(|a| !a.is_empty()), ..Default::default() }
    }
}

/// Where the input went, read when the row began.
#[derive(Clone, Debug, Default)]
pub struct Place {
    pub machine: Option<String>,
    pub session: Option<String>,
    pub window: Option<String>,
    pub program: Option<String>,
    pub cwd: Option<String>,
    pub project: Option<String>,
    pub project_name: Option<String>,
    /// why its text is not kept, read from the pane (text::not_kept)
    pub secret: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub at: i64,
    pub ended: i64,
    pub who: Who,
    pub kind: &'static str,
    pub text: String,
    pub bytes: usize,
    pub pane: String,
    pub place: Place,
    pub error: Option<String>,
    /// a typed row's first text is a space
    pub spaced: bool,
}

/// A page's row of typed keys still open.
struct Open {
    row: Row,
    /// its first text, once typed
    lead: Option<char>,
    last: Instant,
    began: tokio::task::JoinHandle<Place>,
    server: Arc<Server>,
    pane: String,
}

pub struct Inputs {
    tx: Option<SyncSender<Row>>,
    open: Mutex<HashMap<u64, Open>>,
    dropped: Arc<AtomicU64>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

impl Inputs {
    /// The log of `store`, its writer on a thread of its own; none (an Agent's): rows go nowhere.
    pub fn new(store: Option<Arc<crate::tasks::store::Store>>) -> Inputs {
        let dropped = Arc::new(AtomicU64::new(0));
        let tx = store.map(|store| {
            let (tx, rx) = sync_channel(BACKLOG);
            let d = dropped.clone();
            std::thread::Builder::new().name("inputs".into()).spawn(move || writer::run(store, rx, d)).expect("a thread");
            tx
        });
        Inputs { tx, open: Mutex::default(), dropped }
    }

    /// A finished row to the writer; a full channel drops it, counted and logged.
    fn push(&self, row: Row) {
        let Some(tx) = &self.tx else { return };
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = tx.try_send(row) {
            let lost = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            if lost.is_power_of_two() {
                tracing::warn!(event = "inputs.unsaved", lost, error = "the writer is behind");
            }
        }
    }

    /// Rows lost so far.
    #[cfg(test)]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// What terminumd typed for page `who` into `pane` (state key `key`) of `server`, as tmux took it
/// or refused it (`error`): keys join the page's open row, a paste is a row of its own.
pub fn typed(app: &crate::app::Shared, who: Who, server: &Arc<Server>, pane: &str, key: &str, data: &str, error: Option<&str>) {
    let page = who.page.unwrap_or(0);
    for seg in segments(data) {
        match seg {
            Seg::Keys(mut k) => {
                while !k.is_empty() {
                    // up to and with an Enter, which ends the row; what follows begins the next
                    let (head, enter, rest) = match k.find('\r') {
                        Some(i) => (&k[..=i], true, &k[i + 1..]),
                        None => (k, false, ""),
                    };
                    let mut open = app.inputs.open.lock();
                    if open.get(&page).is_some_and(|o| o.row.pane != key) {
                        let o = open.remove(&page).expect("checked");
                        close(app, o, false);
                    }
                    let o = open.entry(page).or_insert_with(|| Open {
                        row: Row { at: now_ms(), ended: 0, who: who.clone(), kind: "typed", text: String::new(), bytes: 0, pane: key.to_string(), place: Place::default(), error: None, spaced: false },
                        lead: None, last: Instant::now(), began: tokio::spawn(describe(app.clone(), server.clone(), pane.to_string(), key.to_string())), server: server.clone(), pane: pane.to_string(),
                    });
                    if o.lead.is_none() {
                        o.lead = text::first_text(head);
                        o.row.spaced = o.lead == Some(' ');
                    }
                    o.row.text.push_str(&text::rendered(head));
                    o.row.bytes += head.len();
                    o.last = Instant::now();
                    if o.row.error.is_none() {
                        o.row.error = error.map(str::to_string);
                    }
                    if enter {
                        let done = open.remove(&page).expect("open");
                        drop(open);
                        close(app, done, true);
                    }
                    k = rest;
                }
            }
            Seg::Paste(p) => {
                let o = app.inputs.open.lock().remove(&page);
                if let Some(o) = o {
                    close(app, o, false);
                }
                let row = Row { at: now_ms(), ended: now_ms(), who: who.clone(), kind: "paste", text: text::cut(p, PASTED_TEXT), bytes: p.len(), pane: key.to_string(), place: Place::default(), error: error.map(str::to_string), spaced: false };
                later(app, server, pane, row);
            }
        }
    }
}

/// A row written by terminumd at once (the Files panel's text, Dispatcher's prompt and launch);
/// `project`: its (id, name) when the pane is not in it yet (a window just opened for it).
#[allow(clippy::too_many_arguments)]
pub fn wrote(app: &crate::app::Shared, who: Who, kind: &'static str, server: &Arc<Server>, pane: &str, text: &str, error: Option<String>, project: Option<(&str, &str)>) {
    let place = Place { project: project.map(|p| p.0.to_string()), project_name: project.map(|p| p.1.to_string()), ..Place::default() };
    let row = Row { at: now_ms(), ended: now_ms(), who, kind, text: text::cut(text, TYPED_TEXT), bytes: text.len(), pane: server.key(pane), place, error, spaced: false };
    later(app, server, pane, row);
}

/// `row` once its pane is described, its text kept or not.
fn later(app: &crate::app::Shared, server: &Arc<Server>, pane: &str, mut row: Row) {
    let (app, server, pane) = (app.clone(), server.clone(), pane.to_string());
    tokio::spawn(async move {
        let found = describe(app.clone(), server, pane, row.pane.clone()).await;
        let (project, project_name) = (row.place.project.take().or(found.project.clone()), row.place.project_name.take().or(found.project_name.clone()));
        row.place = Place { project, project_name, ..found };
        finish(&app, row);
    });
}

/// The page's open row ends: its text kept unless the pane asked for a secret when it began, or,
/// ended by no Enter, when it ends (after an Enter the screen shows the program's answer: the
/// prompt `sudo` opens is not the row's).
fn close(app: &crate::app::Shared, o: Open, enter: bool) {
    let app = app.clone();
    tokio::spawn(async move {
        let mut row = o.row;
        row.ended = now_ms();
        let began = tokio::time::timeout(DESCRIBE_WAIT, o.began).await.ok().and_then(Result::ok);
        let mut place = match began {
            Some(p) => p,
            None => Place { secret: Some("[not kept: the pane did not answer]".into()), ..Place::default() },
        };
        if !enter && place.secret.is_none() {
            place.secret = describe(app.clone(), o.server, o.pane, row.pane.clone()).await.secret;
        }
        row.place = place;
        finish(&app, row);
    });
}

/// The row's text, or why it is not kept, to the writer.
fn finish(app: &crate::app::Shared, mut row: Row) {
    let why = row.place.secret.clone().or_else(|| text::not_kept("", "", row.spaced));
    row.text = match why {
        Some(why) if row.who.sender == crate::tasks::store::HUMAN => why,
        _ => crate::sent::redact(&text::cut(&row.text, TYPED_TEXT)),
    };
    app.inputs.push(row);
}

/// Every page's row that waited IDLE ends; a closed page's at once.
pub fn tick(app: &crate::app::Shared) {
    let live: std::collections::HashSet<u64> = app.pages.all().iter().map(|p| p.id).collect();
    let ended: Vec<Open> = {
        let mut open = app.inputs.open.lock();
        let gone: Vec<u64> = open.iter().filter(|(id, o)| o.last.elapsed() >= IDLE || !live.contains(id)).map(|(id, _)| *id).collect();
        gone.into_iter().filter_map(|id| open.remove(&id)).collect()
    };
    for o in ended {
        close(app, o, false);
    }
}

/// Ends idle rows every half second, while terminumd runs.
pub async fn ticking(app: crate::app::Shared) {
    let mut every = tokio::time::interval(Duration::from_millis(500));
    loop {
        every.tick().await;
        tick(&app);
    }
}

/// The pane's machine, session, window, program, folder and project, and whether it asks for a
/// secret, in one round trip; what tmux does not say stays empty.
async fn describe(app: crate::app::Shared, server: Arc<Server>, pane: String, key: String) -> Place {
    let lines = vec![format!("display -p -t {pane} '#{{pane_current_command}}\t#{{cursor_y}}\t#{{session_name}}\t#{{window_name}}\t#{{pane_current_path}}'"), format!("capture-pane -p -t {pane}")];
    let said = tokio::time::timeout(DESCRIBE_WAIT, server.run_all_on(&pane, lines)).await.unwrap_or_default();
    let fields: Vec<String> = said.first().and_then(|a| a.as_ref().ok()).and_then(|l| l.first()).map(|l| l.splitn(5, '\t').map(str::to_string).collect()).unwrap_or_default();
    let field = |i: usize| fields.get(i).filter(|s| !s.is_empty()).cloned();
    let screen: Vec<String> = said.get(1).and_then(|a| a.as_ref().ok()).cloned().unwrap_or_default();
    let line = field(1).and_then(|y| y.parse::<usize>().ok()).and_then(|y| screen.get(y)).cloned().unwrap_or_default();
    let program = field(0);
    // a pane that did not answer is not judged: its text is not kept
    let secret = if fields.is_empty() { Some("[not kept: the pane did not answer]".to_string()) } else { text::not_kept(program.as_deref().unwrap_or(""), &line, false) };
    let (project, project_name) = app.projects.lock().of(&key).map(|p| (Some(p.id.clone()), Some(p.name.clone()))).unwrap_or_default();
    Place { machine: Some(machine(&server)), session: field(2), window: field(3), program, cwd: field(4), project, project_name, secret }
}

/// The machine a pane runs on: this Mac's name, or the host's.
fn machine(server: &Server) -> String {
    if !server.is_local() {
        return server.name.clone();
    }
    let mut buf = [0u8; 256];
    // SAFETY: a buffer of its stated length; gethostname writes at most that, NUL-terminated or cut
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    if ok { String::from_utf8_lossy(&buf[..n]).into_owned() } else { "this Mac".into() }
}

/// Typed data as keys and the pastes in it (the page's bracketed paste markers, typing.rs).
#[derive(Debug, PartialEq)]
enum Seg<'a> {
    Keys(&'a str),
    Paste(&'a str),
}

fn segments(data: &str) -> Vec<Seg<'_>> {
    use crate::page::typing::{PASTE_END, PASTE_START};
    let mut out = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        match rest.find(PASTE_START) {
            Some(i) => {
                if i > 0 {
                    out.push(Seg::Keys(&rest[..i]));
                }
                let inner = &rest[i + PASTE_START.len()..];
                let j = inner.find(PASTE_END).unwrap_or(inner.len());
                out.push(Seg::Paste(&inner[..j]));
                rest = inner.get(j + PASTE_END.len()..).unwrap_or("");
            }
            None => {
                out.push(Seg::Keys(rest));
                rest = "";
            }
        }
    }
    out
}
