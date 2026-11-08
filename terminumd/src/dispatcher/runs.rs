// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Dispatcher's runs in the store (`terminum-dispatcher.md` §9): every fact a restart needs.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, Row, Transaction};

use crate::tasks::ops::{rows, sql};

/// One RUN: its tasks, what they stand at together, and their project (AC-138).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub id: String,
    /// the task it started from, where what concerns none in particular is said; empty: + Session's, for all its life (AC-142)
    pub task: String,
    /// its tasks' IDs
    pub ids: Vec<String>,
    /// those not DONE or FAILED
    pub open: Vec<String>,
    /// IN PROGRESS while any is, else WAIT FOR HUMAN, else how the last ended
    pub status: String,
    pub project: String,
    /// the time budgets its tasks have, together: one without adds nothing and lifts no limit
    pub budget: Option<i64>,
    pub agent_id: String,
    pub agent_name: String,
    pub agent_color: i64,
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
    pub wait_moved: bool,
    pub no_result_wait: bool,
    pub result: bool,
    pub again_said: bool,
    pub close_tried: bool,
    pub memory_warned: bool,
    pub last_walk: i64,
}

impl Run {
    /// Its pane's server and pane, while its key resolves.
    pub(super) fn place(&self, app: &crate::app::Shared) -> Option<(std::sync::Arc<crate::tmux::Server>, String)> {
        self.pane_key.as_deref().and_then(|k| app.tmux.resolve(k))
    }

    /// Before its prompt is taken: its start is being watched.
    pub fn is_starting(&self) -> bool {
        matches!(self.state.as_str(), "starting" | "asking" | "typed")
    }

    /// Dispatcher no longer touches it.
    pub fn is_over(&self) -> bool {
        matches!(self.state.as_str(), "closed" | "left")
    }

    /// Its run still holds its tasks (AC-138): starting, a task open, or still in a turn after its last.
    pub fn busy(&self) -> bool {
        !self.ad_hoc() && !self.is_over() && (self.is_starting() || !self.open.is_empty() || (self.state == "working" && !self.turn_ended))
    }

    /// A run about to start `agent` in a project's place, nothing of it yet (AC-140, AC-141).
    pub fn of(agent: &crate::tasks::agents::Agent, host: Option<String>, folder: String, project: String, prompt: Option<String>) -> Run {
        Run { agent_id: agent.id.clone(), agent_name: agent.name.clone(), agent_color: agent.color, kind: agent.kind.clone(), command: agent.line.clone(), host, folder, project, prompt, ..Default::default() }
    }

    /// + Session's: no task, no prompt; Dispatcher only watches it (AC-142).
    pub fn ad_hoc(&self) -> bool {
        self.task.is_empty()
    }

    /// It holds a slot of its agent and of its project (AC-141): from RUN until it is closed, let go or its agent is gone.
    pub fn holds_slot(&self) -> bool {
        !self.is_over() && self.state != "gone"
    }

    /// What it is about now: its tasks still open, else all.
    pub fn targets(&self) -> &[String] {
        if self.open.is_empty() { &self.ids } else { &self.open }
    }

    /// Those, in words to its agent or a person.
    pub fn names(&self) -> String {
        self.targets().join(", ")
    }

    /// RUN or a human's own prompt: every clock starts anew (AC-122). The turn is the agent's: a hook says when one starts.
    pub fn new_work(&mut self) {
        self.nudged = false;
        self.nudge_turn = false;
        self.no_result_wait = false;
        self.wait_moved = false;
        self.again_said = false;
        self.idle_for = 0;
        self.closing_for = 0;
        self.budget_used = 0;                          // AC-122: the budget starts anew
        self.warned = false;
        self.memory_warned = false;
    }
}

