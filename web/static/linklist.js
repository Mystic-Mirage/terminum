// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The session's links (AC-102), from the floating column under the sent history: terminumd reads
// its Claude Code or Codex transcript (linklist.rs) and answers each web address once, the newest or the
// most named first; the window filters them, a row opens its address in a new tab and says when
// it was last named.
import { fabPane } from "./fabpane.js";
import { hrefToken } from "./links.js";

const $ = (id) => document.getElementById(id);
const ANSWER = 30000;          // ms the list waits for the Mac's answer
const AGAIN = 5000;            // ms between two reads while new output comes
const BY = "tm.links.by";      // the order picked, kept in this browser
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "1 link", "3 links", "1 of 3 links". */
export function countText(shown, of, total) {
  const n = (k) => `${k} link${k === 1 ? "" : "s"}`;
  if (shown < of) return `${shown} of ${n(of)}`;
  return total > of ? `${of} of ${n(total)}` : n(of);
}

/** When an address was last named, in local time: "14:32" today, "Nov 1" this year, else "Nov 1, 2025"; "" for no time. */
export function whenText(iso, now = new Date()) {
  const d = typeof iso === "string" ? new Date(iso) : null;
  if (!d || Number.isNaN(d.getTime())) return "";
  if (d.toDateString() === now.toDateString()) return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  const day = `${MONTHS[d.getMonth()]} ${d.getDate()}`;
  return d.getFullYear() === now.getFullYear() ? day : `${day}, ${d.getFullYear()}`;
}

/** The items whose address holds q, case aside. */
export function filtered(items, q) {
  const low = q.trim().toLowerCase();
  return low ? items.filter((it) => it.u.toLowerCase().includes(low)) : items;
}

export class LinkList {
  /** send(msg) to terminumd; sid(): the pane shown. */
  constructor({ send, sid }) {
    Object.assign(this, { send, sid });
    this.items = []; this.total = 0; this.ref = ""; this.on = false; this.last = 0; this.soon = 0;
    $("linkq").addEventListener("input", () => this.draw());
    $("linkq").addEventListener("keydown", (e) => { if (e.key === "Escape" && $("linkq").value) { e.preventDefault(); $("linkq").value = ""; this.draw(); } });
    $("linkrefresh").onclick = () => this.ask();
    try { this.by = localStorage.getItem(BY) === "count" ? "count" : "new"; } catch { this.by = "new"; }
    for (const b of $("linkby").querySelectorAll("button")) {
      b.setAttribute("aria-pressed", String(b.dataset.by === this.by));
      b.onclick = () => {
        if (b.dataset.by === this.by) return;
        this.by = b.dataset.by;
        try { localStorage.setItem(BY, this.by); } catch { /* kept for this page only */ }
        for (const o of $("linkby").querySelectorAll("button")) o.setAttribute("aria-pressed", String(o === b));
        this.ask();
      };
    }
    fabPane({ fab: $("linksfab"), pane: $("linkspane"), close: $("linksclose"),
      onToggle: (on) => { this.on = on; if (on) this.ask(); } });
  }

  /** Every message from terminumd (after app.js dropped those of another pane). */
  seen(m) {
    if (m.t === "linklist" && m.ref === this.ref) this.answered(m);
    if (!this.on) return;
    if (m.t === "hist" && m.mode === "reset") this.ask();             // another pane, or its history cleared
    if (m.t === "screen" && !this.soon) {                              // new output: read again, at most once in AGAIN
      this.soon = setTimeout(() => { this.soon = 0; if (this.on) this.ask(); }, Math.max(0, this.last + AGAIN - Date.now()));
    }
  }

  ask() {
    if (!this.sid()) return this.say("No session is shown.");
    clearTimeout(this.soon); this.soon = 0;
    this.last = Date.now();
    const ref = this.ref = Math.random().toString(36).slice(2);
    if (!this.items.length) this.say("Reading the session…");
    this.send({ t: "links", ref, by: this.by });
    setTimeout(() => { if (this.ref === ref) { this.ref = ""; this.same = ""; this.items = []; this.draw(); this.say("The Mac did not answer."); } }, ANSWER);
  }

  answered(m) {
    this.ref = "";
    const same = JSON.stringify([m.error, m.items, m.total]);
    if (same === this.same) return m.error ? this.say(m.error) : this.draw(false);   // nothing new: the rows stay as they are (a hover too)
    this.same = same;
    if (m.error) { this.items = []; this.draw(); return this.say(m.error); }
    // only web addresses are listed, whatever came (terminumd sends only these)
    this.items = (Array.isArray(m.items) ? m.items : []).filter((it) => typeof it?.u === "string" && hrefToken(it.u)?.kind === "url")
      .map((it) => ({ u: it.u, c: it.c ?? 1, w: typeof it.w === "string" ? it.w : null }));
    this.total = Math.max(m.total ?? 0, this.items.length);
    this.draw();
  }

  say(text) { $("linkn").textContent = text; }

  /** The rows again (rows false: the count line only). */
  draw(rows = true) {
    const list = filtered(this.items, $("linkq").value);
    if (rows) $("linkrows").replaceChildren(...list.map((it) => this.row(it)));
    if (!this.items.length) return this.say(this.ref ? "Reading the session…" : "No web addresses in this session.");
    this.say(countText(list.length, this.items.length, this.total));
  }

  row(it) {
    const li = document.createElement("li");
    const a = Object.assign(document.createElement("a"), { className: "lk", href: it.u, target: "_blank", rel: "noopener noreferrer", title: it.u });
    const t = Object.assign(document.createElement("span"), { className: "lkt", textContent: it.u });
    a.append(t);
    if (it.c > 1) a.append(Object.assign(document.createElement("span"), { className: "count", textContent: `×${it.c}` }));
    const w = whenText(it.w);
    if (w) a.append(Object.assign(document.createElement("span"), { className: "lkw", textContent: w, title: new Date(it.w).toLocaleString() }));
    li.append(a);
    return li;
  }
}
