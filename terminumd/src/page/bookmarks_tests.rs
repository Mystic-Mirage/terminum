// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Bookmarks (AC-33): found again by their text nearest their line, kept on the Mac in a file
//! only the user reads, nothing half saved. Ported from iterm-enhancer's test_web_bookmarks.py;
//! the cases of its iTerm2 sessions (chunked reads, a tmux -CC pane attached anew) have no
//! counterpart: a bookmark is keyed by its tmux pane.

use super::*;
use crate::page::bookmarks_find::{loose, Found};
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// The page's cases (bridge/tests/loose_cases.json, web/static/marktext.js).
const LOOSE_CASES: [(&str, &str); 10] = [
    ("Hello, World!", "helloworld"),
    ("  **bold** and `code` ", "boldandcode"),
    ("❯ git status\u{200b}", "gitstatus"),
    ("✳\u{fe0e} Working… (12s)", "working12s"),
    ("ÄÖÜ straße İstanbul", "äöüstraßeistanbul"),
    ("日本語のテキスト 123", "日本語のテキスト123"),
    ("αβγ ΔΕΖ", "αβγδεζ"),
    ("a_b-c.d/e:f", "abcdef"),
    ("⏺ Bash(ls)\n  ⎿  a.txt", "bashlsatxt"),
    ("", ""),
];

#[test]
fn loose_has_the_same_cases_as_the_page() {
    for (text, want) in LOOSE_CASES {
        assert_eq!(loose(text), want, "{text}");
    }
}

fn at(found: Option<Found>) -> Option<(i64, usize, usize)> {
    found.map(|f| (f.n, f.start, f.size))
}

#[test]
fn text_across_a_wrap_and_lines_is_found_where_it_starts() {
    let rows = [(10, "$ make"), (11, "error: the retry loop ne"), (12, "ver resets its counter"), (13, "$ ")];
    assert_eq!(at(find(&rows, "retry loop never resets\nits counter", 3)), Some((11, 0, 30)));
}

#[test]
fn the_place_nearest_the_line_wins() {
    let rows: Vec<(i64, String)> = (0..100).map(|n| (n, if [5, 50, 95].contains(&n) { "$ npm test".into() } else { format!("out {n}") })).collect();
    assert_eq!(find(&rows, "$ npm test", 60).unwrap().n, 50);
    assert_eq!(find(&rows, "$ npm test", 90).unwrap().n, 95);
}

/// The sent history's strip (AC-90): many texts in one read, each nearest its hint, a message
/// without a hint nearest the end, one too short to tell or not there none.
#[test]
fn many_texts_each_nearest_its_hint() {
    let rows: Vec<(i64, String)> = (100..400).map(|n| (n, match n {
        110 | 300 => "> fix the tests please".to_string(),
        200 => "> and the lint too".to_string(),
        201 => "  of the web app".to_string(),
        _ => format!("out {n}"),
    })).collect();
    use crate::page::bookmarks_find::{find_each, Wanted};
    let w = |text: &str, hint: Option<i64>| Wanted { text: text.into(), hint, prompt: false };
    let texts = [w("fix the tests please", Some(120)), w("fix the tests please", Some(290)), w("fix the tests please", None),
        w("and the lint too\nof the web app", Some(5)), w("yes", Some(110)), w("never printed here", None)];
    assert_eq!(find_each(&rows, &texts), [Some(110), Some(300), Some(300), Some(200), None, None]);
    assert_eq!(find_each::<String>(&[], &texts[..1]), [None]);
}

/// An agent's prompt is found where its transcript shows it ("❯ " opens the row), not where an
/// answer or a tool's output quotes the same words, however near the hint.
#[test]
fn an_agents_prompt_on_its_prompt_row_only() {
    use crate::page::bookmarks_find::{find_each, Wanted};
    let rows: Vec<(i64, String)> = (0..400).map(|n| (n, match n {
        100 => "❯ закоммить и релизни".to_string(),
        300 => "     m:%2 11861 page 'закоммить и релизни'".to_string(),
        350 => "  ⎿  закоммить и релизни".to_string(),
        _ => format!("out {n}"),
    })).collect();
    let p = |hint| Wanted { text: "закоммить и релизни".into(), hint, prompt: true };
    assert_eq!(find_each(&rows, &[p(Some(310)), p(None), Wanted { prompt: false, ..p(Some(310)) }]), [Some(100), Some(100), Some(300)]);
    let gone = Wanted { text: "never asked here".into(), hint: None, prompt: true };
    assert_eq!(find_each(&rows, &[gone]), [None]);
}

