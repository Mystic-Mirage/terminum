// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The input log (terminum.md AC-147): its text, what is not kept, its rows in the store.

use super::text::{browser, not_kept, rendered};
use super::*;
use crate::tasks::store::Store;

fn row(at: i64, text: &str) -> Row {
    Row { at, ended: at, who: Who { sender: crate::tasks::store::HUMAN, component: "page", ..Default::default() }, kind: "typed", text: text.into(), bytes: text.len(),
          pane: "tmux:local:t:%1".into(), ..Default::default() }
}

#[test]
fn ac147_typed_keys_read_as_the_keys_sent() {
    assert_eq!(rendered("ls -lx\x7fa\r"), "ls -lx<BSpace>a<Enter>");
    assert_eq!(rendered("\x1b[A\x03"), "<Up><C-c>");
    assert_eq!(rendered("héllo\tworld"), "héllo<Tab>world");
}

#[test]
fn ac147_a_secret_s_text_is_not_kept() {
    assert_eq!(not_kept("sudo", "").as_deref(), Some("[not kept: sudo]"));
    assert_eq!(not_kept("-ssh-add", "").as_deref(), Some("[not kept: ssh-add]"));
    for line in ["Password:", "[sudo] password for alex: ", "Enter passphrase for key '/Users/alex/.ssh/id_ed25519':", "PIN: ", "alex@devbox.example's password:"] {
        assert_eq!(not_kept("make", line).as_deref(), Some("[not kept: hidden input]"), "{line}");
    }
    assert_eq!(super::text::first_text("\x15\x1b[A export TOKEN=1"), Some(' '), "past Ctrl-U and an arrow");
    for line in ["$ ", "Spinning:", "password reset done", "the password: is set below: ok"] {
        assert_eq!(not_kept("zsh", line), None, "{line}");
    }
    // ssh itself is kept: a remote shell's whole session would be lost
    assert_eq!(not_kept("ssh", "alex@devbox:~$ "), None);
}

#[test]
fn ac147_the_browser_in_a_few_words() {
    assert_eq!(browser("Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1"), "Safari 18 · iPhone");
    assert_eq!(browser("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36"), "Chrome 141 · Mac");
    assert_eq!(browser("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15"), "Safari 26 · Mac");
    assert_eq!(browser(""), "");
}

#[test]
fn ac147_a_paste_in_typed_data_is_a_segment_of_its_own() {
    use crate::tmux::keys::{PASTE_END, PASTE_START};
    let data = format!("ab{PASTE_START}pasted\ntext{PASTE_END}\r");
    assert_eq!(segments(&data), vec![Seg::Keys("ab"), Seg::Paste("pasted\ntext"), Seg::Keys("\r")]);
    assert_eq!(segments("plain"), vec![Seg::Keys("plain")]);
}

#[test]
fn ac147_rows_go_into_the_store_and_old_ones_go() {
    let s = Store::open(crate::testutil::fresh_dir("inputs-rows").join("kanban.sqlite"));
    let now = now_ms();
    let old = now - (writer::KEEP_DAYS + 1) * 86_400_000;
    s.with(|tx| writer::insert(tx, &[row(old, "old"), row(now, "a"), row(now, "b"), row(now, "c")])).unwrap();
    s.with(|tx| writer::trim(tx, now, 2)).unwrap();
    let kept: Vec<String> = s.with(|tx| {
        let mut st = tx.prepare("SELECT text FROM inputs ORDER BY id").unwrap();
        let out = st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        Ok(out)
    }).unwrap();
    assert_eq!(kept, ["b", "c"], "older than KEEP_DAYS, then beyond the cap");
    let sender: String = s.with(|tx| tx.query_row("SELECT sender || ' ' || version FROM inputs LIMIT 1", [], |r| r.get(0)).map_err(|e| e.to_string())).unwrap();
    assert_eq!(sender, format!("human {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn ac147_a_store_that_cannot_be_used_drops_rows_counted_and_holds_nothing_up() {
    let log = Inputs::new(Some(Arc::new(Store::absent())));
    let t0 = Instant::now();
    for i in 0..5 {
        log.push(row(i, "x"));
    }
    assert!(t0.elapsed() < Duration::from_millis(50), "pushing never waits for the store");
    let until = Instant::now() + Duration::from_secs(5);
    while log.dropped() < 5 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(log.dropped(), 5);
    // none at all (an Agent's): rows go nowhere, nothing is counted
    let none = Inputs::new(None);
    none.push(row(0, "x"));
    assert_eq!(none.dropped(), 0);
}
