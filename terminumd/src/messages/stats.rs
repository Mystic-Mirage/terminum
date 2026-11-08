// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The Dashboard's numbers (terminum.md AC-152): the human's messages and input rows (Dispatcher's
//! never), in this Mac's local time, over the last N days and the N before them up to the same
//! time of day. Active time: the human's sends and input rows in time order; a pause over GAP ends
//! a stretch, which lasts from its first to its last event and TAIL more (reading the answer).

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::Transaction;
use serde_json::{json, Value};

use super::sql;

pub const GAP: i64 = 15 * 60_000;
pub const TAIL: i64 = 2 * 60_000;
const DAY: i64 = 86_400_000;
const HOUR: i64 = 3_600_000;
/// Projects with their own color in the stacked chart; the rest are Other.
const STACKED: usize = 6;

/// A human message: local ms, project id ("" none), its name, agent ("" shell), pane.
struct Msg { local: i64, project: String, name: String, agent: String, pane: String }

/// A human input row: local first and last key, device, whether it sent (Enter, a paste).
struct Input { local: i64, ended: i64, device: String, sent: bool }

/// SQL of the local time in ms of unix ms `col` (the zone's offset at that moment, DST too).
fn local(col: &str) -> String {
    format!("{col} + (CAST(strftime('%s', {col} / 1000, 'unixepoch', 'localtime') AS INTEGER) - {col} / 1000) * 1000")
}

/// `days` days to today, local midnight to now; `project`: one project's only ("" for none).
/// `colors`: the session list's color of each project still there.
pub fn dashboard(tx: &Transaction, now_ms: i64, days: i64, project: Option<&str>, colors: &HashMap<String, usize>) -> Result<Value, String> {
    let days = days.clamp(1, 180);
    let now: i64 = tx.query_row(&format!("SELECT {}", local("?1")), [now_ms], |r| r.get(0)).map_err(sql)?;
    let today = now.div_euclid(DAY);
    let since = now_ms - (2 * days + 2) * DAY;
    let (msgs, inputs) = read(tx, since, project)?;
    let cur = Window { lo: today - days + 1, hi: today, until: now };
    let prev = Window { lo: today - 2 * days + 1, hi: today - days, until: now - days * DAY };
    let a = summarize(&msgs, &inputs, &cur, colors);
    let b = summarize(&msgs, &inputs, &prev, colors);
    let had_before = msgs.iter().any(|m| m.local.div_euclid(DAY) <= prev.hi);
    Ok(json!({"days": days, "today": civil(today), "offset_min": (now - now_ms) / 60_000,
              "now": a, "before": if had_before { b["tiles"].clone() } else { Value::Null }}))
}

struct Window { lo: i64, hi: i64, until: i64 }

impl Window {
    fn holds(&self, local: i64) -> bool {
        let d = local.div_euclid(DAY);
        d >= self.lo && d <= self.hi && local <= self.until
    }
}

