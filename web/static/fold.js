// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A phone's floating buttons (AC-18): on a touch screen or a narrow window every floating button
// but the keyboard's (#fabs) folds into one ⟨…⟩ at the terminal's top right, where find stood
// (marks.js placeFab puts the others in a column under it). A tap unfolds them; 3 s later they
// fold again, at once when one is pressed, the page is touched elsewhere or the terminal scrolls;
// a finger or the focus on the column holds them. A press on ⟨…⟩ or one of them first puts the
// on-screen keyboard and the hot keys away (away()), so the terminal is seen whole. Folded, it carries the bookmarks' count and the
// reminders' pink. While text is selected the selection's Copy, [+] and Ask stand there instead
// (markpick.js). From a finger's first move scrolling the terminal to its lift, every floating
// button is out of sight (body.scrolling, fold.css), so the text under them can be read; a wheel's,
// a key's or the scroll bar's scroll hides the column alone (body.scrolled), both back 1 s after it.

const $ = (id) => document.getElementById(id);
export const FLOATING = "(hover: none), (max-width: 860px)";   // where the buttons float: the page's one query of it (style.css's #fabs)
let floatsQuery = null;
/** Whether the buttons float now (one MediaQueryList, made when first asked: the per-frame paths ask). */
export const floats = () => (floatsQuery ??= matchMedia(FLOATING)).matches;
// the folded buttons in their column's order: the plan's progress, the plain ones, then set apart the colored ones (marks.js placeFab)
// the plan's progress; the plain ones, each hidden by its owner with `data-off` while it has nothing to show; the colored ones
export const PLAN = ["planfab"], PLAIN = ["findfab", "splitfab", "sentfab", "linksfab", "tasksfab", "attachfab"], COLORED = ["marksfab", "remindfab"];
export const FOLDED = [...PLAN, ...PLAIN, ...COLORED];
const FOR = 3000;              // ms the column stays unfolded
const SCROLL = 10;             // px a finger moves before it scrolls
const SETTLE = 1000;           // ms without a scroll after the lift before they come back
const BACK = SETTLE;           // … and after a wheel's or a key's scroll: the column alone
const ASKED = 400;             // ms after a wheel, a key or a press that a scroll is the user's
const NEAR = 24;               // px round a button where the pointer brings the column back
const SCROLL_KEYS = /^(PageUp|PageDown|Home|End|ArrowUp|ArrowDown| )$/;
const COLUMN = ["foldfab", ...FOLDED, "markcopy", "markadd", "askfab"];

