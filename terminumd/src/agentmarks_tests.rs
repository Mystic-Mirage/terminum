// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use serde_json::json;

use super::*;

const K: &str = "tmux:local:terminum:%1";

fn here() -> Place {
    Place { key: K.into(), title: "api".into(), dir: Some("/Users/alex/p".into()), ..Place::default() }
}

fn rec(title: &str, remind: bool) -> Record {
    Record { title: title.into(), text: format!("{title}: what, where, why, next step"), remind, kind: Some(if remind { "question" } else { "bug" }.into()), file: None }
}

fn ids(told: &[Told]) -> Vec<String> {
    told.iter().filter_map(|t| match t { Told::Mark(i) => Some(i.id.clone()), Told::Unmark(id) => Some(id.clone()), Told::Full(_) => None }).collect()
}

#[test]
fn records_are_the_agents_bookmarks_and_reminders_of_its_pane() {
    let mut s = Store::memory();
    let (said, told) = add(&mut s, &here(), "claude", vec![rec("Final report: retry fixed", false), rec("Decide the API name", true)], &[]).unwrap();
    let [a, b] = [&s.all()[0], &s.all()[1]];
    assert_eq!((a.by.as_deref(), a.note.as_str(), a.remind, a.n), (Some("claude"), "Final report: retry fixed", false, -1));
    assert_eq!((a.key.as_deref(), a.title.as_str(), a.dir.as_deref(), a.kind.as_deref()), (Some(K), "api", Some("/Users/alex/p"), Some("bug")));
    assert!(b.remind && b.resolved.is_none());
    assert_eq!(ids(&told), [a.id.clone(), b.id.clone()]);
    assert!(said.contains(&format!("bookmark {} \"Final report: retry fixed\"", a.id)), "{said}");
    assert!(said.contains(&format!("reminder {} \"Decide the API name\" ({COMES_UP})", b.id)), "{said}");
    let (_, _) = add(&mut s, &here(), "gemini", vec![rec("Other", false)], &[]).unwrap();
    assert_eq!(s.all()[2].by.as_deref(), Some("agent"), "an agent terminum does not know by name");
}

#[test]
fn a_repeat_is_not_kept_twice_and_its_text_is_cleaned() {
    let mut s = Store::memory();
    add(&mut s, &here(), "claude", vec![rec("Next step: paging", false)], &[]).unwrap();
    let (said, told) = add(&mut s, &here(), "claude", vec![rec("next STEP: paging", false), rec("Next step: paging", false)], &[]).unwrap();
    assert!(told.is_empty() && s.all().len() == 1 && said.contains("Not kept, your text was not stored"), "{said}");
    let mut r = rec("Key \u{202E}left", false);
    r.text = format!("token {}-api03-abcdefghijklmnopqrstuvwxyz0123456789 \u{7} here", ["sk", "ant"].join("-"));   // a key's shape, built: no key in the source
    add(&mut s, &here(), "claude", vec![r], &[]).unwrap();
    let it = &s.all()[1];
    assert_eq!(it.note, "Key left");
    assert!(!it.text.contains("abcdefghijklmnop") && !it.text.contains('\u{7}'), "{}", it.text);
}

#[test]
fn a_repeat_and_an_update_are_judged_in_the_pane_outside_its_projects_members() {
    let mut s = Store::memory();
    let a = Place { project: Some("p1".into()), ..here() };
    let b = Place { key: "tmux:local:terminum:%2".into(), project: Some("p1".into()), ..here() };
    add(&mut s, &a, "claude", vec![rec("Final report", false)], &[]).unwrap();
    let (said, _) = add(&mut s, &b, "claude", vec![rec("Final report", false)], &[]).unwrap();
    assert!(s.all().len() == 2 && !said.contains("Not kept"), "no member of the project: {said}");
    assert_eq!(s.all()[1].key.as_deref(), Some("tmux:local:terminum:%2"));
    let first = s.all()[0].id.clone();
    assert!(update(&mut s, &b, &json!({"id": first, "status": "done"})).unwrap_err().contains("No record"), "another session's, not a member");
    assert!(update(&mut s, &a, &json!({"id": first, "status": "done"})).is_ok());
}

