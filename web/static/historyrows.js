// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The Message history's rows (terminum.md AC-151): a message as the session list shows a session (its
// project's bar, the agent's mark, the session's title), what was sent under it with the words' hits
// marked, and, opened, its keys: Open in session, Context, Copy. terminumd's text is only ever text:
// its highlight's marks (\u0002 … \u0003) become <mark> here, nothing of it is markup.
import { el } from "./listparts.js";
import { icon } from "./icons.js";
import { agentMark } from "./nav.js";
import { sessionUrl } from "./tasklink.js";
import { copyText } from "./touchcopy.js";
import { toast } from "./toast.js";
import { api, dotOf, inkOf, projectLabel, timeLabel } from "./msgpage.js";

const HIT = "\u0002", HIT_END = "\u0003";
/** Lines a closed row shows. */
const LINES = 3;

/** `text` with terminumd's hit marks as <mark>s, into `box`; what goes before the first hit's line is left
 *  out when `short` and that line is past the first LINES (an ellipsis says so). */
export function marked(box, text, short) {
  let t = text;
  const first = t.indexOf(HIT);
  if (short && first >= 0) {
    const start = t.lastIndexOf("\n", first) + 1;
    if (t.slice(0, start).split("\n").length > LINES) t = `… ${t.slice(start)}`;
  }
  let hit = false;
  for (const part of t.split(/([\u0002\u0003])/)) {
    if (part === HIT) hit = true;
    else if (part === HIT_END) hit = false;
    else if (part) box.append(hit ? el("mark", "hit", part) : document.createTextNode(part));
  }
}

/** Where a message was sent: its folder (home kept short from its end), and its host when not this Mac. */
function where(it) {
  const path = (it.path ?? "").replace(/^\/Users\/[^/]+|^\/home\/[^/]+/, "~");
  return [it.host && it.host !== "local" ? it.host : "", path].filter(Boolean).join(": ");
}

/** A compact row of the context around a message: its time and first line. */
function contextRow(it, self) {
  const r = el("div", `ctxrow${self ? " self" : ""}`);
  r.append(el("span", "ctxtime", timeLabel(it.ms)), el("span", "ctxtext", (it.text.split("\n").find((l) => l.trim()) ?? "").slice(0, 200)));
  return r;
}

/** Message `it` as a row; `onPick(row, open)` when it is pressed (an opened row shows its keys). */
export function messageRow(it, { projects, onPick }) {
  const row = el("div", "msg");
  row.id = `m${it.id}`;
  row.setAttribute("role", "option");
  row.setAttribute("aria-selected", "false");
  row.dataset.id = it.id;
  const p = projects.get(it.project ?? "") ?? null;
  row.style.setProperty("--dot", inkOf(p));
  const head = el("div", "mhead");
  head.append(agentMark({ agent: it.agent }), el("span", "mtitle2", it.title || (it.agent ? "Agent" : "Shell")));
  const proj = el("span", "mproj");
  proj.append(dotOf(inkOf(p)), el("span", "pname", it.project_name || projectLabel(p ?? { id: it.project })));
  head.append(proj);
  if (it.sender === "dispatcher") {
    const tag = el("span", "mbot");
    tag.title = "Dispatcher typed it, for a task";
    tag.append(icon("bot", 12), document.createTextNode("Dispatcher"));
    head.append(tag);
  }
  const time = el("time", "mtime", timeLabel(it.ms));
  time.dateTime = new Date(it.ms).toISOString();
  time.title = new Date(it.ms).toLocaleString();
  head.append(time);
  const text = el("div", `mtext${it.agent ? "" : " shell"}`);
  marked(text, it.hl ?? it.text, true);
  // cut at its start when long (right to left), the marks keeping its slashes in place (sent.js's way)
  const foot = el("div", "mmeta", `\u200e${[where(it), it.live ? "" : "session ended"].filter(Boolean).join(" · ")}\u200e`);
  if (!it.live) foot.classList.add("gone");
  row.append(head, text, foot);
  row.addEventListener("click", (e) => { if (!e.target.closest("a, button, .ctx")) onPick(row, true); });
  row.open = () => openRow(row, it, text);
  return row;
}

/** The opened row: all its text, its keys, a place for its context. */
function openRow(row, it, text) {
  if (row.querySelector(".macts")) return;
  text.replaceChildren();
  marked(text, it.hl ?? it.text, false);
  row.classList.add("open");
  const acts = el("div", "macts");
  const go = el("a", "textkey primary", "Open in session");
  if (it.live) go.href = sessionUrl(it.key, `msg:${it.id}`);
  else { go.setAttribute("aria-disabled", "true"); go.title = "This session has ended"; }
  const ctx = el("button", "textkey", "Context");
  ctx.type = "button";
  ctx.title = "The messages sent before and after it in that pane";
  const copy = el("button", "textkey", "Copy");
  copy.type = "button";
  const box = el("div", "ctx");
  box.hidden = true;
  ctx.addEventListener("click", () => context(box, it, ctx));
  copy.addEventListener("click", async () => toast((await copyText({ text: it.text })).said));
  acts.append(go, ctx, copy);
  row.append(acts, box);
  row.copy = () => copy.click();
  row.context = () => ctx.click();
  row.go = () => { if (it.live) location.href = go.href; else toast("This session has ended"); };
}

/** The messages sent before and after `it` in its pane, read once, shown or hidden by `key`. */
async function context(box, it, key) {
  if (box.dataset.read) { box.hidden = !box.hidden; key.setAttribute("aria-expanded", String(!box.hidden)); return; }
  box.hidden = false;
  box.replaceChildren(el("p", "msay", "Reading…"));
  try {
    const c = await api(`/api/messages/${it.id}/context?before=3&after=3`);
    box.dataset.read = "1";
    const rows = [...c.before.map((x) => contextRow(x, false)), contextRow(c.item, true), ...c.after.map((x) => contextRow(x, false))];
    box.replaceChildren(...rows);
    if (!c.before.length && !c.after.length) box.append(el("p", "msay", "Nothing else was sent in that pane."));
  } catch (e) {
    box.replaceChildren(el("p", "merr", e.message));
  }
  key.setAttribute("aria-expanded", "true");
}
