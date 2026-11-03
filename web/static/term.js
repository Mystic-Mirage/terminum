// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The terminal view: scrollback + screen lines, kept in sync with incremental server messages.
import { addBlocks, MAX_BLOCK } from "./blocks.js";
import { lineRec, palette } from "./render.js";
import { openEnd } from "./openend.js";
import { glued, inputWraps, wrapsInto } from "./rejoin.js";
import { queueHist } from "./histq.js";

const LOAD_OLDER_AT = 300;   // px from the top that triggers loading older lines
// blocks.js adds to TermView the rows' blocks and the agent's input box (addBlocks: blockCuts,
// its block methods), kept apart for the 500-line cap

// Programs such as Claude Code wrap prose themselves at iTerm's width, with hard line ends.
// Re-flowing that on a narrow screen leaves ragged half-lines. A hard end is treated as a
// wrap (re-joined when copied, AC-37) when the next line's first word would not have fit:
// that is exactly the rule the program used to break the line.
function wrappedHere(a, b, cols, boxed) {
  if (!a || !b || !a.data.e || !cols) return false;
  if (a.el.classList.contains("rule") || b.el.classList.contains("rule")) return false;
  return boxed ? inputWraps(a.txt, b.txt, cols, !!b.el.querySelector(".cur")) : wrapsInto(a.txt, b.txt, cols);
}

// A row that ends in the cursor, past its text: Claude Code's cursor on the space it wrapped at
// (measured: → from a row's last character); re-joined, the cursor's cell stands for that space.
const cursorLast = (data) => { const [t, , , f] = data.r.at(-1) ?? []; return !!(f & 256) && !(Array.isArray(t) ? t.join("") : t).trim(); };

export class TermView {
  /** keep: the most history lines on the page while it follows the output (older ones load
   *  again on scrolling up). onRows(full, keep) after the rows changed (keep: the reader's place is
   *  the widgets' to keep) and onReset (set by the page):
   *  the output widgets (widgets.js). */
  constructor({ term, hist, screen, note, bottomBtn, paused, send, keep = Infinity }) {
    Object.assign(this, { term, histEl: hist, screenEl: screen, note, bottomBtn, paused, send, keep });
    this.theme = null; this.pal = []; this.cols = 0;
    this.recs = new WeakMap();   // row element -> its record (a bookmark's line, AC-57)
    this.end = openEnd({ term, toBottom: () => this.toBottom() });   // a session just opened keeps to its end
    this.reset();
    this.touching = false;
    this.held = 0;           // hold()s not yet let go
    this.thawers = [];       // whenThawed's waiters, in the order asked
    term.addEventListener("scroll", () => this.onScroll(), { passive: true });
    bottomBtn.addEventListener("click", () => this.toBottom());
    // While a finger is down or text is selected, the page must not change under it: a
    // re-drawn line drops the selection (and iOS's long-press). Updates wait and then catch up.
    let lifting = 0;      // a lift's timer must not end the next touch
    term.addEventListener("touchstart", () => { clearTimeout(lifting); this.touching = true; }, { passive: true });
    const lift = (e) => {
      if (e.touches.length) return;                                // another finger is still down
      clearTimeout(lifting); lifting = setTimeout(() => { this.touching = false; this.resume(); }, 400);
    };
    term.addEventListener("touchend", lift, { passive: true });
    term.addEventListener("touchcancel", lift, { passive: true });
    document.addEventListener("selectionchange", () => this.resume());
  }

  reset() {
    this.hist = [];          // line records, oldest first, each with .n
    this.screen = [];        // line records
    this.oldest = 0; this.truncated = false; this.loading = false;
    this.top = 0;            // the screen's first line
    this.alt = false;        // the pane shows its alternate screen (a full-screen program)
    this.mouse = false;      // its program asked for the mouse
    this.queued = []; this.pending = null;
    this.box = this.idle = this.caret = null;    // another pane's (or a cleared screen's) input box and status lines
    this.histEl.replaceChildren(); this.screenEl.replaceChildren();
    this.note.hidden = true;
    this.onReset?.();
  }

  frozen() {
    if (this.touching || this.held) return true;
    const sel = getSelection();
    // a selection in an output widget is in its row's shadow root (AC-56); browsers report it
    // collapsed by the row, with its text (markpick.js)
    if (!sel || (sel.isCollapsed && !sel.toString())) return false;
    for (let n = sel.anchorNode; n; n = n.getRootNode().host) if (this.term.contains(n)) return true;
    return false;
  }

