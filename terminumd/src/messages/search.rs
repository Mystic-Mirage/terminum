// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The Message history's search (terminum.md AC-151): one filter the list, its day strip, its
//! count and the Dashboard's ways into it share; the words typed made into an FTS5 query no text
//! can turn into syntax; pages of PAGE rows, the newest by a cursor, the best matches (BM25) by an
//! offset up to MOST.

use rusqlite::types::Value as Sql;
use rusqlite::Transaction;
use serde_json::{json, Value};

use super::{columns, item, sql};

/// The project id of messages sent in no project, in addresses (the store keeps NULL).
pub const NONE: &str = "-";
/// Rows a page; the most best matches paged through.
pub const PAGE: usize = 50;
pub const MOST: usize = 1000;
/// The highlight's marks around each hit in a text: never in what a page shows as it is.
pub const HIT: char = '\u{2}';
pub const HIT_END: char = '\u{3}';

/// What the list is narrowed to; every field optional.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    /// the words as typed
    pub q: String,
    /// local days, `YYYY-MM-DD`, both inclusive
    pub from: Option<String>,
    pub to: Option<String>,
    /// session list projects' ids; NONE for none
    pub projects: Vec<String>,
    /// `human` or `dispatcher`
    pub sender: Option<String>,
    /// `agent` or `shell`
    pub kind: Option<String>,
    pub pane: Option<String>,
}

/// `q` as an FTS5 query: each word a prefix, a "quoted phrase" as it is, a -word left out, all
/// joined by AND; every term quoted, so nothing typed is syntax; words of one letter are dropped.
/// None when no word to look for is left (a -word alone finds nothing to rank).
pub fn fts_query(q: &str) -> Option<String> {
    let quote = |t: &str| format!("\"{}\"", t.replace('"', "\"\""));
    let (mut want, mut not) = (Vec::new(), Vec::new());
    let mut rest = q.trim();
    while !rest.is_empty() {
        let (neg, r) = match rest.strip_prefix('-') { Some(r) => (true, r), None => (false, rest) };
        let (term, phrase, after) = if let Some(r) = r.strip_prefix('"') {
            let end = r.find('"').unwrap_or(r.len());
            (&r[..end], true, r.get(end + 1..).unwrap_or(""))
        } else {
            let end = r.find(char::is_whitespace).unwrap_or(r.len());
            (&r[..end], false, &r[end..])
        };
        rest = after.trim_start();
        let words = term.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).count();
        if words == 0 || (!phrase && term.chars().filter(|c| c.is_alphanumeric()).count() < 2) {
            continue;
        }
        match (neg, phrase) {
            (true, _) => not.push(quote(term)),
            (false, true) => want.push(quote(term)),
            (false, false) => want.push(format!("{}*", quote(term))),
        }
    }
    if want.is_empty() {
        return None;
    }
    let mut out = want.join(" AND ");
    for n in not {
        out.push_str(&format!(" NOT {n}"));
    }
    Some(out)
}

/// A local day as typed, if it is one.
fn day(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|d| d.len() == 10 && d.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() }))
}

impl Filter {
    /// The SQL condition on `m` (the messages) and its values, dates left out when `dates` is false.
    pub fn condition(&self, dates: bool) -> (String, Vec<Sql>) {
        let mut parts = vec!["1".to_string()];
        let mut args: Vec<Sql> = Vec::new();
        let mut arg = |parts: &mut Vec<String>, cond: &str, v: Sql| {
            args.push(v);
            parts.push(cond.replace('?', &format!("?{}", args.len())));
        };
        if dates {
            if let Some(d) = day(&self.from) {
                arg(&mut parts, "m.at >= CAST(strftime('%s', ?, 'utc') AS INTEGER) * 1000", Sql::Text(d.into()));
            }
            if let Some(d) = day(&self.to) {
                arg(&mut parts, "m.at < CAST(strftime('%s', ?, '+1 day', 'utc') AS INTEGER) * 1000", Sql::Text(d.into()));
            }
        }
        if let Some(s) = &self.sender {
            arg(&mut parts, "m.sender = ?", Sql::Text(s.clone()));
        }
        match self.kind.as_deref() {
            Some("agent") => parts.push("coalesce(m.agent, '') != ''".into()),
            Some("shell") => parts.push("coalesce(m.agent, '') = ''".into()),
            _ => {}
        }
        if let Some(p) = &self.pane {
            arg(&mut parts, "m.pane = ?", Sql::Text(p.clone()));
        }
        if !self.projects.is_empty() {
            let mut any = Vec::new();
            for p in &self.projects {
                args.push(Sql::Text(if p == NONE { String::new() } else { p.clone() }));
                any.push(format!("coalesce(m.project, '') = ?{}", args.len()));
            }
            parts.push(format!("({})", any.join(" OR ")));
        }
        (parts.join(" AND "), args)
    }

    /// FROM and WHERE of the filtered messages, the words' match first when there are words.
    fn from(&self, dates: bool) -> (String, Vec<Sql>, bool) {
        let (cond, mut args) = self.condition(dates);
        match fts_query(&self.q) {
            Some(m) => {
                args.push(Sql::Text(m));
                (format!("FROM messages_fts JOIN messages m ON m.id = messages_fts.rowid WHERE messages_fts MATCH ?{} AND {cond}", args.len()), args, true)
            }
            None => (format!("FROM messages m WHERE {cond}"), args, false),
        }
    }
}

