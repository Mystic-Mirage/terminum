// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Going through a session's bookmarks and reminders (AC-33): the bookmarks' button's pane is a
// navigator, the list on top (the session's own top to bottom, its project's others' after, newest
// first; each its title and its text's start on one line), the chosen one's content under it past
// a line dragged to share the height (kept in this browser), rendered as the terminal renders Markdown
// (an agent's record, AC-98, its own Markdown; the user's, the terminal's text as it was). A press
// or ↑ ↓ chooses one; Show in terminal (Enter) brings it into view, Quote in chat pastes it, Resolve
// (or Delete, ⌫) settles it at once in the list, a reminder resolved and a bookmark deleted, with
// Undo for 5 s: only then terminumd is told. The bar after a jump has the same Resolve. The
// reminders' button's pane is the same navigator of the open reminders only, those up first (AC-91).
import { icon } from "./icons.js";
import { chatText, el, INSERT, openReminder, when } from "./marks.js";
import { oneLine, titleOf } from "./marktext.js";
import { agentMark } from "./nav.js";
import { markdownView } from "./widgetui.js";

const $ = (id) => document.getElementById(id);
const UNDO = 5000;             // ms a ✓ waits before terminumd is told
const SHARE = 0.5, LEAST = 0.15;   // the list's most share of the height at first, and the least it can be
const MOUSE = globalThis.matchMedia?.("(hover: hover) and (pointer: fine)");   // none in node (unit tests)

/** A record's content as Markdown: an agent's as it wrote it, the user's selection in a fence (the
 *  terminal's text exactly), longer than any run of backticks in it. */
