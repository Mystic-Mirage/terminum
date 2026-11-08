// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Who writes on a task: a human, an agent of the pool by its name, or an agent with no run of the pool.

use rusqlite::{OptionalExtension, Transaction};

use super::ops::sql;

pub(crate) fn user_name(tx: &Transaction, user: &str) -> Result<String, String> {
    tx.query_row("SELECT name FROM users WHERE id = ?1", [user], |r| r.get(0)).optional().map_err(sql)?.ok_or_else(|| format!("No user {user}"))
}

/// The user an agent of the pool writes as, by its name (terminum-dispatcher.md AC-137): made the first time; the
/// agent's window renames it.
pub fn pool_user(tx: &Transaction, agent: &str, name: &str) -> Result<String, String> {
    let id = pool_user_id(agent);
    tx.execute("INSERT OR IGNORE INTO users (id, kind, name, role) VALUES (?1, 'agent', ?2, 'agent')", (&id, name)).map_err(sql)?;
    Ok(id)
}

pub fn pool_user_id(agent: &str) -> String {
    format!("agent:pool:{agent}")
}

/// The user an agent with no run of the pool writes as ("claude · api"), made the first time.
pub fn agent_user(tx: &Transaction, agent: &str, session: &str) -> Result<String, String> {
    let name = format!("{agent} · {session}");
    let id = format!("agent:{name}");
    tx.execute("INSERT OR IGNORE INTO users (id, kind, name, role) VALUES (?1, 'agent', ?2, 'agent')", (&id, &name)).map_err(sql)?;
    Ok(id)
}
