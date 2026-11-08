// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What sessions spent (`terminum-dispatcher.md` AC-129, AC-143): a hook reports its session's totals, the
//! store keeps what grew, counted once; a task shows its sessions' sum, an agent its last 7 and 30 days.
//! Dollars are worked out when shown, so a corrected price corrects every row.

use std::collections::HashMap;

use rusqlite::Transaction;
use serde_json::{json, Value};

use crate::tasks::ops::{rows, sql};

/// What a session reports: its totals so far, and its model if said.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Totals {
    pub input: i64,
    pub output: i64,
    pub cache_write: i64,
    pub cache_read: i64,
}

/// Past this a number is no count of tokens: SQLite's integers hold it, a made-up hook's cannot overflow.
const MOST: u64 = 1 << 52;

/// The longest model name kept: the hook's and the store's.
pub const MOST_MODEL: usize = 60;

impl Totals {
    /// A hook's numbers; `None` when one is missing.
    pub fn of(t: &Value) -> Option<Totals> {
        let n = |k: &str| t[k].as_u64().map(|v| v.min(MOST) as i64);
        Some(Totals { input: n("input")?, output: n("output")?, cache_write: n("cache_write").unwrap_or(0), cache_read: n("cache_read")? })
    }

    fn fields(&self) -> [i64; 4] {
        [self.input, self.output, self.cache_write, self.cache_read]
    }

    fn from_fields([input, output, cache_write, cache_read]: [i64; 4]) -> Totals {
        Totals { input, output, cache_write, cache_read }
    }

    /// Tokens that count against a limit: cache reads are not (AC-143).
    pub fn counted(&self) -> i64 {
        self.input + self.output + self.cache_write
    }

    /// As the hook says them.
    pub fn json(&self) -> Value {
        json!({"input": self.input, "output": self.output, "cache_write": self.cache_write, "cache_read": self.cache_read})
    }

    /// As the board shows them: with the tokens that count, so the Page adds nothing up.
    pub fn shown(&self) -> Value {
        let mut v = self.json();
        v["counted"] = json!(self.counted());
        v
    }
}

impl std::ops::Add for Totals {
    type Output = Totals;
    fn add(self, o: Totals) -> Totals {
        let (a, b) = (self.fields(), o.fields());
        Totals::from_fields(std::array::from_fn(|i| a[i].saturating_add(b[i])))
    }
}

/// What grew since `seen`; all of `now` when a total fell (a /clear), nothing when nothing grew.
fn grown(seen: Totals, now: Totals) -> Totals {
    let (s, n) = (seen.fields(), now.fields());
    if n.iter().zip(&s).any(|(n, s)| n < s) {
        return now;
    }
    Totals::from_fields(std::array::from_fn(|i| n[i] - s[i]))
}

/// A report of run `run`: what grew is kept, and the run remembers what it has heard.
pub fn heard(tx: &Transaction, run: &str, agent: &str, now: Totals, model: Option<&str>, at: i64) -> Result<(), String> {
    let seen = tx.query_row("SELECT seen_input, seen_output, seen_write, seen_read FROM runs WHERE id = ?1", [run],
        |r| Ok(Totals { input: r.get(0)?, output: r.get(1)?, cache_write: r.get(2)?, cache_read: r.get(3)? })).map_err(sql)?;
    let d = grown(seen, now);
    if d == Totals::default() {
        return Ok(());
    }
    tx.execute("INSERT INTO usage (run, agent_id, at, input, output, cache_write, cache_read, model) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
               rusqlite::params![run, agent, at, d.input, d.output, d.cache_write, d.cache_read, model]).map_err(sql)?;
    tx.execute("UPDATE runs SET seen_input = ?2, seen_output = ?3, seen_write = ?4, seen_read = ?5 WHERE id = ?1",
               rusqlite::params![run, now.input, now.output, now.cache_write, now.cache_read]).map(drop).map_err(sql)
}

/// A run going on from `from` in the same conversation has heard what it had heard (AC-122).
pub fn carry(tx: &Transaction, from: &str, to: &str) -> Result<(), String> {
    tx.execute("UPDATE runs SET (seen_input, seen_output, seen_write, seen_read) = (SELECT seen_input, seen_output, seen_write, seen_read FROM runs WHERE id = ?1) WHERE id = ?2",
               (from, to)).map(drop).map_err(sql)
}

