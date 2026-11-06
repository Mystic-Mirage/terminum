// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The output widgets of the terminal view (AC-56): code, Markdown and Mermaid regions of the
// output (outparse.js) drawn as widgets. A widget is a shadow root on its region's first row
// (widgetui.js); in its rendered form the region's other rows are hidden (wg-hid), in Raw they
// show as before. Nothing is added to the
// history's or the screen's children: term.js counts them as rows and blocks.
import { Folds } from "./folds.js";
import { insideBox, tableRule } from "./drawntable.js";
import { nearRows } from "./linkmarks.js";
import { MAX_LINES, scan, TABLE_SEP } from "./outparse.js";
import { hostOf, loadStyles, regionWidget, zoomDiagram } from "./widgetui.js";

const STEADY = 150;          // ms a region on the screen stays as it is before it is rendered
const OPEN_TABLE = 500;      // the same, a table that may go on (a Markdown table at the end of the output)
const CACHED = 150;          // rendered widgets kept, the least recently shown go first
const THROTTLE = 100;        // ms between scans of streaming output
const NEAR = 2;              // screens above and below the view whose history regions are drawn

// FNV-1a: a region is known by its kind, language and text
function hash(s) {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193);
  return (h >>> 0).toString(36) + s.length.toString(36);
}

// A row as outparse.js reads it: its text (a wrapped row keeps its spaces) and whether the
// program colored it.
function info(r) {
  if (!r) return { text: "", eol: true, styled: false };
  return (r.wgInfo ??= {
    text: r.data.e ? r.txt : r.el.textContent.replace(/[︎️]/g, ""),
    eol: !!r.data.e,
    styled: r.data.r.some(([, fg, bg]) => (fg != null && fg !== "R") || (bg != null && bg !== "R")),
  });
}

// a dark background ("#rrggbb" or "rgb(r, g, b)") gets Mermaid's dark theme
function dark(bg = "") {
  const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(bg), rgb = /(\d+)\D+(\d+)\D+(\d+)/.exec(bg);
  const [r, g, b] = hex ? hex.slice(1).map((v) => parseInt(v, 16)) : rgb ? rgb.slice(1).map(Number) : [0, 0, 0];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b < 128;
}

export class Widgets {
  /** view: the TermView; boxes: #hist and #screen; copy(text); send(msg) to the bridge (folded lines); notify(text); onZoomClosed(): the viewer closed. */
  constructor({ view, term, boxes, copy, send, notify, onZoomClosed }) {
    Object.assign(this, { view, term, boxes, copy, notify, onZoomClosed });
    this.modes = new Map();      // region key -> "raw" or "on", as the user chose
    this.cache = new Map();      // region key + theme -> its widget element
    this.twins = new Map();      // the same, for a second copy of a region
    this.sources = new WeakMap(); // widget -> its region's source text (for Copy)
    this.seen = new Map();       // key of a screen region -> when it was first seen as it is
    this.lastScan = 0;
    this.styled = false;         // widgets wait for their style sheet
    this.reset();
    this.click = (b, root) => this.onClick(b, root);
    this.folds = new Folds({ view, click: this.click, send, copy, notify });
    loadStyles().then(() => { this.styled = true; this.keepPlace(() => this.update(true)); });
    // scrolled, or shown again (Files, View), or another size: regions come near the view
    let timer = 0;
    const near = () => { clearTimeout(timer); timer = setTimeout(() => { if (this.far) this.keepPlace(() => this.update(false, true)); }, 150); };
    // following the output: as the user left it (a size change or a widget growing is not a scroll away)
    this.following = true;
    term.addEventListener("scroll", () => { this.following = view.atBottom(); near(); }, { passive: true });
    // the terminal's size changes with the window, the keyboard, Files docked or not; shown
    // again (it was 0 high behind Files or View), the regions in sight are drawn at once
    let shownH = 0;
    new ResizeObserver(() => {
      const h = term.clientHeight, back = !shownH && h > 0;
      shownH = h;
      this.keepPlace(() => { this.sizes(); if (back) this.update(false, true); });
      near();
    }).observe(term);
  }

  /** Another session: nothing of the last one stays attached. */
  reset() {
    this.final = [];             // history regions that no longer change: {rows, key, region}
    this.settledN = null;        // the history line the next scan starts at (null: its first)
    this.hosts = new Map();      // a region's first row -> {key, raw, rows, live}: what is attached
    this.held = new Set();       // rows of a table not whole yet (holdOpen)
    this.heldSince = 0;
    this.folds?.reset();
    this.pin = null;             // a bookmark held in view (AC-57) was of the last pane
    clearTimeout(this.steady); clearTimeout(this.later);
  }