#[test]
fn gone_text_is_none_and_text_without_letters_keeps_its_line() {
    let rows = [(1, "a"), (2, "b")];
    assert_eq!(find(&rows, "never printed", 1), None);
    assert_eq!(at(find(&rows, "──── ", 2)), Some((2, 0, 0)));
    assert_eq!(find(&rows, "──── ", 7), None);
}

#[test]
fn a_long_run_of_one_letter_is_quick() {
    let zeros = "0".repeat(200);
    let rows: Vec<(i64, &str)> = (0..10000).map(|n| (n, zeros.as_str())).collect();
    let t = Instant::now();
    assert_eq!(find(&rows, &"0".repeat(300), 5000).unwrap().n, 5000);
    assert_eq!(find(&rows, &"abc".repeat(30), 5000), None);
    // a quadratic search takes minutes; 10 s leaves room for a debug build beside 450 tests on a
    // busy machine (1.5 s seen under load, so 1 s failed now and then)
    assert!(t.elapsed().as_secs_f64() < 10.0, "{:?}", t.elapsed());
}

#[test]
fn the_part_found_nearest_the_line_wins() {
    let mut rows: Vec<(i64, &str)> = vec![(1, "for the install steps and more")];
    rows.extend((2..90).map(|n| (n, "x")));
    rows.push((90, "See [the guide](https://docs.example/guide) for the install steps and more"));
    let rendered = "See the guide for the install steps and more";
    assert_eq!(find(&rows, rendered, 90).unwrap().n, 90);
    assert_eq!(find(&rows, rendered, 3).unwrap().n, 1);
}

#[test]
fn rendered_markdown_is_found_by_its_start() {
    let rows = [(1, "See [the guide](https://docs.example/guide) for the install steps and more")];
    let rendered = "See the guide for the install steps and more";
    let f = find(&rows, rendered, 1).unwrap();
    assert_eq!((f.n, f.size), (1, 20));
    let part: String = loose(rendered).chars().skip(f.start).take(f.size).collect();
    assert!(loose(rows[0].1).contains(&part));
}

#[test]
fn offsets_count_characters_not_bytes() {
    let rows = [(1, "日本語 then"), (2, "the retry"), (3, "日本語")];
    assert_eq!(at(find(&rows, "the retry", 3)), Some((2, 0, 8)), "nearest by characters");
    assert_eq!(at(find(&rows, "日本語", 2)), Some((1, 0, 3)), "7 characters before, 8 after");
    let rows = [(1, "ab"), (2, "日本語日本語"), (3, "xxxxxxxxxx"), (4, "ab")];
    assert_eq!(find(&rows, "ab", 3).unwrap().n, 1, "8 characters before, 10 after (20 and 10 bytes)");
}

/// test_found_again_after_iterm2_wrapped_the_history_anew and test_gone_lines: the stream reads
/// the lines near the bookmark's, then the whole history when its whole print is not there.
#[test]
fn found_again_after_the_history_was_numbered_anew_or_gone() {
    let text = "the retry loop never resets";
    let mut lines: Vec<String> = (0..70).map(|k| format!("l{k}")).collect();
    lines.extend(["the retry loop ne".into(), "ver resets".into(), "$ ".into()]);
    let (a, b) = near(0, lines.len() as i64, 40);
    let rows: Vec<(i64, &str)> = (a..b).map(|n| (n, lines[n as usize].as_str())).collect();
    let found = find(&rows, text, 40);
    assert!(!needs_whole(found.as_ref(), text));
    assert_eq!(located("k3", "tmux:local:terminum:%1", found.as_ref()),
        json!({"t": "located", "id": "k3", "sid": "tmux:local:terminum:%1", "n": 70, "start": 0, "size": 23}));
    assert_eq!(near(0, 5000, 40), (0, 1040));
    assert_eq!(near(0, 5000, 3000), (2000, 4000));
    let gone = find(&[(0, "$ clear"), (1, "$ ")], "the retry loop", 40);
    assert!(needs_whole(gone.as_ref(), text));
    assert_eq!(located("k3", "s", gone.as_ref())["why"], GONE);
    assert!(needs_whole(Some(&Found { n: 1, start: 0, size: 20 }), &"x".repeat(30)), "a part only");
}

