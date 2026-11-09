// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The SGR parser of tmux's captures (AC-02, AS-03): `capture-pane -p -e -J` text holds the
//! pane's characters, `CSI … m` sequences and `OSC 8` hyperlinks (their address is kept, AC-35);
//! every other escape sequence (another CSI, OSC, a DCS, a lone ESC) is skipped and draws nothing.
//!
//! tmux keeps the SGR state running across the lines of one capture (it emits no reset at a
//! line end with `-J`), so a [`Style`] and the open link are carried from one line to the next by
//! the caller.

use std::sync::Arc;

/// The longest hyperlink address kept; a longer one draws as plain text.
const LINK_MAX: usize = 2048;

/// A cell color: the terminal's default, a 256-palette index, or RGB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Color {
    #[default]
    Default,
    Palette(u8),
    Rgb(u8, u8, u8),
}

/// Bold.
pub const BOLD: u16 = 1;
/// Faint (dim).
pub const FAINT: u16 = 2;
/// Italic.
pub const ITALIC: u16 = 4;
/// Underline, any style (`4`, `4:x`, `21`).
pub const UNDERLINE: u16 = 8;
/// Strikethrough.
pub const STRIKE: u16 = 16;
/// Inverse video.
pub const INVERSE: u16 = 32;
/// Invisible (hidden).
pub const INVISIBLE: u16 = 64;
/// Blink.
pub const BLINK: u16 = 128;

/// The SGR state: colors and the page's flag bits (bold 1 … blink 128).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub flags: u16,
}

/// Applies the parameters of one `CSI <params> m` (the bytes between `[` and `m`).
pub fn apply_sgr(style: &mut Style, params: &str) {
    if params.is_empty() {
        *style = Style::default();
        return;
    }
    let mut groups = params.split(';');
    while let Some(group) = groups.next() {
        if group.contains(':') {
            colon_group(style, group);
            continue;
        }
        match num(group) {
            code @ (38 | 48 | 58) => {
                let color = extended(&mut groups);
                set_color(style, code, color);
            }
            code => simple(style, code),
        }
    }
}

/// A parameter as a number: empty is 0, an unreadable one is out of every range.
fn num(s: &str) -> u32 {
    if s.is_empty() { 0 } else { s.parse().unwrap_or(u32::MAX) }
}

fn byte(s: &str) -> Option<u8> {
    u8::try_from(num(s)).ok()
}

/// `38;5;n` / `38;2;r;g;b` in the `;` form, taking its groups from `rest`.
fn extended<'a>(rest: &mut impl Iterator<Item = &'a str>) -> Option<Color> {
    match num(rest.next()?) {
        5 => byte(rest.next()?).map(Color::Palette),
        2 => {
            let (r, g, b) = (rest.next()?, rest.next()?, rest.next()?);
            rgb(r, g, b)
        }
        _ => None,
    }
}

fn rgb(r: &str, g: &str, b: &str) -> Option<Color> {
    Some(Color::Rgb(byte(r)?, byte(g)?, byte(b)?))
}

/// A `:` group: `38:5:n`, `38:2::r:g:b` (or without the color space id), `4:x`.
fn colon_group(style: &mut Style, group: &str) {
    let mut buf = [""; 6];
    let mut n = 0;
    for sub in group.split(':').take(6) {
        buf[n] = sub;
        n += 1;
    }
    let subs = &buf[..n];
    let code = num(subs[0]);
    match code {
        4 => {
            if num(subs[1]) == 0 {
                style.flags &= !UNDERLINE;
            } else {
                style.flags |= UNDERLINE;
            }
        }
        38 | 48 | 58 => {
            let color = match (num(subs[1]), subs.len()) {
                (5, 3..) => byte(subs[2]).map(Color::Palette),
                (2, 6..) => rgb(subs[3], subs[4], subs[5]),
                (2, 5) => rgb(subs[2], subs[3], subs[4]),
                _ => None,
            };
            set_color(style, code, color);
        }
        _ => simple(style, code),
    }
}

fn set_color(style: &mut Style, code: u32, color: Option<Color>) {
    match (code, color) {
        (38, Some(c)) => style.fg = c,
        (48, Some(c)) => style.bg = c,
        _ => {} // 58 (underline color) is not drawn; a malformed color changes nothing
    }
}

fn simple(style: &mut Style, code: u32) {
    let f = &mut style.flags;
    match code {
        0 => *style = Style::default(),
        1 => *f |= BOLD,
        2 => *f |= FAINT,
        3 => *f |= ITALIC,
        4 | 21 => *f |= UNDERLINE, // 21 is a double underline in tmux
        5 | 6 => *f |= BLINK,
        7 => *f |= INVERSE,
        8 => *f |= INVISIBLE,
        9 => *f |= STRIKE,
        22 => *f &= !(BOLD | FAINT),
        23 => *f &= !ITALIC,
        24 => *f &= !UNDERLINE,
        25 => *f &= !BLINK,
        27 => *f &= !INVERSE,
        28 => *f &= !INVISIBLE,
        29 => *f &= !STRIKE,
        30..=37 => style.fg = Color::Palette((code - 30) as u8),
        39 => style.fg = Color::Default,
        40..=47 => style.bg = Color::Palette((code - 40) as u8),
        49 => style.bg = Color::Default,
        90..=97 => style.fg = Color::Palette((code - 90 + 8) as u8),
        100..=107 => style.bg = Color::Palette((code - 100 + 8) as u8),
        _ => {} // 53/55 overline and anything else: not drawn
    }
}

