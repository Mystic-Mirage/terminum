// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The hot keys panel's layout (AC-18): `~/.terminum/hotkeys.json` lists rows of keys by tmux
//! name ("F5", "C-r", "S-Enter"), typed text ("text:git status") or the panel's actions
//! (Copy, Paste, Upload, Fn, Ctrl, Alt). Without the file, or when it is wrong, the panel is
//! iterm-enhancer's four rows of six; a wrong file is refused with the line of the first bad
//! key ("hotkeys.json: line 3: unknown key Foo") and the default stays.
//!
//! File: `{"rows": [["F5", "C-r", {"key": "text:git status", "label": "gs"}]], "fn": [[...]]}`
//! (or the rows alone as an array); a key is a name or `{key, label?, hint?}`. `fn` replaces
//! the rows Fn swaps in for the last two (F1–F12 by default). The page receives
//! `{"rows": [[{label, key, hint?}]], "fn": [...], "error": null | message}`.

use std::path::Path;

use serde_json::{Value, json};

use crate::settings::{MAX_BYTES, Seg, line_of, syntax_message};

const FILE: &str = "hotkeys.json";
pub const MAX_ROWS: usize = 8;
pub const MAX_KEYS: usize = 12;
const MAX_LABEL: usize = 12;
const MAX_TEXT: usize = 200;
/// The panel's own keys: they act in the page, not in tmux.
pub const ACTIONS: [&str; 6] = ["Copy", "Paste", "Upload", "Fn", "Ctrl", "Alt"];