  resume() {
    const frozen = this.frozen();
    const show = frozen && !!(this.queued.length || this.pending);
    if (show && this.paused.hidden) this.paused.style.top = `${this.term.offsetTop + 8}px`;   // under the header, whatever its height
    this.paused.hidden = !show;
    if (frozen || this.term.hidden) return;
    const q = this.queued; this.queued = [];
    for (const m of q) this.onHist(m);
    if (this.pending) this.flush();
    const w = this.thawers; this.thawers = [];
    for (const fn of w) setTimeout(fn);      // each in a task of its own, after the page caught up
  }

  /** Frozen while held, with or without a selection: touchcopy.js takes a selection away a
   *  moment and makes it again, and a line redrawn meanwhile would lose it. */
  hold(on) {
    this.held = Math.max(0, this.held + (on ? 1 : -1));
    if (!this.held) this.resume();
  }

  /** Runs fn now, or when resume() finds the view no longer frozen. */
  whenThawed(fn) { if (this.frozen()) this.thawers.push(fn); else fn(); }

  setTheme(theme) {
    this.theme = theme; this.pal = palette(theme);
    this.keepBottom(() => {
      this.hist = this.hist.map((r) => this.swap(r, r.data, r.n));
      this.screen = this.screen.map((r) => this.swap(r, r.orig ?? r.data));   // the bridge's rows: groupScreen parks again
      this.relinkAll();
      this.onRows?.(true);
    });
  }

  // a line the bridge folded comes as null (AC-56): an empty row until the user shows it
  make(data, n) { const r = lineRec(data ?? { r: [], e: true, gone: true }, this.theme, this.pal); r.n = n; r.gone = !!r.data.gone; this.recs.set(r.el, r); return r; }
  /** The line a row element shows (a screen row's counts from the screen's first), or null. */
  lineOf(el) {
    const r = this.recs.get(el);
    if (!r) return null;
    if (r.n !== undefined) return r.n;
    const i = this.screen.indexOf(r);
    return i < 0 ? null : this.top + i;
  }
  /** The record of line n, in the history or on the screen; null when not on the page. */
  recordAt(n) {
    const base = this.hist.length ? this.hist[0].n : this.top;
    if (n >= this.top) return this.screen[n - this.top] ?? null;
    return this.hist[n - base]?.n === n ? this.hist[n - base] : null;
  }
  swap(old, data, n) { const r = this.make(data, n); old.el.replaceWith(r.el); return r; }

  /** Folded lines the bridge sent when asked (AC-56): they take the place of their empty rows. */
  refill(first, lines) {
    if (!this.hist.length) return;
    const base = this.hist[0].n;
    this.keepBottom(() => {
      lines.forEach((data, k) => {
        const i = first + k - base;
        // only into its own record: a history with a gap (an append that failed) never takes another line's
        if (i >= 0 && i < this.hist.length && this.hist[i].gone && this.hist[i].n === first + k) this.hist[i] = this.swap(this.hist[i], data, first + k);
      });
      const a = Math.max(0, first - base - 1), b = Math.min(this.hist.length, first - base + lines.length + 1);
      for (let i = a; i < b; i++) this.link(i);
      this.onRows?.(false);
    });
  }

  // Neighbors across the history/screen boundary, for re-joining.
  at(i) { return i < this.hist.length ? this.hist[i] : this.screen[i - this.hist.length]; }
  link(i) {
    const a = this.at(i), b = this.at(i + 1);
    if (!a) return;
    const join = wrappedHere(a, b, this.cols, this.boxed?.has(a.el) && this.boxed.has(b.el));
    a.el.classList.toggle("join", join);
    a.el.classList.toggle("glue", join && glued(a.txt, b.txt, this.cols));
    a.el.classList.toggle("curend", join && cursorLast(a.data));
    // a separator at least half of iTerm's width is a line across the whole view; a line pushed
    // to the right by spaces (a notice) loses them on a phone, where they would wrap it
    a.el.classList.toggle("full", a.el.classList.contains("rule") && !a.el.classList.contains("labelled") && a.txt.trim().length * 2 >= this.cols);
    a.el.classList.toggle("ralign", !!this.cols && a.txt.length >= this.cols - 2 && (a.txt.length - a.txt.trimStart().length) * 3 >= this.cols);
    if (b) b.el.classList.toggle("cont", join);
  }
  relinkAll() {
    const n = this.hist.length + this.screen.length;
    for (let i = 0; i < n; i++) this.link(i);
    const all = new DocumentFragment();                            // paragraphs may end elsewhere now
    for (const r of this.hist) all.append(r.el);                   // (no spread: 10000s of arguments)
    this.histEl.replaceChildren(all);
    this.pack();
    this.groupScreen();
  }