  setTheme(theme) {
    this.dark = dark(theme.bg);
    const pal = theme.ansi ?? [];
    const s = this.term.style;
    // the token colors of the panel's highlighter, from the profile's ANSI colors
    [["kw", 13], ["str", 10], ["num", 11], ["fn", 12], ["type", 14], ["tag", 9]].forEach(([k, i]) => { if (pal[i]) s.setProperty(`--tk-${k}`, pal[i]); });
  }

  /** The tallest a diagram in the history is: half the terminal's height. */
  sizes() {
    const t = this.term, h = `${Math.round(t.clientHeight / 2)}px`;
    if (t.clientHeight && t.style.getPropertyValue("--wg-dh") !== h) t.style.setProperty("--wg-dh", h);
  }

  /** After the history or the screen changed (term.js, inside its keepBottom). full: the rows
   *  were made again or older ones came first: scan all of the history again. Streaming output
   *  is scanned at most every THROTTLE ms, unless a widget lost one of its rows (the screen drew
   *  them again), which is put right at once, a table's rule is among the rows looked at (a table
   *  is a widget in the frame it is whole), or `now` (the user's own change). */
  update(full = false, now = false) {
    const v = this.view;
    if (!v.theme) return;
    this.folds.apply();                           // at once: a folded line not sent is an empty row
    if (!this.styled) return;
    clearTimeout(this.later);
    const hist = v.hist, screen = v.screen;
    if (full) { this.final = []; this.settledN = null; }
    const base = hist.length ? hist[0].n : 0;
    let i0 = this.settledN === null ? 0 : Math.min(hist.length, Math.max(0, this.settledN - base));
    // never inside a drawn box: a scan that met the screen behind the history settled further than it thought (AC-31)
    const textAt = (i) => info(i < hist.length ? hist[i] : screen[i - hist.length]).text;
    for (let k = 0; k < MAX_LINES && i0 > 0 && insideBox(textAt(i0), textAt(i0 - 1)); k++) i0--;
    const rows = hist.slice(i0).concat(screen);
    const histPart = hist.length - i0;
    // the output ends at the cursor's row; in a coding agent's pane at its input box's top rule
    // (what is typed there is not output, and changes with every key)
    let end = screen.findIndex((r) => r?.el.querySelector(".cur"));
    const boxTop = v.agent && v.box ? screen.filter(Boolean)[v.box.top]?.el : null;
    const boxAt = boxTop ? screen.findIndex((r) => r?.el === boxTop) : -1;
    if (boxAt >= 0 && (end < 0 || boxAt < end)) end = boxAt;
    // an input taller than the screen begins in the history: the output ends there
    const stop = v.agent && v.box?.top < 0 ? Math.max(0, histPart + v.box.top) : histPart + (end >= 0 ? end : screen.length);
    const wait = this.lastScan + THROTTLE - performance.now();
    const torn = [...this.hosts.values()].some((h) => h.live && h.rows.some((el) => !el.isConnected));
    let rules = 0;                                 // a row's record is new when it changes: each is read once
    for (let i = 0; i < stop; i++) if (rows[i] && (rows[i].wgRule ??= tableRule(info(rows[i]).text) || TABLE_SEP.test(info(rows[i]).text))) rules++;
    const table = wait > 0 && (rules !== this.rules || this.tableWaits != null);
    this.rules = rules;
    if (!full && !now && wait > 0 && !torn && !table) { this.later = setTimeout(() => this.keepPlace(() => this.update()), wait); return; }
    this.lastScan = performance.now();
    // regions wholly in the history before `settled` are final; the screen changes
    const { found, settled, waiting } = scan(rows.map(info), stop, histPart);
    this.tableWaits = waiting;
    let cut = settled;
    for (const r of [...found].reverse()) if (r.to >= cut && r.from < cut) cut = r.from;
    const at = performance.now(), live = [];
    for (const r of found) {
      const key = `${r.kind}:${r.lang}:${hash(r.text)}`, part = rows.slice(r.from, r.to + 1);
      if (r.to < cut) { if (!(part[0].n <= this.final.at(-1)?.rows.at(-1).n)) this.final.push({ rows: part, key, region: r }); }
      else {
        live.push({ rows: part, key, region: r, live: true });
        if (!this.seen.has(key)) this.seen.set(key, at);
      }
    }
    this.settledN = i0 + cut < hist.length ? hist[i0 + cut].n : hist.length ? hist.at(-1).n + 1 : null;
    for (const k of this.seen.keys()) if (!live.some((l) => l.key === k)) this.seen.delete(k);
    // a region whose first line was dropped (trim) is no longer one: its other rows show again
    this.final = this.final.filter((f) => f.rows[0].n >= base);
    // a table that cannot go on is whole: at once, no frame shows its lines; one out of sight meanwhile (`waiting`) no longer than that
    const ready = (l) => (l.region.kind === "table" && !l.region.table.open) || this.cache.has(this.cacheKey(l.key));
    const due = (r) => (r.table?.open && r.from !== waiting ? OPEN_TABLE : STEADY);
    const holding = this.holdOpen(waiting === null ? [] : rows.slice(waiting, stop), at);
    this.apply([...this.final, ...live.filter((l) => at - this.seen.get(l.key) >= due(l.region) || ready(l))]);
    clearTimeout(this.steady);
    if (holding || live.some((l) => !ready(l))) this.steady = setTimeout(() => this.keepPlace(() => this.update(false, true)), STEADY);
  }

