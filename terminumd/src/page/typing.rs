// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Typing and pasting into the shown pane (AC-03). Text goes as `send-keys -l`, special keys
//! by tmux's names (tmux encodes them for the pane's mode), never raw control characters.
//! Keys that arrive while earlier ones are on their way go together in the next round
//! trip, in order: a held key comes out at its repeat rate. Mouse reports (AC-06) share the
//! queue, so a trackpad's wheel on a far host is one `send-keys -H` a round trip, in order
//! with the keys, and never holds up the page's next messages. A bracketed paste goes through
//! a tmux buffer (`paste-buffer -p`), so a program that asked for it gets its markers.

use std::collections::VecDeque;
use std::sync::Arc;

use super::client::error;
use super::Page;
use crate::app::Shared;
use crate::tmux::keys::{PASTE_END, PASTE_START};
use crate::tmux::server::OnCopy;
use crate::tmux::{args, keys, Server};

/// Largest paste (AC-03 limits).
const PASTE_MAX: usize = 1 << 20;
/// Most bytes of one set-buffer command (a host's paste).
const BUFFER_PART: usize = 64 << 10;
/// How long the part of a long run of keys waits for the pane to print before the next goes.
const PART_WAIT: std::time::Duration = std::time::Duration::from_millis(250);
/// Most bytes and items typed but not yet handed to tmux (a slow host): more is refused, loud.
const QUEUE_BYTES: usize = 4 << 20;
const QUEUE_ITEMS: usize = 10_000;
const PASTE_TOO_LARGE: &str = "Paste is larger than 1 MB";
/// Claude Code's words under a paste it folded into "[Pasted text #1 +6 lines]"; the same paste again
/// unfolds it (read in its program, 2.1.318): what it holds is the user's to see (AC-03).
const FOLDED: &str = "paste again to expand";
/// How long its words are waited for: keys typed next wait behind them.
const FOLD_WAIT: std::time::Duration = std::time::Duration::from_millis(800);

/// Whether Claude Code folds this paste: over 800 characters or two line breaks.
fn folds(text: &str) -> bool {
    text.chars().nth(800).is_some() || text.split("\r\n").flat_map(|l| l.split(['\n', '\r'])).nth(3).is_some()
}

/// A paste Claude Code folded, pasted once more; nothing if its words do not come: a paste it left
/// as it is would be doubled, and another version may not say them. `wake`: from before the paste,
/// so its words' output is not missed; the screen is read only when the pane printed.
async fn unfold(app: &Shared, client: &crate::tmux::control::Control, server: &Server, pane: &str, text: &str, mut wake: crate::tmux::server::Wake) -> Result<(), String> {
    let until = tokio::time::Instant::now() + FOLD_WAIT;
    while tokio::time::timeout_at(until, wake.notified()).await.is_ok() {
        if client.run(format!("capture-pane -p -t {pane}")).await.is_ok_and(|rows| rows.iter().any(|r| r.contains(FOLDED))) {
            return paste_buffer(app, client, server, pane, text).await;
        }
    }
    Ok(())
}
pub const OVERLOAD: &str = "tmux is not keeping up: more than 4 MB of typing waits for it — this input was not sent";

/// Typed data not yet handed to tmux.
pub struct Typed {
    server: Arc<Server>,
    pane: String,
    key: String,
    data: String,
    /// a mouse report's bytes (`data` empty)
    mouse: Vec<u8>,
    /// a release to a full-screen program: from when it goes to tmux, what the program copies is this page's (AC-06)
    arm: bool,
    k: Option<super::trace::KeyRec>,
}

#[derive(Default)]
pub struct Queue {
    items: VecDeque<Typed>,
    /// bytes of `items`
    bytes: usize,
    running: bool,
}

impl Typed {
    fn size(&self) -> usize {
        self.data.len() + self.mouse.len()
    }
}

impl Queue {
    /// Takes `item` unless the queue is full (QUEUE_BYTES, QUEUE_ITEMS); false: refused.
    fn push(&mut self, item: Typed) -> bool {
        if self.items.len() >= QUEUE_ITEMS || self.bytes + item.size() > QUEUE_BYTES {
            return false;
        }
        self.bytes += item.size();
        self.items.push_back(item);
        true
    }

    /// Drops what waits (its page closed).
    fn clear(&mut self) {
        self.items.clear();
        self.bytes = 0;
    }

    fn batch(&mut self) -> Option<Vec<Typed>> {
        let first = self.items.front()?;
        let (key, server) = (first.key.clone(), first.server.clone());
        let mut out = Vec::new();
        while self.items.front().is_some_and(|t| t.key == key && Arc::ptr_eq(&t.server, &server)) {
            let t = self.items.pop_front().expect("checked");
            self.bytes -= t.size();
            out.push(t);
        }
        Some(out)
    }
}