/// List prices in dollars per million tokens, Anthropic's (cached 2026-10-06): input, output, cache read; a cache
/// write is 1.25 times input (the 5-minute one). The longest id that begins the model's name is its.
const PRICES: [(&str, f64, f64, f64); 13] = [
    ("claude-fable-5-1", 10.0, 50.0, 0.25),
    ("claude-mythos-5-1", 10.0, 50.0, 0.25),
    ("claude-fable-5", 10.0, 50.0, 1.0),
    ("claude-opus-5-5", 4.0, 20.0, 0.20),
    ("claude-opus-5", 5.0, 25.0, 0.50),
    ("claude-opus-4-8", 5.0, 25.0, 0.50),
    ("claude-opus-4-7", 5.0, 25.0, 0.50),
    ("claude-opus-4-6", 5.0, 25.0, 0.50),
    ("claude-sonnet-5-5", 2.0, 10.0, 0.20),
    ("claude-sonnet-5", 2.0, 10.0, 0.20),
    ("claude-sonnet-4-6", 3.0, 15.0, 0.30),
    ("claude-haiku-5-5", 0.10, 0.50, 0.01),
    ("claude-haiku-4-5", 1.0, 5.0, 0.10),
];

/// An estimate at list prices, when the model is one terminum knows.
pub fn dollars(t: &Totals, model: &str) -> Option<f64> {
    let (_, input, output, read) = PRICES.iter().filter(|(id, ..)| model.starts_with(id)).max_by_key(|(id, ..)| id.len())?;
    Some((t.input as f64 * input + t.output as f64 * output + t.cache_write as f64 * input * 1.25 + t.cache_read as f64 * read) / 1e6)
}

