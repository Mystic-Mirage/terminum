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
const BACKLOG: usize = 1_000;
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

/// A non-empty `s`.
fn some(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

impl Who {
    /// The user, from `component` (`page`, `panel`) of a browser `c`.
    pub fn user(component: &'static str, page: Option<u64>, c: &Client) -> Who {
        Who { sender: crate::tasks::store::HUMAN, component, page, device: Some(if c.web { "web" } else { "mac" }), ip: some(&c.ip),
              browser: some(&c.browser), tailnet_user: c.tailnet_user.clone(), ..Default::default() }
    }

    /// Dispatcher, from `component`, for run `run` of `tasks` and its agent.
    pub fn dispatcher(component: &'static str, run: &str, tasks: &[String], agent: &str) -> Who {
        Who { sender: crate::tasks::store::DISPATCHER, component, run: some(run), tasks: some(&tasks.join(",")), agent: some(agent), ..Default::default() }
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

#[derive(Clone, Debug, Default)]
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
}

impl Row {
    /// A row written at once: `text` whole until `finish` cuts it.
    fn once(who: Who, kind: &'static str, pane: String, text: &str, error: Option<String>) -> Row {
        let at = now_ms();
        Row { at, ended: at, who, kind, text: text.to_string(), bytes: text.len(), pane, error, ..Default::default() }
    }
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

/// What `page` typed into `pane` (state key `key`) of `server`, as tmux took it or refused it
/// (`error`): keys join the page's open row, a paste is a row of its own. Called after the
/// typing's lock is let go: the log never lengthens a key's way to tmux.
pub fn typed(app: &crate::app::Shared, page: &crate::page::Page, server: &Arc<Server>, pane: &str, key: &str, data: &str, error: Option<&str>) {
    let who = || Who::user("page", Some(page.id), &page.client);
    for seg in segments(data) {
        match seg {
            Seg::Keys(k) => {
                for head in k.split_inclusive('\r') {
                    let mut open = app.inputs.open.lock();
                    if open.get(&page.id).is_some_and(|o| o.row.pane != key)
                        && let Some(o) = open.remove(&page.id) {
                        close(app, o, false);
                    }
                    let o = open.entry(page.id).or_insert_with(|| Open {
                        row: Row { at: now_ms(), who: who(), kind: "typed", pane: key.to_string(), ..Default::default() },
                        lead: None, last: Instant::now(), began: tokio::spawn(describe(app.clone(), server.clone(), pane.to_string(), key.to_string())),
                        server: server.clone(), pane: pane.to_string(),
                    });
                    if o.row.text.len() < TYPED_TEXT {                  // a held key never grows a row past what is kept
                        o.row.text.push_str(&text::rendered(head));
                    }
                    o.row.bytes += head.len();
                    o.lead = o.lead.or_else(|| text::first_text(head));
                    o.last = Instant::now();
                    o.row.error = o.row.error.take().or_else(|| error.map(str::to_string));
                    if head.ends_with('\r')
                        && let Some(done) = open.remove(&page.id) {
                        close(app, done, true);
                    }
                }
            }
            Seg::Paste(p) => {
                let o = app.inputs.open.lock().remove(&page.id);
                if let Some(o) = o {
                    close(app, o, false);
                }
                let mut row = Row::once(who(), "paste", key.to_string(), text::cut(p, PASTED_TEXT), error.map(str::to_string));
                row.bytes = p.len();
                later(app, server, pane, row, None);
            }
        }
    }
}

/// A row written by terminumd at once (the Files panel's text, Dispatcher's prompt and launch);
/// `project`: its (id, name) when the pane is not in it yet (a window just opened for it).
#[allow(clippy::too_many_arguments)]
pub fn wrote(app: &crate::app::Shared, who: Who, kind: &'static str, server: &Arc<Server>, pane: &str, text: &str, error: Option<String>, project: Option<(&str, &str)>) {
    let row = Row::once(who, kind, server.key(pane), text, error);
    later(app, server, pane, row, project.map(|(i, n)| (i.to_string(), n.to_string())));
}

/// `row` once its pane is described, its text kept or not.
fn later(app: &crate::app::Shared, server: &Arc<Server>, pane: &str, mut row: Row, project: Option<(String, String)>) {
    let (app, server, pane) = (app.clone(), server.clone(), pane.to_string());
    tokio::spawn(async move {
        let mut place = describe(app.clone(), server, pane, row.pane.clone()).await;
        if let Some((id, name)) = project {
            (place.project, place.project_name) = (Some(id), Some(name));
        }
        row.place = place;
        finish(&app, row);
    });
}

/// The page's open row ends: its text kept unless the pane asked for a secret when it began, or,
/// ended by no Enter, when it ends (after an Enter the screen shows the program's answer: the
/// prompt `sudo` opens is not the row's), or its first text is a space.
fn close(app: &crate::app::Shared, o: Open, enter: bool) {
    let app = app.clone();
    tokio::spawn(async move {
        let mut row = o.row;
        row.ended = now_ms();
        let began = tokio::time::timeout(DESCRIBE_WAIT, o.began).await.ok().and_then(Result::ok);
        let mut place = began.unwrap_or_else(|| Place { secret: Some(text::NO_ANSWER.into()), ..Place::default() });
        if !enter && place.secret.is_none() {
            place.secret = describe(app.clone(), o.server, o.pane, row.pane.clone()).await.secret;
        }
        if o.lead == Some(' ') {
            place.secret = place.secret.or_else(|| Some(text::LEADING_SPACE.into()));
        }
        row.place = place;
        finish(&app, row);
    });
}

/// The row's text, or why it is not kept (a human's only: Dispatcher types no secret), to the writer.
fn finish(app: &crate::app::Shared, mut row: Row) {
    row.text = match &row.place.secret {
        Some(why) if row.who.sender == crate::tasks::store::HUMAN => why.clone(),
        _ => crate::sent::redact(text::cut(&row.text, TYPED_TEXT)),
    };
    app.inputs.push(row);
}

/// Every page's row that waited IDLE ends; a closed page's at once.
pub fn tick(app: &crate::app::Shared) {
    let ended: Vec<Open> = {
        let mut open = app.inputs.open.lock();
        if open.is_empty() {
            return;
        }
        let live: std::collections::HashSet<u64> = app.pages.all().iter().map(|p| p.id).collect();
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
/// secret, on the server's bulk client (never behind the pane's keys); what tmux does not say
/// stays empty, and a pane that does not answer is not judged: its text is not kept.
async fn describe(app: crate::app::Shared, server: Arc<Server>, pane: String, key: String) -> Place {
    let asked = async {
        tokio::join!(server.run(format!("display -p -t {pane} '#{{pane_current_command}}\t#{{cursor_y}}\t#{{session_name}}\t#{{window_name}}\t#{{pane_current_path}}'")),
                     server.run(format!("capture-pane -p -t {pane}")))
    };
    let (fields, screen) = match tokio::time::timeout(DESCRIBE_WAIT, asked).await {
        Ok((Ok(f), s)) => (f.first().map(|l| l.splitn(5, '\t').map(str::to_string).collect::<Vec<_>>()).unwrap_or_default(), s.unwrap_or_default()),
        _ => (Vec::new(), Vec::new()),
    };
    let field = |i: usize| fields.get(i).filter(|s| !s.is_empty()).cloned();
    let line = field(1).and_then(|y| y.parse::<usize>().ok()).and_then(|y| screen.get(y)).cloned().unwrap_or_default();
    let program = field(0);
    let secret = if fields.is_empty() { Some(text::NO_ANSWER.to_string()) } else { text::not_kept(program.as_deref().unwrap_or(""), &line) };
    // the projects' lock is held across their file's save: off the async workers
    let a = app.clone();
    let (project, project_name) = crate::http::off(move || Ok(a.projects.lock().of(&key).map(|p| (Some(p.id.clone()), Some(p.name.clone()))).unwrap_or_default())).await.unwrap_or_default();
    Place { machine: Some(machine(&server)), session: field(2), window: field(3), program, cwd: field(4), project, project_name, secret }
}

/// The machine a pane runs on: this Mac's name, or the host's.
fn machine(server: &Server) -> String {
    static HERE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if server.is_local() { HERE.get_or_init(|| crate::config::host_name().unwrap_or_else(|| "this Mac".into())).clone() } else { server.name.clone() }
}

/// Typed data as keys and the pastes in it (the page's bracketed paste markers, typing.rs).
#[derive(Debug, PartialEq)]
enum Seg<'a> {
    Keys(&'a str),
    Paste(&'a str),
}

fn segments(data: &str) -> Vec<Seg<'_>> {
    use crate::tmux::keys::{PASTE_END, PASTE_START};
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