/// The page typed `data` into its shown pane.
pub fn typed(app: &Shared, page: &Arc<Page>, data: String, k: Option<u64>) {
    let st = page.state.lock();
    let Some(s) = &st.shown else {
        drop(st);
        error(page, "Open a session first.", None);
        return;
    };
    let (server, pane, key) = (s.server.clone(), s.pane.clone(), s.key.clone());
    drop(st);
    let k = super::timing::arrived(page, k);
    queue(app, page, Typed { server, pane, key, data, mouse: Vec::new(), arm: false, k });
}

/// Mouse report bytes for the shown pane `pane` (state key `key`) of `server` (AC-06); `arm`:
/// a release to a full-screen program, whose copy this page gets from when its `send-keys` goes.
pub fn mouse(app: &Shared, page: &Arc<Page>, server: Arc<Server>, pane: String, key: String, bytes: Vec<u8>, arm: bool) {
    queue(app, page, Typed { server, pane, key, data: String::new(), mouse: bytes, arm, k: None });
}

/// Where what the program of `key` copies goes: this page, as `copied` (AC-06).
fn copied_to(page: &Arc<Page>, key: &str) -> OnCopy {
    let (to, sid) = (Arc::downgrade(page), key.to_string());
    Arc::new(move |text: String| {
        if let Some(p) = to.upgrade() {
            p.send_json(&serde_json::json!({"t": "copied", "sid": sid, "text": text}));
        }
    })
}

fn queue(app: &Shared, page: &Arc<Page>, item: Typed) {
    if page.is_closed() {
        return;
    }
    let mut st = page.state.lock();
    let key = item.key.clone();
    if !st.queue.push(item) {
        drop(st);
        error(page, OVERLOAD, Some(&key));
        return;
    }
    if st.queue.running {
        return;
    }
    st.queue.running = true;
    drop(st);
    tokio::spawn(run(app.clone(), page.clone()));
}

/// A paste the page sent as such (`paste` message).
pub fn paste(app: &Shared, page: &Arc<Page>, data: String, bracket: bool) {
    if data.len() > PASTE_MAX {
        error(page, PASTE_TOO_LARGE, None);       // refused before it is copied or queued
        return;
    }
    let wrapped = if bracket { format!("{PASTE_START}{data}{PASTE_END}") } else { data };
    typed(app, page, wrapped, None);
}

async fn run(app: Shared, page: Arc<Page>) {
    loop {
        let mut batch: Vec<Typed> = {
            let mut st = page.state.lock();
            if page.is_closed() {
                st.queue.clear();                      // nobody to type for: what waits is dropped
                st.queue.running = false;
                return;
            }
            let Some(batch) = st.queue.batch() else {
                st.queue.running = false;
                return;
            };
            batch
        };
        let (server, pane, key) = (batch[0].server.clone(), batch[0].pane.clone(), batch[0].key.clone());
        let keys = batch.iter().any(|t| t.mouse.is_empty());
        let ks: Vec<Option<super::trace::KeyRec>> = batch.iter_mut().map(|t| t.k.take()).collect();
        let Some(client) = server.client(&pane) else {
            let what = if keys { "the keys were not sent" } else { "the mouse event was not sent" };
            error(&page, format!("Not connected: tmux on {} does not answer — {what}", server.label()), Some(&key));
            continue;
        };
        if batch.iter().any(|t| t.arm) {
            // armed as the release goes (never while it waits behind a paste): what the program
            // copies once it has it is this page's (AC-06)
            server.arm_copy(&pane, copied_to(&page, &key));
        }
        let mut result = Ok(());
        for run in runs(&batch) {
            result = match run {
                Run::Keys(data) => send(&app, &client, &server, &pane, &data).await,
                Run::Mouse(bytes) => client.run(mouse_line(&pane, &bytes)).await.map(drop).map_err(|e| format!("Mouse event not sent: {e}")),
            };
            if result.is_err() {
                break;
            }
        }
        super::timing::typed(&page, ks, "keys");
        if let Err(e) = result {
            error(&page, e, Some(&key));
            continue;
        }
        // its screen woken, if its pane is still the one shown
        let still_shown = page.state.lock().shown.as_ref().filter(|s| s.key == key && Arc::ptr_eq(&s.server, &server))
            .map(|s| s.ctl.wake.notify_one()).is_some();
        if keys && still_shown {
            super::sizing::typed(&app, &page).await;
        }
    }
}

