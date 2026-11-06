// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Tasks' files (AC-106, AC-109): bytes in `kanban-files/<id>` beside the store, 0600 in a 0700
//! folder, a row in the store. A file dropped in a window not yet saved has no task; such a file
//! goes after 7 days. Nothing here runs or is opened on the page's origin.

use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Transaction};
use serde::Serialize;

use super::ops::{self, sql};

pub const MOST_BYTES: usize = 100 << 20;
const UNUSED_SECONDS: i64 = 7 * 24 * 3600;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct File {
    pub id: String,
    pub name: String,
    pub bytes: i64,
    pub mime: String,
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
    let clean: String = base.chars().filter(|c| !c.is_control()).take(200).collect();
    if clean.trim().is_empty() { Err("A file needs a name".into()) } else { Ok(clean.trim().to_string()) }
}

/// What may show inline, by its bytes: raster pictures alone (never SVG or HTML).
pub fn inline_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Writes the bytes then the row (no task yet); the bytes go again if the row is refused.
pub fn keep(store: &super::store::Store, dir: &Path, name: &str, author: &str, data: &[u8], at: i64) -> Result<File, String> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let name = clean_name(name)?;
    if data.len() > MOST_BYTES {
        return Err(format!("{name} is over 100 MB"));
    }
    let id = crate::auth::crypto::random_hex::<16>();
    let path = path_of(dir, &id)?;
    let written = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(folder(dir))
        .and_then(|()| std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&path))
        .and_then(|mut f| f.write_all(data));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&path);
        return Err(format!("{name} could not be kept: {e}"));
    }
    let mime = inline_mime(data).map_or_else(|| mime_guess::from_path(&name).first_or_octet_stream().to_string(), str::to_string);
    let file = File { id: id.clone(), name, bytes: data.len() as i64, mime };
    let row = store.with(|tx| {
        tx.execute("INSERT INTO files (id, name, bytes, mime, author, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", (&file.id, &file.name, file.bytes, &file.mime, author, at)).map_err(sql)?;
        Ok(())
    });
    if let Err(e) = row {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(file)
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
    let mut st = tx.prepare("SELECT id, name, bytes, mime FROM files WHERE task = ?1 ORDER BY at, rowid").map_err(sql)?;
    let rows = st.query_map([&key], |r| Ok(File { id: r.get(0)?, name: r.get(1)?, bytes: r.get(2)?, mime: r.get(3)? })).map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

/// A file's row and bytes, by its id.
pub fn read(store: &super::store::Store, dir: &Path, id: &str) -> Result<(File, Vec<u8>), String> {
    let path = path_of(dir, id)?;
    let file = store.with(|tx| {
        tx.query_row("SELECT id, name, bytes, mime FROM files WHERE id = ?1", [id], |r| Ok(File { id: r.get(0)?, name: r.get(1)?, bytes: r.get(2)?, mime: r.get(3)? }))
            .optional().map_err(sql)?.ok_or_else(|| "No such file".to_string())
    })?;
    let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", file.name))?;
    Ok((file, data))
}

/// A human's purge (AC-109): the bytes go for good; the history keeps who did it and the name.
pub fn delete(store: &super::store::Store, dir: &Path, id: &str, author: &str, at: i64) -> Result<Option<String>, String> {
    let path = path_of(dir, id)?;
    let task = store.with(|tx| {
        let found: Option<(Option<String>, String)> = tx.query_row("SELECT task, name FROM files WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(sql)?;
        let (task, name) = found.ok_or_else(|| "No such file".to_string())?;
        tx.execute("DELETE FROM files WHERE id = ?1", [id]).map_err(sql)?;
        let mut shown = None;
        if let Some(key) = &task {
            let who: String = tx.query_row("SELECT name FROM users WHERE id = ?1", [author], |r| r.get(0)).map_err(sql)?;
            ops::log(tx, key, author, "edit", &format!("{who} deleted the file {name}"), None, at)?;
            shown = Some(tx.query_row("SELECT 'T-' || number FROM tasks WHERE id = ?1", [key], |r| r.get::<_, String>(0)).map_err(sql)?);
        }
        std::fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(format!("{name}: {e}")) })?;
        Ok(shown)
    })?;
    Ok(task)
}

