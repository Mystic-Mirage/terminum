// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The [+] for a selection in the terminal (AC-57), and Copy above it: on a computer in the floating
// buttons' column, Copy level with the selection's first line, the column's other buttons hidden
// meanwhile (fold.css); on a phone where its folded buttons stand; while text is selected; Ask
// (AC-37) shows then too, under the [+]. It lies outside the terminal's rows
// (their taps, copying and line maths stay as they are). The selection is taken while it is
// there: a tap on iOS clears it first, and the terminal then catches up and moves its lines.
import { copyPreview, hiddenNote, selectedText, unhidden } from "./seltext.js";
import { FLOATING } from "./fold.js";

const KEEP_FOR = 600;          // ms the buttons stay after the selection went (a tap still lands)
const AFTER_COPY = 5000;       // … after Copy: the [+] may still be wanted

// The row a range starts on: in a widget's shadow root (AC-56), the row that holds the widget.
function startRow(range) {
  let node = range.startContainer;
  if (node.nodeType === 1 && !node.closest?.(".ln")) {            // between rows: the next one
    const next = node.childNodes[range.startOffset];
    const row = next?.nodeType === 1 && (next.matches(".ln") ? next : next.querySelector(".ln"));
    if (row) return row;
  }
  while (node) {
    const el = node.nodeType === 1 ? node : node.parentElement;
    const row = el?.closest(".ln");
    if (row) return row;
    node = node.getRootNode?.().host ?? null;
  }
  return null;
}

/** The selected range, also in a widget's shadow root: browsers give the document a collapsed
 *  range by the widget's row there (its text still in toString), and the range itself only to
 *  getComposedRanges given that shadow root (measured with a mouse drag in Playwright's
 *  Chromium and WebKit). */
export function selectedRange(sel, term) {
  const range = sel.getRangeAt(0);
  if (!sel.isCollapsed) return range;
  if (!sel.toString() || !sel.getComposedRanges) return null;
  const roots = [...term.querySelectorAll(".wg-host")].map((h) => h.shadowRoot).filter(Boolean);
  let got;
  try { got = sel.getComposedRanges({ shadowRoots: roots }); } catch { got = sel.getComposedRanges(...roots); }   // the older form
  const s = got?.[0];
  if (!s || s.collapsed) return null;
  const r = document.createRange();
  r.setStart(s.startContainer, s.startOffset);
  r.setEnd(s.endContainer, s.endOffset);
  return r;
}

/** The selection in `term` as {n, text, raw}: the line it starts on and its text as copied
 *  (raw: as it is, for Copy; text: trimmed, for a bookmark); null
 *  when there is none, or it lies in a coding agent's input box or status lines. */
export function selection(term, view) {
  const sel = getSelection();
  const range = sel?.rangeCount ? selectedRange(sel, term) : null;
  if (!range) return null;
  const row = startRow(range);
  if (!row || !term.contains(row) || row.closest(".inbox, .foot")) return null;
  const n = view.lineOf(row);
  const inWidget = range.startContainer.getRootNode() instanceof ShadowRoot;
  const raw = (!inWidget && selectedText(term)) || sel.toString(), text = raw.trim();
  if (n === null || !text) return null;
  return { n, text, raw, rect: firstRect(range) };
}

/** The range's first line on the screen (where the selection starts). */
const firstRect = (range) => [...range.getClientRects()].find((r) => r.height > 0) ?? range.getBoundingClientRect();

/** How far the floating buttons' column's left is from the terminal's text's right edge: a 32 px
 *  button, 8 px from the scrollbar's 18. */
const COLUMN_IN = 18 + 8 + 32;
/** The floating buttons' column's left in the terminal's area (marks.js placeFab; the selection's
 *  buttons too, where the column is not drawn: a split's embedded page). */
export const columnLeft = (term) => `${term.offsetLeft + term.clientLeft + term.clientWidth - COLUMN_IN}px`;
/** The same, in the window (fabpane.js placeWindow). */
export const columnX = (term) => term.getBoundingClientRect().left + term.clientLeft + term.clientWidth - COLUMN_IN;

/** Shows `button` while text in `term` (within `main`) is selected, `copyButton` above it and
 *  `askButton` under it; a press calls onPick({n, text}), onCopy(text) or onAsk(text, n) (the text
 *  as taken while it was selected). Returns {program}: the same for a full-screen program's copy. */
