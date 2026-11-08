// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The Message history at /history/ (terminum.md AC-151): what the user and Dispatcher sent in panes,
// searched by its words (terminumd's FTS5, best matches by BM25) or newest first, narrowed by when,
// project, sender and kind; the day strip says when; a row opens its session at that message.
// Every choice is in the address: Back undoes one, a link shows the same.
import { el } from "./listparts.js";
import { columns } from "./charts.js";
import { showError } from "./kanbanutil.js";
import { messageRow } from "./historyrows.js";
import { $, NONE, addDays, api, dayLabel, inkOf, loadMeta, menu, monthOf, projectLabel, readQuery, shortDay, today, writeQuery } from "./msgpage.js";

const KEYS = ["q", "sort", "from", "to", "project", "sender", "kind"];
const WAIT = 200;                       // typing: a search this long after the last key
let state = readQuery(KEYS), meta = { projects: [] }, projects = new Map();
let next = null, loading = 0, typing = 0, strip = [];

const hasWords = () => /[\p{L}\p{N}]{2}/u.test(state.q.replace(/(^|\s)-\S*/g, ""));
const best = () => hasWords() && state.sort !== "new";

/** The query for terminumd: the filters, the order, a page's cursor. */
function query(cursor) {
  const q = new URLSearchParams();
  for (const k of KEYS) if (state[k]) q.set(k, state[k]);
  q.set("sort", best() ? "best" : "new");
  if (cursor) q.set("cursor", cursor);
  return q;
}

/** What the filters' keys say, from the state. */
function drawKeys() {
  if (document.activeElement !== $("q")) $("q").value = state.q;
  const when = state.from || state.to
    ? (state.from === state.to ? dayLabel(state.from) : `${state.from ? shortDay(state.from) : "…"} – ${state.to ? shortDay(state.to) : "today"}`)
    : "All kept";
  setKey("fwhen", when, !!(state.from || state.to));
  const p = projects.get(state.project);
  setKey("fproject", state.project ? projectLabel(p ?? { id: state.project === NONE ? "" : state.project }) : "All projects", !!state.project);
  setKey("fsender", { human: "You", dispatcher: "Dispatcher" }[state.sender] ?? "You and Dispatcher", !!state.sender);
  setKey("fkind", { agent: "Agents", shell: "Shells" }[state.kind] ?? "Agents and shells", !!state.kind);
  for (const b of $("hsort").querySelectorAll("button")) {
    b.setAttribute("aria-pressed", String((b.dataset.sort === "best") === best()));
    b.disabled = b.dataset.sort === "best" && !hasWords();
  }
  $("hsort").title = hasWords() ? "" : "Type words to rank by how well they match";
  $("stripclear").hidden = !(state.from || state.to);
}

function setKey(id, text, on) {
  const s = $(id).querySelector("summary");
  s.textContent = text;
  $(id).classList.toggle("on", on);
}

/** Changes the state; `push`: a step of Back. */
function change(next, push = true) {
  state = { ...state, ...next };
  writeQuery(state, KEYS, push);
  drawKeys();
  drawMenus();
  load();
}

/** A filter's menu of `key`'s values: [value, label, color?]. */
const one = (key, rows) => rows.map(([value, label, color]) => ({ label, color, on: (state[key] ?? "") === value, pick: () => change({ [key]: value }) }));

function drawMenus() {
  menu($("fproject"), one("project", [["", "All projects"], ...meta.projects.map((p) => [p.id || NONE, `${projectLabel(p)} · ${p.n.toLocaleString("en-US")}`, inkOf(p)])]));
  const t = today(), days = (label, from, to) => ({ label, on: state.from === from && state.to === to, pick: () => change({ from, to }) });
  menu($("fwhen"), [days("All kept", "", ""), days("Today", t, t), days("Last 7 days", addDays(t, -6), t), days("Last 30 days", addDays(t, -29), t)]);
  $("hfrom").value = state.from;
  $("hto").value = state.to;
  menu($("fsender"), one("sender", [["", "You and Dispatcher"], ["human", "You"], ["dispatcher", "Dispatcher"]]));
  menu($("fkind"), one("kind", [["", "Agents and shells"], ["agent", "Agents' prompts"], ["shell", "Shells' commands"]]));
}