/// A batch in order as runs of one kind: keys joined, mouse reports joined; a run is a round
/// trip of its own (typing while scrolling on a host: one each, rare and short).
enum Run {
    Keys(String),
    Mouse(Vec<u8>),
}

fn runs(batch: &[Typed]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for t in batch {
        match (out.last_mut(), t.mouse.is_empty()) {
            (Some(Run::Keys(d)), true) => d.push_str(&t.data),
            (Some(Run::Mouse(b)), false) => b.extend_from_slice(&t.mouse),
            (_, true) => out.push(Run::Keys(t.data.clone())),
            (_, false) => out.push(Run::Mouse(t.mouse.clone())),
        }
    }
    out
}

/// Mouse reports as one tmux command: their bytes in hex.
fn mouse_line(pane: &str, bytes: &[u8]) -> String {
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("send-keys -t {pane} -H {}", hex.join(" "))
}

/// Hands `data` to tmux: bracketed pastes through a buffer, the rest as keys.
async fn send(app: &Shared, client: &crate::tmux::control::Control, server: &Server, pane: &str, data: &str) -> Result<(), String> {
    let mut rest = data;
    while !rest.is_empty() {
        let (before, pasted, after) = match rest.find(PASTE_START) {
            Some(i) => {
                let inner = &rest[i + PASTE_START.len()..];
                match inner.find(PASTE_END) {
                    Some(j) => (&rest[..i], Some(&inner[..j]), &inner[j + PASTE_END.len()..]),
                    None => (&rest[..i], Some(inner), ""),
                }
            }
            None => (rest, None, ""),
        };
        if !before.is_empty() {
            let parts = keys::parts(before);
            let extended = !keys::needs_extended(&parts) || server.extended_keys(client).await;
            // more than a part (moves by lines, held; a long quote typed): a part once the pane
            // printed since the last (the program read it), or PART_WAIT (keys.rs PART_BYTES)
            let cmds = keys::in_parts(pane, &parts, extended);
            let n = cmds.len();
            for (i, part) in cmds.into_iter().enumerate() {
                let seen = server.printed(pane);
                for a in client.run_all(part).await {
                    a.map_err(|e| format!("tmux did not take the keys: {e}"))?;
                }
                let until = tokio::time::Instant::now() + PART_WAIT;
                while i + 1 < n && server.printed(pane) == seen && tokio::time::Instant::now() < until {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            }
        }
        if let Some(text) = pasted {
            let wake = server.wake(pane);
            paste_buffer(app, client, server, pane, text).await?;
            let agent = || super::marks::find_item(&app.layout.read(), &server.key(pane)).is_some_and(|it| it["agent"] == "claude");
            if folds(text) && agent() {
                unfold(app, client, server, pane, text, wake).await?;
            }
        }
        rest = after;
    }
    Ok(())
}

/// A paste through a tmux buffer: a 0600 file and `load-buffer` on this Mac, `set-buffer`
/// in parts on a host; then `paste-buffer -p` (bracketed, the buffer deleted).
async fn paste_buffer(app: &Shared, client: &crate::tmux::control::Control, server: &Server, pane: &str, text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());                                 // tmux makes no buffer of nothing ("no buffer tm-…")
    }
    if text.len() > PASTE_MAX {
        return Err(PASTE_TOO_LARGE.into());
    }
    if text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r')) {
        return Err("The text holds control characters and was not sent".into());
    }
    let name = format!("tm-{}", crate::auth::crypto::random_hex::<6>());
    if server.is_local() {
        let path = app.cfg.dir.join(format!("paste-{name}"));
        crate::config::write_private(&path, text.as_bytes()).map_err(|e| format!("Paste not sent: {e}"))?;
        let loaded = client.run(format!("load-buffer -b {name} {}", args::quote(&path.display().to_string()))).await;
        let _ = std::fs::remove_file(&path);
        loaded.map_err(|e| format!("tmux did not take the paste: {e}"))?;
    } else {
        let mut first = true;
        for part in keys::pieces(text, BUFFER_PART) {
            let append = if first { "" } else { "-a " };
            first = false;
            // `--`: a part that starts with "-" is text, not a flag (tmux stops at the first word)
            client.run(format!("set-buffer {append}-b {name} -- {}", double_quoted(part))).await.map_err(|e| format!("tmux did not take the paste: {e}"))?;
        }
    }
    client.run(format!("paste-buffer -p -d -b {name} -t {pane}")).await.map_err(|e| format!("tmux did not take the paste: {e}"))?;
    Ok(())
}

