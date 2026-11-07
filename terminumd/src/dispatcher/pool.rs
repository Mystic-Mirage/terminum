// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Slots (`terminum-dispatcher.md` AC-141): starting a process takes one of its agent's and one of its
//! project's, or is refused naming what holds them. Decided in the transaction that adds the run, so two
//! presses never both find one free; only starting is gated, a human typing never is.

use std::collections::{BTreeMap, HashMap};

use rusqlite::Transaction;
use serde::Serialize;
use serde_json::{json, Value};

use super::plan;
use super::runs::{self, Run};
use crate::tasks::agents::{self, Agent};
use crate::tasks::limits;
use crate::tasks::ops;

/// A limit reached: what to say, and the idle holder whose closing would make room, if one would.
#[derive(Debug, PartialEq, Serialize)]
pub struct Full {
    pub words: String,
    pub close: Option<Holder>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Holder {
    pub run: String,
    pub tasks: String,
}

/// Whether a session may be closed for a slot, as the store knows it: its run idle (`plan::idle_holder`) and
/// no task taken in it still open. What its pane shows is read just before the close.
pub fn closable(tx: &Transaction, r: &Run) -> Result<bool, String> {
    Ok(plan::idle_holder(r) && !r.pane_key.as_deref().map_or(Ok(false), |key| runs::others_open(tx, key, &r.task))?)
}

fn names(holders: &[&Run]) -> String {
    holders.iter().map(|r| r.names()).collect::<Vec<_>>().join(", ")
}

/// One closing frees a place only for what stands exactly at its limit, and only if it holds a slot of each that does.
fn victim<'a>(tx: &Transaction, full: &[(&[&'a Run], i64)]) -> Result<Option<&'a Run>, String> {
    let Some((first, _)) = full.first() else { return Ok(None) };
    if full.iter().any(|(holders, limit)| holders.len() as i64 != *limit) {
        return Ok(None);
    }
    let mut idle = Vec::new();
    for r in first.iter().copied().filter(|r| full.iter().all(|(holders, _)| holders.iter().any(|h| h.id == r.id))) {
        if closable(tx, r)? {
            idle.push(r);
        }
    }
    Ok(idle.into_iter().max_by_key(|r| r.closing_for))
}

/// What holds slots now, read once for every agent a window offers.
pub struct Slots {
    holding: Vec<Run>,
}

impl Slots {
    pub fn read(tx: &Transaction) -> Result<Slots, String> {
        Ok(Slots { holding: runs::live(tx)?.into_iter().filter(Run::holds_slot).collect() })
    }

    /// Whether a process of `agent` (none: one deleted since, its slot no one's) can start in `project`, `but` run
    /// not counted: the one it goes on from.
    pub fn check(&self, tx: &Transaction, agent: Option<&Agent>, project: &str, but: Option<&str>) -> Result<Option<Full>, String> {
        let holding: Vec<&Run> = self.holding.iter().filter(|r| Some(r.id.as_str()) != but).collect();
        judge(tx, &holding, agent, project)
    }
}

/// `check` of one agent, read now.
pub fn check(tx: &Transaction, agent: Option<&Agent>, project: &str, but: Option<&str>) -> Result<Option<Full>, String> {
    Slots::read(tx)?.check(tx, agent, project, but)
}

