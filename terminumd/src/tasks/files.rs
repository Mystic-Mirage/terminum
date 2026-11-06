// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Tasks' files (AC-106, AC-109): bytes in `kanban-files/<id>` beside the store, 0600 in a 0700
//! folder, a row in the store. A file dropped in a window not yet saved has no task; such a file
//! goes after 7 days. Nothing here runs or is opened on the page's origin.

use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Row, Transaction};
use serde::Serialize;

use super::ops::{self, rows, sql};
use super::store::Store;
use crate::page::upload::raster_mime;

pub const MOST_BYTES: usize = 100 << 20;
const UNUSED_SECONDS: i64 = 7 * 24 * 3600;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct File {
    pub id: String,
    pub name: String,
    pub bytes: i64,
    pub mime: String,
}

impl File {
    /// A row's id, name, bytes and mime, from column `at` on.
    pub(super) fn of_row(r: &Row, at: usize) -> rusqlite::Result<File> {
        Ok(File { id: r.get(at)?, name: r.get(at + 1)?, bytes: r.get(at + 2)?, mime: r.get(at + 3)? })
    }
}

pub fn folder(dir: &Path) -> PathBuf {
    dir.join("kanban-files")
}

/// `id` as a file's name: what terminumd made, never a path.
fn path_of(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("No such file".into());
    }
    Ok(folder(dir).join(id))
}

/// A name as kept: no folders, no control characters, 1–200 characters.
fn clean_name(name: &str) -> Result<String, String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = crate::termtext::clean(base, false).chars().take(200).collect();
    if clean.trim().is_empty() { Err("A file needs a name".into()) } else { Ok(clean.trim().to_string()) }
}

/// What it is kept as: `image/*` only for what shows as a picture, a raster by its bytes (AC-109).
fn mime_of(name: &str, data: &[u8]) -> String {
    raster_mime(data).map_or_else(|| {
        let named = mime_guess::from_path(name).first_or_octet_stream();
        if named.type_() == "image" { "application/octet-stream".to_string() } else { named.to_string() }
    }, str::to_string)
}

/// Writes the bytes then the row (no task yet); the bytes go again if the row is refused.
pub fn keep(store: &Store, dir: &Path, name: &str, data: &[u8], at: i64) -> Result<File, String> {
    use std::os::unix::fs::DirBuilderExt;
    let name = clean_name(name)?;
    if data.len() > MOST_BYTES {
        return Err(format!("{name} is over 100 MB"));
    }
    let id = crate::auth::crypto::random_hex::<16>();
    let path = path_of(dir, &id)?;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(folder(dir))
        .and_then(|()| crate::config::write_private(&path, data))
        .map_err(|e| format!("{name} could not be kept: {e}"))?;
    let file = File { mime: mime_of(&name, data), id, name, bytes: data.len() as i64 };
    store.with(|tx| {
        tx.execute("INSERT INTO files (id, name, bytes, mime, at) VALUES (?1, ?2, ?3, ?4, ?5)", (&file.id, &file.name, file.bytes, &file.mime, at)).map(|_| ()).map_err(sql)
    }).inspect_err(|_| { let _ = std::fs::remove_file(&path); })?;
    Ok(file)
}

/// A file kept for what then failed goes again.
pub fn forget(store: &Store, dir: &Path, id: &str) {
    if let Ok(path) = path_of(dir, id) {
        let _ = std::fs::remove_file(path);
    }
    let _ = store.with(|tx| tx.execute("DELETE FROM files WHERE id = ?1", [id]).map(|_| ()).map_err(sql));
}

/// The files of a window not yet saved join a task, on `event` of its history.
pub fn attach(tx: &Transaction, ids: &[String], task: &str, event: Option<&str>) -> Result<(), String> {
    let key = ops::key_of(tx, task)?;
    for id in ids {
        let done = tx.execute("UPDATE files SET task = ?1, event = ?2 WHERE id = ?3 AND task IS NULL", (&key, event, id)).map_err(sql)?;
        if done == 0 {
            return Err("A file of this window is gone: add it again".into());
        }
    }
    Ok(())
}

pub fn of_task(tx: &Transaction, task: &str) -> Result<Vec<File>, String> {
    let key = ops::key_of(tx, task)?;
    rows(tx, "SELECT id, name, bytes, mime FROM files WHERE task = ?1 ORDER BY at, rowid", [&key], |r| File::of_row(r, 0))
}

