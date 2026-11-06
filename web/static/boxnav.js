// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Moving in a coding agent's input box by the page's own lines (AC-52). The page re-flows the box
// to the browser's width, while ↑ and ↓ sent as they are move by iTerm's rows at iTerm's column,
// somewhere else on the line the user sees. So ↑, ↓, a click and a tap find the character drawn
// there and move to it with ← and →, as many as the characters between: Claude Code steps over
// its text a character (a grapheme: an emoji is one) at a time, one more at a row's wrap at a
// space or a line the user ended, none inside a word it broke at the row's edge (measured in
// iTerm2). The page's lines are read from the characters' boxes, so a blank line or one out of
// sight counts as any other. Backspace or Delete over a selection in the box deletes it (AC-40).

const PREFIX = 2;                 // the prompt ("❯ ", "! ") or the indentation of the next rows
const FAINT = 2;
const ARRIVE = 1500;              // ms a move may take to show; meanwhile the next starts from its target
const RULE_ONLY = /^[─━\s\ufe0e]*$/;   // the box's top rule, as a selection reads it (its lines take a text form)
const seg = globalThis.Intl?.Segmenter ? new Intl.Segmenter() : null;
/** A text's characters as the agent steps over them: graphemes ("❯" with its added text form is one). */
export const graphemes = (s) => (seg ? [...seg.segment(s)].map((g) => g.segment) : [...s]);

/** Characters of a row's data up to its last one typed (not a faint suggestion, not past the
 *  text), counted as the agent steps (a wide character's right half is none). */
export function typedLength(data) {
  let n = 0, end = 0;
  for (const [text, , , flags] of data?.r ?? []) {
    for (const c of Array.isArray(text) ? text : graphemes(text)) {
      if (!c) continue;
      n++;
      if (c.trim() && !(flags & FAINT)) end = n;
    }
  }
  return end;
}

/** Where each row's text starts in the input: [{start, len}] for rows [{len, glue}] (len: its
 *  characters, prompt included; glue: a word goes on in the next row without a space). */
export function layout(rows) {
  let at = 0;
  return rows.map((r) => {
    const row = { start: at, len: Math.max(0, r.len - PREFIX) };
    at += row.len + (r.glue ? 0 : 1);
    return row;
  });
}

/** The keys for `steps` characters right (> 0) or left. */
export const stepKeys = (steps, appCursor) => ((appCursor ? "\x1bO" : "\x1b[") + (steps > 0 ? "C" : "D")).repeat(Math.abs(steps));

/** Whether a list of the agent's is open (measured: "@" lists files and agents below the box):
 *  the word before the cursor starts with "@", or the input is a "/" command so far. Its ↑ and
 *  ↓ are the list's. */
export const listOpen = (before, first) => /(^|\s)@\S*$/.test(before) || (first && /^\/\S*$/.test(before));

/** The page's lines of positions [{o, x, top, h}]: each a list, top to bottom. */
export function lines(positions) {
  const out = [];
  for (const p of [...positions].sort((a, b) => a.top - b.top || a.x - b.x)) {
    const l = out.at(-1);
    if (l && Math.abs(l[0].top - p.top) < Math.max(4, p.h / 2)) l.push(p); else out.push([p]);
  }
  return out;
}

/** The position on `line` nearest x (the earlier of two as near). */
export const nearest = (line, x) => line.reduce((a, b) => (Math.abs(b.x - x) < Math.abs(a.x - x) ? b : a));

export class BoxMoves {
  /** view: the terminal (term.js); screen: its screen element. */
  constructor(view, screen) { Object.assign(this, { view, screen }); this.last = null; }