const SELECT: &str = "SELECT r.id, COALESCE(r.task, ''),
    (SELECT group_concat('T-' || x.number) FROM run_tasks m JOIN tasks x ON x.id = m.task WHERE m.run = r.id),
    (SELECT group_concat('T-' || x.number) FROM run_tasks m JOIN tasks x ON x.id = m.task WHERE m.run = r.id AND x.status NOT IN ('DONE', 'FAILED')),
    COALESCE((SELECT x.status FROM run_tasks m JOIN tasks x ON x.id = m.task WHERE m.run = r.id ORDER BY CASE x.status WHEN 'IN PROGRESS' THEN 0 WHEN 'WAIT FOR HUMAN' THEN 1 WHEN 'TODO' THEN 2 WHEN 'FAILED' THEN 3 ELSE 4 END LIMIT 1), ''),
    r.project,
    (SELECT sum(x.budget_seconds) FROM run_tasks m JOIN tasks x ON x.id = m.task WHERE m.run = r.id),
    r.agent_id, r.agent_name, r.agent_color, r.kind, r.command, r.host, r.folder, r.pane_key, r.agent_session, r.state, r.prompt,
    r.turn_ended, r.open_tools, r.idle_for, r.waited_for, r.closing_for, r.budget_used, r.nudged, r.nudge_turn, r.warned, r.wait_moved, r.no_result_wait,
    NOT EXISTS (SELECT 1 FROM run_tasks m WHERE m.run = r.id AND m.result = 0), r.again_said, r.close_tried, r.last_walk, r.memory_warned
    FROM runs r";
/// A run Dispatcher still holds, over `runs r`.
pub(crate) const STILL: &str = "r.state NOT IN ('closed', 'left')";
const STARTING: &str = "r.state IN ('starting', 'asking', 'typed')";

/// `group_concat`'s list, its order by number.
fn listed(text: Option<String>) -> Vec<String> {
    let mut ids: Vec<String> = text.unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
    ids.sort_by_key(|id| crate::tasks::ops::number_of(id).unwrap_or(0));
    ids
}

fn of_row(r: &Row) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?, task: r.get(1)?, ids: listed(r.get(2)?), open: listed(r.get(3)?), status: r.get(4)?, project: r.get(5)?, budget: r.get(6)?, agent_id: r.get(7)?, agent_name: r.get(8)?, agent_color: r.get(9)?, kind: r.get(10)?, command: r.get(11)?, host: r.get(12)?, folder: r.get(13)?,
        pane_key: r.get(14)?, agent_session: r.get(15)?, state: r.get(16)?, prompt: r.get(17)?, turn_ended: r.get(18)?, open_tools: r.get(19)?, idle_for: r.get(20)?, waited_for: r.get(21)?,
        closing_for: r.get(22)?, budget_used: r.get(23)?, nudged: r.get(24)?, nudge_turn: r.get(25)?, warned: r.get(26)?, wait_moved: r.get(27)?, no_result_wait: r.get(28)?, result: r.get(29)?, again_said: r.get(30)?, close_tried: r.get(31)?, last_walk: r.get(32)?, memory_warned: r.get(33)?,
    })
}

/// The one run `tail` finds, by a statement kept for the next call.
fn one(tx: &Transaction, tail: &str, key: &str) -> rusqlite::Result<Run> {
    tx.prepare_cached(&format!("{SELECT} WHERE {tail}"))?.query_row([key], of_row)
}

/// Every run whose session may still need Dispatcher.
pub fn live(tx: &Transaction) -> Result<Vec<Run>, String> {
    rows(tx, &format!("{SELECT} WHERE {STILL} ORDER BY r.rowid"), [], of_row)
}

/// The run in pane `key`, if any.
pub fn in_pane(tx: &Transaction, key: &str) -> Result<Option<Run>, String> {
    one(tx, &format!("r.pane_key = ?1 AND {STILL} ORDER BY r.rowid DESC LIMIT 1"), key).optional().map_err(sql)
}