/// Walks one line (no `\n`), applying SGR to `style` and OSC 8 to `link` and calling `emit` with
/// every other character (control characters included: the caller decides how they draw), the
/// style it has and the link it is in. An escape sequence cut by the line's end is dropped.
pub fn parse_line(line: &str, style: &mut Style, link: &mut Option<Arc<str>>, mut emit: impl FnMut(char, Style, Option<&Arc<str>>)) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < line.len() {
        // fast path: the text up to the next ESC
        let next = memchr_esc(&bytes[i..]).map_or(line.len(), |k| i + k);
        if next > i {
            for c in line[i..next].chars() {
                emit(c, *style, link.as_ref());
            }
            i = next;
            continue;
        }
        i = skip_escape(line, i + 1, style, link);
    }
}

/// Whether every escape in `line` is one `parse_line` applies: a color (SGR) or a link (OSC 8,
/// ended by BEL or ST). A screen of only those reads as its text (the agents' box, AS-125).
pub fn only_applied(line: &str) -> bool {
    let mut i = 0;
    while let Some(k) = memchr_esc(&line.as_bytes()[i..]) {
        let at = i + k + 1;
        i = skip_escape(line, at, &mut Style::default(), &mut None);
        let seq = &line[at..i];
        let color = seq.strip_prefix('[').and_then(|s| s.strip_suffix('m')).is_some_and(|p| p.bytes().all(|c| c.is_ascii_digit() || c == b';' || c == b':'));
        if !color && !seq.starts_with("]8;") {
            return false;
        }
    }
    true
}

fn memchr_esc(b: &[u8]) -> Option<usize> {
    b.iter().position(|&x| x == 0x1b)
}

/// After an ESC at `i - 1`: consumes the sequence (applying it when it is SGR or OSC 8) and
/// returns the index after it.
fn skip_escape(line: &str, i: usize, style: &mut Style, link: &mut Option<Arc<str>>) -> usize {
    let b = line.as_bytes();
    match b.get(i) {
        Some(b'[') => {
            // CSI: parameter bytes 0x30-0x3F, intermediates 0x20-0x2F, a final 0x40-0x7E
            let start = i + 1;
            let mut j = start;
            while j < b.len() && (0x30..=0x3f).contains(&b[j]) {
                j += 1;
            }
            let params_end = j;
            while j < b.len() && (0x20..=0x2f).contains(&b[j]) {
                j += 1;
            }
            match b.get(j) {
                Some(&f) if (0x40..=0x7e).contains(&f) => {
                    let params = &line[start..params_end];
                    let private = params.starts_with(['<', '=', '>', '?']);
                    if f == b'm' && params_end == j && !private {
                        apply_sgr(style, params);
                    }
                    j + 1
                }
                // a malformed CSI: drop what was read, keep the byte that broke it
                Some(_) => j,
                None => b.len(),
            }
        }
        // OSC, DCS, SOS, PM, APC: a string up to BEL (OSC) or ST (ESC \)
        Some(b']' | b'P' | b'X' | b'^' | b'_') => {
            let mut j = i + 1;
            while j < b.len() {
                let end = match b[j] {
                    0x07 => j + 1,
                    0x1b if b.get(j + 1) == Some(&b'\\') => j + 2,
                    _ => {
                        j += 1;
                        continue;
                    }
                };
                if b[i] == b']' && let Some(rest) = line[i + 1..j].strip_prefix("8;") {
                    *link = hyperlink(rest);
                }
                return end;
            }
            b.len()
        }
        // ESC, intermediates, a final byte (charset designation, ESC 7, ESC =, …)
        Some(&c) if (0x20..=0x7e).contains(&c) => {
            let mut j = i;
            while j < b.len() && (0x20..=0x2f).contains(&b[j]) {
                j += 1;
            }
            match b.get(j) {
                Some(&f) if (0x30..=0x7e).contains(&f) => j + 1,
                _ => j,
            }
        }
        // a lone ESC (line end, or before a non-ASCII or control character)
        _ => i,
    }
}

/// The address of an OSC 8 `params;URI` (after "8;"): none for the empty URI that ends a link,
/// nor for one too long or with a control or invisible character (termtext.rs `control`: it
/// could make the address shown on hover read otherwise), which draws as plain text.
fn hyperlink(rest: &str) -> Option<Arc<str>> {
    let uri = rest.split_once(';')?.1;
    (!uri.is_empty() && uri.len() <= LINK_MAX && !uri.chars().any(crate::termtext::control)).then(|| Arc::from(uri))
}

#[cfg(test)]
/// The lines of one capture (split on `\n`; a final `\n` ends the last line), each as its
/// characters with their styles; `style` starts the first line and ends as the last left it.
pub fn parse_capture(text: &str, style: &mut Style) -> Vec<Vec<(char, Style)>> {
    lines(text)
        .map(|line| {
            let mut out = Vec::with_capacity(line.len());
            parse_line(line, style, &mut None, |c, st, _| out.push((c, st)));
            out
        })
        .collect()
}

/// The lines of a capture: split on `\n`, without the empty piece after a final `\n`.
pub fn lines(text: &str) -> impl Iterator<Item = &str> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    (!text.is_empty()).then_some(body).into_iter().flat_map(|t| t.split('\n'))
}

#[cfg(test)]
#[path = "sgr_tests.rs"]
mod tests;