/// What each of `keys` spent in every session it was in: the sums, dollars when every model's price is known, and
/// the other tasks it shared a session with (AC-129).
pub fn of_tasks(tx: &Transaction, keys: &[&str]) -> Result<HashMap<String, Value>, String> {
    let listed = super::runs::listed_keys(keys);
    let by: Vec<(String, Option<String>, Totals)> = rows(tx, "SELECT m.task, u.model, sum(u.input), sum(u.output), sum(u.cache_write), sum(u.cache_read)
        FROM run_tasks m JOIN usage u ON u.run = m.run WHERE m.task IN (SELECT value FROM json_each(?1)) GROUP BY m.task, u.model",
        [&listed], |r| Ok((r.get(0)?, r.get(1)?, Totals { input: r.get(2)?, output: r.get(3)?, cache_write: r.get(4)?, cache_read: r.get(5)? })))?;
    let mut out: HashMap<String, (Totals, Option<f64>)> = HashMap::new();
    for (task, model, t) in by {
        let usd = model.as_deref().and_then(|m| dollars(&t, m));
        let (sum, cost) = out.entry(task).or_insert((Totals::default(), Some(0.0)));
        *sum = *sum + t;
        *cost = cost.zip(usd).map(|(a, b)| a + b);
    }
    let shared: Vec<(String, String)> = rows(tx, "SELECT DISTINCT m.task, 'T-' || x.number, x.number FROM run_tasks m JOIN run_tasks o ON o.run = m.run AND o.task != m.task
        JOIN tasks x ON x.id = o.task WHERE m.task IN (SELECT value FROM json_each(?1)) AND EXISTS (SELECT 1 FROM usage u WHERE u.run = m.run) ORDER BY x.number",
        [&listed], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut with: HashMap<String, Vec<String>> = HashMap::new();
    for (task, number) in shared {
        with.entry(task).or_default().push(number);
    }
    Ok(out.into_iter().map(|(task, (t, usd))| {
        let mut v = t.shown();
        v["usd"] = json!(usd);
        v["with"] = json!(with.remove(&task).unwrap_or_default());
        (task, v)
    }).collect())
}

/// What each agent spent in the last 7 and 30 days, of every project: tokens that count (AC-143).
pub fn of_agents(tx: &Transaction, now: i64) -> Result<HashMap<String, (i64, i64)>, String> {
    let (week, month) = (now - 7 * 86_400, now - 30 * 86_400);
    Ok(rows(tx, "SELECT agent_id, sum(CASE WHEN at >= ?1 THEN input + output + cache_write ELSE 0 END), sum(input + output + cache_write) FROM usage WHERE at >= ?2 GROUP BY agent_id",
            (week, month), |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::runs::{self, Run};
    use crate::tasks::ops::{self, NewTask};
    use crate::tasks::store::{Store, HUMAN};

    fn t(input: i64, output: i64, cache_write: i64, cache_read: i64) -> Totals {
        Totals { input, output, cache_write, cache_read }
    }

    fn run_of(tx: &Transaction, agent: &str, tasks: &[String]) -> String {
        runs::add(tx, &Run { agent_id: agent.into(), kind: "claude".into(), command: "claude".into(), folder: "/srv".into(), project: "default".into(), ..Default::default() }, tasks, 1).unwrap()
    }

    #[test]
    fn ac129_what_grew_is_counted_once() {
        let s = Store::open(crate::testutil::fresh_dir("usage-once").join("kanban.sqlite"));
        s.with(|tx| {
            let task = ops::create(tx, HUMAN, NewTask { title: "t".into(), ..Default::default() }, 1)?;
            let r = run_of(tx, "a", std::slice::from_ref(&task.key));
            heard(tx, &r, "a", t(1000, 100, 50, 5000), Some("claude-opus-5-5"), 10)?;
            heard(tx, &r, "a", t(1000, 100, 50, 5000), Some("claude-opus-5-5"), 11)?;
            heard(tx, &r, "a", t(1500, 120, 50, 6000), Some("claude-opus-5-5"), 12)?;
            let spent = &of_tasks(tx, &[task.key.as_str()])?[&task.key];
            assert_eq!((spent["input"].as_i64(), spent["output"].as_i64(), spent["cache_read"].as_i64()), (Some(1500), Some(120), Some(6000)), "a Stop and a SessionEnd of one total add nothing");
            assert_eq!(spent["counted"], 1500 + 120 + 50, "cache reads do not count");
            heard(tx, &r, "a", t(200, 10, 0, 0), None, 13)?;
            assert_eq!(of_tasks(tx, &[task.key.as_str()])?[&task.key]["input"], 1700, "a total that fell, after a /clear: all of it");
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac129_a_resumed_conversation_adds_only_what_grew() {
        let s = Store::open(crate::testutil::fresh_dir("usage-resume").join("kanban.sqlite"));
        s.with(|tx| {
            let task = ops::create(tx, HUMAN, NewTask { title: "t".into(), ..Default::default() }, 1)?;
            let first = run_of(tx, "a", std::slice::from_ref(&task.key));
            heard(tx, &first, "a", t(1000, 100, 0, 0), None, 10)?;
            let next = run_of(tx, "a", std::slice::from_ref(&task.key));
            carry(tx, &first, &next)?;
            heard(tx, &next, "a", t(1300, 130, 0, 0), None, 20)?;
            assert_eq!(of_tasks(tx, &[task.key.as_str()])?[&task.key]["input"], 1300);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac129_several_tasks_say_the_session_s_whole_and_whom_with() {
        let s = Store::open(crate::testutil::fresh_dir("usage-shared").join("kanban.sqlite"));
        s.with(|tx| {
            let a = ops::create(tx, HUMAN, NewTask { title: "a".into(), ..Default::default() }, 1)?;
            let b = ops::create(tx, HUMAN, NewTask { title: "b".into(), ..Default::default() }, 1)?;
            let r = run_of(tx, "x", &[a.key.clone(), b.key.clone()]);
            heard(tx, &r, "x", t(40_000, 8_000, 2_000, 90_000), Some("claude-opus-5-5"), 10)?;
            let spent = of_tasks(tx, &[a.key.as_str(), b.key.as_str()])?;
            assert_eq!(spent[&a.key]["input"], spent[&b.key]["input"]);
            assert_eq!(spent[&a.key]["with"], json!([b.id]));
            let usd = spent[&a.key]["usd"].as_f64().unwrap();
            assert!((usd - (40_000.0 * 4.0 + 8_000.0 * 20.0 + 2_000.0 * 5.0 + 90_000.0 * 0.2) / 1e6).abs() < 1e-9, "{usd}");
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ac129_a_price_it_does_not_know_gives_no_dollars() {
        assert_eq!(dollars(&t(1_000_000, 0, 0, 0), "gpt-5-codex"), None);
        assert_eq!(dollars(&t(1_000_000, 0, 0, 0), "claude-opus-5-5"), Some(4.0), "Opus 5.5, not Opus 5");
        assert_eq!(dollars(&t(0, 1_000_000, 0, 0), "claude-opus-5-20260101"), Some(25.0));
        assert_eq!(Totals::of(&json!({"input": 1u64 << 60, "output": 1, "cache_read": 0})).unwrap().input, 1 << 52, "a made-up number is capped");
        assert_eq!(Totals::of(&json!({"input": 1})), None);
    }

    #[test]
    fn ac143_an_agent_s_week_and_month_count_every_project() {
        let s = Store::open(crate::testutil::fresh_dir("usage-agent").join("kanban.sqlite"));
        s.with(|tx| {
            let now = 100 * 86_400;
            let r = run_of(tx, "a", &[]);
            heard(tx, &r, "a", t(100, 10, 5, 999), None, now - 20 * 86_400)?;
            heard(tx, &r, "a", t(300, 30, 5, 999), None, now - 86_400)?;
            heard(tx, &r, "a", t(400, 30, 5, 999), None, now - 40 * 86_400)?;
            let (week, month) = of_agents(tx, now)?["a"];
            assert_eq!((week, month), (220, 335), "reads of the cache not counted; a report 40 days old out of the month");
            Ok(())
        }).unwrap();
    }
}