  /** A table's rows while its last rule is on its way (a frame caught midway, a slow link) stay out
   *  of sight, at most STEADY ms from the first; nothing is lost: after that they show as text.
   *  True while it holds some. */
  holdOpen(rows, at) {
    if (!rows.length) this.heldSince = 0;
    else this.heldSince ||= at;
    const els = at - this.heldSince < STEADY ? rows.filter(Boolean).map((r) => r.el).filter((el) => !this.folds.has(el)) : [];
    const hide = new Set(els);
    for (const el of this.held) if (!hide.has(el)) el.classList.remove("wg-hid");
    for (const el of hide) el.classList.add("wg-hid");
    this.held = hide;
    return hide.size > 0;
  }

  cacheKey(key) { return `${key}:${this.dark ? "d" : "l"}`; }
  // rendered unless the user chose Raw; a diagram guessed from text without a fence starts in Raw
  raw(key, region) { return (this.modes.get(key) ?? (region.guessed ? "raw" : "on")) === "raw"; }

  // The widgets attached to their rows; rows no longer in a region show again. A history
  // region attached as it should be is passed over (its rows stay as they are); a screen row
  // may have been drawn again, so a screen region's rows are all looked at. A history region
  // far from the view (NEAR screens) stays text until it comes near: a long history is not
  // drawn all at once; but for a table, little to draw, and drawn before the reader comes to it.
  apply(list) {
    const hosts = new Map();
    const t = this.term.getBoundingClientRect(), lo = t.top - NEAR * t.height, hi = t.bottom + NEAR * t.height;
    // following the output, the view goes to the end after this: measured from there
    const ahead = this.view.sticking ? this.term.scrollHeight - this.term.clientHeight - this.term.scrollTop : 0;
    const near = (els) => !this.term.hidden && t.height > 0 && els[0].getBoundingClientRect().top - ahead < hi
      && els.at(-1).getBoundingClientRect().bottom - ahead > lo;
    this.far = false;
    const show = (els, keep = new Set()) => { for (const el of els) if (!keep.has(el)) el.classList.remove("wg-hid"); };
    // a row in the next list stays hidden: a table moved by the lines of a repeated row has that row in both
    let staying = null;                            // built when one goes: most scans detach nothing
    const detach = (anchor, was) => {
      staying ??= new Set(list.filter((l) => !away.has(l) && !this.raw(l.key, l.region)).flatMap((l) => l.rows.slice(1).map((r) => r.el)));
      show(was.rows, staying);
      anchor.classList.remove("wg-host", "wg-raw");
      anchor.shadowRoot?.replaceChildren(document.createElement("slot"));
    };
    // rendered output is never folded: a fold over a widget's rows gives way (folds.js giveWay); a
    // part the bridge folded and did not send stays the fold's (no text to draw it from)
    const drawn = new Set(list.flatMap(({ rows }) => (rows.every(Boolean) ? rows.map((r) => r.el) : [])));
    // a fold that gave way showed all its rows: the regions under it hide theirs again
    const gave = this.folds.giveWay(drawn);
    if (gave) this.folds.apply();
    list = list.filter(({ rows }) => rows.length && !rows.some((r) => !r || this.folds.has(r.el)));
    // positions first, then changes: a read after a change would lay out the whole history again
    const away = new Set(list.filter((l) => !l.live && l.region.kind !== "table" && !this.hosts.has(l.rows[0].el) && !near(l.rows.map((r) => r.el))));
    for (const item of list) {
      const { rows, key, region, live } = item;
      const anchor = rows[0].el, raw = this.raw(key, region), els = rows.map((r) => r.el);
      const was = this.hosts.get(anchor);
      if (away.has(item)) { this.far = true; continue; }
      hosts.set(anchor, { key, raw, rows: els, live });
      const root = hostOf(anchor, this.click);
      let mine = root.firstElementChild;
      if (!gave && !live && was?.key === key && was.raw === raw && was.rows.length === els.length && was.rows.at(-1) === els.at(-1)
        && mine?.dataset.key === key) continue;
      if (was && was.key !== key) detach(anchor, was);
      else if (was) show(was.rows, new Set(els));                 // rows the region no longer has
      if (mine?.dataset.key !== key) {
        mine = this.widget(key, region);
        const used = mine.getRootNode().host;
        if (used && used !== anchor && hosts.has(used)) mine = this.twin(key, region, hosts);   // printed twice: one each
        root.replaceChildren(mine, document.createElement("slot"));
      }
      if (!raw) mine.draw();
      anchor.classList.remove("wg-hid");            // it may have been a row of this table where it was
      anchor.classList.add("wg-host");
      anchor.classList.toggle("wg-raw", raw);
      for (const el of els.slice(1)) el.classList.toggle("wg-hid", !raw);
    }
    for (const [anchor, was] of this.hosts) if (!hosts.has(anchor)) detach(anchor, was);
    this.hosts = hosts;
  }

