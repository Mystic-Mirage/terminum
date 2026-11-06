// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! `terminum import`: what iterm-enhancer knew, for terminum (terminum-lifecycle.md §9 Compatibility,
//! terminum-remote.md §9). From ~/.iterm-enhancer/state, each only when terminum has none yet:
//!
//!   workspaces.json     the Files panels of web pages: `web:` entries whose pane key is a tmux
//!                       key; entries of iTerm2 sessions are dropped
//!   web-bookmarks.json  bookmarks of tmux panes
//!   agents.json         its hosts become settings.json hosts with files enabled
//!
//! A key `tmux-remote:<destination>:<socket>:%N` of an imported host becomes
//! `tmux:<name>:<socket>:%N`. Files are written 0600 with one rename.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Map, Value};

fn write_json(path: &Path, data: &Value) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())? + "\n";
    crate::config::write_private(path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

/// The parsed file, or None when it is missing; Err when it is broken.
fn read_json(path: &Path) -> Result<Option<Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    serde_json::from_str(&text).map(Some).map_err(|e| format!("{}: not JSON ({e})", path.display()))
}

/// A short name for a host from its ssh destination: alex@devbox.example → devbox.
fn host_name(ssh: &[String], taken: &Map<String, Value>) -> String {
    let dest = ssh.last().map_or("host", String::as_str);
    let host = dest.rsplit('@').next().unwrap_or(dest).split('.').next().unwrap_or("").to_lowercase();
    let mut base = String::new();
    let mut gap = false;
    for c in host.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
            if std::mem::take(&mut gap) {
                base.push('-');
            }
            base.push(c);
        } else {
            gap = true;
        }
    }
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "host" } else { base };
    let (mut name, mut n) = (base.to_string(), 2);
    while taken.contains_key(&name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    name
}

/// Adds each agents.json host to settings["hosts"] unless one with the same ssh recipe is
/// there; returns {destination: name} of every host known, for the tmux keys.
fn import_hosts(agents: &Map<String, Value>, settings: &mut Map<String, Value>) -> Result<HashMap<String, String>, String> {
    let hosts = settings.entry("hosts").or_insert_with(|| json!({}));
    let Some(hosts) = hosts.as_object_mut() else {
        return Err("settings.json: \"hosts\" is not an object".into());
    };
    let mut names = HashMap::new();
    for (key, e) in agents {
        if !e.is_object() {
            continue;
        }
        let ssh: Vec<String> = match e["ssh"].as_array() {
            Some(a) => a.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).collect(),
            None => vec![key.clone()],
        };
        let same = hosts.iter().find(|(_, h)| h["ssh"] == json!(ssh)).map(|(n, _)| n.clone());
        let name = same.clone().unwrap_or_else(|| host_name(&ssh, hosts));
        if same.is_none() {
            hosts.insert(name.clone(), json!({"ssh": ssh, "files": "enabled"}));
        }
        names.insert(key.clone(), name.clone());
        names.insert(ssh.join(" "), name.clone());
        names.insert(ssh.last().cloned().unwrap_or_default(), name);
    }
    Ok(names)
}

/// A tmux key in terminum's form, or None for a key that is not a tmux pane's.
fn map_key(key: &Value, names: &HashMap<String, String>) -> Option<String> {
    let key = key.as_str()?;
    if key.starts_with("tmux:") {
        return Some(key.to_string());
    }
    let rest = key.strip_prefix("tmux-remote:")?;
    let mut parts = rest.rsplitn(3, ':');
    let (pane, socket, dest) = (parts.next()?, parts.next()?, parts.next()?);
    names.get(dest).map(|name| format!("tmux:{name}:{socket}:{pane}"))
}

/// Keeps `web:` panes whose key is a tmux key ("web:" + JSON [host, key]).
fn workspaces(mut data: Map<String, Value>, names: &HashMap<String, String>) -> (Value, usize) {
    let mut panes = Map::new();
    if let Some(old) = data.get("panes").and_then(Value::as_object) {
        for (k, pane) in old {
            let Some(Ok(Value::Array(pair))) = k.strip_prefix("web:").map(serde_json::from_str::<Value>) else { continue };
            let [host, key] = &pair[..] else { continue };
            if let Some(mapped) = map_key(key, names) {
                let host = if key.as_str().is_some_and(|k| k.starts_with("tmux-remote:")) {
                    json!(mapped.split(':').nth(1).unwrap_or_default())
                } else { host.clone() };
                panes.insert(format!("web:{}", json!([host, mapped])), pane.clone());
            }
        }
    }
    let n = panes.len();
    data.insert("panes".into(), Value::Object(panes));
    (Value::Object(data), n)
}

fn bookmarks(mut data: Map<String, Value>, names: &HashMap<String, String>) -> (Value, usize) {
    let mut items = Vec::new();
    for it in data.get("items").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(obj), Some(mapped)) = (it.as_object(), map_key(&it["key"], names)) {
            let mut obj = obj.clone();
            obj.insert("key".into(), Value::String(mapped));
            items.push(Value::Object(obj));
        }
    }
    let n = items.len();
    data.insert("items".into(), Value::Array(items));
    (Value::Object(data), n)
}

/// Imports; returns the lines to print. Err for a file that cannot be read.
pub fn run(home: &Path, app_dir: &Path, settings_path: &Path) -> Result<Vec<String>, String> {
    let src = home.join(".iterm-enhancer/state");
    if !src.is_dir() {
        return Ok(vec![format!("Nothing to import: {} does not exist", src.display())]);
    }
    let mut said = Vec::new();
    let agents = read_json(&src.join("agents.json"))?;
    let mut settings = match read_json(settings_path)? {
        Some(Value::Object(m)) => m,
        None => Map::new(),
        Some(_) => return Err(format!("{}: expected a JSON object", settings_path.display())),
    };
    let before = settings.get("hosts").cloned().unwrap_or_else(|| json!({}));
    let empty = Map::new();
    let names = import_hosts(agents.as_ref().and_then(Value::as_object).unwrap_or(&empty), &mut settings)?;
    if settings.get("hosts") != Some(&before) {
        write_json(settings_path, &Value::Object(settings))?;
        let mut all: Vec<&String> = names.values().collect();
        all.sort();
        all.dedup();
        said.push(format!("hosts: {} in settings.json (files enabled)", all.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
    }
    type Convert = fn(Map<String, Value>, &HashMap<String, String>) -> (Value, usize);
    for (name, convert) in [("workspaces.json", workspaces as Convert), ("web-bookmarks.json", bookmarks as Convert)] {
        let dest = app_dir.join(name);
        if dest.exists() {
            said.push(format!("{name}: kept terminum's own (it exists already)"));
            continue;
        }
        let Some(Value::Object(data)) = read_json(&src.join(name))? else { continue };
        let (data, n) = convert(data, &names);
        write_json(&dest, &data)?;
        said.push(format!("{name}: {n} {} of tmux panes imported", if n != 1 { "entries" } else { "entry" }));
    }
    if said.is_empty() {
        said.push("Nothing to import: terminum has everything already".into());
    }
    Ok(said)
}