  /** The box's rows, their layout, the cursor's row and offset, the text before it on its row,
   *  the end of the text, whether its first row is the input's (whole); null when the cursor is
   *  not in an agent's input box. */
  state() {
    const v = this.view, rows = v.screen.filter(Boolean);
    if (!v.agent || !v.box) return null;
    const recs = rows.slice(Math.max(0, v.box.top + 1), v.box.bottom);   // the screen's part of a taller input
    const cur = this.screen.querySelector(".cur"), el = cur?.closest(".ln");
    const row = recs.findIndex((r) => r.el === el);
    if (row < 0) return null;
    // the cursor's own column, not clamped to the text: spaces typed at a row's end are not sent
    let col = 0, text = "";
    const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let t = walk.nextNode(); t && !cur.contains(t); t = walk.nextNode()) { col += graphemes(t.data).length; text += t.data; }
    // all before the cursor is typed: spaces at its row's end count, or the next row starts a character early
    const lay = layout(recs.map((r, i) => ({ len: i === row ? Math.max(typedLength(r.data), col) : typedLength(r.data), glue: r.el.classList.contains("glue") })));
    const before = graphemes(text).slice(PREFIX).join("");
    return { recs, lay, cur, row, before, whole: v.box.top >= 0, end: lay.at(-1).start + lay.at(-1).len, at: lay[row].start + Math.max(0, col - PREFIX) };
  }

  /** Every character of the box's text where the page draws it: [{o, x, top, h}], also those out
   *  of sight; a row without one (a blank line) by its own box, the cursor where it stands. */
  positions(s) {
    const out = [];
    s.recs.forEach((r, i) => {
      const { start, len } = s.lay[i];
      const walk = document.createTreeWalker(r.el, NodeFilter.SHOW_TEXT);
      let col = 0, last = null;
      for (let t = walk.nextNode(); t; t = walk.nextNode()) {
        let at = 0;
        for (const g of graphemes(t.data)) {
          const o = start + col - PREFIX;
          if (col >= PREFIX && o < start + len) {
            const range = document.createRange();
            range.setStart(t, at); range.setEnd(t, at + g.length);
            const b = range.getClientRects()[0];
            if (b?.width) { out.push({ o, x: b.left, top: b.top, h: b.height }); last = b; }
          }
          col++; at += g.length;
        }
      }
      if (last) out.push({ o: start + len, x: last.right, top: last.top, h: last.height });   // after its text
      else {
        const b = (r.el.contains(s.cur) ? s.cur : r.el).getClientRects()[0];
        if (b) out.push({ o: start, x: b.left, top: b.top, h: b.height });
      }
    });
    const c = s.cur.getBoundingClientRect();
    out.push({ o: s.at, x: c.left, top: c.top, h: c.height, cur: true });
    return out;
  }

  /** Where a move starts: the cursor, or the target of moves not shown yet (the cursor still
   *  within where they started and went, a moment at most: a finger's slide sends many). */
  base(s) {
    const l = this.last;
    if (l && (s.at === l.to || performance.now() - l.t > ARRIVE || s.at < l.lo || s.at > l.hi)) this.last = null;
    return this.last ? this.last.to : s.at;
  }

  /** Another key went to the pane: the cursor goes on from where it is (and ↑ ↓ from its column). */
  forget() { this.last = this.goal = null; }

  move(from, to, appCursor) {
    if (to !== from) {
      const on = this.last && from === this.last.to ? this.last : null;   // on from a move not shown yet
      this.last = { lo: Math.min(on?.lo ?? from, from, to), hi: Math.max(on?.hi ?? from, from, to), to, t: performance.now() };
    }
    return stepKeys(to - from, appCursor);
  }

  /** ↑ (d = -1) or ↓ (1) by the page's lines: the keys to send ("" for none), or null for the key
   *  as it is: no box, a list open, or the input's first (last) line on the page and its first
   *  (last) row in iTerm, where the agent recalls its history; a repeat (a held key) stops there. */
  line(d, appCursor, repeat = false) {
    const s = this.state();
    if (!s || listOpen(s.before, s.row === 0 && s.whole)) return null;
    const from = this.base(s), ps = this.positions(s);
    const start = (from === s.at && ps.find((p) => p.cur)) || ps.find((p) => p.o === from) || ps.at(-1);
    const ls = lines(ps), k = ls.findIndex((l) => l.includes(start)), next = ls[k + d];
    // ↑ and ↓ in a row keep the column they started from, past a shorter or blank line
    if (this.goal?.at !== from) this.goal = { x: start.x };
    if (next) {
      const to = nearest(next, this.goal.x).o;
      this.goal.at = to;
      return this.move(from, to, appCursor);
    }
    if (repeat) return "";
    if (from === s.at && s.row === (d < 0 ? 0 : s.recs.length - 1)) return null;
    return this.move(from, d < 0 ? 0 : s.end, appCursor);       // as a text field: to the start or the end
  }

  /** The character drawn under viewport point (x, y) in the box (after the line's text right of
   *  it), or null: the cursor goes onto the character clicked, as a terminal's block cursor. A
   *  finger a little off its line (the box's padding) still means the nearest, a line away at most. */
  at(s, x, y) {
    const off = (ln) => (y < ln[0].top ? ln[0].top - y : y >= ln[0].top + ln[0].h ? y - ln[0].top - ln[0].h + 1 : 0);
    const l = lines(this.positions(s)).filter((ln) => off(ln) < ln[0].h).sort((a, b) => off(a) - off(b))[0];
    if (!l) return null;
    let p = l[0];
    for (const q of l) if (q.x <= x + 0.5) p = q;           // sorted by x; the cursor past a typed space too
    return p;
  }

  /** Whether (x, y) is on the input's text (a tap there moves the cursor, not a link). */
  has(x, y) { const s = this.state(); return !!s && !!this.at(s, x, y); }

  /** A click or a tap at (x, y): the keys that move the cursor there, null when it is not in the
   *  box (or the cursor is not). Never past the text typed (→ takes a suggestion). */
  to(x, y, appCursor) {
    const s = this.state(), p = s && this.at(s, x, y);
    this.goal = null;
    return p ? this.move(this.base(s), Math.min(p.o, Math.max(s.end, s.at)), appCursor) : null;
  }

  /** The offset in the input of a selection's end (node, offset), or null when it is not on a
   *  row of the box: the characters before it on its row, after the prompt, at most its text. */
  offsetAt(s, node, offset) {
    const i = s.recs.findIndex((r) => r.el.contains(node));
    if (i < 0) return null;
    const r = document.createRange();
    r.setStart(s.recs[i].el, 0);
    r.setEnd(node, offset);
    const col = graphemes(r.toString().replace(/︎/g, "")).length;
    return s.lay[i].start + Math.min(s.lay[i].len, Math.max(0, col - PREFIX));
  }

  /** The input offsets [a, b] of `range` over the input's text, else null. A hand rarely stops on the
   *  text's edge: an end past its last row (the rule, the footer, a triple click's start of the next
   *  row) is the text's end, a start on the rule above it the text's start. Output above it is not
   *  the input's: none. */
  ends(s, range) {
    if (!range || range.collapsed) return null;
    const row = document.createRange(), past = Math.max(s.end, s.at);
    row.selectNode(s.recs.at(-1).el);
    const below = range.compareBoundaryPoints(Range.END_TO_END, row) > 0;
    row.selectNode(s.recs[0].el);
    const above = range.compareBoundaryPoints(Range.START_TO_START, row) < 0;
    if (above) { row.setEnd(row.startContainer, row.startOffset); row.setStart(range.startContainer, range.startOffset); }
    if (above && !RULE_ONLY.test(row.toString())) return null;
    const a = above ? 0 : this.offsetAt(s, range.startContainer, range.startOffset);
    const b = below ? past : this.offsetAt(s, range.endContainer, range.endOffset);
    return a === null || b === null || b <= a || b > past ? null : [a, b];
  }

  /** Whether cut(range) has keys for it (nothing moves). */
  cuttable(range) { const s = this.state(); return !!s && !!this.ends(s, range); }

  /** Backspace or Delete over `range`, a selection: the keys that delete exactly its characters
   *  (→ or ← to its end, then a backspace each, AC-40), or null when it reaches the output or
   *  is not on the input's text (`ends`). */
  cut(range, appCursor) {
    const s = this.state(), ends = s && this.ends(s, range);
    if (!ends) return null;
    const [a, b] = ends;
    const from = this.base(s);
    this.goal = null;
    this.last = { lo: Math.min(from, a), hi: Math.max(from, b), to: a, t: performance.now() };   // the cursor ends where the selection began
    return stepKeys(b - from, appCursor) + "\x7f".repeat(b - a);
  }
}
