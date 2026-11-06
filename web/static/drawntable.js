// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Tables a program drew with rules (AC-31): Claude Code's and rich's ┌─┬─┐, tabulate's grids,
// mysql's +---+. Pure: outparse.js finds them, widgetui.js shows their cells as an HTML table,
// whose cells wrap at any width where the drawn rules break apart. A line of a box that does not
// fit the grid makes it no table: it stays text. A table that lost its top or bottom rule (scrolled
// away, lost by the terminal, cut off where it was copied) is known by the rest of its grid.

// rules: a left end, runs of a horizontal line between junctions, a right end
const RULE = {
  top: ["┌┏╔╭┍┎╒╓", "┬┳╦┯┰╤╥", "┐┓╗╮┑┒╕╖"],
  mid: ["├┣╠┝┞┟┠┡┢╞╟", "┼╋╬┿╀╁╂╃╄╅╆╇╈╉╊╪╫", "┤┫╣┥┦┧┨┩┪╡╢"],
  end: ["└┗╚╰┕┖╘╙", "┴┻╩┷┸╧╨", "┘┛╝╯┙┚╛╜"],
};
const LINE = "─━═┄┅┈┉╌╍";
const rule = ([l, j, r]) => new RegExp(`^[${l}][${LINE}]+(?:[${j}][${LINE}]+)*[${r}]$`);
const BOX = { top: rule(RULE.top), mid: rule(RULE.mid), end: rule(RULE.end), bar: /[│┃║]/ };
const ASCII = /^\+[-=]+(?:\+[-=]+)*\+$/;     // mysql, prettytable, tabulate's "grid": every rule alike
// before a table's top rule: indentation, and a coding agent's message or tool mark
const LEAD = /^\s*(?:[⏺●⎿]\s*)?/;
const NUMBER = /^[-+]?(?:\d[\d,]*(?:\.\d+)?|\.\d+)\s*(?:%|[kKMGT]?i?B|[kKMGT]|ms|s)?$/;

const JUNCTION = /[┬┳╦┯┰╤╥┼╋╬┿╀╁╂╃╄╅╆╇╈╉╊╪╫┴┻╩┷┸╧╨]/g;
// the columns a rule draws
const width = (t) => (t[0] === "+" ? t.split("+").length - 2 : (t.match(JUNCTION) ?? []).length + 1);

// a box row: a bar at each end, n columns between
const boxRow = (t) => /^[│┃║].*[│┃║]$/.test(t);
const columns = (t) => t.split(BOX.bar).length - 2;
const BOX_START = new RegExp(`^[│┃║${RULE.top[0]}${RULE.mid[0]}${RULE.end[0]}]`);
/** Whether a line is a line of a box: one of its rows or rules, whole or cut off. */
export const boxLine = (text) => BOX_START.test((text ?? "").replace(LEAD, ""));
const asciiLine = (text) => { const t = (text ?? "").trim(); return ASCII.test(t) || /^\|.*\|$/.test(t); };
/** Whether line `text` goes on the table of `prev`, the line before it: a scan starting there would
 *  take a row of it for a head (a box's bottom rule ends it: a table right under it is another). */
export const insideBox = (text, prev) => (boxLine(text) && boxLine(prev) && !BOX.end.test((prev ?? "").replace(LEAD, "").trimEnd()))
  || (asciiLine(text) && asciiLine(prev));
/** Whether a line is a whole rule of a table of two columns or more: a table is being drawn, not a
 *  tree or a panel, whose lines start alike. */
export const tableRule = (text) => {
  const t = (text ?? "").replace(LEAD, "").trimEnd();
  return (BOX.top.test(t) || BOX.mid.test(t) || BOX.end.test(t) || ASCII.test(t)) && width(t) >= 2;
};
const bare = (text) => text.replace(LEAD, "").trimEnd();
const isTop = (t) => (BOX.top.test(t) || ASCII.test(t)) && width(t) >= 2;
// the columns of a row of a box or an ASCII table, 0 for any other line
const rowColumns = (t) => (boxRow(t) ? columns(t) : /^\|.*\|$/.test(t) ? t.split("|").length - 2 : 0);

/** The rule that shows the grid of a table whose top rule is not there (scrolled away, lost by the
 *  terminal, cut off where it was copied), from line i: a middle rule, or rows of one grid and the
 *  first rule under them, with as many columns. Null otherwise, also when the line before it is a
 *  line of a box (a row after a line off a table's grid is one of its rows: the table stays text,
 *  never half text, half a widget). */
function cutTop(lines, i, max) {
  const prev = lines[i - 1]?.text;
  if (boxLine(prev) || asciiLine(prev)) return null;
  const first = bare(lines[i].text);
  if (BOX.mid.test(first)) return width(first) >= 2 ? first : null;
  const n = rowColumns(first);
  if (n < 2) return null;
  const isRule = boxRow(first) ? (t) => BOX.mid.test(t) || BOX.end.test(t) : (t) => ASCII.test(t);
  for (let j = i + 1; j < lines.length && j - i < max; j++) {
    const t = lines[j].text.trim();
    if (isRule(t)) return width(t) === n ? t : null;
    if (rowColumns(t) !== n) return null;
  }
  return null;
}