#[test]
fn a_pane_reads_and_deletes_every_record_of_its_own_and_none_of_another() {
    let mut s = Store::memory();
    let a = Place { project: Some("p1".into()), ..here() };
    let b = Place { key: "tmux:local:terminum:%2".into(), project: Some("p1".into()), ..here() };
    add(&mut s, &a, "claude", vec![rec("Ours", false), rec("Ours too", true)], &[]).unwrap();
    add(&mut s, &b, "claude", vec![rec("Theirs", true)], &[]).unwrap();
    let user = s.add(crate::page::bookmarks::NewMark { key: Some(K.into()), n: 3, text: "selected line\nsecond".into(), project: Some("p1".into()), ..Default::default() }).unwrap();
    let (ours, ours2, theirs) = (s.all()[0].id.clone(), s.all()[1].id.clone(), s.all()[2].id.clone());

    let list: Value = serde_json::from_str(&read(&s, &a, &json!({})).unwrap()).unwrap();
    let got: Vec<(&str, &str, &str, &str)> = list.as_array().unwrap().iter().map(|r| (r["id"].as_str().unwrap(), r["type"].as_str().unwrap(), r["by"].as_str().unwrap(), r["title"].as_str().unwrap())).collect();
    assert_eq!(got, [(ours.as_str(), "bookmark", "claude", "Ours"), (ours2.as_str(), "reminder", "claude", "Ours too"), (user.id.as_str(), "bookmark", "user", "selected line")], "its agent's and the user's, not another session's of its project");
    assert!(list[0].get("text").is_none(), "the list has no text");
    let whole: Value = serde_json::from_str(&read(&s, &a, &json!({"id": ours})).unwrap()).unwrap();
    assert!(whole["text"].as_str().unwrap().starts_with("Ours:"));
    assert!(read(&s, &a, &json!({"id": theirs})).unwrap_err().contains("No record"), "another session's: not read");

    let err = delete(&mut s, &a, &json!({"ids": [ours, theirs]})).unwrap_err();
    assert!(err.contains("No record") && s.get(&ours).is_some() && s.get(&theirs).is_some(), "one not its own: none deleted");
    let (said, told) = delete(&mut s, &a, &json!({"ids": [ours2, user.id]})).unwrap();
    assert!(s.get(&ours2).is_none() && s.get(&user.id).is_none() && told.contains(&Told::Unmark(ours2.clone())) && said.starts_with("Deleted:"), "a reminder and the user's bookmark gone for good");
    assert_eq!(s.all().len(), 2);

    let orphaned = s.orphan(std::slice::from_ref(&ours)).unwrap();
    assert_eq!(orphaned.len(), 1);
    assert!(delete(&mut s, &a, &json!({"ids": [ours]})).unwrap_err().contains("No record"), "orphaned: no pane's");
    assert!(read(&s, &a, &json!({})).unwrap().contains("\"closed\":true"), "read as its project's closed session's");
    assert!(read(&s, &Place { project: None, ..a }, &json!({})).unwrap() == "[]", "and by nobody outside a project");
}

#[test]
fn what_a_pane_reads_of_the_users_record_is_cleaned_and_redacted() {
    let mut s = Store::memory();
    let key = format!("token {}-api03-abcdefghijklmnopqrstuvwxyz0123456789 \u{7} here", ["sk", "ant"].join("-"));
    let user = s.add(crate::page::bookmarks::NewMark { key: Some(K.into()), n: 3, text: key, ..Default::default() }).unwrap();
    let whole = read(&s, &here(), &json!({"id": user.id})).unwrap();
    assert!(!whole.contains("abcdefghijklmnop") && !whole.contains("\\u0007"), "{whole}");
    let list = read(&s, &here(), &json!({})).unwrap();
    assert!(!list.contains("abcdefghijklmnop"), "{list}");
}

