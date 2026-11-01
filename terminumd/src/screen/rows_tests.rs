// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Tests of rows and their encoding; expected lines follow `screen.py`'s `enc_line` rules.

use super::*;
use serde_json::json;

fn widths(rows: &[Row]) -> Vec<(usize, bool)> {
    rows.iter().map(|r| (r.cells.len(), r.hard)).collect()
}

fn enc(text: &str, width: usize) -> Vec<Value> {
    capture_rows(text, width).iter().map(|r| encode(r, None)).collect()
}

#[test]
fn every_line_gives_a_row_and_blank_lines_are_empty() {
    let rows = capture_rows("ab\n\ncd\n", 10);
    assert_eq!(widths(&rows), vec![(2, true), (0, true), (2, true)]);
    assert_eq!(encode(&rows[1], None), json!({"r": [], "e": true}));
    assert!(capture_rows("", 10).is_empty());
}

#[test]
fn a_joined_line_of_two_and_a_half_widths_has_one_hard_end() {
    let line = "x".repeat(25);
    let rows = capture_rows(&format!("{line}\nnext\n"), 10);
    assert_eq!(widths(&rows), vec![(10, false), (10, false), (5, true), (4, true)]);
    assert_eq!(enc(&format!("{line}\n"), 10)[1], json!({"r": [["xxxxxxxxxx", null, null, 0]], "e": false}));
}

#[test]
fn a_line_of_exactly_the_width_is_one_row() {
    assert_eq!(widths(&capture_rows("abcd\n", 4)), vec![(4, true)]);
    assert_eq!(widths(&capture_rows("abcdefgh\n", 4)), vec![(4, false), (4, true)]);
}

#[test]
fn a_wide_char_at_the_edge_moves_to_the_next_row() {
    let rows = capture_rows("abc漢字\n", 4);
    // "abc" + 漢 would straddle the edge: the row ends one cell short
    assert_eq!(widths(&rows), vec![(3, false), (4, true)]);
    assert_eq!(row_text(&rows[0]), "abc");
    assert_eq!(encode(&rows[1], None), json!({"r": [[["漢", "", "字", ""], null, null, 0]], "e": true}));
    // one fitting exactly stays
    assert_eq!(widths(&capture_rows("ab漢x\n", 4)), vec![(4, false), (1, true)]);
}

#[test]
fn styles_group_into_runs_and_carry_across_lines() {
    let got = enc("\x1b[1;31mred\x1b[0m \x1b[38;5;196mx\x1b[38;2;1;2;3my\x1b[4:3mz\n\x1b[44mbg  \n", 20);
    assert_eq!(
        got[0],
        json!({"r": [["red", 1, null, 1], [" ", null, null, 0], ["x", 196, null, 0],
                     ["y", "#010203", null, 0], ["z", "#010203", null, 8]], "e": true})
    );
    // the fg and underline of line 1 are still on; a bg-colored trailing blank is kept
    assert_eq!(got[1], json!({"r": [["bg  ", "#010203", 4, 8]], "e": true}));
}

#[test]
fn trailing_blanks_are_trimmed_unless_they_draw() {
    assert_eq!(enc("ab   \n", 10)[0], json!({"r": [["ab", null, null, 0]], "e": true}));
    // bold and italic blanks draw nothing
    assert_eq!(enc("ab\x1b[1;3m  \n", 10)[0], json!({"r": [["ab", null, null, 0]], "e": true}));
    assert_eq!(enc("ab\x1b[41m  \x1b[0m  \n", 10)[0], json!({"r": [["ab", null, null, 0], ["  ", null, 1, 0]], "e": true}));
    for (sgr, flag) in [("4", 8), ("9", 16), ("7", 32)] {
        let got = &enc(&format!("a\x1b[{sgr}m \n"), 10)[0];
        assert_eq!(*got, json!({"r": [["a", null, null, 0], [" ", null, null, flag]], "e": true}), "{sgr}");
    }
    // inner blanks stay; a line of blanks is empty
    assert_eq!(enc("a  b\n", 10)[0], json!({"r": [["a  b", null, null, 0]], "e": true}));
    assert_eq!(enc("    \n", 10)[0], json!({"r": [], "e": true}));
}

#[test]
fn the_cursor_on_a_cell_gets_its_own_run() {
    let row = &capture_rows("abc\n", 10)[0];
    assert_eq!(
        encode(row, Some(1)),
        json!({"r": [["a", null, null, 0], ["b", null, null, 256], ["c", null, null, 0]], "e": true})
    );
    // on a trailing blank: that blank is kept
    let row = &capture_rows("ab  \n", 10)[0];
    assert_eq!(encode(row, Some(2)), json!({"r": [["ab", null, null, 0], [" ", null, null, 256]], "e": true}));
}