fn judge(tx: &Transaction, holding: &[&Run], agent: Option<&Agent>, project: &str) -> Result<Option<Full>, String> {
    let of_agent: Vec<&Run> = holding.iter().copied().filter(|r| agent.is_some_and(|a| a.id == r.agent_id)).collect();
    let of_project: Vec<&Run> = holding.iter().copied().filter(|r| r.project == project).collect();
    let limit = limits::agent_limit(tx, project)?;
    let mut full: Vec<(&[&Run], i64)> = Vec::new();
    let mut words = None;
    if let Some(a) = agent.filter(|a| of_agent.len() as i64 >= a.at_once) {
        words = Some(format!("{} is at its limit of {}: {}.", a.name, a.at_once, names(&of_agent)));
        full.push((&of_agent, a.at_once));
    }
    if let Some(most) = limit.filter(|most| of_project.len() as i64 >= *most) {
        let name = ops::project_place(tx, project)?.0;
        words.get_or_insert_with(|| format!("{name} runs {most} agents at once, its limit: {}.", names(&of_project)));
        full.push((&of_project, most));
    }
    let Some(words) = words else { return Ok(None) };
    let close = victim(tx, &full)?.map(|r| Holder { run: r.id.clone(), tasks: r.names() });
    Ok(Some(Full { words, close }))
}

