// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Bookmarks of the terminal (AC-57): a selection kept with a note, on the Mac for every
// signed-in browser (the bridge's bookmarks.py). Here: the dialog that makes or edits one, the
// marks on the terminal's right edge, the bar that shows the one brought into view, and the list
// below the sessions of those no session shows (orphaned). Text from the terminal is shown as text only.
import { icon } from "./icons.js";
import { Jumper } from "./markjump.js";
import { columnLeft, markPicker } from "./markpick.js";
import { placeModal, placePane } from "./fabpane.js";
import { nowhere, projectsOf, shownIn } from "./projects.js";
import { Find } from "./find.js";
import { loose, oneLine, railBox, railSlots, titleOf } from "./marktext.js";

import { floats, FOLDED } from "./fold.js";
import { kindMark, MarkNav } from "./marknav.js";

const $ = (id) => document.getElementById(id);
const ANSWER = 10000;          // ms the dialog waits for the Mac to keep a bookmark
const ARMED = 3000;            // ms a Delete pressed once waits for the second press
const MOUSE = globalThis.matchMedia?.("(hover: hover) and (pointer: fine)");   // none in node (unit tests)
// the floating buttons at the terminal's top right, top down: the plan's progress (while there is a plan,
// planpane.js), the plain ones, then the colored ones together; where the keyboard's buttons float (a
// phone, a narrow window) they fold under ⟨…⟩ (fold.js)
const PLAN = FOLDED.slice(0, 1), PLAIN = FOLDED.slice(1, 5), COLORED = FOLDED.slice(5);

