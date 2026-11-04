// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;

fn of(text: &str) -> (Vec<(String, usize)>, usize) {
    let mut f = Found::default();
    f.text(text);
    let (found, total) = f.told(false);
    (found.into_iter().map(|l| (l.url, l.count)).collect(), total)
}

fn l(u: &str, count: usize) -> (String, usize) {
    (u.into(), count)
}

/// AC-102: named twice, once with a final "."; the newest first.
#[test]
fn lists_each_address_once_newest_first() {
    let text = "run https://ci.example/run/7\ndocs: https://docs.example/a.\nagain https://ci.example/run/7\n";
    assert_eq!(of(text), (vec![l("https://ci.example/run/7", 2), l("https://docs.example/a", 1)], 2));
}

/// AC-102 edge: not a link: another scheme, a word before "http", no host, too long, a control character.
#[test]
fn what_is_not_a_link() {
    let long = format!("https://x.example/{}", "y".repeat(2048));
    let text = format!("javascript:alert(1) xhttps://a.example/ https:// {long} ftp://f.example/ (http://b.example/) https://c.example/\x07x\n");
    assert_eq!(of(&text), (vec![l("http://b.example/", 1)], 1));
}

/// AC-102 limit: the newest MAX_LINKS are told, the count says how many there are.
#[test]
fn newest_are_told_past_limit() {
    let text: String = (0..MAX_LINKS + 3).map(|k| format!("https://x.example/{k}\n")).collect();
    let (found, total) = of(&text);
    assert_eq!((found.len(), total), (MAX_LINKS, MAX_LINKS + 3));
    assert_eq!(found[0].0, format!("https://x.example/{}", MAX_LINKS + 2));
}

#[test]
fn answer_and_refusal_shapes() {
    let mut f = Found::default();
    f.text("https://a.example/");
    let (found, total) = f.told(false);
    assert_eq!(answer("r", "tmux:local:terminum:%1", &found, total),
        json!({"t": "linklist", "ref": "r", "sid": "tmux:local:terminum:%1", "items": [{"u": "https://a.example/", "c": 1, "w": null}], "total": 1}));
    assert_eq!(refused("e", "No session is shown."), json!({"t": "linklist", "ref": "e", "error": "No session is shown."}));
    assert_eq!(of(""), (vec![], 0));
}

/// AC-102 limit: past a quarter over KEPT_LINKS distinct addresses, the last named KEPT_LINKS are kept, with their counts.
#[test]
fn keeps_last_named_past_limit() {
    let mut f = Found::default();
    f.text("https://x.example/0");
    for k in 1..=KEPT_LINKS + KEPT_LINKS / 4 {
        f.text(&format!("https://x.example/{k}"));
    }
    f.text("https://x.example/1250 https://x.example/1251");
    let (found, total) = f.told(true);
    assert_eq!(total, KEPT_LINKS + 1, "1250 left out, then named again");
    assert_eq!((found[0].url.as_str(), found[0].count), ("https://x.example/1251", 2));
    let (found, _) = f.told(false);
    assert!(!found.iter().any(|l| l.url == "https://x.example/0"));
}

/// AC-102 edge: an address after other scripts' text; one in a word is none.
#[test]
fn address_after_other_text() {
    assert_eq!(of("ссылка: https://a.example/п. éhttps://b.example/ _https://c.example/").0, vec![l("https://a.example/п", 1)]);
}

/// AC-102: the session found last is kept while the pane's process is the same and its record
/// still names it; another pane process (a new pane with an old one's id) or a record of another
/// session looks again.
#[test]
fn session_found_last_while_same_process() {
    let d = std::env::temp_dir().join(format!("tm-linklist-still-{}", std::process::id()));
    std::fs::create_dir_all(d.join("sessions")).unwrap();
    let me = std::process::id();
    let Some(started) = super::super::agentlinks::proc_start(me) else { return };   // a system that says no start
    std::fs::write(d.join(format!("sessions/{me}.json")), json!({"sessionId": "s-1", "procStart": started}).to_string()).unwrap();
    let last = |s: &str| Some((7, me, s.to_string()));
    assert_eq!(still(&d, last("s-1"), 7).as_deref(), Some("s-1"));
    assert_eq!(still(&d, last("s-1"), 8), None, "another pane process");
    assert_eq!(still(&d, last("s-0"), 7), None, "its record names another session");
    assert_eq!(still(&d, None, 7), None);
    let _ = std::fs::remove_dir_all(&d);
}
