// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
use super::*;

#[test]
fn a_host_group_says_its_state() {
    let s = Server::new("devbox", "default", Link::Ssh { ssh: "ssh".into(), args: vec!["devbox.example".into()] }, Arc::new(tokio::sync::Notify::new()));
    *s.status.lock() = Status::Reconnecting { why: String::new(), retry_s: 2 };
    let g = host_group(&s, None);
    assert_eq!(g["label"], "devbox");
    assert_eq!(g["state"], "reconnecting");
    assert_eq!(g["why"], "reconnecting (2 s)");
    *s.status.lock() = Status::Failed { why: "Permission denied (publickey)".into() };
    assert_eq!(host_group(&s, Some("enabled"))["why"], "Permission denied (publickey)");
}

#[test]
fn a_group_says_how_another_terminal_attaches() {
    let host = Server::new("devbox", "default", Link::Ssh { ssh: "ssh".into(), args: vec!["-p".into(), "2222".into(), "alex@devbox.example".into()] },
        Arc::new(tokio::sync::Notify::new()));
    assert_eq!(attach_command(&host, HOST_SESSION), "ssh -t -p 2222 -- alex@devbox.example \"tmux attach -t '=terminum'\"");
    assert_eq!(host_group(&host, None)["session"], "terminum");
    let local = Server::new("local", "terminum", Link::Local { tmux: "tmux".into(), socket: "terminum".into(), conf: "/dev/null".into() },
        Arc::new(tokio::sync::Notify::new()));
    assert_eq!(attach_command(&local, "main"), "tmux -L terminum attach -t '=main'");
    // zsh (macOS's shell) reads the copied target as written; bare, `=main` is a program's path
    if let Ok(out) = std::process::Command::new("zsh").arg("-c").arg(format!("printf '%s' {}", sh("=main"))).output() {
        assert_eq!(String::from_utf8_lossy(&out.stdout), "=main");
        let bare = std::process::Command::new("zsh").arg("-c").arg("printf '%s' =terminum-no-such-program").output().unwrap();
        assert!(!bare.status.success(), "zsh expanded a bare =word");
    }
    assert_eq!(attach_command(&local, "my work's"), "tmux -L terminum attach -t '=my work'\\''s'");
}

/// AC-88: rows are only tagged, in tmux's order; a quick shell of a quick shell, a second
/// one of a pane, one whose pane is gone, stay ordinary windows.
#[test]
fn quick_shells_are_tagged_under_their_pane() {
    let s = Server::new("local", "terminum", Link::Local { tmux: "tmux".into(), socket: "terminum".into(), conf: "/dev/null".into() },
        Arc::new(tokio::sync::Notify::new()));
    let pane = |id: &str, w: &str, of: &str| PaneFields { pane_id: id.into(), window_id: w.into(), quick_for: of.into(), quick_side: "left".into(), ..Default::default() };
    let ps = [pane("%1", "@1", ""), pane("%2", "@2", "%1"), pane("%3", "@3", "%2"), pane("%4", "@4", "%1"), pane("%5", "@5", "%9")];
    let rows: Vec<Value> = ps.iter().map(|p| json!({"id": s.key(&p.pane_id), "sub": ["zsh"]})).collect();
    let refs: Vec<&PaneFields> = ps.iter().collect();
    let out = quick_rows(&s, rows, &refs);
    let ids: Vec<&str> = out.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["tmux:local:terminum:%1", "tmux:local:terminum:%2", "tmux:local:terminum:%3", "tmux:local:terminum:%4", "tmux:local:terminum:%5"]);
    assert_eq!((out[0]["quick"].as_str(), out[0]["quickSide"].as_str()), (Some("tmux:local:terminum:%2"), Some("left")));
    assert_eq!(out[1]["quickOf"], "tmux:local:terminum:%1");
    assert_eq!(out[1]["sub"], json!(["quick shell", "zsh"]));
    for (i, why) in [(2, "a quick shell's own"), (3, "a second one"), (4, "its pane gone")] {
        assert!(out[i].get("quickOf").is_none() && out[i].get("quick").is_none(), "{why}");
    }
    assert!(out[1].get("quick").is_none(), "a quick shell is never a parent");
}

/// AC-89: a pane reached through two places is listed under the first only.
#[test]
fn a_pane_is_listed_once() {
    let row = |k: &str| json!({"id": k});
    let mut groups = vec![
        json!({"server": "devbox:default", "label": "devbox", "host": "devbox", "session": "terminum", "items": [row("tmux:devbox:default:%1")], "new": "New window"}),
        json!({"server": "devbox:default", "label": "work", "host": "devbox", "session": "work", "items": [row("tmux:devbox:default:%2"), row("tmux:devbox:default:%3")]}),
        json!({"server": "box2:default", "label": "box2", "host": "box2", "session": "terminum", "items": [row("tmux:box2:default:%1")], "new": "New window"}),
        json!({"server": "box2:default", "label": "work2", "host": "box2", "session": "work2", "items": [row("tmux:box2:default:%3"), row("tmux:box2:default:%4")]}),
        json!({"server": "local:terminum", "label": "main", "items": [row("tmux:local:terminum:%1")]}),
    ];
    let same = "devbox\t/tmp/tmux-501/default\t42".to_string();
    let identity = HashMap::from([("devbox:default".to_string(), same.clone()), ("box2:default".to_string(), same),
        ("local:terminum".to_string(), "mac\t/tmp/tmux-501/terminum\t7".to_string())]);
    once_each(&mut groups, &identity);
    assert_eq!(groups[2]["items"], json!([]));
    assert_eq!((groups[2]["state"].as_str(), groups[2]["why"].as_str()), (Some("absent"), Some("the same tmux as devbox's terminum: shown there")));
    assert!(groups[2]["new"].is_null(), "no new window where nothing is listed");
    assert_eq!(groups[3]["items"], json!([row("tmux:box2:default:%4")]), "a linked window's pane goes, the others stay");
    assert!(groups[3].get("state").is_none());
    assert_eq!(groups[4]["items"].as_array().unwrap().len(), 1, "another server's %1 is another pane");
    assert_eq!(groups[1]["items"].as_array().unwrap().len(), 2);
}
