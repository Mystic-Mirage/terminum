// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The text of a selection in the terminal, as iTerm2 would copy it. Line ends are drawn by CSS
// (generated content is never copied), so the lines are joined here: a hard end is a newline, a
// line tmux wrapped continues, and prose a program wrapped (term.js re-joins it) keeps its space.

/** What makes a line read otherwise than it runs, taken out of terminal text that leaves the page
 *  (`unhidden`): bidi controls, zero-width characters, the soft hyphen, word joiners, line
 *  separators, tag characters; never ZWNJ and ZWJ (words, emoji). terminumd's copy of the list
 *  (a program's copy, plans, names): termtext.rs `hides` (web/test/hides.test.mjs keeps them equal). */
export const HIDE = /[\u00ad\u061c\u180e\u200b\u200e\u200f\u2028-\u202e\u2060-\u206f\ufeff\ufff9-\ufffb\u{e0001}\u{e0020}-\u{e007f}]/u;
const HIDES = new RegExp(HIDE.source, "gu");

/** `text` without what HIDE matches, and how many characters it left out. */
export function unhidden(text) {
  let left = 0;
  return { text: text.replace(HIDES, () => { left++; return ""; }), left };
}

/** Said with a copy that left `left` characters out (fail loud: the copy differs from the file). */
export const hiddenNote = (left) => (left ? ` (${left} hidden character${left === 1 ? "" : "s"} left out)` : "");

const PREVIEW = 48;            // characters of the first line said

/** What a copy takes, said on Copy and in its toast: "3 lines: its first line", the first line with
 *  text, its blanks one space, cut at PREVIEW (a program's copy is not drawn as it is). Linear, in
 *  a single pass: a program's copy is up to 1 MB and its own. */
export function copyPreview(text) {
  let end = text.length, lines = 1, first = "";
  while (end && text.charCodeAt(end - 1) === 10) end--;
  for (let at = 0; ; lines++) {
    const nl = text.indexOf("\n", at), stop = nl < 0 || nl >= end ? end : nl;
    if (!first) first = text.slice(at, stop).replace(/\s+/g, " ").trim().slice(0, 4 * PREVIEW);
    if (stop === end) break;
    at = stop + 1;
  }
  const chars = Array.from(first);
  return `${lines === 1 ? "1 line" : `${lines} lines`}: ${chars.length > PREVIEW ? `${chars.slice(0, PREVIEW - 1).join("")}…` : first}`;
}

/** Joins the selected parts of lines: {text, eol, join, glue, cont}, in order. Padding after a hard
 *  end is dropped; a re-joined line goes on after a space, its indentation dropped. */
export function joinLines(parts) {
  let out = "";
  parts.forEach((p, i) => {
    let t = p.text.replace(/\ufe0e/g, "");          // the text-style mark render.js adds
    if (p.eol) t = t.replace(/\s+$/, "");
    if (p.cont && i > 0) t = t.replace(/^\s+/, "");
    out += t;
    if (i < parts.length - 1 && p.eol) out += p.join ? (p.glue ? "" : " ") : "\n";   // glue: a word broken at the edge
  });
  return out;
}

/** The selection's range when text is selected in `term`, else null. */
export function termRange(term) {
  const sel = getSelection();
  if (!sel?.rangeCount || sel.isCollapsed) return null;
  const range = sel.getRangeAt(0);
  return term.contains(range.commonAncestorContainer) ? range : null;
}

/** The selection's text when it lies in `term`; null otherwise (the browser's own text). */
export function selectedText(term) {
  const range = termRange(term);
  if (!range) return null;
  const parts = [];
  for (const ln of term.querySelectorAll(".ln")) {
    if (!range.intersectsNode(ln)) continue;
    const r = document.createRange();
    r.selectNodeContents(ln);
    if (ln.contains(range.startContainer)) r.setStart(range.startContainer, range.startOffset);
    if (ln.contains(range.endContainer)) r.setEnd(range.endContainer, range.endOffset);
    const c = ln.classList;
    parts.push({ text: r.toString(), eol: c.contains("eol"), join: c.contains("join"), glue: c.contains("glue"), cont: c.contains("cont") });
  }
  return parts.length ? joinLines(parts) : null;
}
