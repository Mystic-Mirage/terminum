// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! terminumd's own tmux configuration (OQ-01): `~/.terminum/tmux.conf`, written once when
//! missing (the user may edit it afterwards), read by the private server terminumd starts.

use std::path::Path;

/// tmux keeps this many lines a pane: the page loads TM_HISTORY of them (AS-04).
pub const HISTORY_LIMIT: usize = 12_000;

/// The configuration for a history limit. `window-size latest` stays tmux's policy for a
/// window no page shows; terminumd sets `window-size manual` on a window while it sizes it
/// (AC-13). Extended keys let ⇧↩ and ⌃↩ reach programs that ask for them (AS-10).
pub fn text(history: usize) -> String {
    format!(
        "# terminum's tmux server (tmux -L terminum). Written by terminumd when missing; yours to edit.\n\
         set -g history-limit {history}\n\
         set -g window-size latest\n\
         set -g default-size 120x40\n\
         set -g extended-keys on\n\
         set -g extended-keys-format csi-u\n\
         set -g focus-events on\n\
         set -sg escape-time 0\n\
         set -g allow-rename on\n\
         set -g set-clipboard off\n"
    )
}

/// Writes the file when it is not there; never replaces the user's.
pub fn ensure(path: &Path, history: usize) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    crate::config::write_private(path, text(history).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_once_and_kept() {
        let dir = crate::testutil::temp_root().join(format!("tm-conf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("tmux.conf");
        ensure(&p, 12000).unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("history-limit 12000"));
        std::fs::write(&p, "set -g history-limit 500\n").unwrap();
        ensure(&p, 12000).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "set -g history-limit 500\n", "the user's edit stays");
        std::fs::remove_dir_all(&dir).ok();
    }
}