#[test]
fn the_cursor_past_the_text_is_padded() {
    let row = &capture_rows("$ ls\n", 10)[0];
    assert_eq!(encode(row, Some(4)), json!({"r": [["$ ls", null, null, 0], [" ", null, null, 256]], "e": true}));
    assert_eq!(
        encode(row, Some(7)),
        json!({"r": [["$ ls", null, null, 0], ["   ", null, null, 0], [" ", null, null, 256]], "e": true})
    );
    // past trimmed blanks: the gap counts from the kept cells
    let row = &capture_rows("$  \n", 10)[0];
    assert_eq!(
        encode(row, Some(5)),
        json!({"r": [["$", null, null, 0], ["    ", null, null, 0], [" ", null, null, 256]], "e": true})
    );
    assert_eq!(encode(&Row::default(), Some(0)), json!({"r": [[" ", null, null, 256]], "e": false}));
}

#[test]
fn the_cursor_keeps_its_style_flags() {
    let row = &capture_rows("\x1b[1;32mok\n", 10)[0];
    assert_eq!(encode(row, Some(1)), json!({"r": [["o", 2, null, 1], ["k", 2, null, 257]], "e": true}));
}

#[test]
fn a_combining_char_stays_in_its_cell() {
    let rows = capture_rows("e\u{301}x\n", 10);
    assert_eq!(rows[0].cells.len(), 2);
    assert_eq!(encode(&rows[0], None), json!({"r": [[["e\u{301}", "x"], null, null, 0]], "e": true}));
    assert_eq!(row_text(&rows[0]), "e\u{301}x");
}

#[test]
fn a_zwj_sequence_is_one_wide_cell() {
    let rows = capture_rows("a\u{1F468}\u{200D}\u{1F469}|\n", 10);
    assert_eq!(rows[0].cells.len(), 4);
    assert_eq!(encode(&rows[0], None), json!({"r": [[["a", "\u{1F468}\u{200D}\u{1F469}", "", "|"], null, null, 0]], "e": true}));
}

#[test]
fn vs16_makes_a_narrow_char_wide_as_tmux_does() {
    let rows = capture_rows("\u{2764}\u{FE0F}|\n", 10);
    assert_eq!(encode(&rows[0], None), json!({"r": [[["\u{2764}\u{FE0F}", "", "|"], null, null, 0]], "e": true}));
}

#[test]
fn control_chars_and_nul_become_spaces() {
    assert_eq!(enc("a\0b\tc\x07\n", 10)[0], json!({"r": [["a b c", null, null, 0]], "e": true}));
}

#[test]
fn text_form_only_below_u2000_without_wide_chars() {
    assert_eq!(enc("héllo\n", 10)[0], json!({"r": [["héllo", null, null, 0]], "e": true}));
    // box drawing (U+2500) is single width but forces the cell list
    assert_eq!(enc("\u{250c}\u{2500}a\n", 10)[0], json!({"r": [[["\u{250c}", "\u{2500}", "a"], null, null, 0]], "e": true}));
    // the list form holds only the run that needs it
    assert_eq!(
        enc("ab\x1b[31m\u{2500}\n", 10)[0],
        json!({"r": [["ab", null, null, 0], [["\u{2500}"], 1, null, 0]], "e": true})
    );
}

#[test]
fn encode_str_is_compact_json() {
    let row = &capture_rows("\x1b[1mhi\n", 10)[0];
    let s = encode_str(row, None);
    assert!(!s.contains(' '), "{s}");
    assert_eq!(serde_json::from_str::<Value>(&s).unwrap(), json!({"r": [["hi", null, null, 1]], "e": true}));
}

#[test]
fn row_text_skips_right_halves_and_trims() {
    let rows = capture_rows("\x1b[31m漢x  \n", 10);
    assert_eq!(row_text(&rows[0]), "漢x");
}

#[test]
fn screen_rows_pads_and_truncates_to_the_height() {
    let rows = screen_rows("a\nb\n", 10, 4);
    assert_eq!(widths(&rows), vec![(1, true), (1, true), (0, true), (0, true)]);
    let rows = screen_rows(&format!("{}\nb\n", "x".repeat(25)), 10, 2);
    assert_eq!(widths(&rows), vec![(10, false), (10, false)]);
}

#[test]
fn a_zero_width_char_at_line_start_is_its_own_cell() {
    let rows = capture_rows("\u{301}a\n", 10);
    assert_eq!(rows[0].cells.len(), 2);
}

#[test]
fn width_zero_is_treated_as_one() {
    assert_eq!(widths(&capture_rows("ab\n", 0)), vec![(1, false), (1, true)]);
}

