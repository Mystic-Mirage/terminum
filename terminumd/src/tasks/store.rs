// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The tasks' store (AC-119): one SQLite file, each change one transaction. Called off the async
//! workers. A file that is not a database or is corrupt is set aside, never written over, and an
//! empty one is made only when the user asks; one that is busy, not permitted or newer than this
//! terminum stays as it is, said, and is tried again at the next call.

use std::path::{Path, PathBuf};
use std::time::Duration;

use parking_lot::Mutex;
use rusqlite::{Connection, ErrorCode, Transaction};

/// The schema's versions, oldest first: `PRAGMA user_version` says how many have run.
const MIGRATIONS: [&str; 1] = [include_str!("schema1.sql")];

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

/// What opening found.
enum Opened {
    Ready(Connection),
    /// Not a database or corrupt: the file must go aside.
    Broken(String),
    Refused(String),
}

fn message(e: &rusqlite::Error) -> String {
    format!("The tasks' store: {e}")
}

fn is_broken(e: &rusqlite::Error) -> bool {
    matches!(e.sqlite_error_code(), Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt))
}

/// Brings `conn` to `migrations.len()`, after a copy of a store that has a schema already.
fn upgrade(conn: &mut Connection, path: &Path, migrations: &[&str]) -> rusqlite::Result<()> {
    let version = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
    if version >= migrations.len() {
        return Ok(());
    }
    if version > 0 && path.as_os_str() != ":memory:" {
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
    let version = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
    if version > MIGRATIONS.len() {
        return Ok(Err("The tasks' store is newer than this terminum: upgrade terminum.".into()));
    }
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    upgrade(&mut conn, path, &MIGRATIONS)?;
    let name = std::env::var("USER").unwrap_or_else(|_| "you".into());
    conn.execute("UPDATE users SET name = ?1 WHERE id = ?2 AND name = ''", (name, HUMAN))?;
    Ok(Ok(conn))
}

fn open_file(path: &Path) -> Opened {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir) {
            return Opened::Refused(format!("The tasks' store: {e}"));
        }
    }
    // SQLite gives its -wal and -shm the main file's permissions
    if let Err(e) = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(path) {
        return Opened::Refused(format!("The tasks' store: {e}"));
    }
    let done = Connection::open(path).and_then(|c| prepare(c, path));
    match done {
        Ok(Ok(c)) => Opened::Ready(c),
        Ok(Err(why)) => Opened::Refused(why),
        Err(e) if is_broken(&e) => Opened::Broken(message(&e)),
        Err(e) => Opened::Refused(message(&e)),
    }
}

/// The store and its -wal and -shm go aside as `kanban.sqlite.bad-<time>`; where it went.
fn set_aside(path: &Path) -> String {
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let aside = path.with_file_name(format!("kanban.sqlite.bad-{at}"));
    for ext in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{ext}", path.display()));
        if from.exists() {
            let to = PathBuf::from(format!("{}{ext}", aside.display()));
            if let Err(e) = std::fs::rename(&from, &to) {
                tracing::warn!(event = "tasks.setaside", error = %e);
            }
        }
    }
    aside.display().to_string()
}

fn state_of(path: &Path) -> State {
    match open_file(path) {
        Opened::Ready(c) => State::Open(c),
        Opened::Refused(why) => State::Failed(why),
        Opened::Broken(why) => {
            let aside = set_aside(path);
            tracing::warn!(event = "tasks.broken", why = %why, aside = %aside);
            State::SetAside(format!("The tasks' store could not be read and was set aside as {aside}."))
        }
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
        let tx = conn.transaction().map_err(|e| message(&e))?;
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

    /// SQLite's quick check, once after the start, holding up no list: a corrupt file goes aside.
    pub fn quick_check(&self) {
        let mut state = self.state.lock();
        let State::Open(conn) = &*state else { return };
        let verdict: rusqlite::Result<String> = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0));
        let bad = match verdict {
            Ok(v) => (v != "ok").then_some(v),
            Err(e) if is_broken(&e) => Some(e.to_string()),
            Err(e) => {
                tracing::warn!(event = "tasks.check", error = %e);
                None
            }
        };
        if let Some(why) = bad {
            *state = State::Failed(String::new());     // closes the file before it moves
            let aside = set_aside(&self.path);
            tracing::warn!(event = "tasks.broken", why = %why, aside = %aside);
            *state = State::SetAside(format!("The tasks' store could not be read and was set aside as {aside}."));
        }
    }

    /// The user's press after a store went aside (AC-119).
    pub fn start_empty(&self) -> Result<(), String> {
        let mut state = self.state.lock();
        if !matches!(*state, State::SetAside(_)) {
            return Err("The tasks' store was not set aside.".into());
        }
        *state = state_of(&self.path);
        match &*state {
            State::Open(_) | State::Absent => Ok(()),
            State::Failed(why) | State::SetAside(why) => Err(why.clone()),
        }
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