export function recordMarkdown(item) {
  if (item.by) return item.text;
  const fence = "`".repeat(Math.max(3, ...[...item.text.matchAll(/`+/g)].map((m) => m[0].length + 1)));
  return `${fence}text\n${item.text}\n${fence}`;
}

/** The mark before an entry's title: an agent's for its record, a bell for a reminder, none else. */
export function kindMark(item) {
  if (item.by) return agentMark({ agent: item.by }, 13);
  if (item.remind) { const s = el("span", "mkbell"); s.append(icon("bell", 12)); return s; }
  return null;
}

/** A Resolve key as its item's state says: Undo while pending, else ✓ Resolve. */
function resolveKey(b, pending) {
  b.replaceChildren(...(pending ? ["Undo"] : [icon("check", 14), "Resolve"]));
  b.classList.toggle("pending", pending);
}

export class MarkNav {
  /** marks: marks.js's Marks (here, sid, jump, send, togglePane, shown, hideBar). Its options for
   *  another pane of the kind (the reminders'): pane, its id; what, its title's words; close(); only
   *  and order, which entries and how; peer, the bookmarks' navigator, whose Undo it shares. */
  constructor(marks, { pane = "markpane", what = "Bookmarks and reminders", close = () => marks.togglePane(false), only = null, order = null, peer = null } = {}) {
    this.m = marks;
    Object.assign(this, { what, close, only, order });
    this.pane = $(pane);
    this.pending = peer?.pending ?? new Map();    // id -> timer: settled in the list, terminumd told when it fires
    this.sends = peer?.sends ?? new Map();        // id -> what terminumd is told when its Undo runs out
    this.navs = peer?.navs ?? new Set();          // the navigators sharing them: drawn again with a change of it
    this.navs.add(this);
    this.chosen = null;            // the entry the pane is on
    this.drawn = "";               // what the view shows: its entry's id and text
    this.view = this.makeView();
    this.makeFrame();
    this.pane.tabIndex = -1;
    this.pane.addEventListener("keydown", (e) => this.key(e));
    if (peer) return;
    $("mbdone").onclick = () => { const s = this.m.shown; if (s) (this.pending.has(s.id) ? this.undo(s.id) : this.done(s)); };
    // the page closing or reloading within the Undo's 5 s: what was done is told at once, never lost
    addEventListener("pagehide", () => { for (const [id, msg] of this.sends) { clearTimeout(this.pending.get(id)); this.m.send(msg); } this.sends.clear(); });
  }

  /** The session's bookmarks and open reminders: its own by line (an agent's record not found yet
   *  by its time after them), then its project's others' newest first. */
  entries() {
    const m = this.m, own = (b) => b.sid === m.sid();
    const here = m.here().filter((b) => (!b.remind || openReminder(b) || this.pending.has(b.id)) && (!this.only || this.only(b)));
    if (this.order) return here.sort(this.order);
    const byLine = (a, b) => Number(a.n < 0) - Number(b.n < 0) || (a.n < 0 ? a.at - b.at : a.n - b.n);
    return [...here.filter(own).sort(byLine), ...here.filter((b) => !own(b)).sort((a, b) => b.at - a.at)];
  }

  draw() {
    const pane = this.pane;
    if (pane.hidden) return;
    const all = this.entries(), project = this.m.hereProject();
    if (!all.some((b) => b.id === this.chosen)) this.chosen = all[0]?.id ?? null;   // something is shown at once
    const others = all.some((b) => b.sid !== this.m.sid());
    const { head, heading, empty, list, cols, hint } = this.frame;
    heading.textContent = `${this.what} of ${project ? project.name : "this session"}`;
    // the parts never leave the pane: the list's scroll and a drag of the line outlive a redraw
    if (!head.isConnected) pane.replaceChildren(head, empty, cols, hint);
    empty.hidden = all.length > 0;
    cols.hidden = hint.hidden = !all.length;
    if (!all.length) return void (this.listed = "");
    // drawn again only when what it shows changed (the layout comes often while agents work: a scroll and a press kept)
    // (the records' own changes counted by Marks.changed, so their texts are not compared)
    const key = JSON.stringify([heading.textContent, this.chosen, others, this.m.rev, all.map((b) => [b.id, this.pending.has(b.id), this.m.alive(b.sid)])]);
    if (key === this.listed) return;
    this.listed = key;
    list.replaceChildren(...all.map((item) => this.row(item, others)));
    this.show(all.find((b) => b.id === this.chosen), others);
    list.querySelector(".chosen")?.scrollIntoView({ block: "nearest" });
  }

  /** The pane's parts made once: a head as View's (panewin.css: the title, × at the right), the list
   *  (as tall as its rows, the share at most), the line between it and the view (dragged, or ↑ ↓
   *  while focused; a double click: half), the keys' hint. */
  makeFrame() {
    const head = el("div", "pwhead"), heading = el("h2", "pwtitle"), close = el("button", "icon");
    Object.assign(close, { type: "button", title: "Close (Esc)", onclick: () => this.close() });
    close.setAttribute("aria-label", "Close");
    close.append(icon("x", 16));
    head.append(heading, close);
    const list = el("ul", "mknav"), grip = el("div", "mkgrip"), cols = el("div", "mkcols");
    Object.assign(grip, { tabIndex: 0, title: "Drag to share the height between the list and the content" });
    for (const [k, v] of [["role", "separator"], ["aria-orientation", "horizontal"], ["aria-label", "Share of the list"],
      ["aria-valuemin", String(LEAST * 100)], ["aria-valuemax", String((1 - LEAST) * 100)]]) grip.setAttribute(k, v);
    cols.append(list, grip, this.view.box);
    this.frame = { head, heading, close, empty: el("p", "empty pwbody", "None here."), list, grip, cols,
      hint: el("p", "mkhint", "↑ ↓ choose · Enter: show in terminal · Delete: resolve · Esc: close") };
    const kept = Number(this.m.store?.get("markShare", SHARE));
    this.setShare(Number.isFinite(kept) ? kept : SHARE, false);
    // shares of the height (as the CSS percent reads it); from where the list ends (shorter than its share when its rows are few)
    const room = () => Math.max(1, cols.clientHeight), now = () => list.offsetHeight / room();
    grip.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      e.preventDefault();                          // the pane keeps the keys
      grip.setPointerCapture(e.pointerId);
      cols.classList.add("dragging");
      const y0 = e.clientY, from = list.offsetHeight;
      let moved = false;                           // a press alone keeps the share (a double click's first press)
      const at = (ev) => (from + ev.clientY - y0) / room();
      const move = (ev) => { moved ||= Math.abs(ev.clientY - y0) > 2; if (moved) this.setShare(at(ev), false); };
      const up = (ev) => { if (moved) this.setShare(at(ev), true); };
      const end = () => {                          // also when the capture is lost (the pane closed meanwhile)
        cols.classList.remove("dragging");
        for (const [t, f] of [["pointermove", move], ["pointerup", up], ["lostpointercapture", end]]) grip.removeEventListener(t, f);
      };
      grip.addEventListener("pointermove", move);
      grip.addEventListener("pointerup", up);
      grip.addEventListener("lostpointercapture", end);
    });
    grip.addEventListener("dblclick", () => this.setShare(SHARE, true));
    grip.addEventListener("keydown", (e) => {
      if (e.key === "Escape" || e.key === "Tab") return;
      e.stopPropagation();                         // not the list's keys (↑ ↓, Enter, Delete)
      const step = { ArrowUp: -0.05, ArrowDown: 0.05 }[e.key];
      if (!step) return;
      e.preventDefault();
      this.setShare(now() + step, true);
    });
  }

  /** The list's share `f` of the height at most (LEAST…1−LEAST; the view keeps 200 px, marks.css),
   *  kept in this browser when `save`. */
  setShare(f, save) {
    this.share = Math.min(1 - LEAST, Math.max(LEAST, f));
    const { cols, grip } = this.frame;
    cols.style.setProperty("--list", `${(this.share * 100).toFixed(1)}%`);
    grip.setAttribute("aria-valuenow", String(Math.round(this.share * 100)));
    if (save) this.m.store?.set("markShare", this.share);
  }

  /** The view at the right: its head (title, where and when), its content, three actions. */
  makeView() {
    const box = el("section", "mkview"), head = el("div", "mkvhead"), body = el("div", "mkvbody"), acts = el("div", "pwacts mkvacts");   // the shared window's actions (panewin.css)
    const act = (cls, label, fn, title) => { const b = el("button", cls, label); b.type = "button"; b.title = title; b.onclick = fn; acts.append(b); return b; };
    const item = () => this.entries().find((b) => b.id === this.chosen);
    act("mkvgo", "Show in terminal", () => { const it = item(); if (it) { this.close(); void this.m.jump(it); } }, "Bring it into view in the terminal (Enter)");
    const done = act("mkvdone", "Resolve", () => { const it = item(); if (it) (this.pending.has(it.id) ? this.undo(it.id) : this.done(it)); }, "");
    act("tochat", INSERT, () => { const it = item(); if (it) { this.close(); this.m.toChat(chatText(it)); } }, "Paste its note and its text quoted into the input, no Enter");
    box.append(head, body, acts);
    return { box, head, body, done };
  }

  /** `item` in the view: its content drawn again only when it changed (a list redraw keeps it). */
  show(item, others) {
    const { head, body, done } = this.view;
    const pending = this.pending.has(item.id);
    resolveKey(done, pending);
    done.title = pending ? "Take it back" : item.remind ? "Resolve this reminder (Undo for 5 s; Delete)" : "Delete this bookmark (Undo for 5 s; Delete)";
    const key = `${item.id}\n${item.note}\n${item.text}`;
    if (key === this.drawn) return;
    this.drawn = key;
    const title = el("span", "mkvtitle" + (item.note ? "" : " quiet"));
    const mark = kindMark(item);
    if (mark) title.append(mark);
    const note = oneLine(item.note, true, 200);
    title.append(note || oneLine(item.text, !!item.by, 120));   // titleOf's rule; a note's Markdown marks gone (`**check**`)
    const now = this.m.alive(item.sid), theirs = others && item.sid !== this.m.sid();
    head.replaceChildren(title, el("span", "pwmeta", [theirs && (now ?? `${item.title} (closed)`), item.host, when(item.at)].filter(Boolean).join(" · ")));
    body.scrollTop = 0;
    // a note the title cannot hold (its lines, its Markdown, its length) whole above the text
    const whole = item.note && note !== item.note.trim();
    void markdownView(body, (whole ? `${item.note}\n\n` : "") + recordMarkdown(item));
  }

  row(item, others) {
    const pending = this.pending.has(item.id);
    const li = el("li", [item.remind ? "remind" : "", item.by ? "agent" : "", item.id === this.chosen ? "chosen" : "", pending ? "pending" : ""].filter(Boolean).join(" "));
    li.dataset.id = item.id;
    const b = el("button", "mkitem");
    b.type = "button";
    // where and when on a line of their own, its title under them across the whole width, then its text's start, one line of words
    const words = oneLine(item.text, !!item.by);
    const title = el("span", "note" + (item.note ? "" : " quiet"));
    const mark = kindMark(item);
    if (mark) title.append(mark);
    title.append(titleOf(item, 240));
    // another session's: by its name now, a closed one by its last and "closed" (as marks.js listOf)
    const now = this.m.alive(item.sid), theirs = others && item.sid !== this.m.sid();
    const at = item.due && openReminder(item) ? "up now" : when(item.at);   // a reminder up (AC-91)
    const meta = el("span", "meta" + (at === "up now" ? " up" : ""), (theirs ? [now ?? item.title, at] : [at]).join(" · "));
    if (theirs && now === null) meta.append(el("span", "closed", "closed"));
    b.append(meta, title);
    if (item.note && words) b.append(el("span", "snip", words));
    b.onclick = () => this.choose(item);
    li.append(b);
    if (pending) {
      const undo = el("button", "mkundo", "Undo");
      undo.type = "button";
      undo.onclick = (e) => { e.stopPropagation(); this.undo(item.id); };
      li.append(el("span", "mkdonetext", item.remind ? "Resolved" : "Deleted"), undo);
    }
    return li;
  }

  /** Shows `item` in the view; the keys stay the pane's on a computer. */
  choose(item) {
    this.chosen = item.id;
    this.draw();
    if (MOUSE?.matches && !this.pane.hidden) this.pane.focus({ preventScroll: true });
  }

  /** Settled in the list at once; terminumd told after UNDO unless undone. */
  done(item) {
    if (this.pending.has(item.id)) return;
    const msg = item.remind ? { t: "remind", id: item.id, action: "resolve" } : { t: "unmark", id: item.id };
    this.sends.set(item.id, msg);
    this.pending.set(item.id, setTimeout(() => {
      this.pending.delete(item.id);
      this.sends.delete(item.id);
      this.m.send(msg);
      if (this.m.shown?.id === item.id) this.m.hideBar();
      for (const n of this.navs) n.draw();
    }, UNDO));
    this.sync(item.id);
  }

  undo(id) {
    clearTimeout(this.pending.get(id));
    this.pending.delete(id);
    this.sends.delete(id);
    this.sync(id);
  }

  /** The list and the bar's ✓ after a change of what is pending. */
  sync(id) {
    for (const n of this.navs) n.draw();
    if (this.m.shown?.id === id) this.bar(this.m.shown);
  }

  /** The bar shows `item`: its ✓ as its pending state says. */
  bar(item) {
    resolveKey($("mbdone"), this.pending.has(item.id));
    $("mbdone").title = item.remind ? "Resolve this reminder (Undo for 5 s)" : "Resolve: delete this bookmark (Undo for 5 s)";
  }

  key(e) {
    const all = this.entries(), i = all.findIndex((b) => b.id === this.chosen);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!all.length) return;
      const next = i < 0 ? (e.key === "ArrowDown" ? 0 : all.length - 1) : Math.max(0, Math.min(all.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
      if (next !== i) this.choose(all[next]);
    } else if ((e.key === "Delete" || e.key === "Backspace") && i >= 0) {
      e.preventDefault();
      const item = all[i];
      this.pending.has(item.id) ? this.undo(item.id) : this.done(item);
    } else if (e.key === "Enter" && i >= 0) {
      e.preventDefault();
      this.close();
      void this.m.jump(all[i]);
    }
  }
}
