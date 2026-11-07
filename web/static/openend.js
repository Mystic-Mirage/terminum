// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A session just opened keeps to its last line through what settles late (fonts, widgets and
// images, the phone's list closing, the keyboard coming up), until PIN_OPEN after its first frame
// (however slow the way) or the reader scrolls, presses, types or selects text, or a jump to a
// line takes the view (markjump.js). Checked each frame: the rows lie in the terminal itself (no box of theirs to
// observe), and images, fonts and widgets grow it without a change to the page's rows (AC-04).

const PIN_OPEN = 4000;       // ms from the first frame
const PIN_WAIT = 15000;      // ms it waits for that first frame at most

/** term: the scrolling terminal; toBottom(): the view to its end. {pin, pinned, unpin, drawn}. */
export function openEnd({ term, toBottom }) {
  let until = 0, first = false, ticking = false;
  const chosen = () => !(getSelection()?.isCollapsed ?? true);   // text selected: the reader's place, at once, not from the next frame
  const pinned = () => until > performance.now() && !chosen();
  const unpin = () => { until = 0; };
  for (const ev of ["wheel", "touchmove", "pointerdown", "keydown"]) document.addEventListener(ev, unpin, { capture: true, passive: true });
  const tick = () => {
    if (chosen()) unpin();                         // for good: the selection going later does not pin it again
    if (!pinned()) { ticking = false; return; }
    if (term.scrollHeight - term.scrollTop - term.clientHeight >= 1) toBottom();
    requestAnimationFrame(tick);
  };
  return {
    pinned, unpin,
    /** A session was opened. */
    pin() {
      until = performance.now() + PIN_WAIT; first = true;
      toBottom();
      if (!ticking) { ticking = true; requestAnimationFrame(tick); }
    },
    /** A frame of the screen was drawn: the first one starts the count. */
    drawn() { if (first && pinned()) { first = false; until = performance.now() + PIN_OPEN; } },
  };
}
