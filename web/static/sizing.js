// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The pane's size (AC-13), full-screen programs (AC-05) and the mouse for them (AC-06). The
// page tells terminumd the grid that fits its terminal at the chosen text size: after opening
// a pane, after the view or the text size changed (300 ms after the last change), and with the
// first key typed after a pause (at most once a second); terminumd decides whether it sizes the
// window (the active page does) and keeps it within 45×14 and 240×70. The normal screen is never
// scaled: a pane whose width is not the view's re-flows to it (`reflow`, style.css). A full-screen program (tmux's alternate screen,
// not a coding agent's) is its grid: it hides the history and the widgets, the header marks it, and gets the mouse when
// it asked for it; one another page sized is drawn as it is (app.js scales a wider one) with
// "Sized for another screen" until this page sizes it again, and one whose view changes width
// keeps the chosen text size, clipped at the edge, until tmux has the new width (2 s at most after
// the last change): the text never shrinks to the old width and grows back (AC-04).
import { banner } from "./banners.js";
import { caretAt } from "./cursor.js";

const $ = (id) => document.getElementById(id);
const SETTLE = 300;             // ms after the last change of the view before its size is sent
const TYPED_EVERY = 1000;       // ms: a key sends the size at most this often
const SLIVER = { cols: 20, rows: 5 };   // a view smaller than this is passing: no size sent
const HOLD = 2000;              // ms after the last change a pane may wait for its new width unscaled
const bounded = (cols) => Math.max(45, Math.min(240, cols));   // the columns terminumd gives a grid (its sizing.rs)

/** view: the TermView; sid(): the shown pane's id; refit(): the drawn size may have changed;
 *  notify(text): a toast; copied({text, row}): what a full-screen program copied (AC-06). */