export function markPicker({ term, main, button, copyButton, askButton, view, onPick, onCopy, onAsk }) {
  let kept = null, hide = 0, frame = 0, copied = false;
  const floating = matchMedia(FLOATING);
  const show = (on) => { button.hidden = copyButton.hidden = askButton.hidden = !on; };
  const gone = () => { clearTimeout(hide); hide = 0; kept = null; copied = false; show(false); };
  // Copy, [+], Ask top down in the floating buttons' column (its left: marks.js placeFab): a phone's
  // where its folded buttons stand (fold.js); a computer's from the selection's first line, kept in the terminal
  const place = () => {
    frame = 0;
    const lead = document.getElementById(floating.matches ? "foldfab" : "findfab");
    if (!kept || !lead) return;
    // one gap (the guidelines' --sp-3) between the three, above the first and below the last
    const gap = parseFloat(getComputedStyle(term).getPropertyValue("--sp-3")) || 12;
    const h = button.offsetHeight || 32, step = h + gap, x = lead.style.left || columnLeft(term);   // none drawn: the column's place
    const t = term.getBoundingClientRect(), m = main.getBoundingClientRect();
    const y = floating.matches ? parseFloat(lead.style.top) || t.top - m.top + gap
      : Math.min(Math.max(kept.rect.top + kept.rect.height / 2 - h / 2, t.top + gap), t.bottom - 2 * step - h - gap) - m.top;
    [copyButton, button, askButton].forEach((b, i) => Object.assign(b.style, { left: x, top: `${Math.round(y + i * step)}px` }));
  };
  const later = () => { if (!frame) frame = requestAnimationFrame(place); };
  let pending = 0;                                 // a drag changes the selection on every move: once a frame
  document.addEventListener("selectionchange", () => { if (!pending) pending = requestAnimationFrame(changed); });
  const keep = (got) => {
    if (got.raw !== kept?.raw) {                   // another selection (the same one comes back after a copy)
      copied = false;
      const { text, left } = unhidden(got.raw), says = `Copy ${copyPreview(text)}${hiddenNote(left)}`;   // what it takes
      copyButton.title = says; copyButton.setAttribute("aria-label", says);
    }
    kept = got;
    clearTimeout(hide); hide = 0;
    show(true);
    place();                                       // placed before they are drawn
  };
  const changed = () => {
    pending = 0;
    const got = selection(term, view);
    if (!got && kept?.program) return;               // a program's selection is no browser's
    if (got) keep(got);
    else if (!button.hidden && !hide) {
      // after Copy the selection may go (a browser's copy, a phone): the [+] stays a while for it
      hide = setTimeout(gone, copied ? AFTER_COPY : KEEP_FOR);
    }
  };
  // the selection moves with the view: its first line where it is now
  term.addEventListener("scroll", () => {
    const sel = getSelection(), range = kept && sel?.rangeCount ? selectedRange(sel, term) : null;
    if (range) kept.rect = firstRect(range);
    later();
  }, { passive: true });
  for (const b of [button, copyButton, askButton]) b.addEventListener("mousedown", (e) => e.preventDefault());   // the selection stays
  askButton.addEventListener("click", () => {
    if (!kept) return;
    const { raw, n } = kept;
    gone();
    onAsk(unhidden(raw).text, n);
  });
  button.addEventListener("click", () => {
    if (!kept) return;
    const { n, text } = kept;
    kept = null; show(false);
    getSelection().removeAllRanges();                                 // the terminal catches up
    onPick({ n, text: unhidden(text).text.trim() });
  });
  copyButton.addEventListener("click", () => {
    if (!kept) return;
    copied = true;
    onCopy(kept.raw);                                                  // the selection stays for a bookmark
  });
  // the user moved on: a press in the terminal, a key
  const movedOn = () => { if (copied || kept?.program) gone(); };
  term.addEventListener("pointerdown", movedOn);
  document.addEventListener("keydown", movedOn);
  return {
    /** What a full-screen program copied of its own selection (AC-06): the buttons by `row`, its
     *  upper row, as for a selection there; the same text again only moves them. */
    program({ text, row }) {
      const n = view.lineOf(row);
      if (n === null || !text.trim()) return;
      keep({ n, text: text.trim(), raw: text, rect: row.getBoundingClientRect(), program: true });
    },
  };
}
