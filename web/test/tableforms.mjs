// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-31 the forms of a drawn table for the brute force (tableforms.test.mjs, e2e_tableforms.mjs): a
// model of what a table is, what each way of beginning and ending leaves of it, and what must come
// out. Rows of the model have their cells known; the lines are drawn from them.
import { insideBox } from "../static/drawntable.js";
import { scan } from "../static/outparse.js";

const FAMILIES = {
  light: { top: "┌─┬─┐", mid: "├─┼─┤", end: "└─┴─┘", bar: "│" },
  heavy: { top: "┏━┳━┓", mid: "┣━╋━┫", end: "┗━┻━┛", bar: "┃" },
  double: { top: "╔═╦═╗", mid: "╠═╬═╣", end: "╚═╩═╝", bar: "║" },
  round: { top: "╭─┬─╮", mid: "├─┼─┤", end: "╰─┴─╯", bar: "│" },
  ascii: { top: "+-+-+", mid: "+-+-+", end: "+-+-+", bar: "|", ascii: true },
};
const W = [10, 16];
const ruleLine = (of) => { const [l, h, j, , r] = [...of]; return l + W.map((w) => h.repeat(w + 2)).join(j) + r; };
const rowLine = (bar, cells) => `${bar} ${cells.map((c, i) => c.padEnd(W[i])).join(` ${bar} `)} ${bar}`;

// The parts between a table's rules, as a program draws them: its head, a row wrapped over two lines
// (an ASCII table's never is), two rows; without rules between its rows they are one part.
function partsOf(fam, start, between) {
  const ascii = !!FAMILIES[fam].ascii;
  const head = [["Name", "Note"]], long = ascii ? [["gamma", "three"]] : [["Long", "a first part"], ["", "and a rest"]];
  const data = [[["alpha", "one"]], [["beta", "two"]]];
  const body = { top: [long, ...data], head: [long, ...data], mid: [long, ...data], wrapped: [long, ...data], rest: [long.slice(1), ...data], row: data }[start];
  const parts = between ? body : [body.flat()];
  return start === "top" || start === "head" ? [head, ...parts] : parts;
}

// One line of it read back: a part between rules is a row, its lines' cells joined; a part of many
// rows has a row per line, in a box table a line with an empty first cell going on the row above.
function readOf(fam, start, end, parts) {
  const ascii = !!FAMILIES[fam].ascii;
  const join = (part) => part[0].map((_, c) => part.map((cells) => cells[c]).filter(Boolean).join(" "));
  const first = parts[0], ruled = parts.length > 1 || end === "mid";
  const headed = start === "top" || start === "head" || (start !== "mid" && ruled && first.length === 1 && first[0].every(Boolean));
  const rest = headed ? parts.slice(1) : parts;
  let rows = rest.map(join);
  if (rest.length === 1) {
    rows = [];
    for (const cells of rest[0]) {
      if (!ascii && rows.length && !cells[0]) rows[rows.length - 1] = rows.at(-1).map((v, c) => (cells[c] ? (v ? `${v} ${cells[c]}` : cells[c]) : v));
      else rows.push([...cells]);
    }
  }
  return { head: headed ? join(first) : null, rows };
}

/** The lines of a table from `start` to `end`, and what it reads as. */
function drawn(fam, start, end, between) {
  const f = FAMILIES[fam], mid = ruleLine(f.mid), parts = partsOf(fam, start, between), lines = [];
  if (start === "top") lines.push(ruleLine(f.top));
  if (start === "mid") lines.push(mid);
  parts.forEach((part, i) => {
    if (i) lines.push(mid);
    for (const cells of part) lines.push(rowLine(f.bar, cells));
  });
  if (end === "mid") lines.push(mid);
  if (end === "bottom") lines.push(ruleLine(f.end));
  const ruled = lines.length > parts.flat().length;
  return { lines, read: readOf(fam, start, end, parts), ruled, parts: parts.length };
}

const BEFORE = { none: [], text: ["some text"], blank: ["some text", ""], prompt: ["alex@devbox ~ % ./report"], table: null };
const AFTER = { nothing: [], blankEnd: [""], text: ["next line"], blank: ["", "next line"], prompt: ["alex@devbox ~ % ls"], agent: ["⏺ Done."], table: null };
const LEADS = { none: (l) => l, indent: (l) => `  ${l}`, agent: (l, i) => (i ? `  ${l}` : `⏺ ${l}`), tool: (l, i) => (i ? `     ${l}` : `  ⎿  ${l}`) };
const OTHER = ["┌───┬───┐", "│ x │ y │", "├───┼───┤", "│ 1 │ 2 │", "└───┴───┘"];
const OTHER_READ = { head: ["x", "y"], rows: [["1", "2"]] };
const PIPE_HEAD = ["| Name | Note |", "|---|---|"], PIPE_ROWS = ["| alpha | one |", "| beta | two |"];