/// What the board shows of the slots (AC-138): each agent's sessions, and the agents and projects at their limit.
pub fn board(tx: &Transaction) -> Result<Value, String> {
    let holding = Slots::read(tx)?.holding;
    let mut of_agent: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    let mut in_project: HashMap<&str, i64> = HashMap::new();
    for r in &holding {
        of_agent.entry(&r.agent_id).or_default().push(json!({"tasks": r.names(), "state": r.state, "key": r.pane_key, "project": r.project, "idle": !r.busy()}));
        *in_project.entry(&r.project).or_default() += 1;
    }
    let full: Vec<String> = agents::list(tx)?.into_iter().filter(|a| of_agent.get(a.id.as_str()).map_or(0, Vec::len) as i64 >= a.at_once).map(|a| a.id).collect();
    let full_projects: Vec<String> = limits::agent_limits(tx)?.into_iter().filter(|(p, most)| in_project.get(p.as_str()).copied().unwrap_or(0) >= *most).map(|(p, _)| p).collect();
    Ok(json!({"agents": of_agent, "full": full, "full_projects": full_projects}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::ops::{sql, NewTask};
    use crate::tasks::store::{Store, HUMAN};

    fn store(name: &str) -> Store {
        Store::open(crate::testutil::fresh_dir(name).join("kanban.sqlite"))
    }

    fn agent(id: &str, at_once: i64) -> Agent {
        Agent { id: id.into(), name: id.to_uppercase(), kind: "claude".into(), command: "claude".into(), effort: String::new(), color: 0, line: "claude".into(), at_once }
    }

    fn project(tx: &Transaction, id: &str, limit: Option<i64>) -> Result<(), String> {
        tx.execute("INSERT INTO projects (id, name, color, dir, place, agent_limit) VALUES (?1, ?1, 0, '/srv', 0, ?2)", (id, limit)).map(drop).map_err(sql)
    }

    /// A run on `a` in `p`, working, its task open.
    fn working(tx: &Transaction, a: &str, p: &str) -> (String, ops::Task) {
        let t = ops::create(tx, HUMAN, NewTask { title: format!("t in {p}"), project: Some(p.into()), ..Default::default() }, 1).unwrap();
        let id = runs::add(tx, &Run { agent_id: a.into(), kind: "claude".into(), command: "claude".into(), folder: "/srv".into(), ..Default::default() }, std::slice::from_ref(&t.key), 1).unwrap();
        runs::change(tx, &id, |r| { r.state = "working".into(); r.pane_key = Some(format!("tmux:local:t:%{id}")); }).unwrap();
        (id, t)
    }

    /// The same, finished and quiet.
    fn idle(tx: &Transaction, a: &str, p: &str) -> (String, ops::Task) {
        let (id, t) = working(tx, a, p);
        ops::move_to(tx, HUMAN, &t.id, "DONE", 2).unwrap();
        runs::change(tx, &id, |r| { r.turn_ended = true; r.closing_for = 60; }).unwrap();
        (id, t)
    }

    #[test]
    fn ac141_the_agent_s_limit_names_what_holds_it() {
        store("pool-agent").with(|tx| {
            project(tx, "api", None)?;
            assert_eq!(check(tx, Some(&agent("a", 1)), "api", None)?, None);
            let (_, t) = working(tx, "a", "api");
            let full = check(tx, Some(&agent("a", 1)), "api", None)?.unwrap();
            assert_eq!(full, Full { words: format!("A is at its limit of 1: {}.", t.id), close: None }, "working: nothing to close");
            assert_eq!(check(tx, Some(&agent("a", 2)), "api", None)?, None, "two at once");
            assert_eq!(check(tx, Some(&agent("b", 1)), "api", None)?, None, "another agent's slots are its own");
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac141_the_project_s_limit_counts_every_agent_and_only_its_own_runs() {
        store("pool-project").with(|tx| {
            project(tx, "api", Some(2))?;
            project(tx, "web", None)?;
            let (_, one) = working(tx, "a", "api");
            let (_, two) = working(tx, "b", "api");
            let full = check(tx, Some(&agent("c", 1)), "api", None)?.unwrap();
            assert_eq!(full.words, format!("api runs 2 agents at once, its limit: {}, {}.", one.id, two.id));
            assert_eq!(check(tx, Some(&agent("c", 1)), "web", None)?, None, "another project's");
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac141_an_idle_holder_can_be_closed_for_it_and_only_one() {
        store("pool-idle").with(|tx| {
            project(tx, "api", None)?;
            let (old, t) = idle(tx, "a", "api");
            let full = check(tx, Some(&agent("a", 1)), "api", None)?.unwrap();
            assert_eq!(full.close, Some(Holder { run: old.clone(), tasks: t.id.clone() }));
            working(tx, "a", "api");
            assert_eq!(check(tx, Some(&agent("a", 1)), "api", None)?.unwrap().close, None, "over its limit: one closing would not do, a human may have typed");
            assert!(check(tx, Some(&agent("a", 2)), "api", None)?.unwrap().close.is_some(), "at its limit of two, one idle");
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac141_both_limits_need_one_holder_of_both() {
        store("pool-both").with(|tx| {
            project(tx, "api", Some(1))?;
            project(tx, "web", None)?;
            idle(tx, "a", "web");
            working(tx, "b", "api");
            assert_eq!(check(tx, Some(&agent("a", 1)), "api", None)?.unwrap().close, None, "closing web's frees nothing of api's");
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac141_a_session_with_another_task_open_is_not_idle() {
        store("pool-other").with(|tx| {
            project(tx, "api", None)?;
            let (id, _) = idle(tx, "a", "api");
            let key = runs::change(tx, &id, |_| {})?.pane_key.unwrap();
            let taken = ops::create(tx, HUMAN, NewTask { title: "taken".into(), project: Some("api".into()), session: Some(key), ..Default::default() }, 1)?;
            assert_eq!(check(tx, Some(&agent("a", 1)), "api", None)?.unwrap().close, None, "{} stands on it by take_task", taken.id);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac141_what_it_goes_on_from_does_not_count_and_a_deleted_agent_has_no_slot() {
        store("pool-but").with(|tx| {
            project(tx, "api", None)?;
            let (id, _) = working(tx, "a", "api");
            assert_eq!(check(tx, Some(&agent("a", 1)), "api", Some(&id))?, None);
            assert_eq!(check(tx, None, "api", None)?, None, "only the project's limit could stop it");
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac141_what_is_over_or_gone_holds_nothing() {
        store("pool-over").with(|tx| {
            project(tx, "api", None)?;
            for state in ["closed", "left", "gone"] {
                let (id, _) = working(tx, "a", "api");
                assert!(check(tx, Some(&agent("a", 1)), "api", None)?.is_some());
                runs::change(tx, &id, |r| r.state = state.into())?;
                assert_eq!(check(tx, Some(&agent("a", 1)), "api", None)?, None, "{state}");
            }
            Ok(())
        }).unwrap();
    }
}