/// A file's row, its bytes opened, and how it may show inline: by its first bytes alone.
pub fn open(store: &Store, dir: &Path, id: &str) -> Result<(File, std::fs::File, Option<&'static str>), String> {
    use std::os::unix::fs::FileExt;
    let path = path_of(dir, id)?;
    let file = store.with(|tx| {
        tx.query_row("SELECT id, name, bytes, mime FROM files WHERE id = ?1", [id], |r| File::of_row(r, 0))
            .optional().map_err(sql)?.ok_or_else(|| "No such file".to_string())
    })?;
    let bytes = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", file.name))?;
    let mut head = [0u8; 12];
    let got = bytes.read_at(&mut head, 0).map_err(|e| format!("{}: {e}", file.name))?;
    Ok((file, bytes, raster_mime(&head[..got])))
}

/// A human's purge (AC-109): the bytes go for good; the history keeps who did it and the name.
pub fn delete(store: &Store, dir: &Path, id: &str, author: &str, at: i64) -> Result<Option<String>, String> {
    let path = path_of(dir, id)?;
    store.with(|tx| {
        let found: Option<(Option<String>, String)> = tx.query_row("SELECT task, name FROM files WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(sql)?;
        let (task, name) = found.ok_or_else(|| "No such file".to_string())?;
        tx.execute("DELETE FROM files WHERE id = ?1", [id]).map_err(sql)?;
        let mut shown = None;
        if let Some(key) = &task {
            let who = ops::user_name(tx, author)?;
            ops::log(tx, key, author, "edit", &format!("{who} deleted the file {name}"), None, at)?;
            shown = Some(tx.query_row("SELECT 'T-' || number FROM tasks WHERE id = ?1", [key], |r| r.get::<_, String>(0)).map_err(sql)?);
        }
        std::fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(format!("{name}: {e}")) })?;
        Ok(shown)
    })
}