/** The first page for the state (a newer ask wins), or the next one. */
async function load(more = false) {
  const mine = ++loading;
  $("hsum").textContent = more ? $("hsum").textContent : "Searching…";
  $("hsum").classList.add("busy");
  try {
    const page = await api(`/api/messages?${query(more ? next : null)}`);
    if (mine !== loading) return;
    showError($("herr"), "");
    $("results").classList.remove("stale");
    if (!more) { $("results").replaceChildren(); strip = page.days ?? []; drawStrip(); }
    next = page.next;
    drawRows(page.items, more);
    if (!more) {                                          // terminumd counts with the first page
      const total = page.total ?? 0;
      $("hsum").textContent = total ? `${total.toLocaleString("en-US")} ${total === 1 ? "message" : "messages"}${page.capped ? " · showing the best 1,000" : ""}` : "";
      if (!total) empty();
    }
    $("more").hidden = !next;
  } catch (e) {
    if (mine !== loading) return;
    showError($("herr"), `The messages could not be read: ${e.message}`);
    $("results").classList.add("stale");
    $("hsum").textContent = "";
  } finally {
    if (mine === loading) $("hsum").classList.remove("busy");
  }
}

function empty() {
  const box = el("div", "mempty");
  const any = meta.projects.some((p) => p.n > 0);
  box.append(el("p", "", !any ? "Nothing sent yet. The prompts and commands you send in sessions show here."
    : state.q ? `No message matches “${state.q}”${state.from || state.to || state.project || state.sender || state.kind ? " with these filters" : ""}.` : "Nothing sent with these filters."));
  if (state.from || state.to || state.project || state.sender || state.kind) {
    const b = el("button", "textkey", "Clear filters");
    b.type = "button";
    b.onclick = () => change({ from: "", to: "", project: "", sender: "", kind: "" });
    box.append(b);
  }
  $("results").append(box);
}

/** Rows, under day heads when newest first. */
function drawRows(list, more) {
  const box = $("results");
  let day = more ? box.lastElementChild?.dataset.day : null;
  for (const it of list) {
    const key = it.day;                               // terminumd's local day, as the strip counts
    if (!best() && key !== day) {
      day = key;
      const n = strip.find((s) => s.day === key)?.n;
      const h = el("div", "dayhead", `${dayLabel(key)}${n ? ` · ${n.toLocaleString("en-US")}` : ""}`);
      h.setAttribute("role", "presentation");
      box.append(h);
    }
    const row = messageRow(it, { projects, onPick: choose });
    row.dataset.day = key;
    box.append(row);
  }
}

/** The day strip: every day from the first kept to today, the chosen ones in the accent. */
function drawStrip() {
  const by = new Map(strip.map((s) => [s.day, s.n]));
  const first = meta.first && meta.first < addDays(today(), -179) ? addDays(today(), -179) : meta.first ?? today();
  const days = [];
  for (let d = first; d <= today(); d = addDays(d, 1)) days.push(d);
  if (days.length < 30) while (days.length < 30) days.unshift(addDays(days[0], -1));
  const chosen = (d) => (!state.from || d >= state.from) && (!state.to || d <= state.to) && (state.from || state.to);
  const say = (i) => {
    const words = (n) => `${n.toLocaleString("en-US")} ${hasWords() ? (n === 1 ? "match" : "matches") : n === 1 ? "message" : "messages"}`;
    if (i < 0) { $("stripsay").textContent = `${days.length} days: ${words(strip.reduce((s, x) => s + x.n, 0))}. Press a day to see it`; return; }
    $("stripsay").textContent = `${dayLabel(days[i])} · ${words(by.get(days[i]) ?? 0)}`;
  };
  const bands = days.map((d) => ({ parts: [{ value: by.get(d) ?? 0, color: chosen(d) ? "var(--accent)" : "var(--strip-ink)" }], band: chosen(d) }));
  columns($("stripplot"), bands, { height: 40, sqrt: true, xlabels: (i) => (days[i].endsWith("-01") || i === 0 ? monthOf(days[i]) : ""), select: say,
    open: (i) => change(state.from === days[i] && state.to === days[i] ? { from: "", to: "" } : { from: days[i], to: days[i] }) });
  say(-1);
}

