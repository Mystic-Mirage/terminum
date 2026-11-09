// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The tasks' store (AC-119): one SQLite file, each change one transaction. Called off the async
//! workers. A file that is not a database or is corrupt is set aside, never written over, and an
//! empty one is made only when the user asks; one that is busy, not permitted or newer than this
//! terminum stays as it is, said, and is tried again at the next call.

use std::path::{Path, PathBuf};
use std::time::Duration;

use parking_lot::Mutex;
use rusqlite::{Connection, ErrorCode, Transaction, TransactionBehavior};

/// The schema's versions, oldest first: `PRAGMA user_version` says how many have run.
const MIGRATIONS: [&str; 11] = [include_str!("schema1.sql"), include_str!("schema2.sql"), include_str!("schema3.sql"), include_str!("schema4.sql"), include_str!("schema5.sql"), include_str!("schema6.sql"), include_str!("schema7.sql"), include_str!("schema8.sql"), include_str!("schema9.sql"), include_str!("schema10.sql"), include_str!("schema12.sql")];

/// The default project's and Dispatcher's fixed IDs, seeded by schema1.sql.
pub const DEFAULT_PROJECT: &str = "default";
pub const DISPATCHER: &str = "dispatcher";
/// The one human of a single sign-in (AC-119).
pub const HUMAN: &str = "human";

enum State {
    Open(Connection),
    /// Why it could not be used; the next call tries again.
    Failed(String),
    /// Set aside as unreadable: nothing is made until `start_empty`.
    SetAside(String),
    /// An Agent's: it keeps no tasks.
    Absent,
}

pub struct Store {
    path: PathBuf,
    state: Mutex<State>,
}

pub(super) fn message(e: &impl std::fmt::Display) -> String {
    format!("The tasks' store: {e}")
}

fn schema_version(conn: &Connection) -> rusqlite::Result<usize> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).map(|v| v as usize)
}

fn is_broken(e: &rusqlite::Error) -> bool {
    matches!(e.sqlite_error_code(), Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt))
}

/// Brings `conn` to `migrations.len()`, after a copy of a store that has a schema already.
fn upgrade(conn: &mut Connection, path: &Path, migrations: &[&str]) -> rusqlite::Result<()> {
    let version = schema_version(conn)?;
    if version >= migrations.len() {
        return Ok(());
    }
    if version > 0 {
        let backup = path.with_file_name(format!("kanban.sqlite.bak-{version}"));
        let _ = std::fs::remove_file(&backup);
        conn.execute("VACUUM INTO ?1", [backup.display().to_string()])?;
    }
    let tx = conn.transaction()?;
    for sql in &migrations[version..] {
        tx.execute_batch(sql)?;
    }
    tx.pragma_update(None, "user_version", migrations.len() as i64)?;
    tx.commit()
}

fn prepare(mut conn: Connection, path: &Path) -> rusqlite::Result<Result<Connection, String>> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let version = schema_version(&conn)?;
    if version > MIGRATIONS.len() {
        return Ok(Err("The tasks' store is newer than this terminum: upgrade terminum.".into()));
    }
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // a purge means it (AC-109): freed pages are overwritten, not left in the file
    conn.pragma_update(None, "secure_delete", "ON")?;
    upgrade(&mut conn, path, &MIGRATIONS)?;
    let name = std::env::var("USER").unwrap_or_else(|_| "you".into());
    conn.execute("UPDATE users SET name = ?1 WHERE id = ?2 AND name = ''", (name, HUMAN))?;
    Ok(Ok(conn))
}

/// Beside the store while one is set aside: a restart must not make an empty one unasked (AC-119).
fn marker(path: &Path) -> PathBuf {
    path.with_file_name("kanban.sqlite.aside")
}

fn aside_message(aside: &str) -> String {
    format!("The tasks' store could not be read and was set aside as {aside}.")
}

/// The store and its -wal and -shm go aside as `kanban.sqlite.bad-<time>`; where it went.
fn set_aside(path: &Path) -> String {
    let aside = path.with_file_name(format!("kanban.sqlite.bad-{}", super::ops::now()));
    for ext in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{ext}", path.display()));
        if from.exists() {
            let to = PathBuf::from(format!("{}{ext}", aside.display()));
            if let Err(e) = std::fs::rename(&from, &to) {
                tracing::warn!(event = "tasks.setaside", error = %e);
            }
        }
    }
    let aside = aside.display().to_string();
    if let Err(e) = std::fs::write(marker(path), &aside) {
        tracing::warn!(event = "tasks.setaside", error = %e);
    }
    aside
}