/** Whether line i of `lines` starts a drawn table of two columns or more: its top rule, or (none
 *  there) what cutTop takes for its first line. */
export const tableTop = (lines, i, max) => isTop(bare(lines[i].text)) || !!cutTop(lines, i, max);

/** The table from line i of `lines` ([{text}]): {to (its last line), head (cells, or null), rows
 *  (cells), align ("right" for a column of numbers, else ""), open (it may go on: an ASCII rule with
 *  no line after it and no part above a rule, whose rows may come; a table ended by a line off its
 *  grid, which a frame may redraw)}; null when its bottom rule is not printed yet, also when a line
 *  of a box off its grid, or a blank one, has only blank lines after it (a row printed halfway, a
 *  frame caught half-drawn: the rest is on its way); undefined when it is no table. A line of a box
 *  off its grid with more after it makes it no table; a line of text, a blank one with more after
 *  it, or another table's top rule ends it, its bottom rule lost. A row's cell
 *  wrapped over lines is one cell again: between rules each part is a row, and without rules between
 *  the rows a line whose first cell is empty goes on the row above (box tables only: mysql and its
 *  kind never wrap a cell, and an empty first value is a row of its own). */
export function drawnTable(lines, i, max) {
  const first = bare(lines[i].text), topped = isTop(first), top = topped ? first : cutTop(lines, i, max);
  if (!top) return undefined;
  const ascii = top[0] === "+", n = width(top), midFirst = !topped && BOX.mid.test(first);
  const row = (t) => (ascii ? /^\|.*\|$/.test(t) : boxRow(t));
  // an ASCII table's rules look alike: the one no row follows is its last
  const isRule = (t, kind, next) => width(t) === n
    && (ascii ? ASCII.test(t) && (kind === "end") !== row(next ?? "") : BOX[kind].test(t));
  const parts = [[]];
  // with no top rule the first part may be the rest of any row: a head only as one line with all its cells
  const headed = () => parts.length > 1 && !midFirst && (topped || (parts[0].length === 1 && parts[0][0].every(Boolean)));
  for (let j = topped || midFirst ? i + 1 : i; j < lines.length && j - i < max; j++) {
    const t = (j === i ? lines[j].text.replace(LEAD, "") : lines[j].text).trim(), next = lines[j + 1]?.text.trim();
    if (isRule(t, "end", next) && parts.at(-1).length) return { ...grid(parts, j, ascii, headed()), open: ascii && next === undefined && parts.length < 2 };
    if (isRule(t, "mid", next)) { if (!parts.at(-1).length) return undefined; parts.push([]); continue; }
    const cells = ascii ? t.split("|") : t.split(BOX.bar);
    if (row(t) && cells.length === n + 2) { parts.at(-1).push(cells.slice(1, -1).map((c) => c.trim())); continue; }
    const ended = (t && !boxLine(t) && !asciiLine(t)) || isTop(bare(t)) || (!t && lines.slice(j + 1).some((l) => l.text.trim()));
    if (!ended) return lines.slice(j + 1).some((l) => l.text.trim()) ? undefined : null;
    const some = parts.filter((p) => p.length);
    return some.length ? { ...grid(some, j - 1, ascii, headed()), open: true } : undefined;
  }
  return lines.length - i < max ? null : undefined;
}

// The parts between rules as a head (`headed`: the first part) and rows.
function grid(parts, to, ascii, headed) {
  const join = (part) => part[0].map((_, c) => part.map((cells) => cells[c]).filter(Boolean).join(" "));
  const head = headed ? join(parts[0]) : null, rest = headed ? parts.slice(1) : parts;
  let rows;
  if (rest.length > 1) rows = rest.map(join);
  else {
    rows = [];
    for (const cells of rest[0] ?? []) {
      if (!ascii && rows.length && !cells[0]) rows.at(-1).forEach((v, c) => { if (cells[c]) rows.at(-1)[c] = v ? `${v} ${cells[c]}` : cells[c]; });
      else rows.push([...cells]);
    }
  }
  const align = (head ?? rows[0]).map((_, c) => {
    const vals = rows.map((r) => r[c]).filter((v) => v && !/^[-–—]$/.test(v));   // "-": no value
    return vals.length && vals.every((v) => NUMBER.test(v)) ? "right" : "";
  });
  return { to, head, rows, align };
}

/** The table as Markdown (what Copy gives: it pastes into a document, an issue or a chat). */
export function tableMarkdown({ head, rows, align }) {
  const line = (cells) => `| ${cells.map((c) => c.replace(/\\/g, "\\\\").replace(/\|/g, "\\|")).join(" | ")} |`;
  const first = head ?? rows[0].map(() => "");
  return [line(first), `| ${align.map((a) => (a ? "--:" : "---")).join(" | ")} |`, ...rows.map(line)].join("\n");
}
