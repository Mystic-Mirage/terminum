// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The JSON locator of settings.json, the theme and hot keys files: where a value sits in the
//! text, to name the line of a bad value and to replace one key's value in place (settings_save.rs).

/// A step of a path into a JSON document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg<'a> {
    Key(&'a str),
    Index(usize),
}

/// Where a value sits in the text: byte range and 1-based line of its first byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

/// The value at `path` in a JSON text that parses (the first of duplicate keys), or None.
pub fn locate(text: &str, path: &[Seg]) -> Option<Span> {
    let mut sc = Scan { t: text, i: 0 };
    let (start, end) = sc.find(path)?;
    Some(Span { start, end, line: 1 + text.as_bytes()[..start].iter().filter(|&&b| b == b'\n').count() })
}

/// The line of the value at `path`, or 1 when it cannot be found.
pub fn line_of(text: &str, path: &[Seg]) -> usize {
    locate(text, path).map_or(1, |s| s.line)
}

struct Scan<'t> {
    t: &'t str,
    i: usize,
}

impl Scan<'_> {
    fn peek(&self) -> Option<u8> {
        self.t.as_bytes().get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, b: u8) -> Option<()> {
        self.ws();
        (self.peek()? == b).then(|| self.i += 1)
    }

    /// A string token starting here, decoded.
    fn string(&mut self) -> Option<String> {
        self.ws();
        let start = self.i;
        if self.peek()? != b'"' {
            return None;
        }
        self.i += 1;
        loop {
            match self.peek()? {
                b'\\' => self.i += 2,
                b'"' => break,
                _ => self.i += 1,
            }
        }
        self.i += 1;
        serde_json::from_str(&self.t[start..self.i]).ok()
    }

    /// Skips one value; its byte range.
    fn skip(&mut self) -> Option<(usize, usize)> {
        self.ws();
        let start = self.i;
        match self.peek()? {
            b'"' => {
                self.string()?;
            }
            b'{' | b'[' => {
                let mut depth = 0usize;
                loop {
                    match self.peek()? {
                        b'"' => {
                            self.string()?;
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => depth -= 1,
                        _ => {}
                    }
                    self.i += 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            _ => {
                while !matches!(self.peek(), None | Some(b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n')) {
                    self.i += 1;
                }
            }
        }
        Some((start, self.i))
    }

    fn find(&mut self, path: &[Seg]) -> Option<(usize, usize)> {
        let Some((seg, rest)) = path.split_first() else { return self.skip() };
        self.ws();
        match (seg, self.peek()?) {
            (Seg::Key(want), b'{') => {
                self.i += 1;
                loop {
                    self.ws();
                    if self.peek()? == b'}' {
                        return None;
                    }
                    let key = self.string()?;
                    self.eat(b':')?;
                    if key == *want {
                        return self.find(rest);
                    }
                    self.skip()?;
                    self.ws();
                    if self.peek()? == b',' {
                        self.i += 1;
                    }
                }
            }
            (Seg::Index(want), b'[') => {
                self.i += 1;
                for n in 0.. {
                    self.ws();
                    if self.peek()? == b']' {
                        return None;
                    }
                    if n == *want {
                        return self.find(rest);
                    }
                    self.skip()?;
                    self.ws();
                    if self.peek()? == b',' {
                        self.i += 1;
                    }
                }
                None
            }
            _ => None,
        }
    }
}