export const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
export const when = (s) => new Date(s * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

/** A reminder not resolved (AC-91). */
export const openReminder = (i) => !!i.remind && !i.resolved;

/** The selected text as a Markdown quote, its lines kept. */
export const quote = (text) => text.split("\n").map((l) => `> ${l}`).join("\n");

/** The label of the button that pastes a bookmark or a reminder into the shown pane's input, as a
 *  quote (the sent history's pastes a message as sent: "Insert into chat"); one look everywhere (.tochat). */
export const INSERT = "Quote in chat";
/** What Quote in chat pastes for a bookmark or a reminder: the note, under it its text quoted
 *  (an agent's record, AC-98, too). */
export const chatText = (item) => (item.note ? `${item.note}\n\n` : "") + quote(item.text);

// Delete asks again: a second press within 3 s deletes.
export function armed(button, label, act) {
  let timer = 0;
  button.addEventListener("click", (e) => {
    e.stopPropagation();
    if (!timer) {
      button.textContent = "Delete?"; button.classList.add("armed");
      timer = setTimeout(() => { timer = 0; button.classList.remove("armed"); button.replaceChildren(...label()); }, ARMED);
      return;
    }
    clearTimeout(timer); timer = 0;
    act();
  });
}

export class Marks {
  /** view, widgets, term: the terminal's; send(msg) to the bridge; sid(): the pane shown;
   *  alive(sid): an open pane's title now (renamed or not), null when it closed; pick(sid): show it (and close the drawer); toast(text);
   *  onChange(): the bookmarks changed (the list's strips, reminders); copy(text): to the clipboard;
   *  projects(): terminumd's (AC-92); toChat(text): pasted into the shown pane's input (Insert into chat).
   *  store: this browser's choices (app.js: get(k, d), set(k, v)).
   *  onPlaced (set after): the floating buttons were placed (their panes follow). */
  constructor({ view, widgets, term, send, sid, alive, pick, toast, onChange, copy, panes, projects, openAll, toChat, store }) {
    Object.assign(this, { view, widgets, term, send, sid, alive, pick, toast, onChange, panes, projects, openAll, toChat, store });
    this.listKind = "bookmark";  // what the list below the sessions shows: bookmarks or reminders
    this.items = new Map();
    this.replies = new Map();    // ref -> resolve, for the dialog
    this.shown = null;           // the bookmark in the bar
    this.jumper = new Jumper({ view, widgets, term, send, sid });
    this.search = new Find({ view, widgets, term, send, sid });   // its magnifier stands beside the bookmarks' button
    this.search.onOpen = () => this.togglePane(false);                // one of the two at a time
    document.addEventListener("planfab", () => this.placeFab());      // the plan's progress circle shown or gone (planpane.js)
    this.picker = markPicker({ term, main: $("main"), button: $("markadd"), copyButton: $("markcopy"), askButton: $("askfab"), view,
      onPick: (sel) => this.edit(null, sel), onCopy: copy, onAsk: (text, line) => this.onAsk?.(text, line) });
    $("markbox").querySelector("form").addEventListener("submit", (e) => this.submit(e));
    $("markbox").addEventListener("close", () => { this.editing = null; });
    for (const b of $("markkind").querySelectorAll("button")) b.onclick = () => this.setKind(b.dataset.kind);
    $("markbox").addEventListener("cancel", (e) => { if (this.saving) e.preventDefault(); });   // Esc while saving
    $("mbclose").onclick = () => this.hideBar();
    $("mbedit").onclick = () => this.shown && this.edit(this.shown);
    $("mbsend").onclick = () => this.shown && this.toChat(chatText(this.shown));
    $("mbprev").onclick = () => this.step(-1);
    $("mbnext").onclick = () => this.step(1);
    armed($("mbdel"), () => ["Delete"], () => { if (this.shown) this.send({ t: "unmark", id: this.shown.id }); });
    $("marksbtn").onclick = () => this.toggleList();
    $("marksdlgclose").onclick = () => this.toggleList(false);
    $("marksdlg").addEventListener("close", () => $("marksbtn").setAttribute("aria-expanded", "false"));
    $("marksfab").onclick = () => this.togglePane();
    $("marksfab").addEventListener("mousedown", (e) => e.preventDefault());   // the terminal keeps the keys
    document.addEventListener("keydown", (e) => { if (e.key === "Escape") this.togglePane(false); });
    // a press anywhere else closes it
    document.addEventListener("pointerdown", (e) => { if (!$("markpane").hidden && !e.target.closest("#markpane, #marksfab")) this.togglePane(false); });
    new ResizeObserver(() => this.drawRail()).observe(term);
    this.nav = new MarkNav(this);      // the pane's list: going through them, ✓ (marknav.js)
  }

  /** Every message from the bridge (after app.js dropped those of another pane). */
  seen(m) {
    this.jumper.seen(m);
    this.search.seen(m);
    switch (m.t) {
      case "marks":
        this.items = new Map(m.items.map((i) => [i.id, i]));
        this.most = m.most ?? this.most;               // the characters a bookmark keeps, as terminumd says
        if (m.error) this.toast(`Bookmarks: ${m.error}`, 8000);
        return this.changed();
      case "mark":
        this.items.set(m.item.id, m.item);
        if (this.shown?.id === m.item.id) this.showBar(m.item, this.why);
        return this.changed();
      case "unmark":
        this.items.delete(m.id);
        if (this.shown?.id === m.id) this.hideBar();
        return this.changed();
      case "marked": return this.replies.get(m.ref)?.(m);
      case "hist":                                 // another pane: the bar and the list were of the last one
        if (m.mode === "reset" && this.shown && this.shown.sid !== m.sid) this.hideBar();
        else if (m.mode === "reset") this.jumper.clear();     // its lines are numbered anew: no mark on them
        if (m.mode === "reset" && m.sid !== this.paneSid) this.togglePane(false);
      // falls through
      case "screen": requestAnimationFrame(() => this.drawRail()); return;   // the pane's range changed
      case "layout": requestAnimationFrame(() => this.drawList());                  // a session closed or came back
    }
  }

  /** pane id -> {mark, remind}: the list's marks of a session that shows bookmarks or open reminders. */
  sids() {
    const of = projectsOf(this.projects()), kinds = new Map(), m = new Map();   // "p" + a project's id, "s" + a pane's -> its kinds
    for (const b of this.items.values()) {
      if (b.orphaned) continue;
      const k = b.project ? `p${b.project}` : `s${b.sid}`, v = kinds.get(k) ?? { mark: false, remind: false };
      v.mark ||= !b.remind; v.remind ||= openReminder(b);
      kinds.set(k, v);
    }
    for (const it of this.panes()) {
      const a = of.has(it.id) ? kinds.get(`p${of.get(it.id)}`) : undefined, b = kinds.get(`s${it.id}`);
      const mark = !!(a?.mark || b?.mark), remind = !!(a?.remind || b?.remind);
      if (mark || remind) m.set(it.id, { mark, remind });
    }
    return m;
  }
  /** The bookmarks and reminders the shown pane shows: its project's and its own (asked on every
   *  frame: kept until the bookmarks, the layout, the projects or the pane shown change). */
  here() {
    const panes = this.panes(), projects = this.projects(), sid = this.sid(), h = this.hereOf;
    if (h?.panes === panes && h.projects === projects && h.sid === sid && h.rev === this.rev) return h.list;
    const project = projectsOf(projects).get(sid);
    const list = panes.some((p) => p.id === sid) ? [...this.items.values()].filter((b) => shownIn(b, sid, project)) : [];
    this.hereOf = { panes, projects, sid, rev: this.rev, list, project };
    return list;
  }
  /** The shown pane's project, terminumd's (none outside projects). */
  hereProject() { this.here(); return this.projects().find((p) => p.id === this.hereOf.project) ?? null; }
  busy() { return $("markbox").open || $("askbox").open; }   // a dialog of theirs (Ask: ask.js) has the keys
  changed() { this.rev = (this.rev ?? 0) + 1; this.drawRail(); this.drawList(); this.drawPane(); this.onChange(); document.dispatchEvent(new Event("marks")); }
  /** The shown session's own bookmarks it shows (not orphaned), top to bottom: its marks and the bar's steps
   *  (an agent's record once its place was found, AC-98). */
  mine() { return this.here().filter((i) => i.sid === this.sid() && i.n >= 0).sort((a, b) => a.n - b.n); }

  // ---------- the dialog ----------

  /** A new bookmark of selection sel ({n, text}), or item's note edited. */
  edit(item, sel = null) {
    // one ref per dialog: a save sent again after no answer is kept once (bookmarks.py)
    // what is kept: terminumd's `most` characters (a program's copy can be 1 MB), never half an emoji
    const most = this.most ?? 2000, chars = item ? null : Array.from(sel.text.slice(0, 2 * most + 2)), text = chars?.slice(0, most).join("");
    this.editing = { ref: Math.random().toString(36).slice(2), ...(item ? { id: item.id } : { sid: this.sid(), ...sel, text }) };
    this.setKind(item?.remind ? "reminder" : "bookmark");
    $("marktext").textContent = item ? item.text : text + (chars.length > most ? `\n… (the first ${most.toLocaleString("en")} characters are kept)` : "");
    $("marknote").value = item?.note ?? "";
    $("markerr").hidden = true;
    $("markbox").querySelector(".primary").disabled = false;
    $("markbox").style.setProperty("--term-w", `${$("term").clientWidth}px`);   // its width: 80% of the terminal's (marks.css)
    $("markbox").showModal();
    $("marknote").focus();
  }

  /** The dialog's kind: a bookmark, or a reminder (AC-91). */
  setKind(kind) {
    for (const b of $("markkind").querySelectorAll("button")) b.setAttribute("aria-pressed", String(b.dataset.kind === kind));
    $("marktitle").textContent = kind === "reminder" ? "Reminder" : "Bookmark";
    this.kind = kind;
  }

  async submit(e) {
    if (e.submitter?.value !== "save" || !this.editing) return;   // Cancel closes it
    e.preventDefault();
    const save = $("markbox").querySelector(".primary"), ed = this.editing, ref = ed.ref, note = $("marknote").value;
    save.disabled = true; this.saving = true;
    const reply = await new Promise((resolve) => {
      const timer = setTimeout(() => resolve({ error: "The Mac did not answer: is the connection back?" }), ANSWER);
      this.replies.set(ref, (m) => { clearTimeout(timer); resolve(m); });
      const remind = this.kind === "reminder";
      this.send(ed.id ? { t: "mark", ref, id: ed.id, note, remind } : { t: "mark", ref, sid: ed.sid, n: ed.n, text: ed.text, note, remind });
    });
    this.replies.delete(ref);
    save.disabled = false; this.saving = false;
    if (reply.error) { $("markerr").textContent = reply.error; $("markerr").hidden = false; return; }
    $("markbox").close();
    if (!ed.id && ed.sid === this.sid()) this.jumper.markAt(ed.n, loose(ed.text));   // its wavy line at once, where it stands
    const where = this.hereProject() ? "an agent of this project" : "this session's agent";
    if (!ed.id) this.toast(this.kind === "reminder" ? `Reminder set: it comes up when ${where} ends a turn.` : "Bookmarked.", this.kind === "reminder" ? 3500 : undefined);
  }

  // ---------- the bar ----------

  showBar(item, why = null) {
    this.shown = item; this.why = why;
    const mine = this.mine(), i = mine.findIndex((x) => x.id === item.id);
    $("mbnote").textContent = titleOf(item);
    $("mbnote").classList.toggle("quiet", !item.note);
    $("mbwhy").textContent = why ?? ""; $("mbwhy").hidden = !why;
    $("mbtext").textContent = item.text; $("mbtext").hidden = !why;
    $("mbpos").textContent = i >= 0 && mine.length > 1 ? `${i + 1}/${mine.length}` : "";
    $("mbprev").hidden = $("mbnext").hidden = !(i >= 0 && mine.length > 1);
    $("markbar").style.top = `${this.term.offsetTop + 8}px`;
    $("markbar").hidden = false;
    this.nav.bar(item);
  }
  hideBar() { this.shown = null; $("markbar").hidden = true; this.jumper.clear(); }
  step(d) {
    const mine = this.mine(), i = mine.findIndex((x) => x.id === this.shown?.id);
    if (i >= 0 && mine.length) this.jump(mine[(i + d + mine.length) % mine.length]);
  }

  /** Brings a sent message's `text` into view in the shown pane (AC-90): terminumd finds it
   *  nearest line `n` (its hint), else nearest the end; a coding agent's prompt (`prompt`) only
   *  where its transcript shows it. {ok}, {why} or {cancelled}. */
  jumpText({ text, n, prompt = false }) {
    this.hideBar();
    text = text.replace(/\[Pasted text #\d+[^\]\n]*\]/g, "").trim();   // a paste the input box folded: the transcript shows it whole (sent.rs findable)
    return this.jumper.go({ id: `sent:${Math.random().toString(36).slice(2)}`, sid: this.sid(), text, n, prompt, find: true });
  }

  /** Shows item's pane and brings its text into view (an agent's record: its title, where its call
   *  was printed, AC-98); the bar says how it went. */
  async jump(item) {
    const what = item.by ? "here it is" : "here is what was selected";
    if (item.orphaned) return this.showBar(item, `Its project or its session is gone: ${what}.`);
    if (this.alive(item.sid) === null) return this.showBar(item, `Its session is not open now: ${what}.`);
    const other = item.sid !== this.sid();
    const opened = other ? this.jumper.wait((m) => m.t === "hist" && m.mode === "reset" && m.sid === item.sid) : null;
    this.pick(item.sid);
    if (opened) { try { await opened; } catch (e) { return this.showBar(item, e.message); } }
    this.showBar(item);
    const r = await this.jumper.go(item.by ? { ...item, text: item.note } : item);
    if (r.cancelled || this.shown?.id !== item.id) return;
    const gone = item.by ? "Where it was made is no longer in this session's history: here it is." : `${r.why} Here is what was selected.`;
    if (r.why) this.showBar(item, r.gone ? gone : `Could not bring it into view: ${r.why}`);
    else if (r.note) this.showBar(item, r.note);
  }

  // ---------- the marks on the terminal's edge ----------

  drawRail() {
    this.placeFab();
    const rail = $("markrail"), term = this.term, v = this.view, mine = this.mine();
    const box = railBox(term);
    // a wide window has them on the strip at the left (promptrail.js)
    if (!mine.length || !box || !v.theme || !floats()) { rail.hidden = true; this.railKey = ""; return; }
    const { css, h } = box;
    const slots = railSlots(mine, v.oldest, v.top + v.screen.length, h - 14);
    const key = css + JSON.stringify(slots);       // the screen changes often; the marks seldom
    if (key === this.railKey) return;
    this.railKey = key;
    rail.style.cssText = css;
    const marks = slots.map(({ top, items }) => {
      const b = el("button", "mk");
      b.type = "button";
      b.style.top = `${top}px`;
      const what = items.map((i) => titleOf(i, 60));
      b.title = items.length > 1 ? `${items.length} bookmarks:\n${what.join("\n")}` : what[0];
      b.setAttribute("aria-label", `Bookmark: ${b.title}`);
      if (items.length > 1) b.dataset.count = String(items.length);
      b.onclick = () => this.jump(items[0]);
      return b;
    });
    rail.replaceChildren(...marks);
    rail.hidden = false;
  }

  // ---------- those no session shows, below the sessions ----------

  /** Opens or closes the window of those no session shows (a large one on the right). */
  toggleList(open = !$("marksdlg").open) {
    if (open && !$("marksdlg").open) { $("marksdlg").showModal(); this.drawList(); }
    else if (!open && $("marksdlg").open) $("marksdlg").close();
    $("marksbtn").setAttribute("aria-expanded", String($("marksdlg").open));
  }

  /** The bookmarks or the reminders (the list's segment) no session shows, newest first: orphaned
   *  (their project or session gone) or of a session not listed now (its host out of reach); Delete
   *  here is final; "Open all" shows them in full. */
  drawList() {
    const panes = this.panes(), of = projectsOf(this.projects());
    const left = [...this.items.values()].filter((b) => nowhere(b, panes, of));
    $("markcount").textContent = $("markpeek").textContent = left.length ? String(left.length) : "";   // the button's, the folded foot's
    if (!$("marksdlg").open) return;
    const reminders = this.listKind === "reminder";
    const all = left.filter((b) => !!b.remind === reminders).sort((a, b) => b.at - a.at);
    const seg = el("div", "seg mkkind");
    for (const [k, label] of [["bookmark", "Bookmarks"], ["reminder", "Reminders"]]) {
      const b = el("button", "", label);
      b.type = "button";
      b.setAttribute("aria-pressed", String(this.listKind === k));
      b.onclick = () => { this.listKind = k; this.drawList(); };
      seg.append(b);
    }
    const head = el("div", "mkhead");
    head.append(seg);
    if (all.length) {
      const open = el("button", "mkall", "Open all");
      open.type = "button";
      open.title = "All of them as one document, full screen";
      open.onclick = () => this.openAll(all, reminders ? "Reminders" : "Bookmarks");
      head.append(open);
    }
    const empty = `None here: a ${reminders ? "reminder" : "bookmark"} shows in its project's sessions, or its own session, and comes here once they are gone.`;
    $("marklist").replaceChildren(head, all.length ? this.listOf(all, true, "out of reach") : el("p", "empty", empty));
  }

  /** A list of bookmarks: note, the start of the text, and (all: its session's title, host, when,
   *  "orphaned", or `away` when its session is not listed); a press brings one into view, × (twice) deletes it. */
  listOf(items, all, away = "closed") {
    const ul = el("ul");
    for (const item of items) {
      const li = el("li"), b = el("button", "mkitem");
      b.type = "button";
      const title = el("span", "note" + (item.note ? "" : " quiet")), mark = kindMark(item);
      if (mark) title.append(mark);
      title.append(titleOf(item));
      b.append(title);
      if (item.note) b.append(el("span", "snip", oneLine(item.text, !!item.by)));
      // an open session by its name now (it may have been renamed since), a closed one by its last
      const now = this.alive(item.sid);
      const meta = el("span", "meta", (all ? [now ?? item.title, item.host, when(item.at)] : [when(item.at)]).filter(Boolean).join(" · "));
      // orphaned: its project or session is gone (only below the sessions, until deleted); else its session is not listed now:
      // closed (a project's, shown while the project lives) or out of reach (below the sessions: its host down)
      if (all && item.orphaned) meta.append(el("span", "closed", "orphaned"));
      else if (all && now === null) meta.append(el("span", "closed", away));
      b.append(meta);
      b.onclick = () => { this.togglePane(false); if (all) this.toggleList(false); this.jump(item); };
      const del = el("button", "mkdel");
      del.type = "button";
      del.title = "Delete this bookmark";
      const label = () => [icon("x", 14)];
      del.append(...label());
      armed(del, label, () => this.send({ t: "unmark", id: item.id }));
      li.append(b, del);
      ul.append(li);
    }
    return ul;
  }

  // ---------- this session's bookmarks, from the button at the terminal's top right ----------

  /** The floating buttons stand in one column at the terminal's top right, left of the marks:
   *  find, the split and the sent history; then, set apart, the colored ones: this session's
   *  bookmarks (with their number, while it has one) and the reminders (while one is open). Where
   *  the keyboard's buttons float, ⟨…⟩ stands first and the column, Attach in it (only there), under
   *  it, shown while it is unfolded (fold.js). */
  placeFab() {
    // the bookmarks' pane lists its bookmarks and open reminders (marknav.js): the button counts them
    const term = this.term, here = this.sid() ? this.here() : [], n = here.filter((b) => !b.remind || openReminder(b)).length;
    const shown = !!this.sid() && !term.hidden && term.clientHeight > 0 && !!this.view.theme;
    for (const id of PLAIN) $(id).hidden = !shown;
    $("marksfab").hidden = !(shown && n > 0);
    $("remindfab").hidden = !shown || !here.some(openReminder);
    $("planfab").hidden = !shown || !("pct" in $("planfab").dataset);
    $("marksfabn").textContent = String(n);
    $("marksfab").setAttribute("aria-label", `Bookmarks and reminders of this ${this.hereProject() ? "project" : "session"}: ${n}`);
    if (!shown) this.search.toggle(false);
    if ($("marksfab").hidden) this.togglePane(false);
    if (!shown) return;
    const left = columnLeft(term);
    const shownOf = (ids) => ids.map($).filter((b) => !b.hidden && b.offsetHeight);   // the split is the header's on a computer
    let y = term.offsetTop + 12;                    // the gap between them (--sp-3) above the first too
    if (floats()) { Object.assign($("foldfab").style, { top: `${y}px`, left }); y += ($("foldfab").offsetHeight || 44) + 12; }
    for (const group of [PLAN, PLAIN, COLORED]) {
      const bs = shownOf(group);
      for (const b of bs) { Object.assign(b.style, { top: `${y}px`, left }); y += b.offsetHeight + 12; }
      if (bs.length) y += 8;                       // each group set apart
    }
    this.onPlaced?.();                             // first: ⟨…⟩ folds or not (fold.js), which find's field stands by
    this.search.place();
    if (!$("markpane").hidden) this.placePane();
  }
  placePane() { placePane($("markpane"), () => this.togglePane(false)); }
  /** Opens or closes the list of this session's bookmarks (a second press closes it). */
  togglePane(open = $("markpane").hidden) {
    const had = $("markpane").contains(document.activeElement);
    $("markpane").hidden = !open;
    this.paneSid = open ? this.sid() : null;
    $("marksfab").setAttribute("aria-expanded", String(open));
    // a computer's keys go to the list while it is open (↑ ↓, ✓: marknav.js), back to the terminal after
    if (open) { this.search.toggle(false); this.drawPane(); this.placePane(); if (MOUSE?.matches) $("markpane").focus({ preventScroll: true }); }
    else if (had) $("kbd").focus({ preventScroll: true });
  }
  drawPane() { this.nav.draw(); }
  /** The shown session's open records from its coding agent (AC-98), and a way to them: the Plan pane says so. */
  agentRecords() { return { n: this.here().filter((b) => b.by && (!b.remind || openReminder(b))).length, open: () => this.togglePane(true) }; }
}
