// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A full-screen program's clipboard write (OSC 52) in a pane's `%output` (AC-06): the one
//! escape sequence terminumd reads from output. Control mode writes a byte under 32 and a
//! backslash as `\ooo`, so the write is `\033]52;<targets>;<base64>` closed by `\007` or
//! `\033\134` (ST); base64 never holds a backslash. A write tmux split over two lines is carried.

use crate::termtext::clean;
use std::collections::HashMap;

/// The longest write kept: base64 of 1 MB of text.
const MOST: usize = (1 << 20) / 3 * 4 + 4;
const START: &[u8] = b"\\033]52;";
const BEL: &[u8] = b"\\007";
const ST: &[u8] = b"\\033\\134";

/// Unclosed writes, by pane (one control client's lines).
#[derive(Default)]
pub struct Clips {
    open: HashMap<String, Vec<u8>>,
}

impl Clips {
    /// The texts a `%output` or `%extended-output` line of the pane `%N` (control.rs `notice`)
    /// closes, in order; reads (`?`), bad base64, text that is not UTF-8 and writes over `MOST`
    /// are dropped. Whatever a write still lacks at the line's end (its opener, its text, its ST)
    /// waits for the pane's next line.
    pub fn line(&mut self, pane: &str, line: &[u8]) -> Vec<String> {
        let Some(rest) = payload(line) else { return Vec::new() };
        // most lines: no write open, none in them, none starting at their end
        if !self.open.contains_key(pane) && find(rest, b"]52;").is_none() && opener_tail(rest) == 0 {
            return Vec::new();
        }
        let mut data = self.open.remove(pane).unwrap_or_default();
        data.extend_from_slice(rest);
        let (mut out, mut at) = (Vec::new(), 0);
        let carry = loop {
            let Some(i) = find(&data[at..], START).map(|i| at + i) else {
                break Some(data.len() - opener_tail(&data[at..])); // the line may end inside an opener: `\033]5`
            };
            let s = i + START.len();
            let Some(semi) = data[s..].iter().position(|b| *b == b';') else {
                break data[s..].iter().all(target).then_some(i);
            };
            if !data[s..s + semi].iter().all(target) {
                at = s;
                continue;
            }
            let b = s + semi + 1;
            let Some(e) = data[b..].iter().position(|c| *c == b'\\').map(|e| b + e) else {
                // text that is not base64 never decodes: no use carrying it
                break data[b..].iter().all(|c| c.is_ascii_alphanumeric() || b"+/=".contains(c)).then_some(i);
            };
            let close = &data[e..];
            if close.starts_with(BEL) || close.starts_with(ST) {
                out.extend(text(&data[b..e]));
            } else if ST.starts_with(close) {
                break Some(i); // the line ends inside the ST
            }
            at = e;
        };
        if let Some(c) = carry.filter(|c| data.len() - c <= MOST + 32 && *c < data.len()) {
            data.drain(..c);
            self.open.insert(pane.to_string(), data);
        }
        out
    }

    /// The pane's output was dropped (tmux's flow control): a write it was in is gone.
    pub fn forget(&mut self, pane: &str) {
        self.open.remove(pane);
    }
}

/// A target of xterm's OSC 52: `c`, `p`, `q`, `s`, `0`-`7` (any letter is let through).
fn target(b: &u8) -> bool {
    b.is_ascii_lowercase() || (b'0'..=b'7').contains(b)
}

/// The program's bytes of a `%output %N …` or `%extended-output %N age … : …` line.
fn payload(line: &[u8]) -> Option<&[u8]> {
    let mut words = line.splitn(3, |b| *b == b' ');
    let (kind, rest) = (words.next()?, words.nth(1)?);
    match kind {
        b"%output" => Some(rest),
        b"%extended-output" => find(rest, b" : ").map(|i| &rest[i + 3..]),
        _ => None,
    }
}

/// How many bytes at the end of `b` begin an opener (`\033]5`): 0 for none.
fn opener_tail(b: &[u8]) -> usize {
    (1..START.len()).rev().find(|k| b.ends_with(&START[..*k])).unwrap_or(0)
}

/// Where `needle` starts in `hay`: its first byte found alone first, as most output has none.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let (first, rest) = needle.split_first()?;
    hay.iter().enumerate().position(|(i, b)| b == first && hay[i + 1..].starts_with(rest))
}

/// The text of a write's base64, as safe to paste as a browser's selection: no control
/// characters but tab and line ends (termtext.rs `clean`), so a Copy pasted into a
/// terminal can never end its bracketed paste (`ESC [201~`) or send keys; None when not UTF-8.
fn text(b64: &[u8]) -> Option<String> {
    if b64.len() > MOST {
        return None;
    }
    Some(clean(&String::from_utf8(decode(b64)?).ok()?, true)).filter(|t| !t.trim().is_empty())
}