/// Files of windows never saved, older than 7 days, go; how many.
pub fn sweep(store: &Store, dir: &Path, at: i64) -> Result<usize, String> {
    let gone: Vec<String> = store.with(|tx| {
        let ids: Vec<String> = rows(tx, "SELECT id FROM files WHERE task IS NULL AND at < ?1", [at - UNUSED_SECONDS], |r| r.get(0))?;
        tx.execute("DELETE FROM files WHERE task IS NULL AND at < ?1", [at - UNUSED_SECONDS]).map_err(sql)?;
        Ok(ids)
    })?;
    for id in &gone {
        let _ = std::fs::remove_file(path_of(dir, id)?);
    }
    // bytes whose row a crash never wrote: old ones only, one may be on its way now
    if let Ok(entries) = std::fs::read_dir(folder(dir)) {
        let known: std::collections::HashSet<String> = store.with(|tx| rows(tx, "SELECT id FROM files", [], |r| r.get(0)))?.into_iter().collect();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age.as_secs() as i64 > UNUSED_SECONDS);
            if old && !known.contains(&name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Ok(gone.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::ops::{create, NewTask};
    use crate::tasks::store::{Store, HUMAN};

    fn fresh(name: &str) -> (Store, PathBuf) {
        let dir = crate::testutil::fresh_dir(name);
        (Store::open(dir.join("kanban.sqlite")), dir)
    }

    fn task(s: &Store) -> String {
        s.with(|tx| create(tx, HUMAN, NewTask { title: "t".into(), ..Default::default() }, 1)).unwrap().id
    }

    #[test]
    fn ac109_a_file_is_kept_private_and_joins_its_task() {
        use std::os::unix::fs::PermissionsExt;
        let (s, dir) = fresh("tf-keep");
        let id = task(&s);
        let f = keep(&s, &dir, "../sub/shot.png", b"\x89PNG\r\n\x1a\nrest", 5).unwrap();
        assert_eq!((f.name.as_str(), f.mime.as_str(), f.bytes), ("shot.png", "image/png", 12));
        let path = folder(&dir).join(&f.id);
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(folder(&dir)).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(s.with(|tx| of_task(tx, &id)).unwrap(), vec![], "not until it is attached");
        s.with(|tx| attach(tx, std::slice::from_ref(&f.id), &id, None)).unwrap();
        assert_eq!(s.with(|tx| of_task(tx, &id)).unwrap(), vec![f.clone()]);
        assert_eq!(std::fs::read(&path).unwrap(), b"\x89PNG\r\n\x1a\nrest");
        assert_eq!(open(&s, &dir, &f.id).unwrap().2, Some("image/png"), "as its first bytes say");
        assert!(s.with(|tx| attach(tx, std::slice::from_ref(&f.id), &id, None)).is_err(), "attached once");
    }

    #[test]
    fn ac109_only_raster_pictures_show_inline_by_their_bytes() {
        assert_eq!(raster_mime(b"\x89PNG\r\n\x1a\nx"), Some("image/png"));
        assert_eq!(raster_mime(b"\xff\xd8\xffx"), Some("image/jpeg"));
        assert_eq!(raster_mime(b"GIF89ax"), Some("image/gif"));
        assert_eq!(raster_mime(b"RIFF1234WEBPx"), Some("image/webp"));
        for not in [b"<svg xmlns=".as_slice(), b"<!doctype html>", b"%PDF-1.7", b""] {
            assert_eq!(raster_mime(not), None);
        }
    }

    #[test]
    fn ac109_only_what_shows_as_a_picture_is_kept_as_one() {
        assert_eq!(mime_of("shot.png", b"\x89PNG\r\n\x1a\nx"), "image/png");
        assert_eq!(mime_of("shot.txt", b"\xff\xd8\xffx"), "image/jpeg", "by its bytes, not its name");
        for not_shown in ["x.svg", "x.bmp", "x.png"] {
            assert_eq!(mime_of(not_shown, b"<svg/>"), "application/octet-stream", "{not_shown}");
        }
        assert_eq!(mime_of("notes.txt", b"hello"), "text/plain");
    }

    #[test]
    fn ac109_a_name_is_kept_clean_and_ids_are_never_paths() {
        let (s, dir) = fresh("tf-names");
        assert_eq!(keep(&s, &dir, "a\u{7}b.txt", b"x", 1).unwrap().name, "ab.txt");
        assert_eq!(keep(&s, &dir, "dir/", b"x", 1).unwrap_err(), "A file needs a name");
        assert!(open(&s, &dir, "../../etc/passwd").is_err());
        assert!(open(&s, &dir, "0123456789abcdef0123456789abcdef").is_err());
    }

    #[test]
    fn ac109_over_100_mb_is_refused() {
        let (s, dir) = fresh("tf-big");
        let big = vec![0u8; MOST_BYTES + 1];
        assert_eq!(keep(&s, &dir, "shot.mov", &big, 1).unwrap_err(), "shot.mov is over 100 MB");
        assert!(!folder(&dir).exists() || std::fs::read_dir(folder(&dir)).unwrap().next().is_none());
    }

    #[test]
    fn ac109_a_purge_takes_the_bytes_and_keeps_who_did_it() {
        let (s, dir) = fresh("tf-purge");
        let id = task(&s);
        let f = keep(&s, &dir, "secret.txt", b"key", 1).unwrap();
        s.with(|tx| attach(tx, std::slice::from_ref(&f.id), &id, None)).unwrap();
        assert_eq!(delete(&s, &dir, &f.id, HUMAN, 2).unwrap().as_deref(), Some(id.as_str()));
        assert!(!folder(&dir).join(&f.id).exists());
        assert!(open(&s, &dir, &f.id).is_err());
        let h = s.with(|tx| ops::history(tx, &id)).unwrap();
        assert!(h.as_array().unwrap().iter().any(|e| e["text"].as_str().is_some_and(|t| t.ends_with("deleted the file secret.txt"))));
    }

    #[test]
    fn ac106_a_draft_s_unused_files_go_after_7_days() {
        let (s, dir) = fresh("tf-sweep");
        let old = keep(&s, &dir, "old.txt", b"x", 1000).unwrap();
        let new = keep(&s, &dir, "new.txt", b"x", 1000 + UNUSED_SECONDS).unwrap();
        let id = task(&s);
        let used = keep(&s, &dir, "used.txt", b"x", 1000).unwrap();
        s.with(|tx| attach(tx, std::slice::from_ref(&used.id), &id, None)).unwrap();
        assert_eq!(sweep(&s, &dir, 1001 + UNUSED_SECONDS).unwrap(), 1);
        assert!(!folder(&dir).join(&old.id).exists());
        assert!(folder(&dir).join(&new.id).exists() && folder(&dir).join(&used.id).exists());
    }
}