/** away(): the on-screen keyboard and the hot keys panel put away. */
export function fabFold({ away }) {
  const fold = $("foldfab"), body = document.body;
  let timer = 0, holding = false;
  const inColumn = (el) => FOLDED.some((id) => $(id).contains(el));
  const unfolded = () => body.classList.contains("unfolded");
  const close = () => {
    clearTimeout(timer); timer = 0;
    body.classList.remove("unfolded");
    fold.setAttribute("aria-expanded", "false");
  };
  const arm = () => {
    clearTimeout(timer);
    timer = setTimeout(() => (holding || inColumn(document.activeElement) ? arm() : close()), FOR);
  };
  fold.addEventListener("mousedown", (e) => e.preventDefault());   // the terminal keeps the keys
  fold.addEventListener("click", () => {
    away();
    if (unfolded()) return close();
    body.classList.add("unfolded");
    fold.setAttribute("aria-expanded", "true");
    arm();
  });
  // a press elsewhere folds them; one on the column holds them until it lifts
  document.addEventListener("pointerdown", (e) => {
    if (!unfolded()) return;
    if (inColumn(e.target)) holding = true;
    else if (!fold.contains(e.target)) close();
  }, true);
  const letGo = () => { if (holding) { holding = false; if (unfolded()) arm(); } };
  document.addEventListener("pointerup", letGo, true);
  document.addEventListener("pointercancel", letGo, true);
  for (const id of FOLDED) {
    $(id).addEventListener("pointerdown", () => { if (floats()) away(); });   // before its own click: find's field may take the keys
    $(id).addEventListener("click", () => { if (unfolded()) setTimeout(close); });   // its pane or field opens
  }

  // a finger scrolling the terminal (or swiping over the input, kbdtouch.js) hides them all until it
  // lifts and the terminal stops (its momentum after the lift: SETTLE ms without a scroll)
  let startY = null, settle = 0;
  const mine = (e) => !!e.target.closest?.("#termrow, #kbd, #bottom");
  const settled = () => { clearTimeout(settle); settle = setTimeout(() => body.classList.remove("scrolling"), SETTLE); };
  $("term").addEventListener("scroll", () => { if (startY === null && body.classList.contains("scrolling")) settled(); }, { passive: true });
  document.addEventListener("touchstart", (e) => { startY = mine(e) && e.touches.length === 1 ? e.touches[0].clientY : null; }, { passive: true, capture: true });
  document.addEventListener("touchmove", (e) => {
    if (startY === null || body.classList.contains("scrolling") || !floats()) return;
    if (Math.abs(e.touches[0].clientY - startY) > SCROLL) { clearTimeout(settle); body.classList.add("scrolling"); close(); }
  }, { passive: true, capture: true });
  const lift = () => { startY = null; if (body.classList.contains("scrolling")) settled(); };
  document.addEventListener("touchend", lift, { passive: true, capture: true });
  document.addEventListener("touchcancel", lift, { passive: true, capture: true });
  // a wheel, a key or the scroll bar scrolling the terminal (or its View) hides the column alone
  // (body.scrolled) until BACK ms without a scroll, the pointer near it or the focus on it; never
  // while a pane they opened is open, never for the page's own scroll (output followed)
  let asked = 0, back = 0, px = -1, py = -1;
  const PANES = ["sentpane", "linkspane", "remindpane", "markpane", "findbox", "planpane"];
  const ask = () => { asked = Date.now(); };
  const show = () => { clearTimeout(back); body.classList.remove("scrolled"); };
  for (const t of ["wheel", "pointerdown"]) document.addEventListener(t, (e) => { if (mine(e) && e.pointerType !== "touch") ask(); }, { passive: true, capture: true });
  document.addEventListener("keydown", (e) => { if (SCROLL_KEYS.test(e.key)) ask(); }, true);
  $("term").addEventListener("scroll", () => {
    const going = body.classList.contains("scrolled");   // a scroll on (momentum, a held key) keeps them away
    if ((!going && Date.now() - asked > ASKED) || PANES.some((id) => !$(id).hidden)) return show();
    body.classList.add("scrolled");
    clearTimeout(back); back = setTimeout(show, BACK);
  }, { passive: true });
  document.addEventListener("pointermove", (e) => {
    const moved = Math.hypot(e.clientX - px, e.clientY - py) > 4;
    px = e.clientX; py = e.clientY;
    if (!moved || !body.classList.contains("scrolled") || e.pointerType === "touch") return;
    const near = (b) => { const r = b.getBoundingClientRect(); return r.width && e.clientX > r.left - NEAR && e.clientX < r.right + NEAR && e.clientY > r.top - NEAR && e.clientY < r.bottom + NEAR; };
    if (COLUMN.some((id) => near($(id)))) show();
  }, { passive: true });
  document.addEventListener("focusin", (e) => { if (COLUMN.some((id) => $(id).contains(e.target))) show(); });

  // Esc folds them, the keys back on ⟨…⟩
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || !unfolded()) return;
    const inside = inColumn(document.activeElement);
    close();
    if (inside) fold.focus();
  });

  /** The fold where the buttons float and the column shows, with what they say: after placeFab
   *  (it sets the buttons and the count), and when a reminder falls due (reminders.js sets the bell's class). */
  function sync() {
    const on = floats() && !$("findfab").hidden, remind = $("remindfab");
    body.classList.toggle("folding", on);
    if (fold.hidden !== !on) fold.hidden = !on;
    if (!on) close();
    const n = on && !$("marksfab").hidden ? $("marksfabn").textContent : "";
    if ($("foldfabn").textContent !== n) $("foldfabn").textContent = n;
    fold.classList.toggle("remind", on && !remind.hidden);
    fold.classList.toggle("due", on && !remind.hidden && remind.classList.contains("due"));
  }
  new MutationObserver(sync).observe($("remindfab"), { attributes: true, attributeFilter: ["class"] });
  return { sync };
}