/** The row chosen: selected, and opened when `open`. */
function choose(row, open = false) {
  for (const r of $("results").querySelectorAll('.msg[aria-selected="true"]')) if (r !== row) r.setAttribute("aria-selected", "false");
  row.setAttribute("aria-selected", "true");
  $("q").setAttribute("aria-activedescendant", row.id);
  if (open) row.open();
  row.scrollIntoView({ block: "nearest" });
}
const selected = () => $("results").querySelector('.msg[aria-selected="true"]');
function step(by) {
  const rows = [...$("results").querySelectorAll(".msg")], i = rows.indexOf(selected());
  const to = rows[i < 0 ? 0 : Math.min(rows.length - 1, Math.max(0, i + by))];
  if (to) choose(to);
  else if (by < 0) $("q").focus();
}

$("q").addEventListener("input", () => {
  state.q = $("q").value;
  writeQuery(state, KEYS, false);
  clearTimeout(typing);
  typing = setTimeout(() => { drawKeys(); load(); }, WAIT);
});
$("q").addEventListener("keydown", (e) => {
  if (e.key === "Enter") { e.preventDefault(); clearTimeout(typing); const s = selected(); if (s && e.target.dataset.ran === state.q) s.open(); else { e.target.dataset.ran = state.q; change({}, true); } }
  else if (e.key === "ArrowDown") { e.preventDefault(); step(1); }
  else if (e.key === "ArrowUp") { e.preventDefault(); step(-1); }
  else if (e.key === "Escape") { if ($("q").value) { e.preventDefault(); $("q").value = ""; change({ q: "" }); } else $("q").blur(); }
});
document.addEventListener("keydown", (e) => {
  if (e.metaKey || e.ctrlKey || e.altKey || e.target.closest?.("input, textarea, select")) return;
  const s = selected();
  if (e.key === "/") { e.preventDefault(); $("q").focus(); $("q").select(); }
  else if (e.key === "ArrowDown" || e.key === "j") { if (!e.target.closest?.(".stripplot")) { e.preventDefault(); step(1); } }
  else if (e.key === "ArrowUp" || e.key === "k") { if (!e.target.closest?.(".stripplot")) { e.preventDefault(); step(-1); } }
  else if (s && e.key === "Enter" && !e.target.closest?.("a, button, .stripplot")) { e.preventDefault(); if (s.go) s.go(); else s.open(); }
  else if (s && e.key === " " && !e.target.closest?.("a, button")) { e.preventDefault(); s.open(); s.context(); }
  else if (s && e.key === "c") { s.open(); s.copy(); }
});
$("hsort").addEventListener("click", (e) => { const b = e.target.closest("button"); if (b && !b.disabled) change({ sort: b.dataset.sort === "new" ? "new" : "" }); });
$("stripclear").onclick = () => change({ from: "", to: "" });
$("hrangego").onclick = () => { $("fwhen").open = false; change({ from: $("hfrom").value, to: $("hto").value }); };
$("more").onclick = () => load(true);
$("hhelp").onclick = () => { const box = $("hhelpbox"); box.hidden = !box.hidden; $("hhelp").setAttribute("aria-expanded", String(!box.hidden)); };
$("hfilters").onclick = () => { const on = $("hpicks").classList.toggle("open"); $("hfilters").setAttribute("aria-expanded", String(on)); };
new IntersectionObserver((seen) => { if (seen.some((s) => s.isIntersecting) && next && !$("more").hidden) load(true); }, { rootMargin: "400px" }).observe($("more"));
addEventListener("popstate", () => { state = readQuery(KEYS); drawKeys(); drawMenus(); load(); });
let stripWait = 0;
new ResizeObserver(() => { clearTimeout(stripWait); stripWait = setTimeout(drawStrip, 100); }).observe($("stripplot"));

try {
  meta = await loadMeta();
} catch (e) {
  showError($("herr"), `The filters could not be read: ${e.message}`);
}
projects = new Map(meta.projects.map((p) => [p.id || NONE, p]));
drawMenus();
drawKeys();
load();