#[test]
fn an_agent_turning_the_users_bookmark_into_a_reminder_keeps_to_the_users_limit() {
    let mut s = Store::memory();
    let new = |n: i64, remind: bool| crate::page::bookmarks::NewMark { key: Some(K.into()), n, text: format!("t{n}"), remind, ..Default::default() };
    let one = s.add(new(0, false)).unwrap();
    s.add_all((1..=crate::page::bookmarks::MAX_REMINDERS as i64).map(|n| new(n, true)).collect()).unwrap();
    let why = update(&mut s, &here(), &json!({"id": one.id, "remind": true})).unwrap_err();
    assert!(why.contains("open reminders are kept at most") && !s.get(&one.id).unwrap().remind, "{why}");
}

#[test]
fn past_a_kinds_limit_the_oldest_of_that_kind_in_the_scope_go() {
    let mut s = Store::memory();
    for i in 0..BOOKMARKS_MOST {
        add(&mut s, &here(), "claude", vec![rec(&format!("b{i}"), false)], &[]).unwrap();
    }
    for i in 0..REMINDERS_MOST {
        add(&mut s, &here(), "claude", vec![rec(&format!("r{i}"), true)], &[]).unwrap();
    }
    let (b0, r0, r1) = (s.all()[0].id.clone(), s.all()[BOOKMARKS_MOST].id.clone(), s.all()[BOOKMARKS_MOST + 1].id.clone());
    let (said, told) = add(&mut s, &here(), "claude", vec![rec("one more", false)], &[]).unwrap();
    assert!(s.get(&b0).is_none() && s.all().len() == BOOKMARKS_MOST + REMINDERS_MOST, "the oldest bookmark went, no reminder");
    assert!(told.contains(&Told::Unmark(b0.clone())) && told.contains(&Told::Full(false)) && !told.contains(&Told::Full(true)));
    assert!(said.contains(&format!("these oldest records were deleted: {b0}")), "{said}");
    let (_, told) = add(&mut s, &here(), "claude", vec![rec("one more reminder", true)], &[]).unwrap();
    assert!(s.get(&r0).is_none() && told.contains(&Told::Full(true)) && !told.contains(&Told::Full(false)));
    // a resolved reminder counts in no limit: resolving one makes room
    update(&mut s, &here(), &json!({"id": r1, "status": "done"})).unwrap();
    let (said, told) = add(&mut s, &here(), "claude", vec![rec("room now", true)], &[]).unwrap();
    assert!(!told.iter().any(|t| matches!(t, Told::Unmark(_) | Told::Full(_))) && !said.contains("deleted"), "{said}");
    assert!(s.get(&r1).is_some(), "a resolved reminder is not deleted for room");
    // another scope's records are its own
    let other = Place { key: "tmux:local:terminum:%2".into(), ..Place::default() };
    let (_, told) = add(&mut s, &other, "claude", vec![rec("elsewhere", false)], &[]).unwrap();
    assert!(!told.iter().any(|t| matches!(t, Told::Unmark(_) | Told::Full(_))));
    let e = add(&mut s, &here(), "claude", vec![rec("", false)], &[]).unwrap_err();
    assert!(e.contains("title is empty"), "{e}");
}

