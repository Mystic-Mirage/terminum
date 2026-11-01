// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// How the terminal view groups its rows: paragraphs into blocks, and a coding agent's input box
// with its footer into panels. Its methods are added to TermView's prototype (term.js).
import { codexBox, codexStatus, inputBox, parkedCaret, statusKey } from "./agentbox.js";
import { listOpen } from "./boxnav.js";
import { cursorCol, typedEnd, withCursor } from "./cursor.js";

export const MAX_BLOCK = 400;       // most lines in a paragraph's block: a longer one is cut (one huge soft-wrapped line)

/** Where blocks of lines end: after the first paragraph end (ends[i]) at least `min` lines
 *  into each block, or after `max` lines without one. Returns the end indexes (exclusive);
 *  lines after the last one are left. */
export function blockCuts(ends, min, max = Infinity) {
  const cuts = [];
  let start = 0;
  ends.forEach((end, i) => { if ((end && i + 1 - start >= min) || i + 1 - start >= max) cuts.push(start = i + 1); });
  return cuts;
}

const endsParagraph = (r) => r.el.classList.contains("eol") && !r.el.classList.contains("join");
// nothing drawn: no text, no background, no inverse cell
const blankRow = (r) => !r.txt.trim() && !r.data.r.some(([, , bg, flags]) => bg != null || flags & 32);
// a row's background: the one all its text shares, else null (Codex's input stands on one)
const rowBg = (data) => { const bgs = new Set((data?.r ?? []).filter((x) => (Array.isArray(x[0]) ? x[0].join("") : x[0]).length).map((x) => x[2] ?? null)); return bgs.size === 1 ? [...bgs][0] : null; };
// a row as agentbox.js knows it (the bridge's data: a row the page drew the cursor on keeps it in .orig)
const boxRow = (r) => ({ rule: r.el.classList.contains("rule"), blank: !r.txt.trim(), txt: r.txt, end: typedEnd(r.orig ?? r.data), bg: rowBg(r.orig ?? r.data) });

export class Blocks {
  // Each paragraph (lines up to a hard line end not re-joined) is a block of its own: iOS
  // Safari's selection and Copy menu stay within it and widgets find their regions. The
  // paragraph the history's newest lines are in stays loose: it flows on into the screen, and
  // its last line's end can still change with the screen's first line.
  wrapInBlocks(rows) {
    let start = 0;
    for (const end of blockCuts(rows.map(endsParagraph), 1, MAX_BLOCK)) {
      this.block(rows.slice(start, end));
      start = end;
    }
    return rows.slice(start);
  }
  block(rows) {
    const b = document.createElement("div");
    b.className = "blk";
    rows[0].el.before(b);
    b.append(...rows.map((r) => r.el));
  }
  pack() {
    let loose = 0;
    while (loose < this.hist.length && this.hist.at(-1 - loose).el.parentNode === this.histEl) loose++;
    if (loose > 1) this.wrapInBlocks(this.hist.slice(-loose, -1));
  }
  // The screen's paragraphs in blocks too, but its first: that one goes on from the history's.
  // In a coding agent's pane its input box, with its rules, is one block ("inbox") and the
  // status lines below it another ("foot"): on a phone the page draws the box as a panel and
  // keeps the footer behind a button. The blocks are rebuilt only when they change.
  groupScreen() {
    const rows = this.screen.filter(Boolean);                     // a row not received yet is skipped
    this.park(rows);
    // The screen's empty rows below its last text and the cursor are not shown: after a clear a
    // prompt stands at the top and the view, kept at its end, showed only them.
    let used = rows.findIndex((r) => r.el.querySelector(".cur"));
    for (let i = rows.length - 1; i > used; i--) if (!blankRow(rows[i])) { used = i; break; }
    rows.forEach((r, i) => r.el.classList.toggle("spare", i > used));
    const at = this.agent ? this.findBox(rows) : null;
    this.term.classList.toggle("hasbox", !!at);
    const groups = [{ kind: "", rows: [] }];
    rows.forEach((r, i) => {
      const kind = !at || i < at.top ? "" : i <= at.bottom ? "inbox" : i <= at.end ? "foot" : "";
      let g = groups.at(-1);
      if (g.rows.length && g.kind !== kind) groups.push(g = { kind, rows: [] });
      else if (!g.rows.length && groups.length > 1) g.kind = kind;
      else if (!g.rows.length && kind) groups.push(g = { kind, rows: [] });   // the first group stays loose
      g.rows.push(r);
      if (!kind && endsParagraph(r)) groups.push({ kind: "", rows: [] });
    });
    if (groups.length > 1 && !groups.at(-1).rows.length) groups.pop();
    const cls = (g) => (g.kind ? `blk ${g.kind}` : "blk");
    const box = this.screenEl, kept = groups.every((g, k) => {
      const p = g.rows[0]?.el.parentNode;
      return k === 0 ? g.rows.every((r) => r.el.parentNode === box)
        : p !== box && p?.parentNode === box && p.className === cls(g) && p.childElementCount === g.rows.length && g.rows.every((r) => r.el.parentNode === p);
    });
    if (kept && box.childElementCount === groups[0].rows.length + groups.length - 1) return;
    const all = new DocumentFragment();
    all.append(...groups[0].rows.map((r) => r.el));
    for (const g of groups.slice(1)) {
      const b = document.createElement("div");
      b.className = cls(g);
      b.append(...g.rows.map((r) => r.el));
      all.append(b);
    }
    box.replaceChildren(all);
  }
  // The agent's input box. A cursor gone for a frame (a redraw) keeps the box while its rules
  // stay. The status lines shown below it while nothing is typed are learnt: only those wait
  // behind the button, so a "/" or "@" list or a new notice there stays in sight.
  /** Rows the agent's input box and its footer take on the screen, 0 without one. */
  get boxHeight() { return this.box ? this.box.end - this.box.top + 1 : 0; }