/// A real tmux server on a private socket: colors, a wrapped line and a wide char at the edge.
#[test]
fn real_tmux_capture() {
    let script = concat!(
        "printf '\\033[1;31mred\\033[0m \\033[38;5;196mx\\033[48;2;1;2;3my \\033[0m\\n';",
        "printf 'abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ\\n';",
        "printf 'abcdefghijklmnopqrstuvwxyz0123456789ABC\\346\\274\\242Z\\n';",
        "sleep 30"
    );
    let Some(s) = crate::testutil::Tmux::start("screen", (40, 10), script) else { return };
    let mut text = String::new();
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        text = s.read(&["capture-pane", "-p", "-e", "-J", "-t", "main"]);
        if text.contains('Z') {
            break;
        }
    }
    let rows = screen_rows(&text, 40, 10);
    let got: Vec<Value> = rows.iter().take(6).map(|r| encode(r, None)).collect();
    assert_eq!(
        got,
        vec![
            json!({"r": [["red", 1, null, 1], [" ", null, null, 0], ["x", 196, null, 0], ["y ", 196, "#010203", 0]], "e": true}),
            json!({"r": [["abcdefghijklmnopqrstuvwxyz0123456789ABCD", null, null, 0]], "e": false}),
            json!({"r": [["EFGHIJ", null, null, 0]], "e": true}),
            json!({"r": [["abcdefghijklmnopqrstuvwxyz0123456789ABC", null, null, 0]], "e": false}),
            json!({"r": [[["漢", "", "Z"], null, null, 0]], "e": true}),
            json!({"r": [], "e": true}),
        ],
        "capture: {text:?}"
    );
    assert_eq!(rows.len(), 10);
}

/// AC-04: a row tmux wrapped right after a space keeps it, so the page joins "a b" and not "ab".
#[test]
fn a_wrapped_row_keeps_its_trailing_space() {
    let rows = capture_rows("abcd efgh\n", 5);
    assert_eq!(encode(&rows[0], None), json!({"r": [["abcd ", null, null, 0]], "e": false}));
    assert_eq!(encode(&rows[1], None), json!({"r": [["efgh", null, null, 0]], "e": true}));
}

/// AC-35, AS-03: an OSC 8 hyperlink as tmux 3.6a's `capture-pane -e` gives it (`id=` params, ST
/// ends, an empty URI closes it) keeps its address with its run, also across a wrapped row;
/// one too long or with a control character, or cut by the line's end, is plain text.
#[test]
fn an_osc8_link_keeps_its_address_with_its_run() {
    let mr = "https://git.example/a/-/merge_requests/318";
    let text = format!("see \x1b]8;id=a1;{mr}\x1b\\MR !318\x1b]8;;\x1b\\ end\n");
    assert_eq!(enc(&text, 40), vec![json!({"r": [["see ", null, null, 0], ["MR !318", null, null, 0, 0], [" end", null, null, 0]], "e": true, "h": [mr]})]);
    let wrapped = format!("\x1b]8;;{mr}\x07abcdefgh\x1b]8;;\x07!\n");
    assert_eq!(enc(&wrapped, 5), vec![
        json!({"r": [["abcde", null, null, 0, 0]], "e": false, "h": [mr]}),
        json!({"r": [["fgh", null, null, 0, 0], ["!", null, null, 0]], "e": true, "h": [mr]}),
    ]);
    let long = format!("\x1b]8;;https://x.example/{}\x1b\\a\x1b]8;;\x1b\\\n", "y".repeat(2048));
    assert_eq!(enc(&long, 10), vec![json!({"r": [["a", null, null, 0]], "e": true})]);
    assert_eq!(enc("\x1b]8;;https://x.example/\x01\x1b\\a\n", 10), vec![json!({"r": [["a", null, null, 0]], "e": true})]);
    assert_eq!(enc("a\x1b]8;;https://x.example/cut\n", 10), vec![json!({"r": [["a", null, null, 0]], "e": true})]);
    assert_eq!(enc("\x1b]8;;https://x.example/\u{202E}gpj\x1b\\a\n", 10), vec![json!({"r": [["a", null, null, 0]], "e": true})]);
    for odd in ["\u{200B}", "\u{200D}"] {
        assert_eq!(enc(&format!("\x1b]8;;https://x.example/a{odd}b\x1b\\a\n"), 10), vec![json!({"r": [["a", null, null, 0]], "e": true})]);
    }
}

/// AC-35: one address under many colors is sent once in its row ("h"); a second address gets its own index.
#[test]
fn a_links_address_is_sent_once_per_row() {
    let a = format!("https://x.example/{}", "y".repeat(2000));
    let text = format!("\x1b]8;;{a}\x1b\\\x1b[31mr\x1b[32mg\x1b[34mb\x1b]8;;https://z.example/\x1b\\z\x1b]8;;\x1b\\\n");
    let row = &enc(&text, 10)[0];
    assert_eq!(row["h"], json!([a, "https://z.example/"]));
    assert_eq!(row["r"], json!([["r", 1, null, 0, 0], ["g", 2, null, 0, 0], ["b", 4, null, 0, 0], ["z", 4, null, 0, 1]]));
    assert_eq!(row.to_string().matches(&a).count(), 1);
}

/// AC-35: a link's blank cells are kept at a hard row's end (they are the link's own).
#[test]
fn a_links_trailing_blank_is_kept() {
    assert_eq!(enc("\x1b]8;;https://x.example/\x1b\\a \x1b]8;;\x1b\\\n", 10),
        vec![json!({"r": [["a ", null, null, 0, 0]], "e": true, "h": ["https://x.example/"]})]);
}