#[test]
fn past_all_records_only_orphaned_ones_go_and_the_users_never() {
    let mut s = Store::memory();
    let user = crate::page::bookmarks::NewMark { key: Some(K.into()), n: 3, text: "x".into(), ..Default::default() };
    let mine = s.add(user.clone()).unwrap().id;
    for scope in 0..ALL_MOST / BOOKMARKS_MOST {
        let at = Place { key: format!("tmux:local:terminum:%{}", scope + 10), ..Place::default() };
        for i in 0..BOOKMARKS_MOST {
            add(&mut s, &at, "claude", vec![rec(&format!("s{scope} b{i}"), false)], &[]).unwrap();
        }
    }
    // no orphaned record: a live scope loses only its own past its limit
    let (_, told) = add(&mut s, &here(), "claude", vec![rec("the 501st", false)], &[]).unwrap();
    assert!(!told.iter().any(|t| matches!(t, Told::Unmark(_) | Told::Full(_))));
    let orphans: Vec<String> = s.all().iter().filter(|i| i.key.as_deref() == Some("tmux:local:terminum:%10")).map(|i| i.id.clone()).collect();
    s.orphan(&orphans).unwrap();
    let (said, told) = add(&mut s, &here(), "claude", vec![rec("the 502nd", false), rec("the 503rd", false)], &[]).unwrap();
    let gone: Vec<&String> = told.iter().filter_map(|t| if let Told::Unmark(id) = t { Some(id) } else { None }).collect();
    assert_eq!(gone, orphans.iter().take(3).collect::<Vec<_>>(), "the oldest orphaned go, to 500");
    assert!(!told.contains(&Told::Full(false)) && said.contains(orphans[0].as_str()), "the answer names the ids; no banner for orphans");
    assert!(s.get(&mine).is_some(), "the user's bookmark is never deleted for room");
    // the user's limits stand apart: 300 bookmarks, 100 open reminders
    for _ in 1..crate::page::bookmarks::MAX_ITEMS {
        s.add(user.clone()).unwrap();
    }
    assert!(s.add(user.clone()).unwrap_err().to_string().contains("300 bookmarks"));
    let reminder = crate::page::bookmarks::NewMark { remind: true, ..user };
    for _ in 0..crate::page::bookmarks::MAX_REMINDERS {
        s.add(reminder.clone()).unwrap();
    }
    assert!(s.add(reminder).unwrap_err().to_string().contains("100 open reminders"));
    assert!(add(&mut s, &here(), "claude", vec![rec("still", false)], &[]).is_ok(), "an agent's record past the user's limits");
}

#[test]
fn room_is_made_in_the_records_own_pane_and_after_a_reopen() {
    let mut s = Store::memory();
    let other = Place { key: "tmux:local:terminum:%2".into(), project: Some("p1".into()), ..here() };
    add(&mut s, &other, "claude", vec![rec("another session of the project", true)], &[]).unwrap();
    let early = s.all()[0].id.clone();
    let in_project = Place { project: Some("p1".into()), ..here() };
    for i in 0..REMINDERS_MOST {
        add(&mut s, &in_project, "claude", vec![rec(&format!("r{i}"), true)], &[]).unwrap();
    }
    let first = s.all()[1].id.clone();
    let (_, told) = add(&mut s, &in_project, "claude", vec![rec("one more", true)], &[]).unwrap();
    assert!(s.get(&early).is_some(), "another session's is never deleted for this one's room, of its project too");
    assert!(s.get(&first).is_none() && told.contains(&Told::Full(true)));
    // a resolved one reopened: the oldest other open one makes room, never it
    let oldest_open = s.all().iter().find(|i| i.key.as_deref() == Some(K)).unwrap().id.clone();
    let reopened = s.all().iter().rev().find(|i| i.key.as_deref() == Some(K)).unwrap().id.clone();
    update(&mut s, &in_project, &json!({"id": reopened, "status": "done"})).unwrap();
    add(&mut s, &in_project, "claude", vec![rec("fills it again", true)], &[]).unwrap();
    let (said, told) = update(&mut s, &in_project, &json!({"id": reopened, "status": "open"})).unwrap();
    assert!(s.get(&reopened).is_some_and(|i| i.resolved.is_none()) && s.get(&oldest_open).is_none(), "{said}");
    assert!(told.contains(&Told::Unmark(oldest_open)) && told.contains(&Told::Full(true)));
}

