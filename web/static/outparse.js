// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What in a terminal's output becomes a widget (AC-56): a file printed by a command (cat, bat…),
// fenced code, a Markdown document, a Mermaid diagram and a drawn table. Pure: rows
// are {text, eol, styled}, as term.js knows them; widgets.js draws what this finds. A wrong
// widget hides real output and a missed one costs nothing, so the rules ask for clear signs.
import { drawnTable, insideBox, tableMarkdown, tableTop } from "./drawntable.js";
import { diffTable } from "./difftable.js";

export const MAX_LINES = 2000;      // a longer region stays plain text (rendering it would cost more than it gives)
const GAP = 6;               // the most lines of plain prose between two Markdown signs of one document

const FENCE = /^(\s*)(`{3,}|~{3,})\s*([\w#+.-]*)[^`]*$/;
const HEADING = /^#{1,6}\s+(?!-\*-)\S/;          // not "# -*- coding: utf-8 -*-"
export const TABLE_SEP = /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?\s*$/;
const LIST = /^\s*(?:[-*+]|\d{1,3}[.)])\s+\S/;
const QUOTE = /^\s*>\s?\S/;
const INLINE = /\*\*[^*\s][^*]*\*\*|(?:^|\s)`[^`\s][^`]*`|\[[^\]]+\]\([^)\s]+\)/;
// lines of a program or a configuration rather than prose: they make a run not Markdown
const CODEISH = new RegExp([
  String.raw`[;{}\\]\s*$`, String.raw`^\s*(?:def|class|import|from|function|const|let|var|return|fi|done|esac|then|export|local)\b`,
  String.raw`^\s*[a-z_][\w.-]*:(?:\s+\S|$)`, String.raw`^\s*[\w.$\[\]]+\s*[-+*/]?=\s`, String.raw`\$\{?\w|\$\(|&&|\|\|`,
].join("|"));
// a shell prompt, by its common shapes: "alex@devbox ~ %", "$ ", "❯ ", "➜ dir", "(venv) …$", and
// a prompt mark after a short lead (Starship's "at 12:21 ❯ ", "[main] ❯ ")
const PROMPT = /^(?:\S+@[\w.-]+[:\s].*?[$#%>][\uFE0E\uFE0F]?(?:\s|$)|[$%❯➜λ»›][\uFE0E\uFE0F]?(?:\s|$)|\(\S+\)\s.*?[$%#❯][\uFE0E\uFE0F]?(?:\s|$)|[^❯➜›]{1,40}[❯➜›][\uFE0E\uFE0F]?(?:\s|$))/;   // a variation selector may follow a mark (Starship's ❯︎)
// the end of the output: a prompt, a line a program colored (a prompt's other lines usually are),
// a coding agent's next message or tool result (⏺ or ●, ⎿), or a drawn table (a widget of its own)
const ends = (lines, k) => lines[k].styled || PROMPT.test(lines[k].text) || /^\s*[⏺●⎿]/.test(lines[k].text) || tableTop(lines, k, MAX_LINES);
// a coding agent's tool output begins after this mark on its first line ("  ⎿  # Title")
const TOOL_OUT = /^\s*⎿/;
// prompts compared without their numbers: a clock, a counter or an exit code changes
const shape = (t) => t.replace(/\d/g, "0");
// a command that prints one file: the output after it is that file, up to the next prompt
const PRINT = /(?:^|\s)(?:cat|bat|batcat|head|tail|less|more|nl)\s+(?:-\S+\s+)*([^\s|;&<>'"`]+\.([A-Za-z0-9]+))\s*$/;
// a diagram without a fence starts with a line that is only its keyword ("pie", "timeline" are prose too)
const DIAGRAM = /^(?:(?:graph|flowchart)\s+(?:TD|TB|BT|RL|LR)|sequenceDiagram|classDiagram(?:-v2)?|stateDiagram(?:-v2)?|erDiagram|gitGraph)\s*$/;
const MARKDOWN_EXT = /^(md|markdown|mdown|mkd)$/i, MERMAID_EXT = /^(mmd|mermaid)$/i;
/** A fence's word for a Mermaid diagram. */
export const MERMAID_LANG = /^(mermaid|mermaidjs|mmd)$/i;

/** Rows joined where iTerm wrapped them (no hard end): {text, from, to, styled} with row
 *  indexes; styled: a row of it has colors of its own. */
export function logicalLines(rows) {
  const out = [];
  let cur = null;
  rows.forEach((r, i) => {
    if (!cur) cur = { text: "", from: i, to: i, styled: false };
    cur.text += r.text;
    cur.to = i;
    cur.styled ||= !!r.styled;
    if (r.eol) { out.push(cur); cur = null; }
  });
  if (cur) out.push(cur);
  return out;
}

const BARS = /(?<!\\)\|/g;
const bars = (t) => (t.match(BARS) ?? []).length;
const SEP_START = /^\|\s*:?-{3}/, SEP_PART = /^[-:|]+$/;
const ROW_LINES = 40;        // the most lines one row of a pipe table a program wrapped takes

/** Lines i..j of `lines` as one (line i itself when j is i): their texts joined by `by`, blank ones dropped. */
function merged(lines, i, j, by = " ") {
  if (j === i) return lines[i];
  const part = lines.slice(i, j + 1);
  return { ...lines[i], to: lines[j].to, styled: part.some((l) => l.styled),
    text: part.map((l, k) => (k ? l.text.trim() : l.text.trimEnd())).filter(Boolean).join(by) };
}
// `at`: the lines read once by joinTableRows: {lines, trim (their texts trimmed), bars (their bars),
// next (the first separator line from each)}
/** The last line of a row from line i whose bars, joined, are `need`: -1 when it has more, or
 *  meets a blank line and `blanks` is off. */
function rowEnd(at, i, need, blanks = true) {
  for (let j = i, n = 0; j < at.lines.length && j - i < ROW_LINES; j++) {
    if (!blanks && !at.trim[j]) return -1;
    n += at.bars[j];
    if (n >= need) return n === need ? j : -1;
  }
  return -1;
}
/** The separator from line k with the pieces it was wrapped into (dashes and bars alone): {to, text}. */
function separator(at, k) {
  let j = k;
  while (j + 1 < at.lines.length && j - k < ROW_LINES && SEP_PART.test(at.trim[j + 1])) j++;
  const text = at.trim.slice(k, j + 1).join("");
  return TABLE_SEP.test(text) ? { to: j, text } : null;
}
/** A pipe table's head and separator from line i, whatever the width wrapped them at: {head, sep
 *  (the lines they end on), need (the bars of a row), wrapped (either took more than a line)}, or
 *  null: its separator within ROW_LINES lines and the head's bars as many as its. */
function tableStart(at, i) {
  const k = at.next[i + 1];
  if (k < 0 || k - i > ROW_LINES) return null;
  const sep = separator(at, k), need = sep && bars(sep.text);
  return sep && rowEnd(at, i, need) === k - 1 ? { head: k - 1, sep: sep.to, need, wrapped: k - 1 > i || sep.to > k } : null;
}

/** Lines with table rows a program wrapped at its width put back together (AC-31). In a table
 *  whose head and separator are found, a row goes on until it has as many bars as its separator,
 *  over blank lines too when the head or the separator was wrapped (Claude Code at a phone's width
 *  wraps a padded row at a bar in its middle and into blank lines); a row with more bars is left as
 *  it is and the next rows join again; a separator's pieces join without a space. Elsewhere a line that starts
 *  with "|" but does not end with one goes on in the next lines (their indentation dropped) until
 *  one ends with "|", four lines at most. */
export function joinTableRows(lines) {
  const trim = lines.map((l) => l.text.trim()), next = Array(lines.length + 1).fill(-1);
  for (let k = lines.length - 1; k >= 0; k--) next[k] = SEP_START.test(trim[k]) ? k : next[k + 1];
  const at = { lines, trim, bars: trim.map(bars), next };
  const out = [];
  let need = 0, blanks = false;                     // the bars of a row of the table being read; it was wrapped
  for (let i = 0; i < lines.length; i++) {
    const t = trim[i];
    if (!t.startsWith("|")) need = 0;
    else if (need) {
      const j = rowEnd(at, i, need, blanks);
      if (j >= 0) { out.push(merged(lines, i, j)); i = j; continue; }
    } else if (next[i] !== i) {
      const start = tableStart(at, i);
      if (start) {
        out.push(merged(lines, i, start.head), merged(lines, start.head + 1, start.sep, ""));
        ({ need, wrapped: blanks } = start);
        i = start.sep;
        continue;
      }
    }
    let j = i, text = lines[i].text.trimEnd();
    if (t.startsWith("|") && !t.endsWith("|")) {
      while (j + 1 < lines.length && j - i < 4 && !text.endsWith("|")) {
        const more = trim[j + 1];
        if (!more || more.startsWith("| ") || /^[+-]\|/.test(more)) break;   // a diff's next row (difftable.js)
        text += text.endsWith("-") && more.startsWith("-") ? more : ` ${more}`;
        j++;
      }
    }
    if (j > i && text.endsWith("|")) {
      out.push({ ...merged(lines, i, j), text });
      i = j;
    } else out.push(lines[i]);
  }
  return out;
}

/** A Markdown pipe table from line i: a "|" header row, its "|---|" line, then rows, as GFM
 *  reads them: every next line that starts with "|", its closing "|" or not (a row cut short,
 *  `cut -c200`, or wrapped past repair is a row too), as long as it has two cells at least or
 *  its closing "|": a tree's "|-- dir" or a pipeline's "| grep x" under it ends the table
 *  (GFM would take it): {to, head, rows, align}, or undefined. Its
 *  own widget even where an agent colored other text (Codex's inline code): a pipe table is no
 *  rendering of its own. */
export function pipeTable(lines, i) {
  const row = (k) => /^\s*\|.*\|\s*$/.test(lines[k]?.text ?? "");
  if (!row(i) || !TABLE_SEP.test(lines[i + 1]?.text.trim() ?? "")) return undefined;
  const cells = (k) => lines[k].text.trim().replace(/^\|/, "").replace(/(^|[^\\])\|$/, "$1").split(BARS).map((c) => c.trim());
  let j = i + 2;
  const more = (t) => /^\s*\|/.test(t) && (/\|\s*$/.test(t) || (t.trim().slice(1).match(BARS) ?? []).length > 0);
  while (more(lines[j]?.text ?? "")) j++;
  const head = cells(i), rows = [];
  for (let k = i + 2; k < j; k++) rows.push(cells(k));
  const n = Math.max(head.length, ...rows.map((r) => r.length)), pad = (r) => [...r, ...Array(n - r.length).fill("")];
  const align = lines[i + 1].text.trim().slice(1, -1).split("|").map((c) => /-:\s*$/.test(c));
  return { to: j - 1, head: pad(head), rows: rows.map(pad), align: pad(align).map(Boolean) };
}

/** The regions of `rows` that become widgets, in order and never overlapping:
 *  {kind: "code"|"markdown"|"mermaid"|"table", from, to (row indexes, inclusive), lang, text
 *  (a table's as Markdown), guessed, table (drawntable.js: its cells)}.
 *  `end` (exclusive) is the first row that is not output yet (the cursor's, which may hold the
 *  next prompt); a fence still open there is left for later. Output a program colored itself
 *  (bat, glow, delta, a coding agent's own rendering) is never one: it is rendered already; but
 *  a table it drew is, its rules breaking apart when the view is narrower than the pane. */
export const regions = (rows, end) => scan(rows, end).found;

/** regions(), and `settled`: the first row whose regions may still change as rows are added
 *  after `end` (a fence or a file not finished, a document that may go on); scanning again from
 *  there finds the same regions after it. `changes`: the first row that may change (the
 *  screen's first): settled is never after it, nor inside a drawn box there (AC-31). `waiting`:
 *  the first row of a drawn table whose last rule is not printed yet, or may not be, or null. A
 *  table region's `table.open`: it may go on (nothing after it yet, or its bottom rule not there). */
export function scan(rows, end = rows.length, changes = rows.length) {
  const lines = joinTableRows(logicalLines(rows.slice(0, end)));
  const next = rows[end]?.text ?? null;                           // the cursor's row
  const found = [];
  let open = lines.length;                                        // the first line still undecided
  let noRun = -1, waiting = null;
  const add = (kind, a, b, lang, text, guessed = false, table = undefined) => {
    if (b - a + 1 > MAX_LINES || !text.trim() || (!table && lines.slice(a, b + 1).some((l) => l.styled))) return;
    found.push({ kind, from: lines[a].from, to: lines[b].to, lang, text, guessed, table, line: a, lastLine: b });
  };
  for (let i = 0; i < lines.length; i++) {
    const printed = printedFile(lines, i, next);
    if (printed === null) open = Math.min(open, i);              // its prompt has not come back yet
    if (printed) {
      if (printed.to >= printed.from) add(printed.kind, printed.from, printed.to, printed.lang, body(lines, printed.from, printed.to));
      i = printed.to;
      continue;
    }
    // a Markdown table in a diff first: its head and separator may be context lines a plain table's would take alone
    const changed = diffTable(lines, i, MAX_LINES);
    if (changed) {
      add("table", i, changed.to, "", tableMarkdown(changed), false, { ...changed, open: changed.to + 1 >= lines.length });
      i = changed.to;
      continue;
    }
    const piped = pipeTable(lines, i);                            // a Markdown table, colored text around it or not
    if (piped) {
      add("table", i, piped.to, "", tableMarkdown(piped), false, { ...piped, open: piped.to + 1 >= lines.length });
      i = piped.to;
      continue;
    }
    const table = drawnTable(lines, i, MAX_LINES);
    if (table === null) { open = Math.min(open, i); waiting = lines[i].from; break; }   // its last rule is not printed yet
    if (table) {
      add("table", i, table.to, "", tableMarkdown(table), false, table);
      if (table.open && table.to + 1 >= lines.length) waiting = lines[i].from;   // an ASCII rule with nothing after it yet
      i = table.to;
      continue;
    }
    // a run that is no document has none inside it either (its fences are still looked at); a
    // tool's output may be one from its first line, its mark read as indentation
    const tool = TOOL_OUT.test(lines[i].text) && i > noRun ? lines[i] : null;
    if (tool) lines[i] = { ...tool, text: tool.text.replace("⎿", " "), first: true };
    let doc;
    try {
      doc = i > noRun ? markdownRun(lines, i) : null;
      if (doc?.ok) add("markdown", i, doc.to, "markdown", body(lines, i, doc.to));
    } finally {
      if (tool) lines[i] = tool;
    }
    if (doc?.open) open = Math.min(open, i);
    if (doc?.ok) { i = doc.to; continue; }
    if (doc && !tool) noRun = doc.to;                             // from its next line the output is looked at as before
    const fence = fenced(lines, i);
    if (fence === null && lines.length - i <= MAX_LINES) { open = Math.min(open, i); break; }   // still printing: it may close
    if (fence) {
      const kind = MERMAID_LANG.test(fence.lang) ? "mermaid" : "code";
      add(kind, i, fence.to, fence.lang, fence.text);
      i = fence.to;
      continue;
    }
    if (DIAGRAM.test(lines[i].text) && !lines[i - 1]?.text.trim()) {
      let j = i;
      while (j + 1 < lines.length && lines[j + 1].text.trim() && !ends(lines, j + 1)) j++;
      if (j > i) { add("mermaid", i, j, "mermaid", body(lines, i, j), true); i = j; }
    }
  }
  // a region near the end may still grow (a document, a diagram), the last line too (no hard end);
  // but no further back than a region can be long: a scan never grows with the history
  const moving = lines.findIndex((l) => l.to >= changes);
  open = Math.max(Math.min(open, Math.max(0, lines.length - GAP - 2), moving < 0 ? lines.length : moving), lines.length - MAX_LINES);
  // nor inside a drawn box (a table not whole yet, or off its grid): the next scan, from there,
  // would take a row of it for a head (AC-31)
  while (open > Math.max(0, lines.length - MAX_LINES) && open < lines.length && insideBox(lines[open].text, lines[open - 1].text)) open--;
  for (const r of found) if (r.lastLine >= open) open = Math.min(open, r.line);
  const settled = open < lines.length ? lines[open].from : Math.min(end, rows.length);
  return { found: found.map(({ line, lastLine, ...r }) => r), settled, waiting };
}

// A region's text without the indentation all its lines share: a coding agent indents its tools'
// output, and Markdown takes text indented 4 spaces for code.
function body(lines, a, b) {
  const part = lines.slice(a, b + 1).map((l) => l.text);
  const cut = Math.min(...part.filter((t) => t.trim()).map((t) => t.length - t.trimStart().length));
  return part.map((t) => t.slice(Math.min(cut, t.length - t.trimStart().length))).join("\n");
}

// A command line that prints one file ("alex@devbox ~ % cat README.md"): the lines after it up
// to the next line that starts with the same prompt, numbers aside (the cursor's row too),
// without the blank and colored lines just before it (a prompt of two lines, Starship's). A
// prompt of one or two characters ("$ ") is too common in files to end one. Else undefined:
// the lines are looked at one by one, as any output; null when its prompt has not come back yet.
function printedFile(lines, i, next) {
  const m = lines[i].text.match(PRINT);
  if (!m) return undefined;
  const prompt = lines[i].text.slice(0, m.index + (m[0].startsWith(" ") ? 1 : 0)).trimEnd();
  if (prompt.trim().length < 3 || !(PROMPT.test(prompt + " ") || /[❯➜›λ»$%#>]$/.test(prompt))) return undefined;
  const ext = m[2];
  const kind = MARKDOWN_EXT.test(ext) ? "markdown" : MERMAID_EXT.test(ext) ? "mermaid" : "code";
  const found = (to) => {
    while (to > i && (!lines[to].text.trim() || lines[to].styled)) to--;
    return { kind, lang: kind === "code" ? m[1].split("/").pop() : kind, from: i + 1, to };
  };
  const same = shape(prompt);
  for (let j = i + 1; j < lines.length; j++) if (shape(lines[j].text).startsWith(same)) return found(j - 1);
  return next != null && shape(next).startsWith(same) ? found(lines.length - 1) : lines.length - i <= MAX_LINES ? null : undefined;
}

// A fence opened at line i: {lang, text, to} when closed, null when not closed yet, undefined when
// none: the output went on with no closing fence (a tool's output cut short): a coding agent's next
// message or result, less indented than the fence (Claude Code's marks, Codex's), or a prompt, then no
// closer or another fence opening. It is text, and what follows is read.
function fenced(lines, i) {
  const open = lines[i].text.match(FENCE);
  if (!open) return undefined;
  const [, indent, mark] = open;
  const close = new RegExp(`^\\s*${mark[0] === "`" ? "`" : "~"}{${mark.length},}\\s*$`);
  const moved = (t) => { const m = t.match(/^(\s*)[⏺●⎿•└]/); return !!m && m[1].length < indent.length; };
  let prompted = false;
  for (let j = i + 1; j < lines.length; j++) {
    if (moved(lines[j].text) || (prompted && lines[j].text.match(FENCE)?.[3])) return undefined;
    prompted ||= PROMPT.test(lines[j].text);
    if (close.test(lines[j].text)) {
      const text = lines.slice(i + 1, j).map((l) => (l.text.startsWith(indent) ? l.text.slice(indent.length) : l.text.trimStart())).join("\n");
      return { lang: open[3], text, to: j };
    }
  }
  return prompted ? undefined : null;
}

// What a line says about Markdown: a strong sign, a weak one, or none.
function sign(lines, i) {
  const t = lines[i].text.trimStart();           // indented as a tool's output is
  if (HEADING.test(t)) {
    // a heading stands after a blank line (or first in a tool's output); "#" lines in a run of code are its comments
    const prev = lines[i].first ? "" : lines[i - 1]?.text ?? "";
    return prev.trim() || HEADING.test((lines[i + 1]?.text ?? "").trimStart()) ? "" : "heading";
  }
  if (TABLE_SEP.test(t)) return "table";
  if (FENCE.test(t)) return "fence";
  if (LIST.test(t)) return "list";
  if (QUOTE.test(t)) return "quote";
  if (INLINE.test(t)) return "inline";
  return "";
}

// A "#" line inside a paragraph of three or more of them is a comment of code, as in a shell
// script's comment block (a heading stands after a blank line, a list or text once in a while:
// a compact changelog, a heading right after a line of text).
function comment(lines, i) {
  const hash = (k) => HEADING.test(lines[k]?.text.trimStart() ?? "");
  const prev = lines[i].first ? "" : lines[i - 1]?.text.trimStart() ?? "";
  if (!hash(i) || !prev || LIST.test(prev) || QUOTE.test(prev)) return false;
  let a = i, b = i;
  while (a > 0 && i - a < 50 && lines[a - 1].text.trim()) a--;
  while (b + 1 < lines.length && b - i < 50 && lines[b + 1].text.trim()) b++;
  let count = 0;
  for (let k = a; k <= b; k++) if (hash(k)) count++;
  return count >= 3;
}

// A run of Markdown starting at line i: from a sign, on over gaps of at most GAP lines to the
// last sign and the paragraph after it, never over a prompt; a fence inside it is part of it.
// It is a document (ok) when it has a heading, a table or a fence, two kinds of signs, three
// lines with a sign, and few lines of code. null: no sign at i; open: it reached the end and
// may go on. {ok, to, open}.
function markdownRun(lines, i) {
  if (!sign(lines, i)) return null;
  const kinds = new Set();
  let last = i, signs = 0, codeish = 0, filled = 0, j = i, open = false;
  for (; j < lines.length && j - last <= GAP && j - i < MAX_LINES; j++) {
    const t = lines[j].text;
    if (ends(lines, j)) break;
    const s = sign(lines, j);
    if (s === "fence") {
      const f = fenced(lines, j);
      if (!f) { open = true; break; }                             // not closed (yet): the run ends before it
      kinds.add(s); signs++; filled += f.to - j + 1; last = j = f.to;
      continue;
    }
    if (t.trim()) { filled++; if (CODEISH.test(t) || comment(lines, j)) codeish++; }
    if (s) { kinds.add(s); signs++; last = j; }
  }
  open ||= j >= lines.length && j - i < MAX_LINES;
  const strong = kinds.has("heading") || kinds.has("table") || kinds.has("fence");
  if (!strong || kinds.size < 2 || signs < 3 || codeish > filled * 0.3) return { ok: false, to: last, open };
  let to = last;                                                  // the paragraph after the last sign
  const prose = (k) => k < lines.length && lines[k].text.trim() && !ends(lines, k);
  if (to + 1 < lines.length && !lines[to + 1].text.trim() && prose(to + 2)) to++;   // after a blank line
  while (prose(to + 1)) to++;
  return { ok: true, to, open };
}