/// Standard base64 (padding optional); None for a read (`?`) or a stray character.
fn decode(b64: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(b64.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for &c in b64.iter().take_while(|c| **c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What each line closes, with its pane (as control.rs `notice` reads it).
    fn lines(c: &mut Clips, ls: &[&str]) -> Vec<(String, String)> {
        ls.iter().flat_map(|l| one(c, l).into_iter().map(|t| (l.split(' ').nth(1).unwrap_or_default().to_string(), t))).collect()
    }
    fn one(c: &mut Clips, l: &str) -> Vec<String> {
        c.line(l.split(' ').nth(1).unwrap_or_default(), l.as_bytes())
    }
    const ONE: &str = "%output %3 \\033]52;c;bGF1ZGUgQ29kZSB2Mi4xLjMxNg==\\007";

    /// As Claude Code 2.1.316 wrote it (captured from `tmux -C`): BEL, then ST, the passthrough's
    /// doubled ESC and other output around it.
    #[test]
    fn a_write_closed_by_bel_or_st_is_its_text() {
        let mut c = Clips::default();
        assert_eq!(lines(&mut c, &[ONE]), vec![("%3".into(), "laude Code v2.1.316".into())]);
        assert_eq!(lines(&mut c, &["%output %4 \\033[1mx\\033]52;c;w6kgdGV4dA\\033\\134tail"]), vec![("%4".into(), "é text".into())]);
        assert_eq!(lines(&mut c, &["%output %4 \\033Ptmux;\\033\\033]52;c;eA==\\007\\033\\134"]), vec![("%4".into(), "x".into())]);
        assert_eq!(lines(&mut c, &["%extended-output %5 12 : \\033]52;pc;eQ==\\007"]), vec![("%5".into(), "y".into())]);
        assert_eq!(lines(&mut c, &["%output %5 \\033]52;s0;eQ==\\007"]), vec![("%5".into(), "y".into())]);
    }

    #[test]
    fn a_write_split_over_lines_is_carried_by_pane() {
        let mut c = Clips::default();
        let got = lines(&mut c, &["%output %3 \\033]52;c;bGF1ZGUgQ29k", "%output %9 \\033]52;c;eA", "%output %3 ZSB2Mi4xLjMxNg==\\007 more", "%output %9 ==\\007"]);
        assert_eq!(got, vec![("%3".into(), "laude Code v2.1.316".into()), ("%9".into(), "x".into())]);
    }

    /// tmux cuts its lines where its read of the pane ended: inside the opener, the text or the ST.
    #[test]
    fn a_write_cut_anywhere_comes_whole() {
        let whole = "\\033]52;c;bGF1ZGUgQ29kZSB2Mi4xLjMxNg==\\033\\134";
        // never inside a `\\ooo`: tmux writes an escaped byte whole
        let cuts = (1..whole.len()).filter(|i| !whole[..*i].ends_with(['\\', '0', '3', '1']) || whole[..*i].ends_with("033") || whole[..*i].ends_with("134"));
        for cut in cuts {
            let mut c = Clips::default();
            let got = lines(&mut c, &[&format!("%output %3 x{}", &whole[..cut]), &format!("%output %3 {}y", &whole[cut..])]);
            assert_eq!(got, vec![("%3".into(), "laude Code v2.1.316".into())], "cut at {cut}: {}", &whole[..cut]);
            assert!(c.open.is_empty(), "cut at {cut}");
        }
    }

    /// tmux's flow control drops a paused pane's output: a write it was in is never glued to later text.
    #[test]
    fn a_pause_forgets_the_write_it_cut() {
        let mut c = Clips::default();
        assert!(one(&mut c, "%output %3 \\033]52;c;bGF1").is_empty());
        c.forget("%3");
        assert!(one(&mut c, "%output %3 ZGUgQ29kZQ==\\007").is_empty());
    }

    /// A hostile write (a `cat` of a file) never carries keys into a paste: ESC, C1, CR go, and
    /// nothing that hides what it runs.
    #[test]
    fn control_characters_are_dropped_and_line_ends_kept() {
        let b64 = |s: &str| encode(s.as_bytes());
        let mut c = Clips::default();
        let line = format!("%output %3 \\033]52;c;{}\\007", b64("a\u{1b}[201~\r\nrm -rf x\u{9b}31m\u{7f}\tb\rc"));
        assert_eq!(lines(&mut c, &[&line]), vec![("%3".into(), "a[201~\nrm -rf x31m\tb\nc".into())]);
        // bidi controls and zero-width characters go (a line that reads otherwise than it runs), ZWNJ and ZWJ stay
        let hidden = format!("%output %3 \\033]52;c;{}\\007", b64("ls\u{202e}\u{2066}\u{200b}\u{feff}\u{e0041} x\u{200c}y\u{200d}z"));
        assert_eq!(one(&mut c, &hidden), vec!["ls x\u{200c}y\u{200d}z".to_string()]);
        let blank = format!("%output %3 \\033]52;c;{}\\007", b64("\u{1b}\u{7}"));
        assert!(one(&mut c, &blank).is_empty());
    }

    fn encode(b: &[u8]) -> String {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        b.chunks(3).map(|c| {
            let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
            (0..4).map(|i| if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' }).collect::<String>()
        }).collect()
    }

    #[test]
    fn reads_bad_writes_and_other_lines_give_nothing() {
        let mut c = Clips::default();
        for l in ["%output %3 \\033]52;c;?\\007", "%output %3 \\033]52;c;not*base64\\007", "%output %3 \\033]52;c;\\007",
            "%output %3 \\033]52;c;//79\\007", "%output %3 52;c;eA==", "%begin 1 2 1", "%output %3 \\033]52;c;eA==\\033[m"] {
            assert!(one(&mut c, l).is_empty(), "{l}");
        }
        // an unclosed write over the limit is dropped, and the pane's next write still comes
        let big = format!("%output %3 \\033]52;c;{}", "A".repeat(MOST + 64));
        assert!(one(&mut c, &big).is_empty() && c.open.is_empty());
        // nor is text that is not base64 carried
        assert!(one(&mut c, "%output %3 \\033]52;c;eA== done, more text").is_empty() && c.open.is_empty());
        assert_eq!(lines(&mut c, &[ONE]).len(), 1);
    }
}