/// Task `task`'s newest run.
pub fn last_of(tx: &Transaction, task: &str) -> Result<Option<Run>, String> {
    one(tx, "r.id = (SELECT m.run FROM run_tasks m WHERE m.task = ?1 ORDER BY m.rowid DESC LIMIT 1)", task).optional().map_err(sql)
}

/// A run holding one of agent `agent`'s slots, if any.
pub fn holding_agent(tx: &Transaction, agent: &str) -> Result<Option<Run>, String> {
    Ok(live(tx)?.into_iter().find(|r| r.agent_id == agent && r.holds_slot()))
}

/// Whether session `key` holds an open task other than `lead`'s: one taken there, no run's.
pub fn others_open(tx: &Transaction, key: &str, lead: &str) -> Result<bool, String> {
    tx.query_row("SELECT EXISTS (SELECT 1 FROM tasks WHERE session_key = ?1 AND id != ?2 AND status NOT IN ('DONE', 'FAILED') AND archived_at IS NULL)", (key, lead), |x| x.get(0)).map_err(sql)
}


pub(super) fn by_id(tx: &Transaction, id: &str) -> Result<Run, String> {
    one(tx, "r.id = ?1", id).map_err(sql)
}

/// The run as it stands, changed by `f` and written.
pub fn change(tx: &Transaction, id: &str, f: impl FnOnce(&mut Run)) -> Result<Run, String> {
    let mut r = by_id(tx, id)?;
    f(&mut r);
    save(tx, &r)?;
    Ok(r)
}

/// A new run of `tasks` (keys, the first the one it starts from; none: + Session's), starting, as `like` says: its fields
/// but its id and clocks. Its id.
pub fn add(tx: &Transaction, like: &Run, tasks: &[String], at: i64) -> Result<String, String> {
    let id = crate::auth::crypto::random_hex::<8>();
    tx.execute("INSERT INTO runs (id, task, project, agent_id, agent_name, agent_color, kind, command, host, folder, agent_session, state, prompt, last_walk)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'starting', ?12, ?13)",
               rusqlite::params![id, tasks.first(), like.project, like.agent_id, like.agent_name, like.agent_color, like.kind, like.command, like.host, like.folder, like.agent_session, like.prompt, at]).map_err(sql)?;
    for task in tasks {
        tx.execute("INSERT INTO run_tasks (run, task) VALUES (?1, ?2)", (&id, task)).map_err(sql)?;
    }
    Ok(id)
}

/// Writes what changes as a run goes on.
pub fn save(tx: &Transaction, r: &Run) -> Result<(), String> {
    tx.execute("UPDATE runs SET pane_key = ?2, agent_session = ?3, state = ?4, prompt = ?5, turn_ended = ?6, open_tools = ?7, idle_for = ?8, waited_for = ?9, closing_for = ?10, budget_used = ?11,
                nudged = ?12, nudge_turn = ?13, warned = ?14, wait_moved = ?15, no_result_wait = ?16, again_said = ?17, close_tried = ?18, last_walk = ?19, memory_warned = ?20 WHERE id = ?1",
               rusqlite::params![r.id, r.pane_key, r.agent_session, r.state, r.prompt, r.turn_ended, r.open_tools, r.idle_for, r.waited_for, r.closing_for, r.budget_used,
                                 r.nudged, r.nudge_turn, r.warned, r.wait_moved, r.no_result_wait, r.again_said, r.close_tried, r.last_walk, r.memory_warned]).map(|_| ()).map_err(sql)
}

/// The agent gave `task`'s result (`finish_task`): no screen is kept for it (AC-125).
pub fn finished(tx: &Transaction, task: &str) -> Result<(), String> {
    set_result(tx, task, true)
}

/// Run again: its result is yet to come.
pub fn reopened(tx: &Transaction, task: &str) -> Result<(), String> {
    set_result(tx, task, false)
}

