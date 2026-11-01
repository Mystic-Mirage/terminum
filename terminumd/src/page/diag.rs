// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A pane's diagnostics (AC-100): tmux's own fields of the pane by name, which the page shows
//! with its build, the browser and the row as one text for a bug report. No screen lines,
//! history or sign-in: what tmux says of the pane's state, its folder with ~ for the server's
//! home folder.

use crate::app::Shared;
use crate::screen::frame::FIELDS;
use serde_json::{json, Map, Value};

/// The names of FIELDS, in its order (`#{name}`).
fn names() -> Vec<&'static str> {
    FIELDS.split('\t').map(|f| f.trim_start_matches("#{").trim_end_matches('}')).collect()
}

/// One `display -p` line of FIELDS as {name: value}; a value tmux did not give is ""; the
/// pane's folder under `home` (the server's HOME) as ~.
pub fn fields(line: &str, home: &str) -> Value {
    let mut values = line.split('\t');
    let mut out = Map::new();
    for name in names() {
        let v = values.next().unwrap_or("");
        let v = crate::termtext::clean(v, false);
        let v = if name == "pane_current_path" { crate::tmux::titles::tilde(&v, home) } else { v };
        out.insert(name.into(), json!(v));
    }
    Value::Object(out)
}

/// The page's `{t: "diag", id}`: `{t: "diag", id, build, server, tmux}` or `{t: "diag", id, error}`.
pub async fn show(app: &Shared, page: &super::Page, key: &str) {
    let refuse = |msg: String| { page.send_json(&json!({"t": "diag", "id": key, "error": msg})); };
    let Some((server, pane)) = app.tmux.resolve(key) else { return refuse(super::CLOSED.into()) };
    let Some(c) = server.bulk() else { return refuse(format!("{} does not answer.", server.label())) };
    match c.run_all(vec![format!("display -p -t {pane} '{FIELDS}'")]).await.pop() {
        Some(Ok(lines)) if !lines.is_empty() => {
            let home = server.home().await.unwrap_or_default();
            page.send_json(&json!({"t": "diag", "id": key, "build": crate::config::build_id(), "server": server.label(), "tmux": fields(&lines[0], &home)}));
        }
        Some(Err(e)) => refuse(format!("{} says: {e}", server.label())),
        _ => refuse(format!("{} does not answer.", server.label())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_has_its_tmux_name_and_a_short_answer_leaves_the_rest_empty() {
        let n = names();
        assert_eq!(n.len(), FIELDS.matches("#{").count());
        assert_eq!((n[0], n[2], n[16]), ("cursor_x", "alternate_on", "pane_current_path"));
        let f = fields("3\t7\t1\t120\t80\t24\t1\t1\t12000\t1\t@2\t0\t1\t1\t4242\tclaude\t/Users/alex/api", "/Users/alex");
        assert_eq!((f["alternate_on"].as_str(), f["pane_width"].as_str(), f["pane_current_command"].as_str()), (Some("1"), Some("80"), Some("claude")));
        assert_eq!(f["pane_current_path"], "~/api", "the home folder as ~");
        assert_eq!(fields("0\t0\t0\t0\t80\t24\t0\t0\t0\t0\t@1\t0\t1\t1\t1\tsh\t/srv/app", "/Users/alex")["pane_current_path"], "/srv/app");
        let short = fields("3\t7", "/Users/alex");
        assert_eq!((short["cursor_y"].as_str(), short["pane_current_path"].as_str()), (Some("7"), Some("")));
    }

    #[test]
    fn a_value_never_carries_a_control_character() {
        let f = fields("0\t0\t0\t0\t80\t24\t0\t0\t0\t0\t@1\t0\t1\t1\t1\tvi\x1b[31m\t/tmp", "");
        assert!(!f["pane_current_command"].as_str().unwrap().contains('\x1b'));
    }
}