  build(key, region) {
    const wg = regionWidget(region, { key, dark: this.dark, change: (fn) => this.keepPlace(fn, wg.getRootNode().host ?? null, false),
      raw: () => { this.modes.set(key, "raw"); this.keepPlace(() => this.update(false, true)); } });
    this.sources.set(wg, region.text);
    return wg;
  }

  widget(key, region) {
    const ck = this.cacheKey(key);
    let wg = this.cache.get(ck);
    if (wg) { this.cache.delete(ck); this.cache.set(ck, wg); return wg; }
    this.cache.set(ck, wg = this.build(key, region));
    for (const k of [...this.cache.keys()].slice(0, Math.max(0, this.cache.size - CACHED))) this.cache.delete(k);
    return wg;
  }

  /** The second copy of a region printed twice: the same one while it is redrawn, not one each time. */
  twin(key, region, hosts) {
    const ck = this.cacheKey(key);
    let wg = this.twins.get(ck);
    if (wg && !hosts.has(wg.getRootNode().host)) return wg;
    this.twins.set(ck, wg = this.build(key, region));
    for (const k of [...this.twins.keys()].slice(0, Math.max(0, this.twins.size - CACHED))) this.twins.delete(k);
    return wg;
  }

  /** Runs fn, a change to the page, keeping the reader's place: `row` where it is on screen
   *  (the row the user acted on: Raw, Markdown; `user` false: a widget
   *  that changed by itself, kept unless the user follows the output); else the end while
   *  following the output, else the first row wholly in sight. Not while text is selected or a
   *  finger is down: the page must not change under them. */
  keepPlace(fn, row = null, user = true) {
    if (this.view.frozen()) { this.view.whenThawed(() => this.keepPlace(fn, row, user)); return; }
    if (this.pin) return this.pinned(fn);
    const t = this.term.getBoundingClientRect(), r = row?.isConnected ? row.getBoundingClientRect() : null;
    let at = r && r.bottom > t.top && r.top < t.bottom ? row : null;             // only a row in sight
    if ((!at || !user) && (this.following || this.view.atBottom())) {
      fn();
      this.view.toBottom();                                      // following the output: its end stays in sight
      this.following = true;
      return;
    }
    // a row cut by the top edge may grow inside: the first one wholly in sight
    at ??= nearRows(this.term, this.boxes).find((el) => el.getBoundingClientRect().top >= t.top);
    const before = at?.getBoundingClientRect().top;
    fn();
    if (at?.isConnected && before !== undefined) this.term.scrollTop += at.getBoundingClientRect().top - before;
    this.view.updateBottom();
  }

  /** A bookmark brought into view (AC-57) stays where it is on screen, whatever changes around
   *  it, until the user scrolls: pin() gives its place again (a Range or an element), which
   *  may be in a widget drawn meanwhile. */
  pinned(fn) {
    const top = (t) => (t && (t.startContainer?.isConnected ?? t.isConnected) ? t.getBoundingClientRect().top : undefined);
    const before = top(this.pin());
    if (before === undefined) { this.pin = null; return this.keepPlace(fn); }   // its line is gone: the view is free
    fn();
    const after = top(this.pin());
    if (before !== undefined && after !== undefined) this.term.scrollTop += after - before;
    this.view.updateBottom();
  }

  onClick(b, root) {
    const act = b.dataset.act, wg = b.closest(".wg"), key = wg?.dataset.key;
    if (act === "raw" || act === "on") {
      this.modes.set(key, act);
      if (act === "on" && wg.dataset.failed !== undefined) {
        // it could not be drawn: made again, so it tries again (fbd may be back)
        for (const [k, w] of this.cache) if (w === wg) this.cache.delete(k);
        root.replaceChildren(document.createElement("slot"));
      }
      return this.keepPlace(() => this.update(false, true), root.host);
    }
    if (act === "copy") return this.copy(this.sources.get(wg) ?? "");
    if (act === "png") return wg.copyPng(this.notify);
    if (act === "fold") return this.keepPlace(() => this.folds.toggle(root.host), root.host);
    if (act === "foldcopy") return this.folds.copyOf(root.host);
    if (act === "zoom") return zoomDiagram(b, this.onZoomClosed);
  }
}
