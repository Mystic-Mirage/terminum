// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A wide window's marks on the terminal's right edge (AC-90), placed over the shown pane's
// loadable history as a scrollbar's thumb is: a bar where each message sent there stands (white;
// terminumd finds each by its text near the line it was sent on: `sentmarks`), where each answer
// of its agent ended (pale blue; answers.rs), each bookmark of the session (orange) and each open
// reminder (pink); marks that meet are one, pink over orange over white over pale blue. A press brings it into view: a message as a bookmark's text is, a bookmark with
// its bar. (A narrow window has the bookmarks' own marks there, marks.js.)
import { openReminder } from "./listparts.js";
import { railBox, railSlots } from "./marktext.js";

const $ = (id) => document.getElementById(id);
const WIDE = "(min-width: 861px)";
const KINDS = ["remind", "mark", "sent", "end"];   // which color a dot of several takes, first first
const NAMES = { sent: "Sent", end: "Answer ended", mark: "Bookmark", remind: "Reminder" };

/** view, term: the terminal's; send(msg) to terminumd; sid(): the pane shown; marks: marks.js's
 *  Marks (its bookmarks, jump, jumpText); toast(text). */
export function promptRail({ view, term, send, sid, marks, toast }) {
  const rail = $("msgrail"), wide = matchMedia(WIDE);
  let sent = [], ends = [], key = "", frame = 0, asking = 0;

  /** Asks terminumd where the shown pane's messages stand (a wide window only), once for a
   *  burst (it reads the pane's whole history). */
  function ask() {
    clearTimeout(asking);
    asking = setTimeout(() => { if (wide.matches && sid() && !document.body.classList.contains("nomarks")) send({ t: "sentmarks" }); }, 300);
  }

  function draw() {
    const box = wide.matches && !!sid() && !!view.theme ? railBox(term) : null;
    if (!box) { rail.hidden = true; key = ""; return; }
    const { css, h } = box;
    const own = marks.mine();
    const items = [...sent.map((s) => ({ n: s.n, kind: "sent", text: s.text, prompt: !!s.prompt })), ...ends.map((s) => ({ n: s.n, kind: "end", text: s.text })),
      ...own.filter((b) => !b.remind).map((b) => ({ n: b.n, kind: "mark", b })),
      ...own.filter(openReminder).map((b) => ({ n: b.n, kind: "remind", b }))];
    const slots = railSlots(items, view.oldest, view.top + view.screen.length, h - 14);
    const k = css + JSON.stringify(slots.map((s) => [s.top, s.items.map((i) => `${i.kind}${i.n}${i.b?.id ?? ""}`)]));
    if (k === key) return;                         // the screen changes often; the dots seldom
    key = k;
    rail.style.cssText = css;
    rail.replaceChildren(...slots.map(({ top, items: here }) => {
      const kind = KINDS.find((x) => here.some((i) => i.kind === x)), it = here.find((i) => i.kind === kind);
      const b = document.createElement("button");
      b.type = "button";
      b.className = `dot ${kind}`;
      b.style.top = `${top}px`;
      b.title = here.map((i) => `${NAMES[i.kind]}: ${i.text ?? (i.b.note || i.b.text.split("\n")[0])}`.slice(0, 160)).join("\n");
      b.setAttribute("aria-label", b.title);
      b.onclick = () => go(KINDS.flatMap((x) => here.filter((i) => i.kind === x)));
      return b;
    }));
    rail.hidden = false;
  }

  /** Brings the first of `items` (one bar's, its color's kind first) that is found into view: a
   *  message whose text is not there now gives way to the next. */
  async function go(items) {
    let why = "";
    for (const it of items) {
      if (it.b) return marks.jump(it.b);
      const r = await marks.jumpText({ text: it.text, n: it.n, prompt: it.prompt });
      if (r.cancelled || (r.ok && !r.note)) return;
      why = r.why ?? r.note ?? why;
    }
    if (why) toast(why, 5000);
  }

  const later = () => { if (!frame) frame = requestAnimationFrame(() => { frame = 0; draw(); }); };
  new ResizeObserver(later).observe(term);
  wide.addEventListener("change", () => { ask(); later(); });
  document.addEventListener("sidemarks", () => { ask(); later(); });   // the menu's switch (prefs.js)

  return {
    /** Every message from terminumd (app.js dropped another pane's). */
    seen(m) {
      if (m.t === "sentmarks") { sent = Array.isArray(m.items) ? m.items : []; ends = Array.isArray(m.ends) ? m.ends : []; later(); }
      else if (m.t === "hist" && m.mode === "reset") { sent = ends = []; ask(); later(); }   // another pane, or its lines numbered anew
      else if ((m.t === "sentnew" || m.t === "endnew") && m.key === sid()) ask();   // kept in this pane: its dots
      else if (m.t === "hist" || m.t === "screen") later();
    },
    draw: later,
  };
}
