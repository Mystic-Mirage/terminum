// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-57 for e2e_marks.mjs: its history, its bookmarks, a fake bridge's answers and how a check reaches them.
import { pressFab } from "./fake.mjs";
import { loose } from "../static/marktext.js";

export const line = (text) => ({ r: [[text, null, null, 0]], e: true });
export const PROMPT = "alex@devbox ~/app % ";
export const TEXT = Array.from({ length: 3000 }, (_, i) => `out ${i} lorem ipsum`);
TEXT[150] = "needle: the retry loop never resets its counter";
TEXT.splice(300, 11, `${PROMPT}cat README.md`, "# App", "", "A small **tool** that keeps bookmarks.", "", "## Install", "", "- fast", "- small", "", `${PROMPT}echo hi`);
for (let i = 500; i <= 530; i++) TEXT[i] = `similar ${i}`;
TEXT[515] = "folded secret line 515";
TEXT[2200] = "See [the guide](https://docs.example/guide) for the install steps";
TEXT[100] = "find alpha here";
TEXT[2650] = "alpha beta ALPHA";
TEXT[2700] = "x".repeat(250) + " gam";                             // a long row iTerm wrapped, "gamma" across the wrap
TEXT[2701] = "ma end";
export const LONG = ["Long answer: the first paragraph says what changes and why it matters to the reader,",   // over 200 letters
  "and it goes on over a second row of the same paragraph with more words in it", "",
  "The second paragraph explains the check that was run on the delivered tree and",
  "ends here with the closing words of the answer"];
TEXT.splice(2400, LONG.length, ...LONG);
export const HISTORY = TEXT.map((t, n) => (n === 2700 ? { ...line(t), e: false } : line(t)));
export const FOLDS = [{ n: 500, to: 530, label: "⋯ 31 similar lines" }];
export const GONE = "These lines are no longer in this session's history.";
export const at0 = 1790000000;
export const ITEMS = [
  { id: "A", sid: "S1", key: null, n: 140, text: "the retry loop never resets", note: "ask about the retry loop", title: "One", host: null, at: at0 + 1 },
  { id: "B", sid: "S1", key: null, n: 303, text: "A small tool that keeps bookmarks.", note: "rendered", title: "One", host: null, at: at0 + 2 },
  { id: "C", sid: "S1", key: null, n: 515, text: "folded secret line 515", note: "", title: "One", host: null, at: at0 + 3 },
  { id: "D", sid: "S1", key: null, n: 10, text: "never printed anywhere", note: "gone", title: "One", host: null, at: at0 + 4 },
  { id: "E", sid: "S9", key: null, n: 7, text: "from a session that closed", note: "closed one", title: "Old", host: null, at: at0 + 5, orphaned: at0 + 9 },
  { id: "F", sid: "S1", key: null, n: 2200, text: "See the guide for the install steps", note: "guide", title: "One", host: null, at: at0 + 6 },
  { id: "G", sid: "S1", key: null, n: 2300, text: "phantom words", note: "phantom", title: "One", host: null, at: at0 + 7 },
  { id: "H", sid: "S1", key: null, n: 2400, text: LONG.join("\n"), note: "long answer", title: "One", host: null, at: at0 + 8 },
];
export const LAYOUT = [{ kind: "window", label: "Window 1", where: "", host: null, wid: "W1", new: "New tab", pool: "", items: [
  { id: "S1", title: "One", sub: ["zsh", "~/p"], index: "1", host: null, profile: "Default", agent: null, shell: true, state: "" },
  { id: "S2", title: "Two", sub: ["zsh", "~/q"], index: "2", host: null, profile: "Default", agent: null, shell: true, state: "" }] }];

// the bridge's search (bookmarks.py find), enough for these checks: the line where the text (its
// first 200 letters and digits, PRINT), or else a 20-letter part of it, starts
export function locate(item) {
  if (item.id === "G") return { n: 2300, start: 0, size: 12 };    // as if the bridge found it, the page does not
  const whole = loose(item.text).slice(0, 200);
  let joined = "";
  const starts = TEXT.map((t) => { const s = joined.length; joined += loose(t); return s; });
  const parts = whole.length > 20 ? [...Array.from({ length: Math.floor(whole.length / 20) }, (_, k) => k * 20), whole.length - 20] : [];
  const tries = [[0, whole.length], ...parts.map((o) => [o, 20])];
  for (const [start, size] of tries) {
    const i = joined.indexOf(whole.slice(start, start + size));
    if (i < 0) continue;
    let n = 0;
    while (n + 1 < starts.length && starts[n + 1] <= i) n++;
    return { n, start, size };
  }
  return { n: null, why: GONE };
}

