// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What one page has of a pane's history (AC-32), for folding: the lines it was sent, the folds
//! it was told of, and when the newest lines are looked at next.

use serde_json::Value;
use std::collections::BTreeMap;

use super::super::pystr::text_of;
use super::{folds, Fold, CONTEXT, SAME_HEAD, SAME_TAIL, SCAN_EVERY};

/// What one page has of a pane's history, for folding: the text of the lines it was sent and
/// the folds it was told of (absolute line numbers).
pub struct History {
    keep: usize,                    // lines kept (the page's own limit)
    text: BTreeMap<i64, (String, bool)>, // n -> (text, hard end)
    told: BTreeMap<i64, i64>,       // first n -> last n of a fold told
    pending: bool,                  // lines scrolled off since the last look
    scanned: f64,                   // when they were last looked at
}

impl History {
    pub fn new(keep: usize) -> Self {
        History { keep, text: BTreeMap::new(), told: BTreeMap::new(), pending: false, scanned: 0.0 }
    }

    #[cfg(test)]
    /// Lines kept for folding (the page's limit and CONTEXT more).
    pub fn known(&self) -> usize {
        self.text.len()
    }

    /// Lines first.. as the page is to get them, and the folds found. `bulk` (opening the
    /// pane, older lines): a folded line is null, sent only when asked for. Else (lines that
    /// scrolled off the screen, which the page has already): sent as they are; their folds
    /// come from scan(), at most every SCAN_EVERY seconds.
    pub fn take(&mut self, first: i64, lines: Vec<Value>, bulk: bool) -> (Vec<Value>, Vec<Fold>) {
        for (k, line) in lines.iter().enumerate() {
            self.text.insert(first + k as i64, (text_of(line), line["e"].as_bool().unwrap_or(false)));
        }
        let cap = self.keep + CONTEXT as usize;
        if self.text.len() > cap {
            let oldest: Vec<i64> = self.text.keys().take(self.text.len() - cap).copied().collect();
            for n in oldest {
                self.text.remove(&n);
            }
            let low = *self.text.keys().next().expect("lines kept");
            self.told.retain(|_, b| *b >= low); // folds the page has no more
        }
        if !bulk {
            self.pending = self.pending || !lines.is_empty();
            return (lines, Vec::new());
        }
        let len = lines.len() as i64;
        let found = self.find(first, first + len + CONTEXT, &[]); // newer lines known: the end of a fold
        let mut out = lines;
        for f in &found {
            for n in f.n.max(first)..=f.to.min(first + len - 1) {
                out[(n - first) as usize] = Value::Null;
            }
        }
        (out, found)
    }

    /// Whether lines scrolled off since the last look, SCAN_EVERY seconds ago or more.
    pub fn due(&self, now: f64) -> bool {
        self.pending && now - self.scanned >= SCAN_EVERY
    }

    /// Folds among the newest lines (they scrolled off the screen meanwhile), the screen's rows
    /// (from line `top`) read after them: a tool's output that scrolled off whole folds while
    /// the next tool's ⏺ is still on the screen, not only once that scrolled off too. A fold
    /// reaching into the screen waits; the screen folds its own.
    pub fn scan(&mut self, now: f64, top: Option<i64>, screen: &[(&str, bool)]) -> Vec<Fold> {
        (self.pending, self.scanned) = (false, now);
        let Some(&last) = self.text.keys().next_back() else { return Vec::new() };
        let after = if top == Some(last + 1) { screen } else { &[] };   // right after them, no gap
        self.find(last + 1 - CONTEXT, last + 1, after)
    }

    /// New folds among the known lines from lo (a contiguous run of them, up to hi); rows
    /// `after` the newest known lines are read too, never folded here.
    fn find(&mut self, lo: i64, hi: i64, after: &[(&str, bool)]) -> Vec<Fold> {
        let mut ns: Vec<i64> = Vec::new();
        for n in lo..hi {
            if self.text.contains_key(&n) {
                ns.push(n);
            } else if !ns.is_empty() {
                break;
            }
        }
        let newest = ns.last().is_none_or(|l| Some(l) == self.text.keys().next_back());
        let mut rows: Vec<(&str, bool)> = ns.iter().map(|n| (self.text[n].0.as_str(), self.text[n].1)).collect();
        if newest {
            rows.extend_from_slice(after);
        }
        let mut found = Vec::new();
        for f in folds(&rows, newest) {
            if f.to >= ns.len() {
                continue; // it reaches the rows after: not yet
            }
            let (a, mut b, mut label) = (ns[f.from], ns[f.to], f.label);
            let told: Vec<i64> = self.told.iter().filter(|&(x, y)| *x <= b && a <= *y).map(|(x, _)| *x).collect();
            if let Some(&low) = told.iter().min() {
                // told already, or part of one told; nearly equal lines going on into a fold
                // told with newer lines (an older page of a long listing): the older part folds
                b = low - 1;
                let overlaps = self.told.iter().any(|(x, y)| *x <= b && a <= *y);
                if !label.ends_with("similar lines") || b - a + 1 < (SAME_HEAD + SAME_TAIL) as i64 || overlaps {
                    continue;
                }
                label = format!("⋯ {} similar lines", b - a + 1);
            }
            self.told.insert(a, b);
            found.push(Fold { n: a, to: b, label });
        }
        found
    }

    /// a..b lies in folds the page was told of, one after another (it may ask only for such
    /// lines; it shows back-to-back folds of nearly equal lines as one).
    pub fn told(&self, mut a: i64, b: i64) -> bool {
        while a <= b {
            match self.told.range(..=a).next_back() {
                Some((_, &y)) if a <= y => a = y + 1,
                _ => return false,
            }
        }
        true
    }
}