/** Every form: {name, lines, want (its tables: from, to, head, rows), waits (its end is not there
 *  yet, or may not be), of (what its first lines may show as they are printed)}. */
export function* forms() {
  for (const before of Object.keys(BEFORE)) for (const after of Object.keys(AFTER)) {
    for (const lead of Object.keys(LEADS)) for (const fam of Object.keys(FAMILIES)) for (const start of ["top", "head", "row", "wrapped", "rest", "mid"])
      for (const end of ["bottom", "none", "mid"]) for (const between of [true, false]) {
        const ascii = !!FAMILIES[fam].ascii;
        if (ascii && (start === "wrapped" || start === "rest" || start === "mid" || end === "mid")) continue;   // no wraps; every rule of it alike
        yield expected({ fam, start, end, between, lead, before, after }, drawn(fam, start, end, between), ascii);
      }
    // a Markdown table: its head and separator, or rows alone, which are no sign of one
    for (const lead of ["none", "indent"]) for (const start of ["top", "row"]) {
      const lines = [...(start === "top" ? PIPE_HEAD : []), ...PIPE_ROWS].map(LEADS[lead]);
      const d = { lines, ruled: start === "top", parts: 2, read: { head: ["Name", "Note"], rows: [["alpha", "one"], ["beta", "two"]] } };
      yield expected({ fam: "pipes", start, end: "none", between: true, lead, before, after }, d, false, true);
    }
  }
}

// What must come out of one form.
function expected(o, d, ascii, pipes = false) {
  const name = `${o.fam} ${o.start} ${o.end}${o.between ? "" : " no-rules"} lead:${o.lead} before:${o.before} after:${o.after}`;
  const lines = pipes ? d.lines : d.lines.map(LEADS[o.lead]);
  const pre = o.before === "table" ? [...OTHER, "between"] : BEFORE[o.before], post = o.after === "table" ? OTHER : AFTER[o.after];
  const from = pre.length, to = from + lines.length - 1, table = { from, to, ...d.read };
  const said = post.some((l) => l.trim()), other = (at) => ({ from: at, to: at + OTHER.length - 1, ...OTHER_READ });
  // rows with no rule are text; an end not there with nothing under it waits; else it is a table
  const shows = d.ruled && (pipes || o.end === "bottom" || said);
  const waits = !pipes && d.ruled && (o.end !== "bottom" ? !said : ascii && d.parts < 2 && !post.length);
  const want = [...(o.before === "table" ? [other(0)] : []), ...(shows ? [table] : []), ...(o.after === "table" ? [other(to + 1)] : [])];
  return { name, lines: [...pre, ...lines, ...post], want, waits, of: table };
}

/** The first table of `lines`. */
export function oneTable(lines) {
  const r = scan(lines.map((text) => ({ text, eol: true, styled: false }))).found.find((x) => x.kind === "table");
  return r && { from: r.from, to: r.to, head: r.table.head, rows: r.table.rows };
}

/** As widgets.js does: scans at the given line counts, each from where the last one settled (walked
 *  back out of a box), the rows above `changes` no longer changing; then one at the end, with all.
 *  The tables that came out final, and those of the last scan: {from, to, head, rows}. */
export function play(lines, cuts) {
  const rows = lines.map((text) => ({ text, eol: true, styled: false }));
  const final = [];
  let settled = 0, last = [];
  for (const cut of [...cuts.filter((c) => c < rows.length), rows.length]) {
    let i0 = settled;
    for (let k = 0; i0 > 0 && k < 2000 && insideBox(lines[i0], lines[i0 - 1]); k++) i0--;
    const part = rows.slice(i0, cut), { found, settled: s } = scan(part, part.length, part.length);
    let c = s;
    for (const r of [...found].reverse()) if (r.to >= c && r.from < c) c = r.from;
    last = [];
    for (const r of found) {
      if (r.kind !== "table") continue;
      const t = { from: i0 + r.from, to: i0 + r.to, head: r.table.head, rows: r.table.rows };
      if (r.to < c) { if (!(final.at(-1)?.to >= t.from)) final.push(t); } else last.push(t);
    }
    settled = i0 + c;
  }
  return [...final, ...last];
}