export function paneSizing({ view, send, sid, refit, notify, copied }) {
  const term = $("term");
  let last = null, typedAt = -Infinity, settle = 0, elsewhere = null, full = false;
  let holdUntil = 0, holdTimer = 0, heldFor = null;   // a hold, once for each size sent (heldFor)

  // a full-screen program: on the alternate screen, but not Codex's own (its input box, footer
  // and history are the page's); Claude Code draws none, so one in its pane is a program's
  const fullscreen = () => !!view.alt && view.agent !== "codex";
  /** The grid that fits the terminal at the chosen text size (not the scaled one). */
  const grid = () => view.gridFit(view.chosenSize());

  function sendSize(typing) {
    if (!sid() || term.hidden || !term.clientWidth) return;
    const g = grid();
    // a sliver (an animation, a drag, a hidden part) keeps the pane's size; terminumd keeps the
    // others within 45×14 and 240×70
    if (!(g.cols >= SLIVER.cols && g.rows >= SLIVER.rows)) return;
    if (!typing && last && last.cols === g.cols && last.rows === g.rows) return;
    last = { ...g };
    send({ t: "size", cols: g.cols, rows: g.rows, typing });
  }

  /** The program on the screen changed (alt): a full-screen one hides history and widgets. */
  function sync() {
    const now = fullscreen();
    if (now === full) return;
    full = now;
    term.classList.toggle("fullscreen", full);
    reflow();
    showSized();
    refit();
  }

  /** True while the pane waits for this view's new width: drawn at the chosen size, clipped,
   *  not scaled (AC-04): a full-screen program's grid only. It starts when a pane at the width this
   *  page sent no longer has the view's width, and ends when tmux's width is the view's, another
   *  screen sized it, or HOLD. */
  function holding() {
    if (elsewhere || !last || !view.cols || !full) return false;
    const fits = view.cols === bounded(grid().cols);
    if (holdUntil > performance.now()) return !fits;
    if (fits || view.cols !== bounded(last.cols) || heldFor === last) return false;
    heldFor = last;
    extend();
    return true;
  }
  /** A hold lasts HOLD after the view's last change; its end draws the pane as it is then. */
  function extend() {
    holdUntil = performance.now() + HOLD;
    clearTimeout(holdTimer);
    holdTimer = setTimeout(() => { holdUntil = 0; refit(); }, HOLD);
  }

  /** The normal screen of a pane whose width is not the view's re-flows to the view (AC-04). */
  function reflow() {
    const on = !full && !!view.cols && view.cols !== grid().cols;
    if (on !== term.classList.contains("reflow")) view.keepBottom(() => term.classList.toggle("reflow", on));
  }

  // the view's size: the window, the session list, docked Files, the keyboard, the hot keys
  const changed = () => {
    reflow();
    clearTimeout(settle); settle = setTimeout(() => sendSize(false), SETTLE);
    if (holdUntil > performance.now()) extend();   // a dragged edge: the hold lasts while it moves
  };
  new ResizeObserver(changed).observe(term);

  // Codex on tmux's alternate screen (started without --no-alt-screen) has no scrollback to load:
  // scrolling up there says why and how, once a pane (AC-39)
  const toldAlt = new Set();
  term.addEventListener("wheel", (e) => {
    if (e.deltaY >= 0 || term.scrollTop > 0 || !view.alt || view.agent !== "codex" || view.mouse || toldAlt.has(sid())) return;
    toldAlt.add(sid());
    notify('This Codex runs on the alternate screen, where tmux keeps no scrollback. Start it with "codex --no-alt-screen", or set alternate_screen = "never" under [tui] in ~/.codex/config.toml.');
  }, { passive: true });

  // said over a full-screen program, whose grid is drawn as it is, and for a refused resize
  // anywhere; the normal screen re-flows whatever the size (AC-04)
  function showSized() {
    banner("sized", !elsewhere || (!full && !elsewhere.error) ? "" : `Sized for another screen (${elsewhere.cols}×${elsewhere.rows})` + (elsewhere.error ? `: ${elsewhere.error}` : ""));
  }

  // ---------- the mouse (AC-06) ----------
  // a full-screen program that asked for the mouse gets it all; a coding agent on the alternate
  // screen the wheel only (its transcript scrolls), presses and drags select text; Shift selects
  const mouseOn = (e) => !!view.mouse && fullscreen() && !e?.shiftKey;
  const wheelOn = () => !!view.mouse && !!view.alt;
  /** {x, y} (0-based cells of the screen) at a viewport point, or null off the screen. */
  function cellAt(cx, cy) {
    const rows = view.screen.filter(Boolean);
    if (!rows.length) return null;
    let y = rows.findIndex((r) => r.el.getBoundingClientRect().bottom > cy);
    if (y < 0) y = rows.length - 1;
    const box = rows[y].el.getBoundingClientRect();
    const w = view.charRatio() * parseFloat(getComputedStyle(term).fontSize);
    const x = Math.max(0, Math.min((view.cols || 1) - 1, Math.floor((cx - box.left) / w)));
    return { x, y: view.screen.indexOf(rows[y]) };
  }
  const mods = (e) => ({ shift: !!e.shiftKey, alt: !!e.altKey, ctrl: !!e.ctrlKey });
  function report(action, at, button, e) {
    flushSteps();                                 // the wheel's steps first (a key typed meanwhile may pass them: a frame)
    if (at) send({ t: "mouse", x: at.x, y: at.y, button, action, mods: mods(e ?? {}) });
  }
  // The wheel: one step a text line scrolled, at most one an event (a trackpad sends many small
  // ones; a wheel's notch is one step), as xterm.js does; a frame's steps go as one message
  // with their count, which terminumd sends in one round trip (AC-06).
  let steps = null, stepFrame = 0, lines = 0;
  function step(action, at, e) {
    if (!at) return;
    if (steps && steps.action !== action) flushSteps();
    steps ??= { action, mods: mods(e ?? {}), n: 0 };
    Object.assign(steps, { at, n: steps.n + 1 });
    stepFrame ||= requestAnimationFrame(flushSteps);
  }
  function flushSteps() {
    cancelAnimationFrame(stepFrame);
    stepFrame = 0;
    const s = steps;
    steps = null;
    if (s) send({ t: "mouse", x: s.at.x, y: s.at.y, button: 0, action: s.action, mods: s.mods, ...(s.n > 1 ? { count: s.n } : {}) });
  }
  const BUTTON = { 0: 0, 1: 1, 2: 2 };
  let held = null;                                // a mouse button down: {button, x, y, y0}
  let released = null;                            // the last left release to the program: the selection's upper row
  let shiftFrom = null;                           // a Shift selection's start, while the button is down
  term.addEventListener("pointerdown", (e) => {
    if (e.pointerType !== "mouse" || !(e.button in BUTTON)) return;
    if (e.shiftKey && e.button === 0 && mouseOn()) {
      // Shift: the browser's own selection, drawn here from the press to the pointer. The
      // browsers' Shift extends the caret, Chrome's and Firefox's in the keyboard field (nothing,
      // or from where it was), and Chrome then moves a caret along the drag.
      const p = caretAt(e.clientX, e.clientY);
      if (!p || !term.contains(p.node)) return;
      e.preventDefault();
      if (document.activeElement?.id === "kbd") document.activeElement.blur();   // Chrome keeps a focused field's caret
      shiftFrom = p;
      getSelection().collapse(p.node, p.offset);
      return;
    }
    if (!mouseOn(e)) return;
    e.preventDefault();                           // no text selection: the drag is the program's
    // and it ends a browser (Shift) selection, as a press does: the keys go back on the release (input.js)
    if (String(getSelection())) getSelection().removeAllRanges();
    const at = cellAt(e.clientX, e.clientY);
    held = { button: BUTTON[e.button], ...at, y0: at?.y };   // y0: the press's row, as moves change y
    report("press", at, held.button, e);
  });
  term.addEventListener("pointermove", (e) => {
    const q = shiftFrom && e.buttons & 1 && caretAt(e.clientX, e.clientY);
    if (q && term.contains(q.node)) getSelection().setBaseAndExtent(shiftFrom.node, shiftFrom.offset, q.node, q.offset);
    if (e.pointerType !== "mouse" || !held || !mouseOn()) return;
    const at = cellAt(e.clientX, e.clientY);
    if (!at || (at.x === held.x && at.y === held.y)) return;
    Object.assign(held, at);
    report("move", at, held.button, e);
  });
  addEventListener("pointerup", (e) => {
    shiftFrom = null;
    if (e.pointerType !== "mouse" || !held) return;
    const { button: b, y0 } = held; held = null;
    if (!mouseOn()) return;
    const at = cellAt(e.clientX, e.clientY);
    report("release", at, b, e);
    if (b === 0) released = at ? { y: Math.min(y0 ?? at.y, at.y) } : null;
  });
  // What the program copied of its own selection after that release (terminumd hands it on only
  // within 2 s of the latest left release, notices.rs COPY_WAIT): the selection's buttons by its
  // upper row, with its text as the program copied it (AC-06).
  function onCopied(m) {
    const row = released && view.screen[released.y]?.el, text = String(m.text ?? "");
    if (m.sid !== sid() || !row?.isConnected) return;
    copied({ text, row });
  }
  term.addEventListener("contextmenu", (e) => { if (mouseOn(e)) e.preventDefault(); });
  term.addEventListener("wheel", (e) => {
    if (!wheelOn() || !e.deltaY) return;
    e.preventDefault();
    const line = parseFloat(getComputedStyle(term).lineHeight) || 16;
    lines += e.deltaMode === 1 ? e.deltaY : e.deltaMode === 2 ? e.deltaY * (view.screen.length || 24) : e.deltaY / line;
    if (Math.abs(lines) < 1) return;
    step(lines < 0 ? "wheelup" : "wheeldown", cellAt(e.clientX, e.clientY), e);
    lines %= 1;
  }, { passive: false });

  // Touch: a tap is a click, a two-finger tap a right click, a swipe the wheel; a long press
  // selects text as in the terminal and sends nothing.
  let touch = null;
  term.addEventListener("touchstart", (e) => {
    if (!wheelOn()) { touch = null; return; }
    const t = e.touches[0];
    if (e.touches.length === 1) touch = { x: t.clientX, y: t.clientY, at: Date.now(), fingers: 1, moved: false, rest: t.clientY };
    else if (touch) touch.fingers = Math.max(touch.fingers, e.touches.length);
  }, { passive: true });
  term.addEventListener("touchmove", (e) => {
    if (!touch || !wheelOn() || touch.fingers > 1) return;
    const t = e.touches[0];
    if (!touch.moved && Math.hypot(t.clientX - touch.x, t.clientY - touch.y) < 10) return;
    touch.moved = true;
    e.preventDefault();                           // the swipe is the program's: the alternate screen has no history
    const line = parseFloat(getComputedStyle(term).lineHeight) || 16;
    const at = cellAt(touch.x, touch.y);
    while (Math.abs(t.clientY - touch.rest) >= line) {
      const up = t.clientY > touch.rest;          // a finger moving down shows what is above
      step(up ? "wheelup" : "wheeldown", at, e);
      touch.rest += up ? line : -line;
    }
  }, { passive: false });
  term.addEventListener("touchend", (e) => {
    if (!touch || e.touches.length || !wheelOn()) return;
    const t = touch; touch = null;
    if (t.moved || Date.now() - t.at > 350 || !mouseOn()) return;   // a swipe, a long press (a selection), an agent's tap
    const at = cellAt(t.x, t.y), button = t.fingers > 1 ? 2 : 0;
    report("press", at, button, e);
    report("release", at, button, e);
  });

  return {
    fullscreen, mouseOn, sync, holding, reflow, onCopied,
    /** The grid that fits the terminal at the chosen text size, to send with "sub" ({} for none:
     *  hidden, or a sliver). */
    grid: () => { const g = grid(); return !term.hidden && term.clientWidth && g.cols >= SLIVER.cols && g.rows >= SLIVER.rows ? g : {}; },
    /** Another screen sized the pane: drawn as it is until this page sizes it. */
    elsewhere: () => !!elsewhere,
    /** A pane was opened: this view's size goes to terminumd. */
    opened() {
      last = null; elsewhere = null; showSized();
      holdUntil = 0; heldFor = null; clearTimeout(holdTimer);   // a hold was the pane shown before
      sync();
      sendSize(false);
    },
    /** The view or the text size changed. */
    changed,
    /** A key was typed: this page sizes the pane (AC-13); a size for another screen ends. */
    typed() {
      if (elsewhere) { elsewhere = null; showSized(); refit(); }
      if (performance.now() - typedAt < TYPED_EVERY) return;
      typedAt = performance.now();
      sendSize(true);
    },
    /** terminumd sized the pane: another screen's size (or a refused resize) is said and drawn as it is. */
    onSized(m) {
      elsewhere = m.mine && !m.error ? null : { cols: m.cols, rows: m.rows, error: m.error };
      showSized();
      refit();
    },
  };
}
