// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A pane's quick shell (AC-88): a tmux window of its own, made beside the pane's window in
//! the pane's folder and marked `@tm-quick <pane>` (`@tm-quick-side bottom|left`), so the list
//! puts it under the pane and every page shows it with it. A window and not a split: a page
//! sizes the window it shows, and the quick shell is shown by a second page (`/?embed=`).

use std::sync::Arc;

use serde_json::{json, Value};

use super::client::error;
use super::sessions::{allowed_now, folder_of, said, window_of};
use super::Page;
use crate::app::Shared;
use crate::tmux::args;

pub async fn handle(app: &Shared, page: &Arc<Page>, msg: &Value) {
    let s = |k: &str| msg[k].as_str().unwrap_or("").to_string();
    let side = match s("side").as_str() { "left" => "left", "right" => "right", _ => "bottom" };
    let result = match msg["t"].as_str().unwrap_or("") {
        "quick" => open(app, page, &s("id"), side).await,
        "quickside" => set_side(app, &s("id"), side).await,
        _ => Ok(()),
    };
    if let Err(e) = result {
        error(page, e, None);
    }
}

/// The pane's quick shell: the one it has in its session, else a new one; the page is told its
/// key. Asked for a quick shell's own pane (a phone shows it alone), its pane's.
async fn open(app: &Shared, page: &Arc<Page>, id: &str, side: &str) -> Result<(), String> {
    let (server, mut pane) = app.tmux.resolve(id).ok_or(super::CLOSED)?;
    // (its session, the pane whose quick shell it is)
    let read = async |p: &str| -> (String, String) {
        let line = server.run(format!("display -p -t {p} '#{{session_id}}\t#{{@tm-quick}}'")).await.ok().and_then(|l| l.into_iter().next()).unwrap_or_default();
        line.split_once('\t').map(|(s, q)| (s.to_string(), q.to_string())).unwrap_or_default()
    };
    let (session, of) = read(&pane).await;
    if !of.is_empty() && read(&of).await.0 == session {
        pane = of;
    }
    let parent = server.key(&pane);
    let marks = server.run("list-windows -a -F '#{@tm-quick}\t#{session_id}\t#{pane_id}'").await.map_err(|e| said(&e))?;
    let had = marks.iter().map(|l| l.splitn(3, '\t').collect::<Vec<_>>()).find(|f| f.len() == 3 && f[0] == pane && f[1] == session).map(|f| f[2].to_string());
    let quick = match had {
        Some(p) => p,
        None => {
            if !allowed_now(page) {
                return Err("Wait a second: one new window, session or split a second.".into());
            }
            let window = window_of(&server, &pane).await?;
            let (dir, _) = folder_of(&server, &pane).await;
            let made = server.run(format!("new-window -d -a -t {window} -c {} -P -F '#{{pane_id}}\t#{{window_id}}'", args::format_arg(&dir)))
                .await.map_err(|e| format!("No quick shell: {}", said(&e)))?;
            let (new_pane, new_window) = made.first().and_then(|l| l.split_once('\t')).ok_or("No quick shell: tmux said nothing")?;
            for line in [format!("set-option -w -t {new_window} @tm-quick {pane}"), format!("set-option -w -t {new_window} @tm-quick-side {side}")] {
                server.run(line).await.map_err(|e| format!("No quick shell: {}", said(&e)))?;
            }
            new_pane.to_string()
        }
    };
    crate::lists::refresh_server(app, &server).await;
    page.send_json(&json!({"t": "layout", "groups": app.layout.read().clone()}));
    page.send_json(&json!({"t": "quickmade", "id": server.key(&quick), "for": parent}));
    Ok(())
}

/// ⇄ on the quick shell's bar: the other side, for every page.
async fn set_side(app: &Shared, id: &str, side: &str) -> Result<(), String> {
    let (server, pane) = app.tmux.resolve(id).ok_or(super::CLOSED)?;
    let window = window_of(&server, &pane).await?;
    server.run(format!("set-option -w -t {window} @tm-quick-side {side}")).await.map_err(|e| said(&e))?;
    crate::lists::request(app, &server);
    Ok(())
}
