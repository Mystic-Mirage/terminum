// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Dispatcher's runs in the store (`terminum-dispatcher.md` §9): every fact a restart needs.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, Row, Transaction};

use crate::tasks::ops::{rows, sql};

/// One RUN, with its task's ID, status and project.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub id: String,
    pub task: String,
    pub task_id: String,
    pub status: String,
    pub project: String,
    pub agent_id: String,
    pub kind: String,
    pub command: String,
    pub host: Option<String>,
    pub folder: String,
    pub pane_key: Option<String>,
    pub agent_session: Option<String>,
    pub state: String,
    pub prompt: Option<String>,
    pub turn_ended: bool,
    pub open_tools: i64,
    pub idle_for: i64,
    pub waited_for: i64,
    pub closing_for: i64,
    pub budget_used: i64,
    pub nudged: bool,
    pub nudge_turn: bool,
    pub warned: bool,
    pub held: bool,
    pub wait_moved: bool,
    pub no_result_wait: bool,
    pub result: bool,
    pub again_said: bool,
    pub close_tried: bool,
    pub last_walk: i64,
}

impl Run {
    /// Before its prompt is taken: its start is being watched.
    pub fn is_starting(&self) -> bool {
        matches!(self.state.as_str(), "starting" | "asking" | "typed")
    }

    /// Dispatcher no longer touches it.
    pub fn is_over(&self) -> bool {
        matches!(self.state.as_str(), "closed" | "left")
    }

    /// RUN or a human's own prompt: every clock starts anew, a human's hold ends (AC-122, AC-124).
    pub fn new_work(&mut self) {
        self.held = false;
        self.nudged = false;
        self.nudge_turn = false;
        self.no_result_wait = false;
        self.wait_moved = false;
        self.turn_ended = false;
        self.again_said = false;
        self.idle_for = 0;
        self.closing_for = 0;
    }
}

const SELECT: &str = "SELECT r.id, r.task, 'T-' || t.number, t.status, t.project, r.agent_id, r.kind, r.command, r.host, r.folder, r.pane_key, r.agent_session, r.state, r.prompt,
    r.turn_ended, r.open_tools, r.idle_for, r.waited_for, r.closing_for, r.budget_used, r.nudged, r.nudge_turn, r.warned, r.held, r.wait_moved, r.no_result_wait, r.result, r.again_said, r.close_tried, r.last_walk
    FROM runs r JOIN tasks t ON t.id = r.task";
const STILL: &str = "r.state NOT IN ('closed', 'left')";

fn of_row(r: &Row) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?, task: r.get(1)?, task_id: r.get(2)?, status: r.get(3)?, project: r.get(4)?, agent_id: r.get(5)?, kind: r.get(6)?, command: r.get(7)?, host: r.get(8)?, folder: r.get(9)?,
        pane_key: r.get(10)?, agent_session: r.get(11)?, state: r.get(12)?, prompt: r.get(13)?, turn_ended: r.get(14)?, open_tools: r.get(15)?, idle_for: r.get(16)?, waited_for: r.get(17)?,
        closing_for: r.get(18)?, budget_used: r.get(19)?, nudged: r.get(20)?, nudge_turn: r.get(21)?, warned: r.get(22)?, held: r.get(23)?, wait_moved: r.get(24)?, no_result_wait: r.get(25)?, result: r.get(26)?, again_said: r.get(27)?, close_tried: r.get(28)?, last_walk: r.get(29)?,
    })
}

/// Every run whose session may still need Dispatcher.
pub fn live(tx: &Transaction) -> Result<Vec<Run>, String> {
    rows(tx, &format!("{SELECT} WHERE {STILL} ORDER BY r.rowid"), [], of_row)
}

/// The run in pane `key`, if any.
pub fn in_pane(tx: &Transaction, key: &str) -> Result<Option<Run>, String> {
    tx.query_row(&format!("{SELECT} WHERE r.pane_key = ?1 AND {STILL} ORDER BY r.rowid DESC LIMIT 1"), [key], of_row).optional().map_err(sql)
}

