// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The one source of the plan rules that `terminum mcp` (cli/mcp.rs), terminumd's /agent
//! (agentapi.rs), agents' records (agentmarks.rs) and the hooks' plan ops (cli/hookplan.rs)
//! share: words, limits and the cleaning of model text. Lengths count characters after `clean`.

/// An item's statuses.
pub const STATUSES: [&str; 4] = ["pending", "in_progress", "completed", "blocked"];
/// The answer to a call from outside a terminum pane.
pub const NOT_IN_PANE: &str = "This session does not run in a terminum pane; do not call terminum's tools again.";

/// A plan's items, an item's text, doing, title, the ETA's minutes.
pub const ITEMS_MOST: usize = 50;
pub const ITEM_TEXT: usize = 200;
pub const DOING: usize = 120;
pub const TITLE: usize = 80;
pub const ETA_MOST: u64 = 1440;

/// `s` without control characters (line breaks and tabs kept when `lines`), what hides what a
/// line reads (termtext.rs `clean`) and secrets' values, trimmed.
pub fn clean_with(s: &str, lines: bool) -> String {
    crate::sent::redact(crate::termtext::clean(s, lines).trim())
}

/// `s` cleaned, line breaks and tabs kept.
pub fn clean(s: &str) -> String {
    clean_with(s, true)
}

/// A whole number of minutes from 1 to ETA_MOST; a float or anything else is None.
pub fn eta_minutes(v: &serde_json::Value) -> Option<u64> {
    v.as_u64().filter(|m| (1..=ETA_MOST).contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaning_drops_controls_and_bidi_keeps_lines() {
        assert_eq!(clean(" a\u{202E}b\u{2066}c\u{200F}\u{7}\nd\t "), "abc\nd");
        assert_eq!(clean("z\u{200B}w\u{FEFF}j\u{200D}\u{2028}x"), "zwj\u{200D}\nx");
        assert_eq!(clean_with("a\nb\u{2069}", false), "ab");
        assert_eq!(clean("\u{202A}\u{202C}"), "");
        assert_eq!(clean("é ✓ 中"), "é ✓ 中", "other text stays");
    }

    #[test]
    fn eta_is_a_whole_number_in_range() {
        use serde_json::json;
        assert_eq!(eta_minutes(&json!(30)), Some(30));
        assert_eq!(eta_minutes(&json!(1440)), Some(1440));
        assert_eq!(eta_minutes(&json!(30.0)), None);
        assert_eq!(eta_minutes(&json!(0)), None);
        assert_eq!(eta_minutes(&json!(1441)), None);
        assert_eq!(eta_minutes(&json!("30")), None);
    }
}