fn state_of(path: &Path) -> State {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if let Ok(aside) = std::fs::read_to_string(marker(path)) {
        return State::SetAside(aside_message(&aside));
    }
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir) {
            return State::Failed(message(&e));
        }
    // SQLite gives its -wal and -shm the main file's permissions
    if let Err(e) = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(path) {
        return State::Failed(message(&e));
    }
    match Connection::open(path).and_then(|c| prepare(c, path)) {
        Ok(Ok(c)) => State::Open(c),
        Ok(Err(why)) => State::Failed(why),
        Err(e) if is_broken(&e) => {
            let aside = set_aside(path);
            tracing::warn!(event = "tasks.broken", why = %message(&e), aside = %aside);
            State::SetAside(aside_message(&aside))
        }
        Err(e) => State::Failed(message(&e)),
    }
}

impl Store {
    pub fn open(path: PathBuf) -> Store {
        let state = Mutex::new(state_of(&path));
        Store { path, state }
    }

    /// An Agent's, which never opens a file.
    pub fn absent() -> Store {
        Store { path: PathBuf::new(), state: Mutex::new(State::Absent) }
    }

    /// One transaction: `f` commits on Ok and rolls back on Err. On a store that cannot be used,
    /// its message.
    pub fn with<T>(&self, f: impl FnOnce(&Transaction) -> Result<T, String>) -> Result<T, String> {
        let mut state = self.state.lock();
        if let State::Failed(_) = *state {
            *state = state_of(&self.path);
        }
        let conn = match &mut *state {
            State::Open(c) => c,
            State::Failed(why) | State::SetAside(why) => return Err(why.clone()),
            State::Absent => return Err("An agent keeps no tasks.".into()),
        };
        // it takes the write first: read and then written, a writer from outside between them would fail it at once, past the busy timeout
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| message(&e))?;
        let out = f(&tx)?;
        tx.commit().map_err(|e| message(&e))?;
        Ok(out)
    }

    /// Why it cannot be used, for the board's banner.
    pub fn problem(&self) -> Option<String> {
        match &*self.state.lock() {
            State::Open(_) | State::Absent => None,
            State::Failed(why) | State::SetAside(why) => Some(why.clone()),
        }
    }

    /// SQLite's quick check, once after the start, on a connection of its own: it holds up no
    /// list. A corrupt file goes aside.
    pub fn quick_check(&self) {
        if !matches!(*self.state.lock(), State::Open(_)) {
            return;
        }
        let verdict = Connection::open_with_flags(&self.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .and_then(|c| c.query_row("PRAGMA quick_check(1)", [], |r| r.get::<_, String>(0)));
        let bad = match verdict {
            Ok(v) => (v != "ok").then_some(v),
            Err(e) if is_broken(&e) => Some(e.to_string()),
            Err(e) => {
                tracing::warn!(event = "tasks.check", error = %e);
                None
            }
        };
        if let Some(why) = bad {
            let mut state = self.state.lock();
            *state = State::Failed(String::new());     // closes the file before it moves
            let aside = set_aside(&self.path);
            tracing::warn!(event = "tasks.broken", why = %why, aside = %aside);
            *state = State::SetAside(aside_message(&aside));
        }
    }

    /// After a purge: what the write-ahead log still holds of the old text goes too (AC-109).
    pub fn checkpoint(&self) {
        if let State::Open(conn) = &*self.state.lock()
            && let Err(e) = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())) {
                tracing::warn!(event = "tasks.checkpoint", error = %e);
            }
    }

    /// Whether a store was set aside and waits for the user's Start empty.
    pub fn is_set_aside(&self) -> bool {
        matches!(*self.state.lock(), State::SetAside(_))
    }

    /// The user's press after a store went aside (AC-119).
    pub fn start_empty(&self) -> Result<(), String> {
        let mut state = self.state.lock();
        if !matches!(*state, State::SetAside(_)) {
            return Err("The tasks' store was not set aside.".into());
        }
        crate::config::remove_if_there(&marker(&self.path)).map_err(|e| message(&e))?;
        *state = state_of(&self.path);
        match &mut *state {
            // an older projects.json is no copy of what was set aside: the user asked for empty
            State::Open(c) => c.execute("INSERT OR IGNORE INTO meta (key, value) VALUES ('projects_imported', '1')", []).map(|_| ()).map_err(|e| message(&e)),
            State::Absent => Ok(()),
            State::Failed(why) | State::SetAside(why) => Err(why.clone()),
        }
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