  /** Typing: for 3 s, and at once, keep the cursor's row in sight (an agent's input box
   *  stands above its footer, so the screen's last row is not where the user types). */
  typed() { this.followUntil = performance.now() + 3000; this.showCursor(); }
  showCursor() {
    const cur = this.screenEl.querySelector(".cur");
    if (!cur) return this.toBottom();
    const t = this.term.getBoundingClientRect(), r = cur.getBoundingClientRect();
    const margin = 2 * (r.height || 16);
    if (r.bottom > t.bottom - margin || r.top < t.top) this.term.scrollTop += r.bottom - (t.bottom - margin);
    this.updateBottom();
  }

  atBottom() { return this.end.pinned() || this.term.scrollHeight - this.term.scrollTop - this.term.clientHeight < 8; }
  toBottom() { this.term.scrollTop = this.term.scrollHeight; this.updateBottom(); }
  updateBottom() { this.bottomBtn.hidden = this.atBottom(); }
  // sticking: fn runs before the view goes to the end (the widgets measure from there)
  keepBottom(fn) {
    const stick = this.atBottom();
    this.sticking = stick;
    try { fn(); } finally { this.sticking = false; }
    // at the end it stays there: the button hides without measuring the page a second time
    if (stick) { this.term.scrollTop = this.term.scrollHeight; this.bottomBtn.hidden = true; } else this.updateBottom();
  }

  onHist(m) {
    if (!this.theme) return;
    // frozen, or behind Files or View on a phone: drawn when the terminal shows again (resume)
    if ((this.frozen() || this.term.hidden) && m.mode !== "reset") { this.queued = queueHist(this.queued, m); this.resume(); return; }
    const rows = m.lines.map((data, i) => this.make(data, m.first + i));
    this.oldest = m.oldest;
    this.truncated = m.truncated;
    if (m.mode !== "prepend") this.top = m.first + m.lines.length;
    if (m.mode === "reset") {
      this.hist = rows;
      this.keepBottom(() => { this.relinkAll(); this.onRows?.(true); });   // it puts the lines in place
    } else if (m.mode === "append") {
      const known = this.hist.length ? this.hist.at(-1).n : -1;
      const fresh = rows.filter((r) => r.n > known);
      const from = this.hist.length - 1;
      this.hist.push(...fresh);
      // the cap applies while the user follows the output: not while an older page is on its
      // way (it would leave a gap), nor behind Files or File (a hidden view is "at the bottom")
      const follow = !this.loading && !this.holding && !this.term.hidden && this.atBottom();   // holding: a bookmark's lines (AC-57)
      this.keepBottom(() => {
        this.histEl.append(...fresh.map((r) => r.el));
        for (let i = Math.max(0, from); i <= this.hist.length; i++) this.link(i);
        this.trim(follow);
        this.pack();
        this.groupScreen();
        this.onRows?.(false);
      });
    } else {                                        // prepend older lines, keep the view still
      const first = this.hist.length ? this.hist[0].n : Infinity;
      const older = rows.filter((r) => r.n < first);
      this.loading = false;
      if (older.length && first !== Infinity && older.at(-1).n !== first - 1) return;   // a gap: scrolling asks again
      const h = this.term.scrollHeight;
      this.hist.unshift(...older);
      this.histEl.prepend(...older.map((r) => r.el));
      for (let i = 0; i <= older.length; i++) this.link(i);
      const last = this.hist.length === older.length ? 1 : 0;         // the last line stays loose (pack)
      const rest = older.length > last ? this.wrapInBlocks(older.slice(0, older.length - last)) : [];
      const next = rest.at(-1)?.el.nextElementSibling;              // their paragraph goes on there
      if (next?.classList.contains("blk")) {
        if (rest.length + next.childElementCount <= MAX_BLOCK) next.prepend(...rest.map((r) => r.el));
        else this.block(rest);                                      // cut, as a longer paragraph is
      }
      this.term.scrollTop += this.term.scrollHeight - h;
      this.onRows?.(true, true);                                    // measured from the place kept
    }
    this.showNote();
  }