  findBox(rows) {
    const cursor = rows.findIndex((r) => r.el.querySelector(".cur"));
    const brs = rows.map(boxRow);
    let at = inputBox(brs, cursor, (k) => this.histRow(k)) ?? codexBox(brs, cursor);
    const was = this.box, rule = (i) => (i < 0 ? this.hist.at(i) : rows[i])?.el.classList.contains("rule");
    if (!at && cursor < 0 && was && was.end < rows.length && rule(was.top) && rule(was.bottom)) at = was;
    this.box = at;
    rows.forEach((r, i) => r.el.classList.toggle("boxedge", !!at?.codex && (i === at.top || i === at.bottom)));   // drawn as the rules it has not
    this.relinkBox(at ? rows.slice(Math.max(0, at.top + 1), at.bottom) : []);
    if (!at) return null;
    if (cursor > at.top && cursor < at.bottom) this.caret = { top: at.top, bottom: at.bottom, row: cursor, col: cursorCol(rows[cursor].data), txt: rows[cursor].txt };
    const below = rows.slice(at.bottom + 1, at.end + 1);
    // learnt while only the prompt is there (a faint suggestion is not typed): every status line seen so, idle or working, the latest 40
    if (cursor >= 0 && typedEnd(rows[cursor].data) <= 2) this.idle = new Set([...(this.idle ?? []), ...below.map((r) => statusKey(r.txt))].slice(-40));
    // the footer waits behind ⓘ but for a "/" or "@" list the input opened (those rows are never learnt)
    const typed = cursor > at.top && cursor < at.bottom ? [...rows[cursor].txt].slice(2, Math.max(2, cursorCol(rows[cursor].data))).join("") : "";
    const list = listOpen(typed, cursor === at.top + 1);
    for (const r of below) r.el.classList.toggle("status", !list || !!this.idle?.has(statusKey(r.txt)) || (!!at.codex && codexStatus(r.txt)));
    return at;
  }
  /** The history's k-th row from its end as inputBox's (the top of an input taller than the screen). */
  histRow(k) { const r = this.hist.at(-k); return r && boxRow(r); }
  // The cursor Claude Code left below its footer (agentbox.js parkedCaret) is drawn on its caret,
  // so the box, a tap and ↑ ↓ go on from there; rows drawn so go back to the bridge's data first.
  park(rows) {
    const data = (r) => r.orig ?? r.data;
    const cursor = rows.findIndex((r) => cursorCol(data(r)) >= 0);
    const at = this.agent ? parkedCaret(rows.map(boxRow), cursor, this.caret, (k) => this.histRow(k), (txt) => !!this.idle?.has(statusKey(txt))) : null;
    const want = new Map(at ? [[cursor, null], [at.row, at.col]] : []);
    const base = this.hist.length;
    rows.forEach((r, i) => {
      const col = want.get(i);
      if (col === undefined ? !r.orig : r.orig && r.parked === col) return;
      const k = this.screen.indexOf(r), was = data(r);
      const n = rows[i] = this.screen[k] = this.swap(r, col === undefined ? was : withCursor(was, col));
      if (col !== undefined) Object.assign(n, { orig: was, parked: col });
      this.link(base + k - 1); this.link(base + k);
    });
  }
  // The rows typed in the box re-join by the input's own rule (inputWraps); rows that left it
  // by the general one.
  relinkBox(rows) {
    const old = this.boxed ?? new Set();
    this.boxed = new Set(rows.map((r) => r.el));
    if (old.size === this.boxed.size && [...old].every((el) => this.boxed.has(el))) return;
    const base = this.hist.length;
    for (const r of new Set([...rows, ...this.screen.filter((x) => x && old.has(x.el))])) {
      const i = this.screen.indexOf(r);
      if (i >= 0) { this.link(base + i - 1); this.link(base + i); }
    }
  }
  /** The shown pane runs a coding agent ("claude", "codex"; null: none): its input box is drawn as a panel. */
  setAgent(on) {
    if (this.agent === on) return;
    this.agent = on;
    this.box = this.idle = this.caret = null;
    this.onAlt?.();                              // a coding agent's own full screen is no full-screen program (sizing.js)
    this.keepBottom(() => { this.relinkBox([]); this.groupScreen(); });
  }
}

/** Adds the Blocks methods to a class's prototype. */
export function addBlocks(cls) {
  for (const [k, d] of Object.entries(Object.getOwnPropertyDescriptors(Blocks.prototype))) if (k !== "constructor") Object.defineProperty(cls.prototype, k, d);
}