type Key = (&'static str, &'static str, &'static str);

/// The default panel (web/static/input.js): label, key, hint.
const DEFAULT: [[Key; 6]; 4] = [
    [("Copy", "Copy", "Copy the selected text"), ("Paste", "Paste", ""), ("Upload", "Upload", "Upload a file and paste its path"),
        ("⇧↩", "S-Enter", "Shift+Enter: a new line in apps that tell it from Enter"), ("Tab", "Tab", ""), ("Enter", "Enter", "Enter (Return)")],
    [("Esc", "Escape", ""), ("^C", "C-c", "Interrupt"), ("^D", "C-d", "End of input"), ("^Z", "C-z", "Suspend"),
        ("^R", "C-r", "Search history"), ("Fn", "Fn", "F1–F12 in place of the last two rows")],
    [("Ctrl", "Ctrl", ""), ("Alt", "Alt", ""), ("←", "Left", ""), ("↑", "Up", ""), ("↓", "Down", ""), ("→", "Right", "")],
    [("Home", "Home", ""), ("End", "End", ""), ("PgUp", "PageUp", ""), ("PgDn", "PageDown", ""), ("⇧←", "S-Left", "Shift+Left"),
        ("⇧Tab", "BTab", "Shift+Tab")],
];

/// Whether tmux has a name for this key (one rule, checked against tmux: tmux/keys.rs).
pub fn tmux_key_ok(name: &str) -> bool {
    crate::tmux::keys::names_ok(name)
}

/// A key of the file: a panel action, typed text, or a key tmux names.
fn key_ok(key: &str) -> bool {
    match key.strip_prefix("text:") {
        Some(t) => !t.is_empty() && t.chars().count() <= MAX_TEXT && !t.chars().any(crate::termtext::stray),
        None => ACTIONS.contains(&key) || tmux_key_ok(key),
    }
}

/// The label a key gets when the file gives none: the default panel's, else its name.
pub fn label_for(key: &str) -> String {
    if let Some(t) = key.strip_prefix("text:") {
        return t.chars().take(MAX_LABEL).collect();
    }
    let named = DEFAULT.iter().flatten().chain(FN.iter().flatten()).find(|k| k.1.eq_ignore_ascii_case(key)).map(|k| k.0);
    let alias = match key {
        "PPage" | "PgUp" => Some("PgUp"),
        "NPage" | "PgDn" => Some("PgDn"),
        "BSpace" => Some("⌫"),
        _ => None,
    };
    if let Some(l) = named.or(alias) {
        return l.to_string();
    }
    match key.strip_prefix("C-") {
        Some(c) if c.len() == 1 && c.as_bytes()[0].is_ascii_alphabetic() => format!("^{}", c.to_ascii_uppercase()),
        _ => key.to_string(),
    }
}

const FN: [[Key; 6]; 2] = [
    [("F1", "F1", ""), ("F2", "F2", ""), ("F3", "F3", ""), ("F4", "F4", ""), ("F5", "F5", ""), ("F6", "F6", "")],
    [("F7", "F7", ""), ("F8", "F8", ""), ("F9", "F9", ""), ("F10", "F10", ""), ("F11", "F11", ""), ("F12", "F12", "")],
];

fn rows_json(rows: &[[Key; 6]]) -> Value {
    Value::Array(
        rows.iter()
            .map(|r| {
                Value::Array(
                    r.iter()
                        .map(|(label, key, hint)| {
                            let mut k = json!({"label": label, "key": key});
                            if !hint.is_empty() {
                                k["hint"] = json!(hint);
                            }
                            k
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

/// The default panel, as the page receives it.
pub fn default_layout() -> Value {
    json!({"rows": rows_json(&DEFAULT), "fn": rows_json(&FN), "error": null})
}

/// Rows of a file at `at` (the path of the rows array), checked.
fn rows(v: &Value, text: &str, at: &[Seg]) -> Result<Value, String> {
    let bad = |path: &[Seg], what: String| format!("{FILE}: line {}: {what}", line_of(text, path));
    let Some(list) = v.as_array().filter(|l| (1..=MAX_ROWS).contains(&l.len())) else {
        return Err(bad(at, format!("expected 1 to {MAX_ROWS} rows")));
    };
    let mut out = Vec::new();
    for (r, row) in list.iter().enumerate() {
        let rp = [at, &[Seg::Index(r)]].concat();
        let Some(keys) = row.as_array().filter(|k| (1..=MAX_KEYS).contains(&k.len())) else {
            return Err(bad(&rp, format!("a row is 1 to {MAX_KEYS} keys")));
        };
        let mut items = Vec::new();
        for (i, entry) in keys.iter().enumerate() {
            let ep = [&rp[..], &[Seg::Index(i)]].concat();
            let (key, label, hint) = match entry {
                Value::String(k) => (k.as_str(), None, None),
                Value::Object(o) => match o.get("key").and_then(Value::as_str) {
                    Some(k) => (k, o.get("label"), o.get("hint")),
                    None => return Err(bad(&ep, "a key needs \"key\"".into())),
                },
                _ => return Err(bad(&ep, "expected a key name".into())),
            };
            if !key_ok(key) {
                return Err(bad(&ep, format!("unknown key {}", key.chars().take(40).collect::<String>())));
            }
            let label = match label {
                None => label_for(key),
                Some(Value::String(l)) if (1..=MAX_LABEL).contains(&l.chars().count()) && !l.chars().any(crate::termtext::stray) => l.clone(),
                Some(_) => return Err(bad(&ep, format!("a label is 1 to {MAX_LABEL} characters"))),
            };
            let mut item = json!({"label": label, "key": key});
            match hint {
                None => {}
                Some(Value::String(h)) if h.chars().count() <= 100 && !h.chars().any(crate::termtext::stray) => item["hint"] = json!(h),
                Some(_) => return Err(bad(&ep, "a hint is at most 100 characters".into())),
            }
            items.push(item);
        }
        out.push(Value::Array(items));
    }
    Ok(Value::Array(out))
}

/// The layout from the file's text: the user's rows, or the default with the error.
pub fn parse(text: &str) -> Value {
    match layout(text) {
        Ok(v) => v,
        Err(e) => {
            let mut d = default_layout();
            d["error"] = json!(e);
            d
        }
    }
}

fn layout(text: &str) -> Result<Value, String> {
    if text.len() > MAX_BYTES {
        return Err(format!("{FILE}: larger than 64 KB, not read"));
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("{FILE}: line {}: {}", e.line().max(1), syntax_message(&e)))?;
    let (main, fn_rows) = match &v {
        Value::Array(_) => (rows(&v, text, &[])?, rows_json(&FN)),
        Value::Object(o) => {
            if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "rows" | "fn")) {
                return Err(format!("{FILE}: line {}: unknown key {k}", line_of(text, &[Seg::Key(k)])));
            }
            let main = o.get("rows").ok_or_else(|| format!("{FILE}: expected \"rows\""))?;
            let main = rows(main, text, &[Seg::Key("rows")])?;
            let fn_rows = match o.get("fn") {
                Some(f) => rows(f, text, &[Seg::Key("fn")])?,
                None => rows_json(&FN),
            };
            (main, fn_rows)
        }
        _ => return Err(format!("{FILE}: expected rows of keys")),
    };
    Ok(json!({"rows": main, "fn": fn_rows, "error": null}))
}

/// The file at `path`: the default panel when it is missing.
pub fn load(path: &Path) -> Value {
    match crate::settings::read_capped(path) {
        Ok(None) => default_layout(),
        Ok(Some(text)) => parse(&text),
        Err(why) => {
            let mut d = default_layout();
            d["error"] = json!(format!("{FILE}: {why}"));
            d
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn tmux_names_with_prefixes_are_known_and_others_are_not() {
        for ok in ["F5", "f12", "C-r", "M-x", "S-Enter", "C-M-S-Left", "BTab", "pageup", "PgDn", "DC", "Space", "a", "~", "é", "C--", "BSpace", "Insert"] {
            assert!(tmux_key_ok(ok), "{ok}");
        }
        for bad in ["Foo", "F13", "F0", "F05", "C-", "", " ", "\n", "ab", "C-Foo", "X-a", "Ctrl", "Any"] {
            assert!(!tmux_key_ok(bad), "{bad:?}");
        }
        assert!(key_ok("Ctrl") && key_ok("Paste") && key_ok("text:git status"));
        assert!(!key_ok("text:") && !key_ok("text:rm\n") && !key_ok("paste"));
    }

    #[test]
    fn the_default_is_the_four_rows_of_six_of_iterm_enhancer() {
        let d = default_layout();
        assert_eq!(d["error"], Value::Null);
        let labels: Vec<Vec<&str>> = d["rows"].as_array().unwrap().iter()
            .map(|r| r.as_array().unwrap().iter().map(|k| k["label"].as_str().unwrap()).collect()).collect();
        assert_eq!(labels, vec![
            vec!["Copy", "Paste", "Upload", "⇧↩", "Tab", "Enter"],
            vec!["Esc", "^C", "^D", "^Z", "^R", "Fn"],
            vec!["Ctrl", "Alt", "←", "↑", "↓", "→"],
            vec!["Home", "End", "PgUp", "PgDn", "⇧←", "⇧Tab"],
        ]);
        assert_eq!(d["fn"][1][5], json!({"label": "F12", "key": "F12"}));
        assert_eq!(d["rows"][1][1], json!({"label": "^C", "key": "C-c", "hint": "Interrupt"}));
        for k in d["rows"].as_array().unwrap().iter().flat_map(|r| r.as_array().unwrap()) {
            assert!(key_ok(k["key"].as_str().unwrap()), "{k}");
        }
        assert_eq!(load(Path::new("/nonexistent/terminum/hotkeys.json")), d);
    }

    #[test]
    fn a_users_rows_with_names_text_and_labels() {
        let v = parse(r#"{"rows": [["F5", "C-r", "text:git status", "Paste"], [{"key": "C-a", "label": "tmux", "hint": "prefix"}, "S-Left", "C-x"]]}"#);
        assert_eq!(v["error"], Value::Null);
        assert_eq!(v["rows"][0], json!([{"label": "F5", "key": "F5"}, {"label": "^R", "key": "C-r"},
            {"label": "git status", "key": "text:git status"}, {"label": "Paste", "key": "Paste"}]));
        assert_eq!(v["rows"][1], json!([{"label": "tmux", "key": "C-a", "hint": "prefix"}, {"label": "⇧←", "key": "S-Left"}, {"label": "^X", "key": "C-x"}]));
        assert_eq!(v["fn"], default_layout()["fn"], "F1–F12 unless the file gives fn");
        let bare = parse(r#"[["Escape", "M-b"]]"#);
        assert_eq!(bare["rows"], json!([[{"label": "Esc", "key": "Escape"}, {"label": "M-b", "key": "M-b"}]]));
        let f = parse(r#"{"rows": [["Up"]], "fn": [["F1", "F2"]]}"#);
        assert_eq!(f["fn"], json!([[{"label": "F1", "key": "F1"}, {"label": "F2", "key": "F2"}]]));
        assert_eq!(label_for("text:a long command line here"), "a long comma");
    }

    #[test]
    fn an_unknown_key_is_refused_with_its_line_and_the_default_stays() {
        let text = "{\"rows\": [\n  [\"F5\", \"Paste\"],\n  [\"C-r\", \"Foo\"]\n]}";
        let v = parse(text);
        assert_eq!(v["error"], "hotkeys.json: line 3: unknown key Foo");
        assert_eq!(v["rows"], default_layout()["rows"]);
        let cases = [
            ("[\n[\"Up\"],\n[{\"label\": \"x\"}]]", "hotkeys.json: line 3: a key needs \"key\""),
            ("[[\"Up\", 5]]", "hotkeys.json: line 1: expected a key name"),
            ("[[]]", "hotkeys.json: line 1: a row is 1 to 12 keys"),
            ("[]", "hotkeys.json: line 1: expected 1 to 8 rows"),
            ("{\"rows\": [[\"Up\"]],\n\"cols\": 3}", "hotkeys.json: line 2: unknown key cols"),
            ("{\"fn\": [[\"F1\"]]}", "hotkeys.json: expected \"rows\""),
            ("[[{\"key\": \"Up\", \"label\": \"a very long label\"}]]", "hotkeys.json: line 1: a label is 1 to 12 characters"),
            ("[[{\"key\": \"Up\", \"label\": \"up\\u202e\"}]]", "hotkeys.json: line 1: a label is 1 to 12 characters"),
            ("[[\"text:rm\\u200b -rf\"]]", "hotkeys.json: line 1: unknown key text:rm\u{200b} -rf"),
            ("{\"rows\": [[\"Up\"]], \"fn\": [[\"F13\"]]}", "hotkeys.json: line 1: unknown key F13"),
            ("[[\"Up\",]]", "hotkeys.json: line 1: trailing comma"),
            ("\"Up\"", "hotkeys.json: expected rows of keys"),
        ];
        for (text, want) in cases {
            assert_eq!(parse(text)["error"], want, "{text}");
        }
        let many = format!("[{}]", ["[\"Up\"]"; MAX_ROWS + 1].join(","));
        assert_eq!(parse(&many)["error"], "hotkeys.json: line 1: expected 1 to 8 rows");
    }

    #[test]
    fn the_file_is_read_and_a_big_one_refused() {
        let d = crate::testutil::temp_root().join(format!("tm-hotkeys-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let p = d.join("hotkeys.json");
        fs::write(&p, r#"[["F5"]]"#).unwrap();
        assert_eq!(load(&p)["rows"], json!([[{"label": "F5", "key": "F5"}]]));
        fs::write(&p, format!("[[\"F5\"]]{}", " ".repeat(MAX_BYTES))).unwrap();
        assert_eq!(load(&p)["error"], "hotkeys.json: larger than 64 KB, not read");
        let _ = fs::remove_dir_all(&d);
    }
}
