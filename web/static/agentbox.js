// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A coding agent's input box and footer on its screen (AC-52): Claude Code draws its input
// between two rules around the cursor (the top one may carry the session's title) and a footer
// of status lines below (its "/" and "@" lists open there too). On a phone the page draws the
// box as a panel and keeps the status lines it showed while idle behind a button (term.js,
// style.css). A menu the agent draws between rules too (its questions, a permission) is not an
// input: the box's first row is the prompt. Pure: rows are {rule, blank, txt}, as term.js knows them.

const FOOTER = 12;       // the most rows below the box: status lines, or a list it opened
// the input's first row: "❯" (older Claude Code: ">"), or "!" for a shell command, then a space
// (Claude Code: a no-break one) or nothing (trimmed); a menu's chosen item ("❯ 1. Yes") has a
// plain space and its number
const PROMPT = /^[❯>!](\s|$)/, ITEM = /^[❯>] +\d+\./;
const NEXT = /^( {2}|$)/;   // the input's next rows: under the prompt's two columns, or blank
const CODEX = /^›(\s|$)/;    // Codex's prompt

/** {top, bottom, end}: the box's rules are rows top and bottom, its footer bottom+1..end
 *  (end < bottom+1: none); null when the cursor is not in such a box near the screen's end.
 *  above(k): the history's k-th row from its end ({rule, blank, txt}, undefined past it), asked
 *  for when the screen has no rule above the cursor: an input taller than the screen has its top
 *  rule and prompt up there (measured), and top counts back from the screen's first row (-1: the
 *  history's last row). */
export function inputBox(rows, cursor, above = () => undefined) {
  if (cursor == null || cursor < 0 || cursor >= rows.length || rows[cursor].rule) return null;
  let top = cursor - 1, bottom = cursor + 1;
  // the rules nearest the cursor, however far: a long input grows the box (the prompt below its
  // top rule and a footer to the screen's end below its bottom one tell it from output)
  while (top >= 0 && !rows[top].rule) top--;
  while (bottom < rows.length && !rows[bottom].rule) bottom++;
  if (bottom >= rows.length) return null;
  let end = rows.length - 1;
  while (end > bottom && rows[end].blank) end--;
  if (end - bottom > FOOTER) return null;                 // more below than a footer: not an input box
  for (let i = bottom + 1; i <= end; i++) if (rows[i].rule) return null;
  const tall = top < 0 ? tallTop(rows, bottom, above) : null;
  if (top < 0 && !tall) return null;
  const first = tall ? tall.txt : rows[top + 1].txt;
  if (!PROMPT.test(first) || ITEM.test(first)) return null;     // a menu: its ↑ and ↓ are its own
  return { top: tall ? tall.top : top, bottom, end };
}

// An input taller than the screen: {top, txt} of its rule and prompt rows, or null. Every row of it on the screen and in the history below the prompt is the input's next
// rows (indented, or blank), so a prompt and rule left far up in the history by an input sent
// long ago are not taken (the output since is not indented so).
function tallTop(rows, bottom, above) {
  for (let i = 1; i < bottom; i++) if (!NEXT.test(rows[i].txt)) return null;
  let k = 0, row = rows[0];
  while (NEXT.test(row.txt)) {
    const up = above(++k);
    if (!up || up.rule) return null;
    row = up;
  }
  if (!above(k + 1)?.rule) return null;
  return { top: -(k + 1), txt: row.txt };
}

/** The caret last seen ({top, bottom, row, col, txt}) in the box `at`, its row's text unchanged; else null. */
const seenCaret = (rows, seen, at) => (seen && at && seen.top === at.top && seen.bottom === at.bottom && seen.row > at.top && seen.row < at.bottom && rows[seen.row].txt === seen.txt
  ? { row: seen.row, col: Math.max(2, seen.col) } : null);

/** Where the agent's caret is while the cursor stands below its box: Claude Code puts the
 *  terminal's cursor on its caret after a key, but a redraw of its own (a status line's refresh,
 *  a notice) leaves it on the row after its footer (measured in iTerm2), and one after the pane
 *  was resized (the page's hot keys opened) on one of its status lines, where no caret can be
 *  (status(txt): a status line learnt while idle; a shell's prompt after the agent quit never is
 *  one). {row, col}: the
 *  caret last seen in this box ({top, bottom, row, col, txt}: the same rules, its row's text
 *  unchanged), else the end of the input's last row (rows' `end`: the column after its typed
 *  text); null when the cursor is elsewhere. A frame caught between the two writes of a redraw
 *  (AC-02: the cursor on a row above the box, the spinner's) has the caret last seen in the box
 *  too, and no other: a cursor above it stays where it is. above: as inputBox's. */
export function parkedCaret(rows, cursor, seen, above, status = () => false) {
  if (cursor == null || cursor < 0 || cursor >= rows.length) return null;
  if (seen && cursor < seen.top && rows[seen.top]?.rule && rows[seen.bottom]?.rule) {
    const caret = seenCaret(rows, seen, inputBox(rows, seen.bottom - 1));
    if (caret) return caret;
  }
  if (!(rows[cursor].blank || status(rows[cursor].txt))) return null;
  const bottom = rows.findLastIndex((r, i) => i < cursor && r.rule);
  if (bottom <= 0) return null;
  const at = inputBox(rows, bottom - 1, above);
  if (!at || at.bottom !== bottom || (rows[cursor].blank && cursor <= at.end)) return null;   // a blank row after the footer, or a status line in it
  return seenCaret(rows, seen, at) ?? { row: bottom - 1, col: Math.max(2, rows[bottom - 1].end ?? 0) };
}

/** Codex's input (measured: codex 0.16x in tmux, its alternate screen): no rules, its prompt row
 *  "› " on a background of its own (the rows above and below it, of that background too, come
 *  blank: tmux drops a row's trailing blanks), its footer below. {top, bottom, end, codex} as
 *  inputBox's, top and bottom those blank rows; null when the cursor is not in it. rows: as
 *  inputBox's, with bg (the background all of a row's text shares, null for none or the default). */
export function codexBox(rows, cursor) {
  const bg = rows[cursor]?.bg;
  if (bg == null) return null;
  let first = cursor, last = cursor;
  while (first > 0 && rows[first - 1].bg === bg) first--;              // a long input: its rows, of the same background
  while (last + 1 < rows.length && rows[last + 1].bg === bg) last++;
  const top = first - 1, bottom = last + 1;
  if (!CODEX.test(rows[first].txt) || top < 0 || bottom >= rows.length || !rows[top].blank || !rows[bottom].blank) return null;
  let end = rows.length - 1;
  while (end > bottom && rows[end].blank) end--;
  return end - bottom > FOOTER ? null : { top, bottom, end, codex: true };
}
/** A line of Codex's footer that only says how things stand ("model · folder · branch · Ready"):
 *  its words change while it works, so it is known by its form. */
export const codexStatus = (txt) => / · /.test(txt);

/** A status line as it stays while it only counts (context, limits, shells) or says how things
 *  stand after its stem ("bypass permissions on (shift+tab to cycle)" idle, "… · 1 shell" while
 *  working): digits left out, and all after its first " · " or " (". */
export const statusKey = (txt) => txt.trim().replace(/\d+/g, "#").split(/ · | \(/)[0];
