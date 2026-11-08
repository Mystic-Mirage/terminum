// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The typing gate (`terminum-dispatcher.md` §9): the only way Dispatcher types. All in order, on the
//! pane's typing queue; any miss types nothing, or no Enter, and says which. The paste and the Enter
//! are tmux's own to refuse: it reads the mark and the exit in the same command that types.

use std::time::Duration;

use super::runs::Run;
use super::screen::{self, Screen};
use crate::app::Shared;
use crate::tmux::{args, Server};

/// How long what was pasted may take to show in the box.
const SHOWN: Duration = Duration::from_millis(1500);

/// What missed.
#[derive(Debug, PartialEq)]
pub enum Miss {
    /// the box holds a draft, or a question stands there
    NotEmpty(&'static str),
    Other(String),
}

impl std::fmt::Display for Miss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Miss::NotEmpty(why) => f.write_str(why),
            Miss::Other(why) => f.write_str(why),
        }
    }
}

fn other(why: impl Into<String>) -> Miss {
    Miss::Other(why.into())
}

/// The screen of `kind`'s agent in `pane`.
pub async fn read(server: &Server, pane: &str, kind: &str) -> Result<Screen, String> {
    let rows = server.run(format!("capture-pane -e -p -t {pane}")).await.map_err(|e| format!("tmux did not give its screen: {e}"))?;
    Ok(screen::read(kind, &rows))
}

/// `line`, if at that moment the pane is still `run`'s with its agent's exit not recorded: tmux judges both.
fn guarded(run: &str, pane: &str, line: &str) -> String {
    let test = format!("#{{&&:#{{==:#{{@terminum-run}},{run}}},#{{==:#{{@terminum-exit}},}}}}");
    format!("if-shell -F -t {pane} {} {}", args::quote(&test), args::quote(line))
}

/// Pastes `text` into `run`'s pane and sends it. `first`: its first prompt, no turn has ended yet:
/// the caller knows the agent has started.
pub async fn deliver(app: &Shared, run: &Run, server: &Server, pane: &str, text: &str, first: bool) -> Result<(), Miss> {
    let _held = server.typing(pane).lock_owned().await;
    let said = server.run(format!("display -p -t {pane} '#{{@terminum-run}}\t#{{pane_in_mode}}\t#{{@terminum-exit}}\t#{{pane_current_command}}'")).await.map_err(|e| other(format!("tmux does not answer: {e}")))?;
    let fields: Vec<&str> = said.first().map_or(Vec::new(), |l| l.split('\t').collect());
    let field = |i: usize| fields.get(i).copied().unwrap_or("");
    if field(0) != run.id {
        return Err(other("its pane is not this run's"));
    }
    // its last screen still shows a box: a shell under it is told by the exit, or by its name
    if !field(2).is_empty() || crate::tmux::titles::SHELLS.contains(&crate::termtext::shell_name(field(3))) {
        return Err(other("the agent has ended: only its shell remains"));
    }
    if field(1) != "0" {
        return Err(other("the pane is in a tmux mode"));
    }
    let key = server.key(pane);
    if !first && (!run.turn_ended || app.hooks.working(&key)) {
        return Err(other("the agent is working"));
    }
    match read(server, pane, &run.kind).await.map_err(other)? {
        Screen::Empty => {}
        Screen::Holds(_) => return Err(Miss::NotEmpty("its input box holds a draft")),
        Screen::Asks => return Err(Miss::NotEmpty("the agent asks a question")),
        Screen::Unknown => return Err(other(format!("screen not known ({})", run.kind))),
    }
    let client = server.client(pane).ok_or_else(|| other("tmux does not answer"))?;
    let buffer = crate::page::typing::load_buffer(app, &client, server, text).await.map_err(other)?;
    let pasted = client.run(guarded(&run.id, pane, &format!("paste-buffer -p -b {buffer} -t {pane}"))).await;
    let shown = async {
        pasted.map_err(|e| other(format!("tmux did not take the paste: {e}")))?;
        let until = tokio::time::Instant::now() + SHOWN;
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if screen::shows(&read(server, pane, &run.kind).await.map_err(other)?, text) {
                return Ok(());
            }
            if tokio::time::Instant::now() >= until {
                return Err(other("what was pasted did not show in its box: no Enter"));
            }
        }
    }.await;
    let _ = client.run(format!("delete-buffer -b {buffer}")).await;
    let entered = match shown {
        Ok(()) => {
            app.dispatcher.remember_typed(&key, text);
            client.run(guarded(&run.id, pane, &format!("send-keys -t {pane} Enter"))).await.map(drop).map_err(|e| other(format!("tmux did not take Enter: {e}")))
        }
        Err(m) => Err(m),
    };
    // what went in, and what tmux refused of it (AC-147)
    if let Some((server, _)) = app.tmux.resolve(&key) {
        let words = if entered.is_ok() { format!("{text}<Enter>") } else { text.to_string() };
        let who = crate::inputs::Who::dispatcher("dispatcher.gate", &run.id, &run.ids, &run.agent_id);
        crate::inputs::wrote(app, who, "prompt", &server, pane, &words, entered.as_ref().err().map(Miss::to_string), None);
    }
    entered
}