/// Text in tmux's double quotes, where `\n`, `\t`, `\r` escape and `$` would expand.
pub fn double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn claude_codes_folding_rule() {
        assert!(!folds("one\ntwo\nthree"), "two breaks stay");
        assert!(folds("one\ntwo\nthree\nfour"));
        assert!(folds("one\r\ntwo\r\nthree\r\nfour"));
        assert!(!folds("one\r\ntwo\r\nthree"), "a CRLF is one break");
        assert!(!folds(&"я".repeat(800)), "characters, not bytes");
        assert!(folds(&"я".repeat(801)));
    }

    use super::*;

    fn item(data: &str, mouse: &[u8]) -> Typed {
        let link = crate::tmux::server::Link::Local { tmux: "tmux".into(), socket: "tm-test".into(), conf: "/dev/null".into() };
        let server = Server::new("local", "tm-test", link, Arc::new(tokio::sync::Notify::new()));
        Typed { server, pane: "%1".into(), key: String::new(), data: data.into(), mouse: mouse.to_vec(), arm: false, k: None }
    }

    #[test]
    fn a_batch_keeps_its_order_and_joins_each_kind() {
        let batch = [item("a", b""), item("b", b""), item("", b"\x1b[<64;1;1M"), item("", b"\x1b[<64;1;1M"), item("c", b"")];
        let got: Vec<String> = runs(&batch).into_iter().map(|r| match r {
            Run::Keys(d) => format!("keys:{d}"),
            Run::Mouse(b) => format!("mouse:{}", String::from_utf8_lossy(&b)),
        }).collect();
        assert_eq!(got, ["keys:ab", "mouse:\x1b[<64;1;1M\x1b[<64;1;1M", "keys:c"]);
        assert_eq!(mouse_line("%3", b"\x1b["), "send-keys -t %3 -H 1b 5b");
    }

    #[test]
    fn queued_typing_keeps_servers_and_replacement_generations_separate() {
        let mut first = item("a", b"");
        first.key = "tmux:local:tm-test:%1".into();
        let mut same = item("b", b"");
        same.key = first.key.clone();
        same.server = first.server.clone();
        let mut remote = item("remote", b"");
        remote.key = "tmux:devbox:default:%1".into();
        let mut replacement = item("replacement", b"");
        replacement.key = first.key.clone();
        let mut mouse = item("", b"mouse");
        mouse.key = remote.key.clone();
        mouse.server = remote.server.clone();
        let destinations = [first.server.clone(), remote.server.clone(), replacement.server.clone(), remote.server.clone()];
        let mut q = Queue::default();
        for t in [first, same, remote, replacement, mouse] {
            assert!(q.push(t));
        }
        for (server, length) in destinations.iter().zip([2, 1, 1, 1]) {
            let batch = q.batch().unwrap();
            assert_eq!(batch.len(), length);
            assert!(batch.iter().all(|t| Arc::ptr_eq(&t.server, server)));
        }
        assert!(q.batch().is_none());
        assert_eq!(q.bytes, 0);
    }

    #[test]
    fn a_flood_past_the_bound_is_refused_and_memory_stays_bounded() {
        let mut q = Queue::default();
        let paste = "x".repeat(PASTE_MAX);
        let mut taken = 0;
        for _ in 0..100 {
            taken += usize::from(q.push(item(&paste, b"")));
        }
        assert_eq!(taken, QUEUE_BYTES / PASTE_MAX, "a few pastes wait; the rest are refused");
        assert!(q.bytes <= QUEUE_BYTES);
        assert!(!q.push(item("a", b"")), "full: even a key is refused until tmux takes some");
        q.batch().unwrap();                             // one paste handed to tmux (each item its own server here)
        assert_eq!(q.bytes, QUEUE_BYTES - PASTE_MAX);
        assert!(q.push(item("a", b"")), "drained: typing goes again");
        // the words say the limits the code holds
        assert!(OVERLOAD.contains(&format!("{} MB", QUEUE_BYTES >> 20)) && PASTE_TOO_LARGE.contains(&format!("{} MB", PASTE_MAX >> 20)));
    }

    #[test]
    fn many_small_items_are_bounded_by_count_and_clear_drops_them() {
        let mut q = Queue::default();
        let taken = (0..QUEUE_ITEMS + 5).filter(|_| q.push(item("", b"m"))).count();
        assert_eq!(taken, QUEUE_ITEMS);
        q.clear();
        assert!(q.items.is_empty() && q.bytes == 0);
    }

    #[test]
    fn double_quotes_escape_what_tmux_would_read() {
        assert_eq!(double_quoted("a\nb $HOME \"q\" \\ \t"), r#""a\nb \$HOME \"q\" \\ \t""#);
    }
}
