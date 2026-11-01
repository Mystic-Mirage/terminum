// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Files dropped on a pane of this Mac from Finder or another app (terminum-terminal.md AC-36):
//! a browser gives the page their bytes, names, sizes and times, never their paths, but macOS
//! keeps the drag's files on its drag pasteboard until the next drag. `POST /api/os/dropped`
//! (this Mac's browser only: auth/checks.rs) reads it with osascript (JXA, no Apple Events, so
//! no Automation prompt) and answers the paths when every file dropped is one of them by name
//! and kind, and a file also by size and time (2 s): nothing is uploaded, at any size. Else
//! it says which case it is (`matched`) and the page uploads copies as before.

use std::path::Path;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::app::Shared;
use crate::http::{err, ApiResult};

/// The drag pasteboard's files as a JSON list of [path, its name composed (NFC)]: the page
/// sends File.name composed too, and a name on the disk may be decomposed ("é" as e + ´).
const SCRIPT: &str = "ObjC.import('AppKit');\
const pb = $.NSPasteboard.pasteboardWithName('Apple CFPasteboard drag');\
const urls = pb.readObjectsForClassesOptions($([$.NSURL]), $({}));\
const out = [];\
for (let i = 0; i < (urls.isNil() ? 0 : urls.count); i++) { const u = urls.objectAtIndex(i);\
  if (u.isFileURL) out.push([ObjC.unwrap(u.path), ObjC.unwrap(u.lastPathComponent.precomposedStringWithCanonicalMapping)]); }\
JSON.stringify(out);";
/// osascript starts in about 0.1 s; the pasteboard server may be slow, never the page.
const WAIT: Duration = Duration::from_secs(3);
/// A browser's lastModified and the file's time agree within this (file systems round).
const SLACK_MS: i64 = 2000;
/// More files than a drop sensibly holds are not matched one by one.
const MAX_FILES: usize = 1000;

#[derive(Deserialize)]
pub struct Dropped {
    name: String,
    #[serde(default)]
    size: u64,
    /// The browser's File.lastModified (ms since 1970).
    #[serde(default)]
    mtime: i64,
    #[serde(default)]
    dir: bool,
}

#[derive(Deserialize)]
pub struct DroppedBody {
    sid: String,
    files: Vec<Dropped>,
}

/// What is known of a path on the pasteboard: (is a folder, size, time in ms), or None.
type Meta = Option<(bool, u64, i64)>;
/// A file on the drag pasteboard: its path and its name composed.
type OnBoard = (String, String);

fn meta(p: &str) -> Meta {
    let m = std::fs::metadata(p).ok()?;
    let ms = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as i64;
    Some((m.is_dir(), m.len(), ms))
}

/// The pasteboard's path for each file dropped, in the drop's order, each path taken once; else
/// why not, the first file that is not there named (`unmatched`), or a path with a control or
/// invisible character (each would be a key at the input, or hide what it is).
pub fn matched(files: &[Dropped], board: &[OnBoard], meta: impl Fn(&str) -> Meta) -> Result<Vec<String>, String> {
    if board.iter().any(|(p, _)| !p.starts_with('/') || crate::termtext::has_control(p)) {
        return Err("a path the drag held has a control or invisible character or is not absolute".into());
    }
    let known: Vec<(&String, &String, Meta)> = board.iter()
        .filter(|(p, _)| Path::new(p.as_str()).file_name().is_some())
        .map(|(p, name)| (p, name, meta(p))).collect();
    let mut used = vec![false; known.len()];
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let Some(i) = known.iter().enumerate().position(|(i, (_, name, m))| !used[i] && **name == f.name && differs(f, *m).is_none()) else {
            return Err(unmatched(f, &known, &used));
        };
        used[i] = true;
        out.push(known[i].0.clone());
    }
    Ok(out)
}

/// How a file on the disk differs from the item dropped of its name; None: it is the one (its
/// kind, a file also its size and its time within SLACK_MS).
fn differs(f: &Dropped, m: Meta) -> Option<String> {
    let (dir, size, ms) = match m {
        None => return Some("cannot be read (is it gone?)".into()),
        Some((dir, _, _)) if dir != f.dir => return Some(format!("is not the {} dropped", if f.dir { "folder" } else { "file" })),
        Some(m) => m,
    };
    let mut why = Vec::new();
    if !dir && size != f.size {
        why.push(format!("{size} bytes on the disk, {} dropped", f.size));
    }
    if !dir && (ms - f.mtime).abs() > SLACK_MS {
        why.push(format!("changed {} apart", apart((ms - f.mtime).abs())));
    }
    (!why.is_empty()).then(|| format!("is not the file dropped: {}", why.join(", ")))
}

/// A time apart as a person reads it: 4.0 s, 12 min, 3 h, 2 days.
fn apart(ms: i64) -> String {
    match ms / 1000 {
        s if s < 60 => format!("{:.1} s", ms as f64 / 1000.0),
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 2 * 86400 => format!("{} h", s / 3600),
        s => format!("{} days", s / 86400),
    }
}

