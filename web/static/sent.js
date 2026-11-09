// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Sent history (AC-90): what the user sent in the shown pane's project (its host and folder),
// kept by terminumd in its message history (the Message history at /history/ searches it all). On Enter the page tells terminumd what stood at the cursor: a coding
// agent's input box (its hooks, when they speak, give the prompt exactly and this is dropped),
// or a shell's command line after its prompt; nothing in other programs (an editor, less). The
// button under the split opens the last one as Markdown (a command as a shell block), ↑ older,
// ↓ newer, Copy; at its foot Jump to message brings it into view in the shown pane (terminumd
// finds its text nearest the line it was sent on, which Enter tells with it) and Insert into
// chat pastes it into the shown pane's input.
import { typedEnd } from "./cursor.js";
import { fabPane } from "./fabpane.js";
import { when } from "./listparts.js";
import { agentName } from "./nav.js";
import { markdownInto } from "./widgetui.js";

const $ = (id) => document.getElementById(id);
// where a shell's prompt ends: "$ ", "% ", "# ", "❯ ", "› ", "> ", "» " after a space or the line's start
// (a variation selector may follow the mark: Starship's "at 19:20 ❯︎ ")
const PROMPT = /(^|\s)[$%#❯›>»][\uFE0E\uFE0F]? /;
const BOX_PROMPT = /^[❯>›!]\s?/;     // an agent's input box: its first row's prompt
const WRAPS = 10;                     // the most rows one command line wraps over

/** The text sent by Enter and the row it begins on: in an agent's input box {top, bottom} the
 *  typed text of its rows (the prompt and the next rows' two columns dropped); else, at a shell
 *  prompt (`shell`), the cursor's line with the rows it wrapped from, after the prompt; text ""
 *  otherwise. Pure: rows {txt, hard (a line ends there), cur (the cursor's row), end (the column
 *  after its typed text)}. */
export function sentAt(rows, box, shell) {
  if (box) {
    const row = Math.max(0, box.top + 1);
    const lines = rows.slice(row, box.bottom).map((r, i) => {
      const txt = r.end == null ? r.txt : [...r.txt].slice(0, r.end).join("");
      return i === 0 && box.top >= -1 ? txt.replace(BOX_PROMPT, "") : txt.replace(/^ {2}/, "");
    });
    return { text: lines.join("\n").trim(), row };
  }
  const cur = rows.findIndex((r) => r.cur);
  if (!shell || cur < 0) return { text: "", row: -1 };
  let first = cur;
  while (first > 0 && cur - first < WRAPS && !rows[first - 1].hard) first--;
  const line = rows.slice(first, cur + 1).map((r) => r.txt).join("");
  const m = PROMPT.exec(line);
  // only its end trimmed: a command after a space is one the shell keeps out of its history, and so does terminumd
  return { text: (m ? line.slice(m.index + m[0].length) : line.trimStart()).trimEnd(), row: first };
}

/** The text sent by Enter (sentAt's). */
export const sentText = (rows, box, shell) => sentAt(rows, box, shell).text;

/** The sent history's button and pane. view: term.js's view; item(): the shown pane's row;
 *  toChat(text): pasted into the shown pane's input; jump({text, n}): brings a message into view
 *  (marks.js jumpText). */
export function sentHistory({ view, send, item, copy, toast, toChat, jump }) {
  let n = 0, last = null;
  const pane = fabPane({ fab: $("sentfab"), pane: $("sentpane"), close: $("spclose"), onToggle: (on) => { if (on) ask(0); } });

  // the screen's rows, each with its absolute line (n)
  function rows() {
    return view.screen.map((r, i) => r && { txt: r.txt, hard: !!r.data?.e, cur: !!r.el.querySelector(".cur"), end: typedEnd(r.orig ?? r.data), n: view.top + i }).filter(Boolean);
  }
  const say = (text, failed = false) => { $("spwhy").textContent = text; $("spwhy").hidden = !text; $("spwhy").classList.toggle("err", failed); };

  async function draw(m) {
    last = m;
    say("");
    const body = $("spbody"), it = m.item;
    $("sppos").textContent = m.error ? "" : m.total ? `${m.n + 1} of ${m.total}` : "";
    $("spolder").disabled = !!m.error || m.n + 1 >= m.total;
    $("spnewer").disabled = !!m.error || m.n === 0;
    $("spcopy").disabled = $("spinsert").disabled = $("spjump").disabled = !it;
    // cut at its start when long (right to left), the marks keeping its slashes in place
    $("spwhere").textContent = m.path ? `\u200e${m.host && m.host !== "local" ? `${m.host}: ${m.path}` : m.path}\u200e` : "";
    $("spmeta").textContent = it ? [agentName(it.agent) ?? "Shell", it.title, when(it.at)].filter(Boolean).join(" · ") : "";
    if (m.error || !it) { body.className = "quiet"; body.textContent = m.error ?? "Nothing sent in this folder yet."; pane.place(); return; }
    body.className = "markdown";
    const fence = "`".repeat(Math.max(3, ...[...it.text.matchAll(/`+/g)].map((x) => x[0].length + 1)));   // longer than any run in it
    const md = it.agent ? it.text : `${fence}sh\n${it.text}\n${fence}`;
    await markdownInto(body, md);
    pane.place();
  }

  function ask(k) { n = Math.max(0, k); send({ t: "sentget", n }); }

  $("spolder").onclick = () => ask(n + 1);
  $("spnewer").onclick = () => ask(n - 1);
  $("spcopy").onclick = () => { if (last?.item) { copy(last.item.text); toast("Copied"); } };
  $("spinsert").onclick = () => { if (last?.item) { pane.toggle(false); toChat(last.item.text); } };
  // where it was sent: nearest its line in its own pane, else nearest the end; not found, the pane says so
  $("spjump").onclick = async () => {
    const it = last?.item;
    if (!it) return;
    say("Looking for it…");
    const r = await jump({ text: it.text, n: it.key === item()?.id ? it.line : undefined, prompt: !!it.agent });
    if (r.cancelled) return say("");
    if (r.why) return say(r.why, true);
    say("");
    pane.toggle(false);
  };
  // the Message history's Open in session (terminum.md AC-151): its message asked by id once this pane is
  // shown, then found as Jump to message finds it; what cannot be found is said where the page is
  let wanted = null;
  document.addEventListener("sentjump", (e) => { wanted = e.detail; send({ t: "sentget", id: wanted }); });
  async function jumpTo(m) {
    wanted = null;
    const it = m.item;
    if (m.error || !it) return toast(m.error ?? "That message is no longer kept.", 4000);
    const r = await jump({ text: it.text, n: it.key === item()?.id ? it.line : undefined, prompt: !!it.agent });
    if (r.why) toast(r.why, 4000);
  }
  $("sentpane").addEventListener("keydown", (e) => {
    if (e.key === "ArrowUp" && !$("spolder").disabled) { e.preventDefault(); ask(n + 1); }
    else if (e.key === "ArrowDown" && !$("spnewer").disabled) { e.preventDefault(); ask(n - 1); }
  });

  return {
    /** Enter goes to the shown pane now: what it sends. */
    enter() {
      const it = item();
      if (!it || !(it.agent || it.shell)) return;                    // an editor, less: nothing is sent
      const all = rows(), { text, row } = sentAt(all, it.agent ? view.box : null, it.shell);
      if (text) send({ t: "sent", text, agent: it.agent ?? "", line: all[row]?.n });
    },
    onMessage(m) {
      // the answer to the last ask for the pane shown; an older one (a quick ↑, another pane) is dropped
      if (m.t === "sentone" && m.id !== undefined) { if (m.id === wanted) void jumpTo(m); }
      else if (m.t === "sentone" && pane.open() && (m.error || (m.n === n && m.sid === item()?.id))) void draw(m);
      else if (m.t === "sentnew" && pane.open() && last && m.host === last.host && m.path === last.path && n === 0) ask(0);
    },
    /** Another pane is shown: its project's history, when open. */
    shown() { if (pane.open()) ask(0); },
    place: pane.place,
  };
}