static DIRS: AtomicUsize = AtomicUsize::new(0);

/// A private folder of its own for one test, removed when it ends.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        let k = DIRS.fetch_add(1, Ordering::SeqCst);
        let p = crate::testutil::temp_root().join(format!("tm-marks-{}-{k}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Dir(p)
    }

    fn file(&self) -> PathBuf {
        self.0.join("web-bookmarks.json")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn mark(key: &str, n: i64, text: &str, note: &str) -> NewMark {
    NewMark { key: Some(key.into()), n, text: text.into(), note: note.into(), title: "api refactor".into(), ..NewMark::default() }
}

#[test]
fn kept_across_a_restart_readable_by_the_user_only() {
    let d = Dir::new();
    let mut s = Store::load(d.file());
    let a = s.add(mark("tmux:local:terminum:%1", 4, "x", "n")).unwrap();
    s.set_note(&a.id, "later".into(), None).unwrap();
    let b = s.add(NewMark { host: Some("devbox.example".into()), created: Some(1792000000), ..mark("tmux:devbox.example:default:%2", 9, "y", "") }).unwrap();
    assert!(s.remove(&b.id).unwrap());
    assert!(!s.remove(&b.id).unwrap(), "gone already");
    assert_eq!(fs::metadata(d.file()).unwrap().permissions().mode() & 0o777, 0o600);
    let again = Store::load(d.file());
    let kept: Vec<(&str, &str, Option<&str>)> = again.all().iter().map(|i| (i.id.as_str(), i.note.as_str(), i.key.as_deref())).collect();
    assert_eq!(kept, [(a.id.as_str(), "later", Some("tmux:local:terminum:%1"))]);
    assert_eq!(again.all()[0], Item { note: "later".into(), ..a.clone() });
    // one made on a full-screen program's screen keeps saying so; others write no `alt`
    let c = s.add(NewMark { alt: true, ..mark("tmux:local:terminum:%1", 5, "z", "") }).unwrap();
    assert!(Store::load(d.file()).get(&c.id).is_some_and(|i| i.alt) && c.wire()["alt"] == true && a.wire().get("alt").is_none());
    assert!(s.remove(&c.id).unwrap());
    let data: Value = serde_json::from_str(&fs::read_to_string(d.file()).unwrap()).unwrap();
    assert_eq!(data["version"], 4);
    assert_eq!(a.id.len(), 12);
    assert!(a.id.chars().all(|c| c.is_ascii_alphanumeric()));
    assert_eq!(a.wire()["sid"], "tmux:local:terminum:%1");
    assert_eq!(fs::read_dir(&d.0).unwrap().count(), 1, "no temporary file is left");
}

#[test]
fn a_version_1_file_is_read() {
    let d = Dir::new();
    let v1 = json!({"version": 1, "items": [
        {"id": "AbC_-12", "sid": "w0t0p0:1", "key": "tmux:devbox.example:/tmp/s:%7", "n": "12", "text": "x".repeat(2100),
         "note": "ask", "title": "api", "host": "devbox.example", "at": 1792003600},
        {"id": 7, "sid": "w0t0p1:2", "key": null, "n": 3.9, "text": "y", "host": ""}]});
    fs::write(d.file(), v1.to_string()).unwrap();
    let s = Store::load(d.file());
    assert_eq!(s.broken(), None);
    let a = &s.all()[0];
    assert_eq!((a.id.as_str(), a.key.as_deref(), a.created, a.n, a.text.chars().count(), a.at),
        ("AbC_-12", Some("tmux:devbox.example:/tmp/s:%7"), None, 12, MAX_TEXT, 1792003600));
    let b = &s.all()[1];
    assert_eq!((b.id.as_str(), b.key.as_deref(), b.n, b.note.as_str(), b.title.as_str(), b.host.as_deref(), b.at), ("7", None, 3, "", "", None, 0));
}

#[test]
fn an_unreadable_file_is_left_as_it_is_and_saves_say_why() {
    let d = Dir::new();
    fs::write(d.file(), "{not json").unwrap();
    let mut s = Store::load(d.file());
    assert!(s.broken().unwrap().contains("cannot be read"), "{:?}", s.broken());
    let e = s.add(mark("k", 1, "x", "")).unwrap_err();
    assert!(e.0.contains("move it away"), "{e}");
    assert_eq!(fs::read_to_string(d.file()).unwrap(), "{not json");
    fs::write(d.file(), r#"{"version": 2, "items": [{"id": "a", "text": "x"}]}"#).unwrap();
    assert!(Store::load(d.file()).broken().unwrap().contains("\"n\""), "an item without its line");
}

#[test]
fn the_cap_and_a_failed_write_keep_nothing_half_saved() {
    let d = Dir::new();
    let mut s = Store::load(d.file());
    for k in 0..MAX_ITEMS {
        s.items.push(Item { id: k.to_string(), ..Item::default() });
    }
    assert!(s.add(mark("k", 1, "x", "")).unwrap_err().0.contains("at most"));
    let mut s = Store::load(d.file());
    let a = s.add(mark("k", 1, "x", "")).unwrap();
    fs::set_permissions(&d.0, fs::Permissions::from_mode(0o500)).unwrap(); // the write fails
    let e = s.add(mark("k", 2, "y", "")).unwrap_err();
    assert!(e.0.starts_with("Not saved: ") && !e.0.contains("os error"), "{e}");
    assert!(s.set_note(&a.id, "z".into(), None).is_err());
    assert!(s.remove(&a.id).is_err());
    assert_eq!(s.all(), std::slice::from_ref(&a));
    fs::set_permissions(&d.0, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(s.set_note("gone", "z".into(), None).unwrap_err().0, DELETED);
    assert_eq!(Store::load(d.file()).all(), [a]);
}

#[test]
fn a_mark_without_text_or_with_a_long_note_is_refused_with_why() {
    let why = |m: Value| mark_of(&m).unwrap_err().0;
    assert!(why(json!({"n": 1, "text": "  ", "note": ""})).contains("Select some text"));
    assert!(why(json!({"n": 1, "text": "x", "note": "y".repeat(1001)})).contains("longer than"));
    assert!(why(json!({"n": 1, "text": "  ", "note": "y".repeat(1001)})).contains("longer than"), "the note is checked first");
    assert!(why(json!({"n": 1, "text": "x", "note": null})).contains("must be text"));
    assert!(why(json!({"text": "x"})).contains("line is not known"));
    assert!(why(json!({"n": "x1", "text": "x"})).contains("line is not known"));
    let (text, note, n) = mark_of(&json!({"n": "40", "text": "z".repeat(2100), "note": " ask "})).unwrap();
    assert_eq!((text.chars().count(), note.as_str(), n), (MAX_TEXT, "ask", 40));
    assert_eq!(note_of(&json!({})).unwrap(), "");
}

#[test]
fn a_save_sent_again_is_kept_once() {
    let mut s = Store::memory();
    let a = s.add(mark("k", 40, "the retry loop", "")).unwrap();
    s.remember("same", &a.id);
    assert_eq!(s.ref_id("same"), Some(a.id.as_str()));
    for k in 0..REFS {
        s.remember(&format!("r{k}"), "x");
    }
    assert_eq!(s.ref_id("same"), None, "the oldest of more than REFS go");
    assert_eq!(s.ref_id("r99"), Some("x"));
}

#[test]
fn a_reminder_comes_up_at_each_turns_end_until_resolved() {
    let d = Dir::new();
    let mut s = Store::load(d.file());
    let k = "tmux:local:terminum:%1";
    let r = s.add(NewMark { remind: true, ..mark(k, 4, "the retry loop", "check it") }).unwrap();
    let b = s.add(mark(k, 5, "a bookmark", "")).unwrap();
    s.add(NewMark { remind: true, ..mark("tmux:local:terminum:%2", 1, "other", "") }).unwrap();
    let up = s.due(&ended(k)).unwrap();
    assert_eq!(up.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), [r.id.as_str()], "its pane's reminders, not a bookmark or another pane's");
    assert!(s.due(&ended(k)).unwrap().is_empty(), "up already");
    assert!(Store::load(d.file()).get(&r.id).unwrap().due, "kept across a restart");
    assert!(!s.settle(&r.id, false).unwrap().due, "closed");
    assert_eq!(s.due(&ended(k)).unwrap().len(), 1, "up again at the next turn's end");
    let done = s.settle(&r.id, true).unwrap();
    assert!(done.resolved.is_some() && !done.due);
    assert!(s.due(&ended(k)).unwrap().is_empty(), "resolved: never again");
    let made = s.set_note(&b.id, "now".into(), Some(true)).unwrap();
    assert!(made.remind && made.note == "now", "a bookmark made a reminder, its note changed, saved once");
    assert!(s.set_note(&r.id, String::new(), Some(true)).unwrap().resolved.is_some(), "the same kind: as it was");
    s.set_note(&r.id, String::new(), Some(false)).unwrap();
    assert!(s.set_note(&r.id, String::new(), Some(true)).unwrap().resolved.is_none(), "made a reminder again: open");
    let wire = b.wire();
    assert!(wire.get("remind").is_none() && wire.get("due").is_none(), "a bookmark's file entry is as before");
}

/// Pane `k` ended a turn.
fn ended(k: &str) -> Vec<String> {
    vec![k.to_string()]
}

#[test]
fn a_reminder_comes_up_only_when_its_own_session_ends_a_turn_whatever_its_project() {
    let d = Dir::new();
    let mut s = Store::load(d.file());
    let r = s.add(NewMark { remind: true, project: Some("p1".into()), ..mark("tmux:local:terminum:%1", 4, "retry", "") }).unwrap();
    let other = s.add(NewMark { remind: true, project: Some("p2".into()), ..mark("tmux:local:terminum:%1", 5, "css", "") }).unwrap();
    let own = s.add(NewMark { remind: true, ..mark("tmux:local:terminum:%2", 6, "own", "") }).unwrap();
    assert!(s.due(&ended("tmux:local:terminum:%7")).unwrap().is_empty(), "another session of the project: not shown there, not up");
    let up = s.due(&ended("tmux:local:terminum:%1")).unwrap();
    assert_eq!(up.len(), 2, "its own, of any project");
    assert!(up.iter().any(|i| i.id == r.id) && up.iter().any(|i| i.id == other.id));
    assert_eq!(s.due(&ended("tmux:local:terminum:%2")).unwrap()[0].id, own.id);
    assert_eq!(Store::load(d.file()).get(&r.id).unwrap().project.as_deref(), Some("p1"), "kept");
}

#[test]
fn an_orphaned_reminder_never_comes_up_and_stays_orphaned() {
    let d = Dir::new();
    let mut s = Store::load(d.file());
    let r = s.add(NewMark { remind: true, project: Some("p1".into()), ..mark("tmux:local:terminum:%1", 4, "retry", "") }).unwrap();
    let b = s.add(mark("tmux:local:terminum:%2", 5, "own", "")).unwrap();
    let keep = s.add(NewMark { project: Some("p2".into()), ..mark("tmux:local:terminum:%3", 6, "kept", "") }).unwrap();
    let lost = super::lost(s.all(), |k| k == "tmux:local:terminum:%1" || k == "tmux:local:terminum:%2");
    assert_eq!(lost, [r.id.clone(), b.id.clone()], "its session closed, in a project or not; an open session keeps its own, its project gone or not");
    assert_eq!(s.orphan(&lost).unwrap().len(), 2);
    assert!(super::lost(s.all(), |k| k == "tmux:local:terminum:%2").is_empty(), "named once");
    assert!(s.orphan(&lost).unwrap().is_empty(), "orphaned already: unchanged");
    assert!(s.due(&ended("tmux:local:terminum:%1")).unwrap().is_empty());
    let again = Store::load(d.file());
    assert!(again.get(&r.id).unwrap().orphaned.is_some() && again.get(&keep.id).unwrap().orphaned.is_none(), "kept across a restart");
    assert!(again.get(&b.id).unwrap().wire()["orphaned"].is_i64());
}

#[test]
fn a_file_before_version_4_orphans_a_projects_record_whose_pane_is_not_of_that_project_once() {
    let d = Dir::new();
    let item = |id: &str, key: &str, project: Option<&str>| json!({"id": id, "key": key, "n": 1, "text": "t", "note": "", "title": "t", "at": 1, "project": project});
    std::fs::write(d.file(), json!({"version": 3, "items": [item("kept", "tmux:local:t:%1", Some("p1")), item("stale", "tmux:local:t:%5", Some("p1")),
        item("other", "tmux:local:t:%2", Some("p1")), item("gone", "tmux:local:t:%7", Some("p9")), item("own", "tmux:local:t:%8", None)]}).to_string()).unwrap();
    let projects: std::collections::HashMap<String, String> = [("tmux:local:t:%1", "p1"), ("tmux:local:t:%2", "p2"), ("tmux:local:t:%5", "p2")].map(|(k, p)| (k.to_string(), p.to_string())).into();
    let mut s = Store::load(d.file());
    s.take_sessions(&projects).unwrap();
    let orphaned = |s: &Store, id: &str| s.get(id).unwrap().orphaned.is_some();
    assert!(!orphaned(&s, "kept") && !orphaned(&s, "own"), "a member of its project; outside projects: its own rule");
    assert!(orphaned(&s, "stale") && orphaned(&s, "other") && orphaned(&s, "gone"), "a pane of another project now, or a project gone: never shown there");
    let again = Store::load(d.file());
    assert!(orphaned(&again, "stale") && !again.unscoped, "kept, and the file is of version 4");
    std::fs::write(d.file(), json!({"version": 4, "items": [item("late", "tmux:local:t:%5", Some("p1"))]}).to_string()).unwrap();
    let mut late = Store::load(d.file());
    late.take_sessions(&projects).unwrap();
    assert!(!orphaned(&late, "late"), "version 4: a session moved to another project keeps its own");
}

#[test]
fn a_restored_server_keeps_its_bookmarks() {
    let d = Dir::new();
    let mut s = Store::load(d.file());
    let a = s.add(mark("tmux:local:terminum:%55", 1, "a", "")).unwrap();
    let gone = s.add(mark("tmux:local:terminum:%56", 2, "b", "")).unwrap();
    let host = s.add(mark("tmux:devbox:default:%55", 3, "c", "")).unwrap();
    let project = s.add(NewMark { project: Some("p1".into()), ..mark("tmux:local:terminum:%57", 4, "d", "") }).unwrap();
    let moved = s.restarted("tmux:local:terminum:", &[("%55".into(), "%3".into())]).unwrap();
    assert_eq!(moved.len(), 3);
    assert!(s.get(&project.id).unwrap().orphaned.is_some(), "a project's too: a restored pane may take its id, and shows only its own");
    assert_eq!(s.get(&a.id).unwrap().key.as_deref(), Some("tmux:local:terminum:%3"), "its pane's new key");
    assert!(s.get(&gone.id).unwrap().orphaned.is_some(), "its pane not made again");
    assert_eq!(s.get(&host.id).unwrap().key.as_deref(), Some("tmux:devbox:default:%55"), "another server's");
    assert!(s.restarted("tmux:local:terminum:", &[("%3".into(), "%3".into()), ("%56".into(), "%9".into())]).unwrap().is_empty(), "an orphaned one keeps its key; a kept pane (paired with itself) its own");
    assert_eq!(s.get(&gone.id).unwrap().key.as_deref(), Some("tmux:local:terminum:%56"));
}

#[test]
fn a_file_before_version_3_takes_its_sessions_projects_once() {
    let d = Dir::new();
    let v2 = json!({"version": 2, "items": [
        {"id": "a", "key": "tmux:local:terminum:%5", "n": 1, "text": "x", "dir": "/Users/alex/api"},
        {"id": "b", "key": "tmux:local:terminum:%6", "n": 2, "text": "y", "dir": "/Users/alex/api"}]});
    fs::write(d.file(), v2.to_string()).unwrap();
    let members = std::collections::HashMap::from([("tmux:local:terminum:%5".to_string(), "p1".to_string())]);
    let mut s = Store::load(d.file());
    s.take_projects(&members).unwrap();
    assert_eq!((s.get("a").unwrap().project.as_deref(), s.get("b").unwrap().project.as_deref()), (Some("p1"), None), "in a project now, or its session's");
    let mut again = Store::load(d.file());
    again.take_projects(&std::collections::HashMap::from([("tmux:local:terminum:%6".to_string(), "p2".to_string())])).unwrap();
    assert_eq!(again.get("b").unwrap().project, None, "version 3: taken once");
}