fn read(tx: &Transaction, since: i64, project: Option<&str>) -> Result<(Vec<Msg>, Vec<Input>), String> {
    let only = if project.is_some() { "AND coalesce(project, '') = ?2" } else { "" };
    let p = project.unwrap_or("").to_string();
    let mut st = tx.prepare(&format!("SELECT {}, coalesce(project, ''), coalesce(project_name, ''), coalesce(agent, ''), pane FROM messages
                                      WHERE sender = 'human' AND at >= ?1 {only} ORDER BY at", local("at"))).map_err(sql)?;
    let row = |r: &rusqlite::Row| Ok(Msg { local: r.get(0)?, project: r.get(1)?, name: r.get(2)?, agent: r.get(3)?, pane: r.get(4)? });
    let msgs = if project.is_some() { st.query_map(rusqlite::params![since, p], row) } else { st.query_map(rusqlite::params![since], row) }
        .map_err(sql)?.collect::<Result<Vec<_>, _>>().map_err(sql)?;
    let mut st = tx.prepare(&format!("SELECT {}, {}, coalesce(device, ''), component = 'page' AND (kind = 'paste' OR (kind = 'typed' AND text LIKE '%<Enter>'))
                                      FROM inputs WHERE sender = 'human' AND at >= ?1 {only} ORDER BY at", local("at"), local("ended"))).map_err(sql)?;
    let row = |r: &rusqlite::Row| Ok(Input { local: r.get(0)?, ended: r.get(1)?, device: r.get(2)?, sent: r.get(3)? });
    let inputs = if project.is_some() { st.query_map(rusqlite::params![since, p], row) } else { st.query_map(rusqlite::params![since], row) }
        .map_err(sql)?.collect::<Result<Vec<_>, _>>().map_err(sql)?;
    Ok((msgs, inputs))
}

/// The stretches of active time over `events` (local ms, sorted): [start, end).
pub fn stretches(events: &[i64]) -> Vec<(i64, i64)> {
    let mut out: Vec<(i64, i64)> = Vec::new();
    for &e in events {
        match out.last_mut() {
            Some((_, end)) if e - (*end - TAIL) <= GAP => *end = (*end).max(e + TAIL),
            _ => out.push((e, e + TAIL)),
        }
    }
    out
}

fn median(mut v: Vec<i64>) -> Option<i64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[v.len() / 2])
}

/// Every number of window `w`.
fn summarize(msgs: &[Msg], inputs: &[Input], w: &Window, colors: &HashMap<String, usize>) -> Value {
    let ms: Vec<&Msg> = msgs.iter().filter(|m| w.holds(m.local)).collect();
    let ins: Vec<&Input> = inputs.iter().filter(|i| w.holds(i.local)).collect();
    let n_days = (w.hi - w.lo + 1) as usize;
    let idx = |local: i64| (local.div_euclid(DAY) - w.lo) as usize;
    // projects, most first
    let mut per_project: HashMap<&str, (i64, &str)> = HashMap::new();
    for m in &ms {
        let e = per_project.entry(m.project.as_str()).or_insert((0, m.name.as_str()));
        e.0 += 1;
        if e.1.is_empty() { e.1 = m.name.as_str(); }
    }
    let mut ranked: Vec<(&str, i64, &str)> = per_project.iter().map(|(id, (n, name))| (*id, *n, *name)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let stacked: Vec<&str> = ranked.iter().take(STACKED).map(|p| p.0).collect();
    let projects: Vec<Value> = ranked.iter().map(|(id, n, name)| json!({"id": id, "name": name, "n": n, "color": colors.get(*id)})).collect();
    // events and stretches
    let mut events: Vec<i64> = ms.iter().map(|m| m.local).chain(ins.iter().flat_map(|i| [i.local, i.ended.max(i.local)])).collect();
    events.sort_unstable();
    let runs = stretches(&events);
    let mut active = vec![0i64; n_days];
    let mut spans: Vec<Vec<[i64; 2]>> = vec![Vec::new(); n_days];
    let mut heat = vec![[0i64; 24]; 7];
    for &(s, e) in &runs {
        let mut t = s;
        while t < e {
            let next = ((t.div_euclid(HOUR)) + 1) * HOUR;
            let end = next.min(e);
            let d = t.div_euclid(DAY);
            if d >= w.lo && d <= w.hi {
                let i = (d - w.lo) as usize;
                active[i] += end - t;
                let wd = (d + 4).rem_euclid(7) as usize;              // 1970-01-01 was a Thursday; 0: Sunday
                heat[wd][(t.rem_euclid(DAY) / HOUR) as usize] += end - t;
                let (a, b) = ((t.rem_euclid(DAY)) / 60_000, (end - d * DAY) / 60_000);
                match spans[i].last_mut() {
                    Some(last) if last[1] >= a => last[1] = b,
                    _ => spans[i].push([a, b]),
                }
            }
            t = end;
        }
    }
    let mut weekdays = [0i64; 7];
    for d in w.lo..=w.hi {
        weekdays[(d + 4).rem_euclid(7) as usize] += 1;
    }
    let heat: Vec<Vec<i64>> = heat.iter().enumerate().map(|(wd, row)| row.iter().map(|v| v / 60_000 / weekdays[wd].max(1)).collect()).collect();
    // per day
    let mut count = vec![0i64; n_days];
    let mut by: Vec<BTreeMap<String, i64>> = vec![BTreeMap::new(); n_days];
    let mut panes: Vec<HashSet<&str>> = vec![HashSet::new(); n_days];
    let mut hourly: Vec<HashMap<i64, HashSet<&str>>> = vec![HashMap::new(); n_days];
    for m in &ms {
        let i = idx(m.local);
        count[i] += 1;
        let key = if stacked.contains(&m.project.as_str()) { m.project.clone() } else { "other".into() };
        *by[i].entry(key).or_default() += 1;
        panes[i].insert(m.pane.as_str());
        hourly[i].entry(m.local.div_euclid(HOUR)).or_default().insert(m.pane.as_str());
    }
    let peak: Vec<i64> = hourly.iter().map(|h| h.values().map(|s| s.len() as i64).max().unwrap_or(0)).collect();
    let series: Vec<Value> = (0..n_days).map(|i| json!({"day": civil(w.lo + i as i64), "n": count[i], "by": by[i], "active_ms": active[i],
        "spans": spans[i], "sessions": panes[i].len(), "peak": peak[i]})).collect();
    // agents or a shell; the devices that sent
    let mut kinds: BTreeMap<&str, i64> = BTreeMap::new();
    for m in &ms {
        *kinds.entry(if m.agent.is_empty() { "shell" } else { m.agent.as_str() }).or_default() += 1;
    }
    let mut devices: BTreeMap<String, i64> = BTreeMap::new();
    for i in ins.iter().filter(|i| i.sent) {
        *devices.entry(if i.device.is_empty() { "mac".into() } else { i.device.clone() }).or_default() += 1;
    }
    let active_days: Vec<usize> = (0..n_days).filter(|&i| count[i] > 0 || active[i] > 0).collect();
    let total_active: i64 = active.iter().sum();
    let sent_by_device: i64 = devices.values().sum();
    let tiles = json!({
        "messages": ms.len(), "active_ms": total_active, "active_days": active_days.len(), "days": n_days,
        "per_hour": if total_active > 0 { ms.len() as f64 / (total_active as f64 / HOUR as f64) } else { 0.0 },
        "sessions": median(active_days.iter().map(|&i| panes[i].len() as i64).collect()),
        "peak": median(active_days.iter().map(|&i| peak[i]).collect()),
        "phone": if sent_by_device > 0 { Some(*devices.get("web").unwrap_or(&0) as f64 / sent_by_device as f64) } else { None },
    });
    json!({"tiles": tiles, "series": series, "stacked": stacked, "projects": projects, "heat": heat,
           "kinds": kinds, "devices": devices})
}

/// `YYYY-MM-DD` of day `z` since 1970-01-01 (Howard Hinnant's civil_from_days).
pub fn civil(z: i64) -> String {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}