#[test]
fn update_resolves_deletes_and_edits_only_an_agents_record_of_the_scope() {
    let mut s = Store::memory();
    add(&mut s, &here(), "codex", vec![rec("Remind me", true), rec("A finding", false)], &[]).unwrap();
    let (r, b) = (s.all()[0].id.clone(), s.all()[1].id.clone());
    let (said, told) = update(&mut s, &here(), &json!({"id": r, "status": "done"})).unwrap();
    assert!(said.ends_with("updated (resolved).") && s.get(&r).unwrap().resolved.is_some() && ids(&told) == [r.clone()], "{said}");
    update(&mut s, &here(), &json!({"id": r, "status": "open", "title": "Remind me again"})).unwrap();
    assert_eq!((s.get(&r).unwrap().resolved, s.get(&r).unwrap().note.as_str()), (None, "Remind me again"));
    let (said, told) = update(&mut s, &here(), &json!({"id": b, "status": "done"})).unwrap();
    assert!(s.get(&b).is_none() && told == [Told::Unmark(b.clone())], "{said}");
    let other = Place { key: "tmux:local:terminum:%2".into(), ..Place::default() };
    assert!(update(&mut s, &other, &json!({"id": r, "status": "done"})).unwrap_err().contains("No record"));
    let user = s.add(crate::page::bookmarks::NewMark { key: Some(K.into()), n: 3, text: "mine".into(), ..Default::default() }).unwrap();
    let (said, _) = update(&mut s, &here(), &json!({"id": user.id, "title": "renamed"})).unwrap();
    assert!(said.contains("updated") && s.get(&user.id).unwrap().note == "renamed", "the user's of this pane too: {said}");
    assert!(update(&mut s, &here(), &json!({"id": user.id, "status": "done"})).is_ok() && s.get(&user.id).is_none(), "a bookmark of this pane, the user's, deleted");
    assert!(update(&mut s, &here(), &json!({"id": r})).unwrap_err().contains("nothing to change"));
    assert!(update(&mut s, &here(), &json!({"id": r, "status": "x"})).unwrap_err().contains("not one of open, done"));
    // a reminder made a bookmark and done at once: deleted, never a resolved bookmark
    let (_, told) = update(&mut s, &here(), &json!({"id": r, "remind": false, "status": "done"})).unwrap();
    assert!(s.get(&r).is_none() && told == [Told::Unmark(r.clone())]);

    // the pane's own made before it joined a project stay the agent's there
    let mut s = Store::memory();
    add(&mut s, &here(), "claude", vec![rec("Before the project", false)], &[]).unwrap();
    let id = s.all()[0].id.clone();
    let joined = Place { project: Some("p1".into()), ..here() };
    assert!(update(&mut s, &joined, &json!({"id": id, "title": "Still mine"})).is_ok());
}

#[test]
fn the_notes_of_0_23_become_agents_bookmarks_once() {
    let dir = std::env::temp_dir().join(format!("tm-notes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.json"), json!({"version": 1, "next": 3, "notes": [
        {"id": 1, "kind": "bug", "text": "The parser drops a BOM\nin src/parse.rs", "file": "src/parse.rs", "status": "open", "at": 1, "key": K, "session": "s", "project": null},
        {"id": 2, "kind": "deferred", "text": "done one", "status": "done", "at": 1, "key": K, "session": "s"}]}).to_string()).unwrap();
    let mut s = Store::memory();
    import_notes(&mut s, &dir);
    assert_eq!(s.all().len(), 1);
    let it = &s.all()[0];
    assert_eq!((it.note.as_str(), it.text.as_str(), it.by.as_deref(), it.kind.as_deref(), it.file.as_deref()),
               ("The parser drops a BOM", "The parser drops a BOM\nin src/parse.rs", Some("agent"), Some("bug"), Some("src/parse.rs")));
    assert!(!dir.join("notes.json").exists() && dir.join("notes.json.imported").exists());
    import_notes(&mut s, &dir);
    assert_eq!(s.all().len(), 1, "once");
    // set aside but not renamed (a failed rename): imported again, nothing kept twice
    std::fs::rename(dir.join("notes.json.imported"), dir.join("notes.json")).unwrap();
    import_notes(&mut s, &dir);
    assert_eq!(s.all().len(), 1, "a note already kept is not kept again");
    let _ = std::fs::remove_dir_all(&dir);
}