export function bridge(items) {
  let k = 0;
  return (msg, say) => {
    if (msg.t === "more") {
      const first = Math.max(0, msg.to ?? msg.before - 1000);
      const lines = HISTORY.slice(first, msg.before).map((l, j) => (FOLDS.some((f) => first + j >= f.n && first + j <= f.to) ? null : l));
      say({ t: "hist", mode: "prepend", sid: "S1", first, oldest: 0, truncated: false, lines, folds: FOLDS.filter((f) => f.to >= first && f.n < msg.before) });
    }
    if (msg.t === "mark" && msg.note === "FAIL") say({ t: "marked", ref: msg.ref, error: "300 bookmarks are kept at most: delete some first." });
    else if (msg.t === "mark" && !msg.id) {
      const item = { id: `N${++k}`, sid: msg.sid, key: null, n: msg.n, text: msg.text, note: msg.note, title: "One", host: null, at: at0 + 100 + k, ...(msg.remind ? { remind: true } : {}) };
      items.set(item.id, item);
      say({ t: "marked", ref: msg.ref, id: item.id });
      say({ t: "mark", item });
    }
    if (msg.t === "unmark") { items.delete(msg.id); say({ t: "unmark", id: msg.id }); }
    if (msg.t === "askno") say({ t: "askno", sid: "S1", n: 7 });   // the pane's next quote number (AC-37)
    if (msg.t === "locate") say({ t: "located", id: msg.id, sid: "S1", ...locate(items.get(msg.id)) });
    if (msg.t === "find") {                                           // the bridge's search (search.py), line by line
      const q = msg.q.toLowerCase(), hits = [];
      TEXT.forEach((t, n) => {
        if (n === 2701) return;                                       // the rest of row 2700's line
        const text = (n === 2700 ? t + TEXT[2701] : t).toLowerCase();
        let i = -1, k = 0;
        while ((i = text.indexOf(q, i + 1)) >= 0) hits.push([n, k++]);
      });
      say({ t: "found", ref: msg.ref, sid: "S1", q: msg.q, hits, total: hits.length });
    }
    if (msg.t === "sub" && msg.id === "S2") {                         // a second pane, short
      say({ t: "theme", sid: "S2", theme: { bg: "#1e1e1e", fg: "#d4d4d4", cursor: "#fff", cursorText: "#000", ansi: Array(16).fill("#888"), font: null, size: null } });
      say({ t: "hist", mode: "reset", sid: "S2", first: 0, oldest: 0, truncated: false, lines: TEXT.slice(0, 600).map(line) });
      say({ t: "screen", sid: "S2", full: true, n: 1, cols: 80, rows: 1, ch: [[0, line("two")]] });
    }
  };
}

// the bookmark marked now: its text, whether it is in a widget, and where it is in the terminal
export const marked = (page) => page.evaluate(() => {
  const line = document.getElementById("markline"), r = line?.range, t = document.getElementById("term").getBoundingClientRect();
  if (!r || line.hidden) return null;
  const b = r.getBoundingClientRect(), l = line.getBoundingClientRect();
  return { text: r.toString(), shadow: r.startContainer?.getRootNode() instanceof ShadowRoot, top: Math.round(b.top - t.top), h: Math.round(t.height),
    line: l.left < b.left && l.top <= b.top + 1 && l.bottom >= b.bottom - 1 };
});
export const inView = (m) => m && m.top >= 0 && m.top < m.h;

/** Brings the shown session's bookmark noted `text` into view from its pane (the top right button). */
export async function fromPane(page, text) {
  if (await page.evaluate(() => document.getElementById("markpane").hidden)) await pressFab(page, "#marksfab");
  await page.click(`#markpane .mkitem:has-text("${text}")`);   // chosen: shown beside the list
  await page.click("#markpane .mkvgo");                          // Show in terminal
}

// the line marking a bookmark's text: its fill, the bookmarks' and the reminders' colors resolved
export const lineColor = (page) => page.evaluate(() => {
  const l = document.getElementById("markline"), v = (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  const probe = (c) => { const d = document.createElement("div"); d.style.color = c; document.body.append(d); const x = getComputedStyle(d).color; d.remove(); return x; };
  return { fill: l && getComputedStyle(l).backgroundColor, shown: !!l && !l.hidden, mark: probe(v("--mark")), remind: probe(v("--remind")) };
});

export async function selectRow(page, text) {
  await page.evaluate((text) => {
    const row = [...document.querySelectorAll("#term .ln")].find((el) => el.textContent.includes(text));
    row.scrollIntoView({ block: "center" });
    const r = document.createRange();
    r.selectNodeContents(row);
    getSelection().removeAllRanges();
    getSelection().addRange(r);
  }, text);
  await page.waitForSelector("#markadd:not([hidden])", { timeout: 3000 }).catch(() => {});
}
