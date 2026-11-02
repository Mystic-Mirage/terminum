// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A Markdown table in a diff (AC-31): Claude Code shows an edit of a Markdown file as numbered
// rows, "  7 -| Version | 0.11.1 |", "  7 +| Version | 0.12.0 |", "  8  | Date | … |"; `git diff`
// (an agent's tool output too) as "+| Version | 0.12.0 |", " | Date | … |", its mark in one column.
// Two of its table rows in a row or more are drawn as a table: removed rows struck, added ones
// marked, the header from the row above a "|---|" when the diff shows it. Pure: lines are
// outparse.js's logical lines ({text}).

const ROW = /^\s*\d+\s*([+-])?\s*(\|.*\|)\s*$/;
const UNI = /^(\s*)([+ -])(\|.*)$/;
const SEP = /^\|(\s*:?-{3,}:?\s*\|)+$/;
// a diff's own lines and an agent's next message: never the rest of a row cut at the width
const NOT_ROW = /^\s*(@@|diff --git |index |--- |\+\+\+ |[⏺●⎿…])/;
// a table's row: a second bar (two cells, or one closed): never a code line "-| A"
const CELLS = /^\|[^|]*\|/;
const ROW_LINES = 40;        // the most lines one row the width cut takes

// a row cut short (by the width, or a program's limit) has no closing "|": its last cell kept
const cellsOf = (t) => t.slice(1, t.endsWith("|") ? -1 : undefined).split(/(?<!\\)\|/).map((c) => c.trim().replace(/\\\|/g, "|"));

/** Claude Code's numbered rows from line i: [{mark, text, to}]. */
function numbered(lines, i, max) {
  const got = [];
  for (let j = i; j < lines.length && j - i < max; j++) {
    const m = ROW.exec(lines[j].text);
    if (!m) break;
    got.push({ mark: m[1] ?? "", text: m[2].trim(), to: j });
  }
  return got;
}

/** A unified diff's rows from line i, their mark in line i's column; a row without its closing
 *  "|" goes on in the next lines whose text starts in that column (the width wrapped it there; a
 *  diff's line has its mark or a space there). None when no row is added or removed: a table
 *  indented by a space is a plain one. */
function unified(lines, i, max) {
  const first = UNI.exec(lines[i].text);
  if (!first) return [];
  const col = first[1].length, got = [];
  // at column 0 (git's own output) a line is never wrapped into the next: only an agent's indented output is
  const rest = (t) => col > 0 && t.length > col && !t.slice(0, col).trim() && !/[\s+-]/.test(t[col]) && !NOT_ROW.test(t);
  for (let j = i; j < lines.length && j - i < max; j++) {
    const t = lines[j].text, m = UNI.exec(t), last = got.at(-1);
    if (m && m[1].length === col && CELLS.test(m[3])) got.push({ mark: m[2].trim(), text: m[3].trimEnd(), to: j, at: j });
    else if (last && !last.text.endsWith("|") && rest(t) && j - last.at < ROW_LINES) {
      last.text += ` ${t.trim()}`;
      last.to = j;
    } else break;
  }
  return got.some((r) => r.mark) ? got : [];
}

/** The table of diff rows from line i: {to, head, rows, align, marks ("+", "-" or "" a row)}, or
 *  undefined when fewer than two of its rows are a table's. */
export function diffTable(lines, i, max = 2000) {
  const t = lines[i]?.text ?? "";
  if (!t.includes("|")) return undefined;                      // every row of either kind has a bar: one look for most lines
  const got = ROW.test(t) ? numbered(lines, i, max) : unified(lines, i, max);
  const sep = got.findIndex((r) => SEP.test(r.text));
  const body = got.filter((r, k) => k !== sep && !(sep > 0 && k === sep - 1));
  if (body.length < 2 && !(sep > 0 && body.length)) return undefined;
  const head = sep > 0 ? cellsOf(got[sep - 1].text) : null;
  const rows = body.map((r) => cellsOf(r.text)), n = Math.max(head?.length ?? 0, ...rows.map((r) => r.length));
  const pad = (r) => [...r, ...Array(n - r.length).fill("")];
  return { to: got.at(-1).to, head: head && pad(head), rows: rows.map(pad), align: Array(n).fill(false), marks: body.map((r) => r.mark) };
}