fn kinded(title: &str, kind: &str, remind: bool) -> Record {
    Record { title: title.into(), text: "what, where, why".into(), remind, kind: Some(kind.into()), file: None }
}

#[test]
fn only_a_question_reminds() {
    let mut s = Store::memory();
    let (said, _) = add(&mut s, &here(), "claude", vec![kinded("Fix the parser", "bug", true), kinded("Which name?", "question", true)], &[]).unwrap();
    assert!(!s.all()[0].remind && s.all()[1].remind);
    assert!(said.contains("only a question reminds: Fix the parser"), "{said}");
    let id = s.all()[0].id.clone();
    assert!(update(&mut s, &here(), &json!({"id": id, "remind": true})).unwrap_err().contains("only a question reminds"));
}

#[test]
fn a_record_replaces_the_agents_own_and_leaves_the_users() {
    let mut s = Store::memory();
    add(&mut s, &here(), "claude", vec![kinded("Old question", "question", true), kinded("Old bug", "bug", false)], &[]).unwrap();
    let old: Vec<String> = s.all().iter().map(|i| i.id.clone()).collect();
    let (said, told) = add(&mut s, &here(), "claude", vec![kinded("New question", "question", true)], &[old[0].clone()]).unwrap();
    assert_eq!(s.all().iter().map(|i| i.note.as_str()).collect::<Vec<_>>(), ["Old bug", "New question"]);
    assert!(said.contains(&format!("Replaced: {}", old[0])), "{said}");
    assert!(told.contains(&Told::Unmark(old[0].clone())));
    assert!(add(&mut s, &here(), "claude", vec![kinded("x", "bug", false)], &["none".into()]).unwrap_err().contains("No record none"), "nothing of that call happens");
    assert!(s.all().iter().all(|i| i.note != "x"));
}

#[test]
fn three_reports_a_session_and_no_banner() {
    let mut s = Store::memory();
    for n in 1..=3 {
        add(&mut s, &here(), "claude", vec![kinded(&format!("Report {n}"), "report", false)], &[]).unwrap();
    }
    let first = s.all()[0].id.clone();
    let (said, told) = add(&mut s, &here(), "claude", vec![kinded("Report 4", "report", false)], &[]).unwrap();
    assert_eq!(s.all().iter().map(|i| i.note.as_str()).collect::<Vec<_>>(), ["Report 2", "Report 3", "Report 4"]);
    assert!(said.contains(&format!("Older reports went ({REPORTS_MOST} a session): {first}")), "{said}");
    assert!(told.contains(&Told::Unmark(first)) && !told.iter().any(|t| matches!(t, Told::Full(_))));
    add(&mut s, &here(), "claude", vec![kinded("A bug", "bug", false)], &[]).unwrap();
    assert_eq!(s.all().len(), 4, "only a report makes a report go");
}

#[test]
fn an_answer_lists_the_sessions_other_open_records() {
    let mut s = Store::memory();
    let (said, _) = add(&mut s, &here(), "claude", vec![kinded("First", "bug", false)], &[]).unwrap();
    assert!(!said.contains("Other open"), "{said}");
    let (said, _) = add(&mut s, &here(), "claude", vec![kinded("Second", "deferred", false)], &[]).unwrap();
    assert!(said.contains(&format!("Other open records of this session: {} [bug] First", s.all()[0].id)), "{said}");
    assert!(!said.contains("[deferred] Second"), "not the one just made");
    let many: Vec<Record> = (0..12).map(|n| kinded(&format!("N{n}"), "bug", false)).collect();
    add(&mut s, &here(), "claude", many, &[]).unwrap();
    let (said, _) = add(&mut s, &here(), "claude", vec![kinded("Last", "bug", false)], &[]).unwrap();
    assert!(said.contains("[bug] N11") && said.contains("and 4 more"), "{said}");
}