/// Why `f` is none of the pasteboard's files not taken yet.
fn unmatched(f: &Dropped, known: &[(&String, &String, Meta)], used: &[bool]) -> String {
    let named = |k: &&(&String, &String, Meta)| *k.1 == f.name;
    if let Some(((path, _, m), _)) = known.iter().zip(used).find(|(k, u)| !**u && named(k)) {
        return format!("{path} {}", differs(f, *m).unwrap_or_default());
    }
    if known.iter().any(|k| named(&k)) {
        return format!("{} was dropped more times than the drag held it", f.name);
    }
    match known {
        [] => "the drag held no file on this Mac's disk (a drag from Mail, a browser or an app's window has none)".into(),
        [(_, one, _)] => format!("{} is not among the files the drag held (it held {one}), perhaps an earlier drag's", f.name),
        [(_, first, _), ..] => format!("{} is not among the files the drag held (it held {}, {first} first), perhaps an earlier drag's", f.name, known.len()),
    }
}

/// The drag pasteboard's files, or why not.
async fn drag_paths() -> Result<Vec<OnBoard>, String> {
    let osascript = crate::config::find_program("osascript", "TM_OSASCRIPT").ok_or("osascript is not found")?;
    let run = tokio::process::Command::new(osascript).args(["-l", "JavaScript", "-e", SCRIPT])
        .stdin(std::process::Stdio::null()).kill_on_drop(true).output();
    let out = tokio::time::timeout(WAIT, run).await.map_err(|_| "macOS's drag pasteboard did not answer within 3 s".to_string())?
        .map_err(|e| format!("osascript: {e}"))?;
    if !out.status.success() {
        return Err(format!("osascript: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("osascript's answer: {e}"))
}

/// `POST /api/os/dropped` {sid, files: [{name, size, mtime, dir}]} -> {paths} in the drop's
/// order, or an error saying why (the page then uploads copies).
pub async fn os_dropped(State(app): State<Shared>, Json(b): Json<DroppedBody>) -> ApiResult {
    if !cfg!(target_os = "macos") {
        return Err(err(StatusCode::NOT_IMPLEMENTED, "not_macos", "A drop's own paths are found on macOS only"));
    }
    if b.files.is_empty() || b.files.len() > MAX_FILES {
        return Err(err(StatusCode::BAD_REQUEST, "bad_request", format!("1 to {MAX_FILES} files")));
    }
    match app.tmux.resolve(&b.sid) {
        None => return Err(err(StatusCode::NOT_FOUND, "closed", crate::page::CLOSED)),
        Some((server, _)) if !server.is_local() => {
            return Err(err(StatusCode::BAD_REQUEST, "on_host", format!("The session runs on {}: its files are uploaded", server.name)));
        }
        _ => {}
    }
    let paths = drag_paths().await.map_err(|e| err(StatusCode::BAD_GATEWAY, "pasteboard", e))?;
    let files = b.files;
    let found = tokio::task::spawn_blocking(move || matched(&files, &paths, meta)).await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))?;
    match found {
        Ok(paths) => Ok(Json(json!({"paths": paths})).into_response()),
        Err(why) => Err(err(StatusCode::NOT_FOUND, "not_found", why)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pasteboard's files, their names as the JXA gives them (already composed here).
    fn board(paths: &[&str]) -> Vec<OnBoard> {
        paths.iter().map(|p| (p.to_string(), Path::new(p).file_name().unwrap_or_default().to_string_lossy().into_owned())).collect()
    }

    fn f(name: &str, size: u64, mtime: i64) -> Dropped {
        Dropped { name: name.into(), size, mtime, dir: false }
    }

    fn on_disk(p: &str) -> Meta {
        match p {
            "/Users/alex/a/x.zip" => Some((false, 10, 1_000_000)),
            "/Users/alex/b/x.zip" => Some((false, 20, 1_000_000)),
            "/Users/alex/a/src" => Some((true, 96, 5)),
            "/Users/alex/c/src" => Some((false, 7, 5)),
            _ => None,
        }
    }

    #[test]
    fn each_file_dropped_is_its_own_path_on_the_pasteboard() {
        let paths = board(&["/Users/alex/a/x.zip", "/Users/alex/b/x.zip", "/Users/alex/a/src", "/Users/alex/c/src"]);
        let src = Dropped { name: "src".into(), size: 0, mtime: 0, dir: true };
        let got = matched(&[f("x.zip", 20, 1_001_500), f("x.zip", 10, 998_000), src, f("src", 7, 5)], &paths, on_disk);
        assert_eq!(got.unwrap(), ["/Users/alex/b/x.zip", "/Users/alex/a/x.zip", "/Users/alex/a/src", "/Users/alex/c/src"], "same names told apart by size, a folder by its kind, in the drop's order");
        assert!(matched(&[f("x.zip", 10, 1_002_001)], &paths, on_disk).is_err(), "2 s apart at most");
        assert!(matched(&[f("x.zip", 11, 1_000_000)], &paths, on_disk).is_err(), "another size: another drag's");
        assert!(matched(&[f("x.zip", 10, 1_000_000), f("y.txt", 1, 1)], &paths, on_disk).is_err(), "one not there: none at all");
        assert!(matched(&[f("x.zip", 10, 1_000_000), f("x.zip", 10, 1_000_000)], &paths, on_disk).is_err(), "a path is taken once");
    }

    #[test]
    fn why_a_drop_is_not_the_drags_files_names_its_case() {
        let paths = board(&["/Users/alex/a/x.zip", "/Users/alex/a/src", "/Users/alex/d/gone.txt"]);
        let why = |files: &[Dropped], board: &[OnBoard]| matched(files, board, on_disk).unwrap_err();
        let dir = |name: &str| Dropped { name: name.into(), size: 0, mtime: 0, dir: true };
        assert_eq!(why(&[f("pic.png", 5, 0)], &[]), "the drag held no file on this Mac's disk (a drag from Mail, a browser or an app's window has none)");
        assert_eq!(why(&[f("pic.png", 5, 0)], &board(&["/"])), why(&[f("pic.png", 5, 0)], &[]), "a root alone holds no file");
        assert_eq!(why(&[f("y.txt", 1, 1)], &paths), "y.txt is not among the files the drag held (it held 3, x.zip first), perhaps an earlier drag's");
        assert_eq!(why(&[f("y.txt", 1, 1)], &board(&["/Users/alex/a/x.zip"])), "y.txt is not among the files the drag held (it held x.zip), perhaps an earlier drag's");
        assert_eq!(why(&[f("x.zip", 10, 1_000_000), f("y.txt", 1, 1)], &paths), "y.txt is not among the files the drag held (it held 3, x.zip first), perhaps an earlier drag's",
            "the first matched, the second named");
        assert_eq!(why(&[f("x.zip", 11, 1_004_000)], &paths), "/Users/alex/a/x.zip is not the file dropped: 10 bytes on the disk, 11 dropped, changed 4.0 s apart");
        assert_eq!(why(&[f("x.zip", 11, 1_000_000)], &paths), "/Users/alex/a/x.zip is not the file dropped: 10 bytes on the disk, 11 dropped");
        assert_eq!(why(&[f("x.zip", 10, 1_000_000 + 86_400_000)], &paths), "/Users/alex/a/x.zip is not the file dropped: changed 24 h apart");
        assert_eq!(why(&[f("src", 96, 5)], &paths), "/Users/alex/a/src is not the file dropped");
        assert_eq!(why(&[dir("x.zip")], &paths), "/Users/alex/a/x.zip is not the folder dropped");
        assert_eq!(why(&[f("gone.txt", 1, 1)], &paths), "/Users/alex/d/gone.txt cannot be read (is it gone?)");
        assert_eq!(why(&[f("x.zip", 10, 1_000_000), f("x.zip", 10, 1_000_000)], &board(&["/Users/alex/a/x.zip"])),
            "x.zip was dropped more times than the drag held it");
        assert_eq!(why(&[f("x.zip", 30, 1_000_000)], &board(&["/Users/alex/a/x.zip", "/Users/alex/b/x.zip"])),
            "/Users/alex/a/x.zip is not the file dropped: 10 bytes on the disk, 30 dropped", "two of the name, both another: the first said");
    }

    #[test]
    fn a_time_apart_reads_as_a_person_reads_it() {
        assert_eq!([apart(4_000), apart(59_900), apart(720_000), apart(10_800_000), apart(2 * 86_400_000)], ["4.0 s", "59.9 s", "12 min", "3 h", "2 days"]);
    }

    #[test]
    fn a_path_with_a_control_character_refuses_the_drop() {
        let unsafe_path = "a path the drag held has a control or invisible character or is not absolute";
        assert_eq!(matched(&[f("x.zip", 10, 1_000_000)], &board(&["/Users/alex/a/x.zip", "/Users/alex/a/new\nline"]), on_disk).unwrap_err(), unsafe_path);
        assert_eq!(matched(&[f("x.zip", 10, 1_000_000)], &board(&["relative/x.zip"]), |_| Some((false, 10, 1_000_000))).unwrap_err(), unsafe_path);
        assert_eq!(matched(&[f("x.zip", 10, 1_000_000)], &board(&["/Users/alex/a/\u{202e}piz.exe"]), on_disk).unwrap_err(), unsafe_path, "a bidi override");
    }

    #[test]
    fn a_name_is_matched_composed_and_the_path_given_as_the_disk_has_it() {
        let decomposed = "/Users/alex/a/cafe\u{301}.txt";
        let on = vec![(decomposed.to_string(), "caf\u{e9}.txt".to_string())];
        assert_eq!(matched(&[f("caf\u{e9}.txt", 3, 0)], &on, |_| Some((false, 3, 0))).unwrap(), [decomposed]);
    }
}