/// A page of the list: `best` ranks by BM25 (when there are words), else newest first; `cursor`
/// the last page's `next`. Each item has its text, with HIT marks around the words' hits
/// (`hl`) when there are words. {items, next, total (the first page's), capped}
pub fn page(tx: &Transaction, f: &Filter, best: bool, cursor: Option<&str>) -> Result<Value, String> {
    let (from, mut args, words) = f.from(true);
    // counted once, with the first page
    let total: Option<i64> = match cursor {
        None => Some(tx.query_row(&format!("SELECT count(*) {from}"), rusqlite::params_from_iter(args.iter()), |r| r.get(0)).map_err(sql)?),
        Some(_) => None,
    };
    let cols = columns("m.");
    let hl = if words { format!(", highlight(messages_fts, 0, char({}), char({}))", HIT as u32, HIT_END as u32) } else { String::new() };
    let ranked = best && words;
    let (order, offset) = if ranked {
        let offset = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0).min(MOST);
        ("ORDER BY bm25(messages_fts), m.at DESC, m.id DESC".to_string(), offset)
    } else {
        let mut keyset = String::new();
        if let Some((at, id)) = cursor.and_then(|c| c.split_once(':')).and_then(|(a, i)| Some((a.parse::<i64>().ok()?, i.parse::<i64>().ok()?))) {
            args.push(Sql::Integer(at));
            args.push(Sql::Integer(id));
            keyset = format!("AND (m.at, m.id) < (?{}, ?{})", args.len() - 1, args.len());
        }
        (format!("{keyset} ORDER BY m.at DESC, m.id DESC"), 0)
    };
    let limit = if ranked { PAGE.min(MOST.saturating_sub(offset)) } else { PAGE };
    let query = format!("SELECT {cols}{hl} {from} {order} LIMIT {} OFFSET {offset}", limit + 1);
    let mut st = tx.prepare(&query).map_err(sql)?;
    let mut items: Vec<Value> = st.query_map(rusqlite::params_from_iter(args.iter()), |r| {
        let mut v = item(r)?;
        if words {
            v["hl"] = json!(r.get::<_, String>(super::N_COLUMNS)?);
        }
        Ok(v)
    }).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    let more = items.len() > limit;
    items.truncate(limit);
    let next = match (more, items.last()) {
        (false, _) | (_, None) => Value::Null,
        (true, Some(_)) if ranked => json!((offset + limit).to_string()),
        (true, Some(last)) => json!(format!("{}:{}", last["ms"], last["id"])),
    };
    Ok(json!({"items": items, "next": next, "total": total, "capped": ranked && total.is_some_and(|t| t as usize > MOST)}))
}

/// Messages a local day of the filtered list, dates left out (the day strip): [{day, n}], oldest first.
pub fn days(tx: &Transaction, f: &Filter) -> Result<Vec<Value>, String> {
    let (from, args, _) = f.from(false);
    let mut st = tx.prepare(&format!("SELECT date(m.at / 1000, 'unixepoch', 'localtime') AS d, count(*) {from} GROUP BY d ORDER BY d")).map_err(sql)?;
    let rows = st.query_map(rusqlite::params_from_iter(args.iter()), |r| Ok(json!({"day": r.get::<_, String>(0)?, "n": r.get::<_, i64>(1)?}))).map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

/// The `before` messages sent in message `id`'s pane before it and the `after` after it, oldest first.
pub fn context(tx: &Transaction, id: i64, before: usize, after: usize) -> Result<Option<Value>, String> {
    let Some(it) = super::by_id(tx, id)? else { return Ok(None) };
    let (pane, at) = (it["key"].as_str().unwrap_or("").to_string(), it["ms"].as_i64().unwrap_or(0));
    let side = |cmp: &str, order: &str, most: usize| -> Result<Vec<Value>, String> {
        let mut st = tx.prepare(&format!("SELECT {} FROM messages WHERE pane = ?1 AND (at, id) {cmp} (?2, ?3) ORDER BY at {order}, id {order} LIMIT ?4", columns(""))).map_err(sql)?;
        let rows = st.query_map(rusqlite::params![pane, at, id, most as i64], item).map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)
    };
    let mut earlier = side("<", "DESC", before)?;
    earlier.reverse();
    Ok(Some(json!({"item": it, "before": earlier, "after": side(">", "ASC", after)?})))
}

/// What the filters offer: the projects with their counts, most first; the first day kept.
pub fn meta(tx: &Transaction) -> Result<Value, String> {
    let mut st = tx.prepare("SELECT coalesce(project, ''), max(project_name), count(*) AS n FROM messages GROUP BY coalesce(project, '') ORDER BY n DESC").map_err(sql)?;
    let projects: Vec<Value> = st.query_map([], |r| Ok(json!({"id": r.get::<_, String>(0)?, "name": r.get::<_, Option<String>>(1)?, "n": r.get::<_, i64>(2)?})))
        .map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    let first: Option<String> = tx.query_row("SELECT date(min(at) / 1000, 'unixepoch', 'localtime') FROM messages", [], |r| r.get(0)).map_err(sql)?;
    Ok(json!({"projects": projects, "first": first}))
}