fn set_result(tx: &Transaction, task: &str, given: bool) -> Result<(), String> {
    tx.execute(&format!("UPDATE run_tasks SET result = ?2 WHERE task = ?1 AND run IN (SELECT r.id FROM runs r WHERE {STILL})"), (task, given)).map(|_| ()).map_err(sql)
}

/// The agent last chosen for a project's run, if any.
pub fn last_agent(tx: &Transaction, project: &str) -> Result<Option<String>, String> {
    tx.query_row("SELECT agent_id FROM runs WHERE project = ?1 ORDER BY rowid DESC LIMIT 1", [project], |r| r.get(0)).optional().map_err(sql)
}

/// That of every project: a task made now starts on it.
pub fn last_agents(tx: &Transaction) -> Result<HashMap<String, String>, String> {
    Ok(rows(tx, "SELECT project, agent_id FROM runs ORDER BY rowid", [], |r| Ok((r.get(0)?, r.get(1)?)))?.into_iter().collect())
}

/// `keys` as one SQL argument: a page's tasks, however many.
pub(super) fn listed_keys(keys: &[&str]) -> String {
    serde_json::to_string(keys).unwrap_or_else(|_| "[]".to_string())
}

/// What a card says of its task's run: starting, asking, or `no_result`: DONE with no result from the agent (AC-123).
pub fn card_marks(tx: &Transaction, keys: &[&str]) -> Result<HashMap<String, String>, String> {
    Ok(rows(tx, &format!("SELECT m.task, CASE WHEN {STARTING} THEN r.state ELSE 'no_result' END FROM run_tasks m JOIN runs r ON r.id = m.run JOIN tasks t ON t.id = m.task
              WHERE m.task IN (SELECT value FROM json_each(?1)) AND m.rowid = (SELECT MAX(rowid) FROM run_tasks WHERE task = m.task)
                AND ({STARTING} OR (t.status = 'DONE' AND m.result = 0 AND t.archived_at IS NULL))"), [listed_keys(keys)], |r| Ok((r.get(0)?, r.get(1)?)))?.into_iter().collect())
}

/// The agent each of `keys` stays on: its newest run's, as that run knew it (AC-138).
pub fn card_agents(tx: &Transaction, keys: &[&str]) -> Result<HashMap<String, serde_json::Value>, String> {
    Ok(rows(tx, "SELECT m.task, r.agent_id, r.agent_name, r.agent_color FROM run_tasks m JOIN runs r ON r.id = m.run
              WHERE m.task IN (SELECT value FROM json_each(?1)) AND m.rowid = (SELECT MAX(rowid) FROM run_tasks WHERE task = m.task)", [listed_keys(keys)],
            |r| Ok((r.get(0)?, serde_json::json!({"id": r.get::<_, String>(1)?, "name": r.get::<_, String>(2)?, "color": r.get::<_, i64>(3)?}))))?.into_iter().collect())
}

/// The human gives up on `task`'s run (AC-138): its open tasks fail, Dispatcher lets go of the window, which stays.
pub fn give_up(tx: &Transaction, task: &str) -> Result<Run, String> {
    let run = last_of(tx, task)?.filter(|r| !r.is_over()).ok_or("It has no run to give up.")?;
    let_go(tx, &run.id)
}

/// The same by the run: a + Session's too, its host out of reach (AC-142).
pub fn let_go(tx: &Transaction, id: &str) -> Result<Run, String> {
    let run = by_id(tx, id).map_err(|_| "That session is gone.".to_string())?;
    if run.is_over() {
        return Err("Dispatcher holds that session no more.".into());
    }
    super::move_task(tx, &run, "FAILED", "The human gave up on it.")?;
    change(tx, &run.id, |r| r.state = "left".into())
}

/// A run that still holds `task` open: its session stays until it ends, or the human gives up (AC-138).
pub fn holding(tx: &Transaction, task: &str) -> Result<Option<Run>, String> {
    Ok(last_of(tx, task)?.filter(Run::busy))
}

#[cfg(test)]
#[path = "runs_tests.rs"]
mod tests;
