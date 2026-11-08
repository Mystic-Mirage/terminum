// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A new window's place and a project's window, on a private tmux (`-L tm-test-*`).

use std::time::Duration;

use serde_json::Value;

use super::*;
use crate::page::client::Shown;
use crate::page::{trace, Pages};
use crate::testutil::{self, Tmux};

fn server_on(s: &Tmux, name: &str) -> Arc<Server> {
    let link = crate::tmux::server::Link::Local { tmux: "tmux".into(), socket: s.socket.clone(), conf: "/dev/null".into() };
    Server::new(name, &s.socket, link, Default::default())
}

/// A new window's pane id, in `main` of `s`.
fn window(s: &Tmux, dir: &std::path::Path) -> String {
    s.read(&["new-window", "-t", "main:", "-c", dir.to_str().unwrap(), "-P", "-F", "#{pane_id}", "sleep 60"]).trim().to_string()
}

fn panes_in_order(s: &Tmux, session: &str) -> Vec<String> {
    s.read(&["list-windows", "-t", session, "-F", "#{pane_id}"]).lines().map(str::to_string).collect()
}

/// The next message of `t` the page was told.
async fn told(rx: &mut tokio::sync::mpsc::Receiver<String>, t: &str) -> Value {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.expect("the page was told nothing").unwrap();
        let m: Value = serde_json::from_str(&m).unwrap();
        if m["t"] == t {
            return m;
        }
    }
}

/// AC-132: a `new` queued behind a slow server goes beside the pane shown when it was asked, in that
/// pane's folder, whichever is shown by the time it runs.
#[tokio::test]
async fn ac132_a_new_window_goes_beside_the_pane_shown_when_it_was_asked() {
    let Some(s) = Tmux::start("new-beside", (80, 24), "sleep 60") else { return };
    let (root, app) = testutil::app("new-beside");
    let server = app.tmux.attached_or(&s.socket, || server_on(&s, "local"));
    server.sync_clients(&["main".to_string()]).unwrap();
    let (page, mut rx) = Pages::default().add(crate::inputs::Client { web: true, ..Default::default() }, String::new(), trace::Recorder::new(root.join("trace"))).unwrap();
    let group = format!("local:{}/main", s.socket);
    let first = panes_in_order(&s, "main")[0].clone();
    let asked = window(&s, &s.dir);
    let now = window(&s, &root);
    page.state.lock().shown = Some(Shown { key: server.key(&now), server: server.clone(), pane: now.clone(), ctl: Default::default(), task: tokio::spawn(async {}) });

    let at = (server.key(&asked), server.clone(), asked.clone());
    new_window(&app, &page, Some(&at), &group).await.unwrap();
    let order = panes_in_order(&s, "main");
    assert_eq!(order.len(), 4);
    assert_eq!((&order[0], &order[1], &order[3]), (&first, &asked, &now), "right after the asked pane's window: {order:?}");
    let made = &order[2];
    let cwd = s.read(&["display", "-p", "-t", made, "#{pane_current_path}"]);
    assert_eq!(cwd.trim(), s.dir.to_str().unwrap(), "in the asked pane's folder");
    let created = told(&mut rx, "created").await;
    assert_eq!(created["id"], server.key(made));
    assert_eq!(created["stay"], true, "the user went on meanwhile: they stay where they are");

    // nothing was shown, or a pane of another server: the end of the session
    new_window(&app, &page, None, &group).await.unwrap();
    assert_eq!(panes_in_order(&s, "main")[..4], order[..], "nothing shown: at the end");
    let other = app.tmux.local.clone();
    new_window(&app, &page, Some(&(other.key("%0"), other, "%0".to_string())), &group).await.unwrap();
    assert_eq!(panes_in_order(&s, "main")[..4], order[..], "another server's pane says nothing of this session");
    assert_eq!(new_window(&app, &page, None, "").await, Err("That session is gone.".to_string()));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

/// AC-92: a project's window: its session by the project's name, made once, in the folder asked or
/// the one kept; the pane joins the project. What cannot be done is said.
#[tokio::test]
async fn ac92_a_projects_window_opens_in_its_session_and_joins_it() {
    let Some(s) = Tmux::start("project-window", (80, 24), "sleep 60") else { return };
    let (root, app) = testutil::app("project-window");
    let server = server_on(&s, "devbox");
    app.tmux.add_host(server.clone());
    server.sync_clients(&["main".to_string()]).unwrap();
    let (page, mut rx) = Pages::default().add(crate::inputs::Client { web: true, ..Default::default() }, String::new(), trace::Recorder::new(root.join("trace"))).unwrap();
    let id = app.projects.lock().create("My.App:1", Some("devbox".into()), String::new()).unwrap();
    let members = || app.projects.lock().by_id(&id).unwrap().members.clone();

    assert_eq!(project_window(&app, &page, None, &id, "").await, Err("This project has no folder yet.".to_string()));
    let missing = root.join("no-such-folder").display().to_string();
    assert_eq!(project_window(&app, &page, None, &id, &missing).await, Err(format!("The folder {missing} does not exist.")));
    assert_eq!(project_window(&app, &page, None, "gone", "").await, Err("That project is gone.".to_string()));
    assert!(members().is_empty(), "refused: nothing joined");

    let dir = s.dir.display().to_string();
    project_window(&app, &page, None, &id, &dir).await.unwrap();
    let session = "My-App-1";
    let first = panes_in_order(&s, session);
    assert_eq!(first.len(), 1, "a session by the project's name");
    assert_eq!(members(), [server.key(&first[0])]);
    assert_eq!(app.projects.lock().by_id(&id).unwrap().dir, dir, "its folder kept");
    assert_eq!(told(&mut rx, "created").await["id"], server.key(&first[0]));

    project_window(&app, &page, None, &id, "").await.unwrap();
    let both = panes_in_order(&s, session);
    assert_eq!(both.len(), 2, "that session again, a new window in it");
    assert_eq!(members(), [server.key(&both[0]), server.key(&both[1])]);
    assert_eq!(s.read(&["display", "-p", "-t", &both[1], "#{pane_current_path}"]).trim(), dir, "in the folder kept");

    app.tmux.remove_host("devbox");
    assert_eq!(project_window(&app, &page, None, &id, "").await, Err("devbox is not connected.".to_string()));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
