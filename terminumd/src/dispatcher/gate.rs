// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The typing gate (`terminum-dispatcher.md` §9): the only way Dispatcher types. All in order, a
//! human's keys held behind it; any miss types nothing, or no Enter, and says which.

use std::time::Duration;

use super::runs::Run;
use super::screen::{self, Screen};
use crate::app::Shared;
use crate::tmux::Server;

/// How long what was pasted may take to show in the box.
const SHOWN: Duration = Duration::from_millis(1500);

async fn rows(server: &Server, pane: &str) -> Result<Vec<String>, String> {
    server.run(format!("capture-pane -e -p -t {pane}")).await.map_err(|e| format!("tmux did not give its screen: {e}"))
}

/// The screen of `kind`'s agent in `pane`.
pub async fn read(server: &Server, pane: &str, kind: &str) -> Result<Screen, String> {
    Ok(screen::read(kind, &rows(server, pane).await?))
}

/// Pastes `text` into `run`'s pane and sends it. `first`: its first prompt, no turn has ended yet:
/// the caller knows the agent has started.
/// The `Err` names what missed.
pub async fn deliver(app: &Shared, run: &Run, server: &Server, pane: &str, text: &str, first: bool) -> Result<(), String> {
    let _held = server.typing.lock().await;
    let said = server.run(format!("display -p -t {pane} '#{{@terminum-run}}\t#{{pane_in_mode}}\t#{{@terminum-exit}}'")).await.map_err(|e| format!("tmux does not answer: {e}"))?;
    let fields: Vec<&str> = said.first().map_or(Vec::new(), |l| l.split('\t').collect());
    let (mark, mode, exit) = (fields.first().copied().unwrap_or(""), fields.get(1).copied().unwrap_or("0"), fields.get(2).copied().unwrap_or(""));
    if mark != run.id {
        return Err("its pane is not this run's".into());
    }
    if !exit.is_empty() {
        return Err("the agent has ended: only its shell remains".into());   // its last screen still shows a box
    }
    if mode != "0" {
        return Err("the pane is in a tmux mode".into());
    }
    let key = server.key(pane);
    if !first && app.hooks.working(&key) {
        return Err("the agent is working".into());
    }
    match read(server, pane, &run.kind).await? {
        Screen::Empty => {}
        Screen::Holds(_) => return Err("its input box holds a draft".into()),
        Screen::Asks => return Err("the agent asks a question".into()),
        Screen::Unknown => return Err(format!("screen not known ({})", run.kind)),
    }
    let client = server.client(pane).ok_or("tmux does not answer")?;
    crate::page::typing::paste_buffer(app, &client, server, pane, text).await?;
    let until = tokio::time::Instant::now() + SHOWN;
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if screen::shows(&read(server, pane, &run.kind).await?, text) {
            break;
        }
        if tokio::time::Instant::now() >= until {
            return Err("what was pasted did not show in its box: no Enter".into());
        }
    }
    app.dispatcher.remember_typed(&key, text);
    server.run(format!("send-keys -t {pane} Enter")).await.map(drop).map_err(|e| format!("tmux did not take Enter: {e}"))
}