/// Task `task`'s newest run.
pub fn last_of(tx: &Transaction, task: &str) -> Result<Option<Run>, String> {
    tx.query_row(&format!("{SELECT} WHERE r.task = ?1 ORDER BY r.rowid DESC LIMIT 1"), [task], of_row).optional().map_err(sql)
}

fn by_id(tx: &Transaction, id: &str) -> Result<Run, String> {
    tx.query_row(&format!("{SELECT} WHERE r.id = ?1"), [id], of_row).map_err(sql)
}

/// The run as it stands, changed by `f` and written.
pub fn change(tx: &Transaction, id: &str, f: impl FnOnce(&mut Run)) -> Result<Run, String> {
    let mut r = by_id(tx, id)?;
    f(&mut r);
    save(tx, &r)?;
    Ok(r)
}

/// A new run, starting, as `like` says: its fields but its id and clocks. Its id.
pub fn add(tx: &Transaction, like: &Run, at: i64) -> Result<String, String> {
    let id = crate::auth::crypto::random_hex::<8>();
    tx.execute("INSERT INTO runs (id, task, agent_id, kind, command, host, folder, agent_session, state, prompt, last_walk) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'starting', ?9, ?10)",
               rusqlite::params![id, like.task, like.agent_id, like.kind, like.command, like.host, like.folder, like.agent_session, like.prompt, at]).map_err(sql)?;
    Ok(id)
}

/// Writes what changes as a run goes on.
pub fn save(tx: &Transaction, r: &Run) -> Result<(), String> {
    tx.execute("UPDATE runs SET pane_key = ?2, agent_session = ?3, state = ?4, prompt = ?5, turn_ended = ?6, open_tools = ?7, idle_for = ?8, waited_for = ?9, closing_for = ?10, budget_used = ?11,
                nudged = ?12, nudge_turn = ?13, warned = ?14, held = ?15, wait_moved = ?16, no_result_wait = ?17, result = ?18, again_said = ?19, close_tried = ?20, last_walk = ?21 WHERE id = ?1",
               rusqlite::params![r.id, r.pane_key, r.agent_session, r.state, r.prompt, r.turn_ended, r.open_tools, r.idle_for, r.waited_for, r.closing_for, r.budget_used,
                                 r.nudged, r.nudge_turn, r.warned, r.held, r.wait_moved, r.no_result_wait, r.result, r.again_said, r.close_tried, r.last_walk]).map(|_| ()).map_err(sql)
}

/// A human moved `task`: terminum's own signals leave it there until RUN or a prompt they type (AC-124).
pub fn hold(tx: &Transaction, task: &str) -> Result<(), String> {
    tx.execute("UPDATE runs SET held = 1 WHERE task = ?1 AND state NOT IN ('closed', 'left')", [task]).map(|_| ()).map_err(sql)
}

/// The agent gave its result (`finish_task`): no screen is kept for it (AC-125).
pub fn finished(tx: &Transaction, task: &str) -> Result<(), String> {
    tx.execute("UPDATE runs SET result = 1 WHERE task = ?1 AND state NOT IN ('closed', 'left')", [task]).map(|_| ()).map_err(sql)
}

/// The agent last chosen for a project's run, if any.
pub fn last_agent(tx: &Transaction, project: &str) -> Result<Option<String>, String> {
    tx.query_row("SELECT r.agent_id FROM runs r JOIN tasks t ON t.id = r.task WHERE t.project = ?1 ORDER BY r.rowid DESC LIMIT 1", [project], |r| r.get(0)).optional().map_err(sql)
}

/// What a card says of its task's run: starting, asking, or `no_result`: DONE with no result from the agent (AC-123).
pub fn card_marks(tx: &Transaction) -> Result<HashMap<String, String>, String> {
    Ok(rows(tx, "SELECT r.task, CASE WHEN r.state IN ('starting', 'asking', 'typed') THEN r.state ELSE 'no_result' END FROM runs r JOIN tasks t ON t.id = r.task
              WHERE r.state IN ('starting', 'asking', 'typed') OR (t.status = 'DONE' AND r.result = 0 AND r.held = 0 AND t.archived_at IS NULL AND r.rowid = (SELECT MAX(rowid) FROM runs WHERE task = r.task))", [], |r| Ok((r.get(0)?, r.get(1)?)))?.into_iter().collect())
}
