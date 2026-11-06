// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The last lines in the terminal after a restart (AC-96): a pane a restore makes again on a
//! server on this Mac (terminum's tmux, the user's own) starts by printing the lines kept of
//! the pane it replaces, then its shell, so they are its history: the page shows them, and the
//! next restart keeps them. The text is the plain text `capture-pane -J` gave (control
//! characters dropped), in a 0600 file of the 0700 lastlines folder that the pane's command
//! deletes once printed. A host's panes are not printed into (their lines are on this Mac).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::app::Shared;
use crate::lastlines::{file, folder, usable_identity};
use crate::restore_srv::Exec;

/// The files the panes print start so (leftovers of a restore that did not print go next time).
const PRINT: &str = "print-";
/// Said under the lines printed, before the shell's prompt.
const SAID: &str = "terminum: the lines above are from before tmux restarted";

/// The command each old pane's replacement starts with (old pane id -> shell command), for the
/// panes of `sessions` (a checkpoint's) that have lines kept under the checkpoint's identity
/// `saved`; empty for a host's server, with lines off, or a server whose panes start another
/// command than a shell (`default-command`).
pub async fn commands(app: &Shared, exec: &Exec<'_>, server_id: &str, saved: Option<&str>, sessions: &[Value]) -> HashMap<String, String> {
    let Some(old) = saved.filter(|i| usable_identity(i)).map(str::to_string) else { return HashMap::new() };
    if !matches!(exec, Exec::Local { .. }) || crate::lastlines::wanted(app) == 0 {
        return HashMap::new();
    }
    // read from its configuration: after a kill-server no server runs yet (it ends again at once)
    let Ok(said) = exec.run(&["start-server", ";", "show", "-gv", "default-shell", ";", "show", "-gqv", "default-command"]).await else { return HashMap::new() };
    // the shell alone: no default-command (its empty line trimmed)
    let shell = match said.lines().collect::<Vec<_>>()[..] {
        [shell] if shell.starts_with('/') => shell.to_string(),
        _ => return HashMap::new(),
    };
    let ids: Vec<String> = sessions.iter().flat_map(|s| s["windows"].as_array().into_iter().flatten())
        .flat_map(|w| w["panes"].as_array().into_iter().flatten()).filter_map(|p| p["id"].as_str().map(str::to_string)).collect();
    let dir = app.cfg.dir.join("lastlines").join(folder(server_id));
    tokio::task::spawn_blocking(move || prepare(&dir, &old, &ids, &shell)).await.unwrap_or_default()
}

/// The print files of `ids` (old panes of identity `old`) written, with their commands.
fn prepare(dir: &Path, old: &str, ids: &[String], shell: &str) -> HashMap<String, String> {
    for f in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if f.file_name().to_string_lossy().starts_with(PRINT) {
            let _ = std::fs::remove_file(f.path());
        }
    }
    let mut out = HashMap::new();
    for id in ids {
        let Some(text) = kept(dir, old, id) else { continue };
        let path = dir.join(format!("{PRINT}{}-{}.txt", old, id.trim_start_matches('%')));
        if crate::config::write_private(&path, text.as_bytes()).is_ok() {
            out.insert(id.clone(), command(&path, shell));
        }
    }
    out
}

/// The text kept of old pane `id`, as the restore hands it on (lastlines::hand_on: its own
/// lines, or those from an earlier restart while it showed only a prompt), printable; None
/// when nothing is kept.
fn kept(dir: &Path, old: &str, id: &str) -> Option<String> {
    let text = |p: PathBuf| std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["text"].as_str().map(str::to_string));
    let live = text(file(dir, old, id, false));
    let printed = |t: &str| t.lines().filter(|l| !l.trim().is_empty()).count();
    let t = match live {
        Some(t) if printed(&t) > crate::lastlines::ONLY_A_PROMPT => t,
        live => text(file(dir, old, id, true)).or(live)?,
    };
    // the note an earlier restore printed is not printed again inside the lines
    let note = format!("[{SAID}]");
    let t: String = t.lines().filter(|l| *l != note).collect::<Vec<_>>().join("\n");
    let t: String = t.chars().filter(|c| *c == '\n' || *c == '\t' || !c.is_control()).collect();
    (!t.trim().is_empty()).then_some(t)
}

/// `s` as one word of a POSIX shell.
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The pane's command: the lines, then the file gone, a line that says what they are, and the
/// shell as tmux starts it (a login shell); a respawn later (the file gone) starts the shell only.
fn command(path: &Path, shell: &str) -> String {
    let f = quoted(&path.display().to_string());
    format!("[ -r {f} ] && {{ cat -- {f}; rm -f -- {f}; printf '\\n[%s]\\n\\n' {}; }}; exec {} -l", quoted(SAID), quoted(shell))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_prints_its_kept_lines_then_starts_its_shell() {
        let dir = crate::testutil::temp_root().join(format!("tm-print-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let put = |name: &str, text: &str| std::fs::write(dir.join(name), serde_json::json!({"text": text}).to_string()).unwrap();
        put("abc_3.json", &format!("old\n[{SAID}]\ndone\x1b[31m in 4 s\nall tests pass\n$ "));
        put("abc_4.json", "$ ");
        put("abc_4.before.json", "an agent's last turn\nits answer\nmore");
        put("abc_5.json", "  \n");
        std::fs::write(dir.join("print-abc-9.txt"), "left").unwrap();
        let ids: Vec<String> = ["%3", "%4", "%5", "%6"].map(String::from).to_vec();
        let out = prepare(&dir, "abc", &ids, "/bin/zsh");
        assert!(!dir.join("print-abc-9.txt").exists(), "a leftover goes");
        assert_eq!(out.len(), 2, "nothing printed for a blank pane or one with nothing kept: {out:?}");
        assert_eq!(std::fs::read_to_string(dir.join("print-abc-3.txt")).unwrap(), "old\ndone[31m in 4 s\nall tests pass\n$ ", "control characters and an earlier note dropped");
        assert_eq!(std::fs::read_to_string(dir.join("print-abc-4.txt")).unwrap(), "an agent's last turn\nits answer\nmore", "a prompt alone: the earlier lines");
        let f = dir.join("print-abc-3.txt").display().to_string();
        assert_eq!(out["%3"], format!("[ -r '{f}' ] && {{ cat -- '{f}'; rm -f -- '{f}'; printf '\\n[%s]\\n\\n' '{SAID}'; }}; exec '/bin/zsh' -l"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_word_with_quotes_stays_one_word() {
        assert_eq!(quoted("/Users/alex/it's"), "'/Users/alex/it'\\''s'");
    }
}