/// Files of windows never saved, older than 7 days, go; how many.
pub fn sweep(store: &super::store::Store, dir: &Path, at: i64) -> Result<usize, String> {
    let gone: Vec<String> = store.with(|tx| {
        let ids: Vec<String> = tx.prepare("SELECT id FROM files WHERE task IS NULL AND at < ?1").map_err(sql)?
            .query_map([at - UNUSED_SECONDS], |r| r.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
        tx.execute("DELETE FROM files WHERE task IS NULL AND at < ?1", [at - UNUSED_SECONDS]).map_err(sql)?;
        Ok(ids)
    })?;
    for id in &gone {
        let _ = std::fs::remove_file(path_of(dir, id)?);
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
        let f = keep(&s, &dir, "../sub/shot.png", HUMAN, b"\x89PNG\r\n\x1a\nrest", 5).unwrap();
        assert_eq!((f.name.as_str(), f.mime.as_str(), f.bytes), ("shot.png", "image/png", 12));
        let path = folder(&dir).join(&f.id);
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(folder(&dir)).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(s.with(|tx| of_task(tx, &id)).unwrap(), vec![], "not until it is attached");
        s.with(|tx| attach(tx, std::slice::from_ref(&f.id), &id, None)).unwrap();
        assert_eq!(s.with(|tx| of_task(tx, &id)).unwrap(), vec![f.clone()]);
        assert_eq!(read(&s, &dir, &f.id).unwrap().1, b"\x89PNG\r\n\x1a\nrest");
        assert!(s.with(|tx| attach(tx, std::slice::from_ref(&f.id), &id, None)).is_err(), "attached once");
    }

    #[test]
    fn ac109_only_raster_pictures_show_inline_by_their_bytes() {
        assert_eq!(inline_mime(b"\x89PNG\r\n\x1a\nx"), Some("image/png"));
        assert_eq!(inline_mime(b"\xff\xd8\xffx"), Some("image/jpeg"));
        assert_eq!(inline_mime(b"GIF89ax"), Some("image/gif"));
        assert_eq!(inline_mime(b"RIFF1234WEBPx"), Some("image/webp"));
        for not in [b"<svg xmlns=".as_slice(), b"<!doctype html>", b"%PDF-1.7", b""] {
            assert_eq!(inline_mime(not), None);
        }
    }

    #[test]
    fn ac109_a_name_is_kept_clean_and_ids_are_never_paths() {
        let (s, dir) = fresh("tf-names");
        assert_eq!(keep(&s, &dir, "a\u{7}b.txt", HUMAN, b"x", 1).unwrap().name, "ab.txt");
        assert_eq!(keep(&s, &dir, "dir/", HUMAN, b"x", 1).unwrap_err(), "A file needs a name");
        assert!(read(&s, &dir, "../../etc/passwd").is_err());
        assert!(read(&s, &dir, "0123456789abcdef0123456789abcdef").is_err());
    }

    #[test]
    fn ac109_over_100_mb_is_refused() {
        let (s, dir) = fresh("tf-big");
        let big = vec![0u8; MOST_BYTES + 1];
        assert_eq!(keep(&s, &dir, "shot.mov", HUMAN, &big, 1).unwrap_err(), "shot.mov is over 100 MB");
        assert!(!folder(&dir).exists() || std::fs::read_dir(folder(&dir)).unwrap().next().is_none());
    }

    #[test]
    fn ac109_a_purge_takes_the_bytes_and_keeps_who_did_it() {
        let (s, dir) = fresh("tf-purge");
        let id = task(&s);
        let f = keep(&s, &dir, "secret.txt", HUMAN, b"key", 1).unwrap();
        s.with(|tx| attach(tx, std::slice::from_ref(&f.id), &id, None)).unwrap();
        assert_eq!(delete(&s, &dir, &f.id, HUMAN, 2).unwrap().as_deref(), Some(id.as_str()));
        assert!(!folder(&dir).join(&f.id).exists());
        assert!(read(&s, &dir, &f.id).is_err());
        let h = s.with(|tx| ops::history(tx, &id)).unwrap();
        assert!(h.as_array().unwrap().iter().any(|e| e["text"].as_str().is_some_and(|t| t.ends_with("deleted the file secret.txt"))));
    }

    #[test]
    fn ac106_a_draft_s_unused_files_go_after_7_days() {
        let (s, dir) = fresh("tf-sweep");
        let old = keep(&s, &dir, "old.txt", HUMAN, b"x", 1000).unwrap();
        let new = keep(&s, &dir, "new.txt", HUMAN, b"x", 1000 + UNUSED_SECONDS).unwrap();
        let id = task(&s);
        let used = keep(&s, &dir, "used.txt", HUMAN, b"x", 1000).unwrap();
        s.with(|tx| attach(tx, std::slice::from_ref(&used.id), &id, None)).unwrap();
        assert_eq!(sweep(&s, &dir, 1001 + UNUSED_SECONDS).unwrap(), 1);
        assert!(!folder(&dir).join(&old.id).exists());
        assert!(folder(&dir).join(&new.id).exists() && folder(&dir).join(&used.id).exists());
    }
}
