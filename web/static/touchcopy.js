// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Selecting and copying terminal text on a touch screen (AC-52). With the keyboard up iOS
// selects nothing on a long press, so input.js selects the word itself; these keep that word
// in sight while the keyboard closes and offer Copy, as iOS shows no menu for a selection the
// page made.
import { copyPreview, hiddenNote, selectedText, termRange, unhidden } from "./seltext.js";

/** Keeps the selection at viewport height y for `ms` while the page settles: the keyboard
 *  closing grows the page, which then scrolls to its end (app.js fitViewport runs first). At
 *  the end of the output there is no room to scroll, and the word only stays in sight. */
export function keepSelectionAt(term, y, ms = 1000) {
  const fix = () => {
    const r = termRange(term)?.getBoundingClientRect();
    if (r?.height) term.scrollTop += r.top - y;
  };
  const on = [[window.visualViewport, "resize"], [window.visualViewport, "scroll"], [window, "resize"]].filter(([t]) => t);
  for (const [t, e] of on) t.addEventListener(e, fix);
  requestAnimationFrame(fix);
  setTimeout(() => { for (const [t, e] of on) t.removeEventListener(e, fix); }, ms);
}

/** Shows `button` while text in `term` is selected; a tap copies it with copy(text). The tap
 *  may clear the selection first (iOS does), and the terminal then catches up and moves its
 *  lines, so the text is taken while it is selected (the terminal is paused then). */
export function copyButton(button, term, copy) {
  let kept = "", hide = 0;
  document.addEventListener("selectionchange", () => {
    if (termRange(term)) {
      kept = selectedText(term) ?? "";
      clearTimeout(hide); hide = 0;
      button.hidden = false;
    } else if (!button.hidden && !hide) {
      hide = setTimeout(() => { hide = 0; kept = ""; button.hidden = true; }, 600);   // a tap on it still lands
    }
  });
  button.addEventListener("mousedown", (e) => e.preventDefault());   // focus stays where it is
  button.addEventListener("click", async () => {
    if (!kept) return;
    await copy(kept);
    getSelection().removeAllRanges();                                  // done: the terminal catches up
  });
}

/** On a touch screen a selection in `term` shows no menu of its own (Copy, Find Selection,
 *  Translate over the text, AC-37): the page's Copy, [+] and Ask stand at the terminal's top right.
 *  iOS shows none for a selection the page made, so when a finger leaves a selection (`lift` ms
 *  after any touch ends: a long press, a tap on it; after a handle, which the page's touch events
 *  do not see, `quiet` ms without a change, once for each range) the page clears it, which puts
 *  iOS's menu away, and makes the same range again `gap` ms later (measured in the iPhone
 *  simulator: a frame is too soon, the menu stays; 120 ms puts it away). The page's own changes
 *  are not taken for the finger's; hold(on) keeps the terminal from drawing meanwhile (a line
 *  redrawn under the range taken away would lose it). */
export function quietSelection(term, touch, { lift = 60, quiet = 600, gap = 120, hold = () => {} } = {}) {
  let wait = 0, mineUntil = 0, fingers = 0, last = "";
  const later = (ms, again) => { clearTimeout(wait); if (touch.matches) wait = setTimeout(() => remake(again), ms); };
  function remake(again) {
    if (fingers) return;                               // the lift calls again
    const sel = getSelection(), range = termRange(term);
    if (!range) return;
    const r = range.cloneRange(), key = `${r.startOffset}:${r.endOffset}:${r.toString()}`;
    if (key === last && !again) return;
    last = key;
    mineUntil = performance.now() + gap + 200;
    hold(true);
    sel.removeAllRanges();
    setTimeout(() => {
      if (!getSelection().rangeCount && r.startContainer.isConnected) getSelection().addRange(r);
      hold(false);
    }, gap);
  }
  document.addEventListener("selectionchange", () => { if (performance.now() >= mineUntil) later(quiet, false); });
  const count = (e) => { fingers = e.touches.length; if (!fingers && e.type !== "touchstart") later(lift, true); };
  for (const t of ["touchstart", "touchend", "touchcancel"]) document.addEventListener(t, count, { passive: true, capture: true });
}

/** The older copy command for text that is no longer selected: it selects a copy of it, then
 *  the user's selection again (Copy beside a selection keeps it for a bookmark, AC-57). */
export function copyByCommand(text) {
  const el = document.createElement("span"), sel = getSelection();
  const was = sel.rangeCount && !sel.isCollapsed ? sel.getRangeAt(0).cloneRange() : null;
  el.textContent = text;
  el.style.cssText = "position:fixed;left:-9999px;top:0;white-space:pre;-webkit-user-select:text;user-select:text";
  document.body.append(el);
  sel.selectAllChildren(el);
  try { return document.execCommand("copy"); } finally {
    el.remove();
    sel.removeAllRanges();
    if (was) sel.addRange(was);
  }
}

/** Copies `got`: what hides what a line runs left out; the clipboard API needs HTTPS (or localhost), elsewhere `fallback`.
 *  What to say of it, and whether the clipboard API took it. */
export async function copyText(got, fallback = copyByCommand) {
  const { text, left } = unhidden(got), said = `Copied ${copyPreview(text)}${hiddenNote(left)}`;
  if (window.isSecureContext && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return { said, api: true };
    } catch { /* fall through */ }
  }
  return { said: fallback(text) ? said : "Copying is blocked here; use the system Copy menu.", api: false };
}