  // Drop lines older than the server's limit, so a long-running session stays light; while
  // following the output, also whole blocks beyond `keep` lines.
  trim(follow) {
    let drop = 0;
    while (drop < this.hist.length && this.hist[drop].n < this.oldest) drop++;
    if (drop) {
      for (let i = 0; i < drop; i++) this.hist[i].el.remove();
      this.hist = this.hist.slice(drop);
    }
    let cut = 0;
    for (let b = this.histEl.firstElementChild; b?.classList.contains("blk"); b = this.histEl.firstElementChild) {
      const n = b.childElementCount;
      if (n && !(follow && this.hist.length - cut - n >= this.keep)) break;
      b.remove();
      cut += n;
    }
    if (cut) this.hist = this.hist.slice(cut);
  }

  showNote() {
    const all = !this.hist.length || this.hist[0].n <= this.oldest;
    this.note.hidden = !(all && this.truncated);
  }

  onScreen(m) {
    // Coalesce bursts: apply the latest state once per animation frame.
    if (!this.pending || m.full) this.pending = { full: m.full, n: m.n, ch: new Map() };
    this.pending.n = m.n;
    if (m.cols !== this.cols) { this.cols = m.cols; this.onCols?.(); }
    if (m.rows) this.rows = m.rows;                 // the pane's rows (ask.js: what an agent's box may take)
    if ("mouse" in m) this.mouse = !!m.mouse;     // the program asked for mouse events (AC-06)
    if (!!m.alt !== !!this.alt) { this.alt = !!m.alt; this.onAlt?.(); }   // a full-screen program (mc, vim) began or ended
    for (const [i, data] of m.ch) this.pending.ch.set(i, data);
    if (!this.raf) this.raf = requestAnimationFrame(() => this.flush());
  }

  flush() {
    this.raf = null;
    if (this.term.hidden) return;                  // kept in pending: resume() draws it when shown
    if (this.frozen()) { this.resume(); return; }
    const p = this.pending; this.pending = null;
    if (!p || !this.theme) return;
    this.keepBottom(() => {
      if (p.full) { this.screen = []; this.screenEl.replaceChildren(); }
      while (this.screen.length > p.n) this.screen.pop().el.remove();
      const changed = [...p.ch].sort((a, b) => a[0] - b[0]);
      for (const [i, data] of changed) {
        if (this.screen[i]) this.screen[i] = this.swap(this.screen[i], data);
        else { this.screen[i] = this.make(data); this.screenEl.append(this.screen[i].el); }
      }
      if (p.full) this.relinkAll();
      else {
        for (const [i] of changed) { this.link(this.hist.length + i - 1); this.link(this.hist.length + i); }
        this.groupScreen();
      }
      this.onRows?.(false);
    });
    this.end.drawn();
    if (performance.now() < (this.followUntil ?? 0)) this.showCursor();
    this.onDrawn?.();
  }

  onScroll() {
    this.updateBottom();
    const first = this.hist.length ? this.hist[0].n : null;
    if (this.term.scrollTop < LOAD_OLDER_AT && !this.loading && first !== null && first > this.oldest) {
      this.loading = true;
      this.send({ t: "more", before: first });
    }
  }

  /** The text area's inner size in px. */
  inner() {
    const cs = getComputedStyle(this.term);
    return { cs, w: this.term.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight),
      h: this.term.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom) };
  }

  /** One cell's advance per px of font size, for the terminal font now loaded. */
  charRatio() {
    if (this.ratio) return this.ratio;
    const probe = document.createElement("span");
    probe.textContent = "0".repeat(100);
    probe.style.cssText = "position:absolute;visibility:hidden;white-space:pre;font-size:100px";
    this.term.append(probe);
    this.ratio = probe.getBoundingClientRect().width / 100 / 100;
    probe.remove();
    return this.ratio;
  }

  /** The text size chosen (the page's --size); a pane wider than the view is drawn smaller (app.js). */
  chosenSize() { return parseFloat(getComputedStyle(this.term).getPropertyValue("--size")); }
  /** True when tmux's rows are wider than the visible area at the chosen size. */
  tooWide() { return this.cols * this.charRatio() * this.chosenSize() > this.inner().w; }
  /** The text size that fits the pane's columns in the view's width, at most a little over the chosen size (AC-04). */
  fitPx() { return Math.min(Math.floor(((this.inner().w - 1) / this.cols / this.charRatio()) * 10) / 10, this.chosenSize() + 4); }

  /** Columns and rows that fit the visible area at font size px (the current one by default). */
  gridFit(px = null) {
    const { cs, w, h } = this.inner(), now = parseFloat(cs.fontSize), size = px || now;
    const line = (parseFloat(cs.lineHeight) || now * 1.25) / now * size;
    return { cols: Math.floor(w / (this.charRatio() * size)), rows: Math.floor(h / line) };
  }
}

addBlocks(TermView);
export { blockCuts } from "./blocks.js";
