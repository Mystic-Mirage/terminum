// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Writes to settings.json made by terminumd (a host added, a session attached): one key's
//! value is replaced in the text, so the user's order, spacing and other keys stay as written.

use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::{locate, syntax_message, Seg, FILE, MAX_BYTES};

/// Changes the `hosts` object with `f` (AC-87).
pub fn save_hosts(path: &Path, f: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    save_object(path, "hosts", f)
}

/// Changes the top-level object `key` with `f` (see `save_key`).
pub fn save_object(path: &Path, key: &str, f: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    save_key(path, key, |old| {
        let mut value = match old {
            Some(Value::Object(h)) => h.clone(),
            None | Some(Value::Null) => Map::new(),
            Some(_) => return Err(format!("{FILE}: {key} must be an object")),
        };
        f(&mut value);
        Ok(Value::Object(value))
    })
}

/// Sets the top-level `key` to `value` (see `save_key`).
pub fn save_value(path: &Path, key: &str, value: Value) -> Result<(), String> {
    save_key(path, key, |_| Ok(value))
}

/// Sets the top-level `key` to what `f` makes of its value and writes the file atomically,
/// replacing only the text of that value (or adding the key): the rest of the file stays byte for
/// byte. A file that does not parse is left alone: its line is reported for the user to fix first.
fn save_key(path: &Path, key: &str, f: impl FnOnce(Option<&Value>) -> Result<Value, String>) -> Result<(), String> {
    // one write at a time: "Add host" and "Attach tmux sessions" at once must not lose either
    static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{FILE}: cannot be read: {e}")),
    };
    if text.len() > MAX_BYTES {
        return Err(format!("{FILE}: larger than 64 KB, not read"));
    }
    let top: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&text)
            .map_err(|e| format!("{FILE}: line {}: {}; fix it first", e.line().max(1), syntax_message(&e)))?
    };
    let Some(obj) = top.as_object() else { return Err(format!("{FILE}: expected an object")) };
    let value = f(obj.get(key))?;
    let rendered = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?.replace('\n', "\n  ");
    let out = if text.trim().is_empty() {
        format!("{{\n  \"{key}\": {rendered}\n}}\n")
    } else if let Some(sp) = locate(&text, &[Seg::Key(key)]) {
        format!("{}{rendered}{}", &text[..sp.start], &text[sp.end..])
    } else {
        let end = text.rfind('}').ok_or_else(|| format!("{FILE}: expected an object"))?;
        let head = text[..end].trim_end();
        let comma = if obj.is_empty() { "" } else { "," };
        format!("{head}{comma}\n  \"{key}\": {rendered}\n{}", &text[end..])
    };
    write_atomic(path, out.as_bytes()).map_err(|e| format!("{FILE}: cannot be written: {e}"))
}

/// Write beside the file, keep its permissions, then rename over it.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    crate::config::write_replacing(path, data, 0o666, fs::metadata(path).ok().map(|m| m.permissions()))
}

