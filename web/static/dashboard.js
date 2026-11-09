// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The Dashboard at /dashboard/ (terminum.md AC-152): how much the user drives sessions, from terminumd's
// numbers (the human's only, never Dispatcher's): a few figures against the period before, the
// messages and active time of each day by project, the working day, the week's hours, the projects,
// the sessions juggled. A day leads to its messages in the Message history.
import { PROJECT_HUES, el, hueColor } from "./listparts.js";
import { columns, spark } from "./charts.js";
import { heatMap, ranked, workingDays } from "./dashcharts.js";
import { showError } from "./kanbanutil.js";
import { $, NONE, WEEK, api, clock, count, dayDate, dayLabel, dotOf, hours, inkOf, loadMeta, menu, projectLabel, readQuery, shortDay, writeQuery } from "./msgpage.js";

const KEYS = ["days", "project"];
const PERIODS = ["7", "30", "90"];
let state = readQuery(KEYS), meta = { projects: [] }, data = null, mode = "n", loading = 0;
const days = () => (PERIODS.includes(state.days) ? Number(state.days) : 30);

/** The Message history of `from` … `to` (local days), the human's, of the project in view or `project` ("-": none). */
function historyUrl(from, to, project = state.project) {
  const q = new URLSearchParams({ from, to, sender: "human" });
  if (project) q.set("project", project);
  return `/history/?${q}`;
}

/** "▲ 12% vs the 30 days before"; small or missing earlier numbers say so plainly. */
function delta(now, before, { small = 20, fmt = count } = {}) {
  if (before === null || before === undefined) return "no earlier data";
  if (now === before) return `same as the ${days()} days before`;
  const up = now > before, arrow = up ? "▲" : "▼";
  if (before < small) return `${arrow} ${fmt(Math.abs(now - before))} vs the ${days()} days before`;
  return `${arrow} ${Math.round(Math.abs(now / before - 1) * 100)}% vs the ${days()} days before`;
}

function tile(label, value, sub, line) {
  const t = el("div", "tile");
  t.append(el("div", "tlabel", label), el("div", "tvalue", value));
  if (sub) t.append(el("div", "tsub", sub));
  if (line) { const s = el("div", "tspark"); spark(s, line); t.append(s); }
  return t;
}

function drawTiles() {
  const a = data.now.tiles, b = data.before, s = data.now.series;
  const pct = (x) => `${Math.round(x * 100)}%`;
  const agents = Object.entries(data.now.kinds).filter(([k]) => k !== "shell").reduce((n, [, v]) => n + v, 0);
  const perDay = a.active_days ? a.active_ms / a.active_days : 0;
  $("tiles").replaceChildren(
    tile("Messages", count(a.messages), `${delta(a.messages, b?.messages)}${a.messages ? ` · ${pct(agents / a.messages)} to agents` : ""}`, s.map((d) => d.n)),
    tile("Active time", hours(a.active_ms), `${hours(perDay)} an active day · ${delta(a.active_ms, b?.active_ms, { small: 7_200_000, fmt: hours })}`, s.map((d) => d.active_ms)),
    tile("Active days", `${a.active_days} of ${a.days}`, delta(a.active_days, b?.active_days, { small: 1000, fmt: String })),
    tile("Messages an active hour", a.per_hour ? a.per_hour.toFixed(1) : "—", b ? delta(Math.round(a.per_hour * 10), Math.round(b.per_hour * 10), { small: 1e9, fmt: (x) => (x / 10).toFixed(1) }) : "no earlier data",
      s.map((d) => (d.active_ms ? d.n / (d.active_ms / 3_600_000) : 0))),
    tile("Sessions a day", a.sessions ?? "—", `${a.peak ? `most in an hour: ${a.peak}` : ""}${b ? ` · ${delta(a.sessions ?? 0, b.sessions ?? 0, { small: 1000, fmt: String })}` : ""}`, s.map((d) => d.sessions)),
    tile("From the phone", a.phone === null ? "—" : pct(a.phone), a.phone === null ? "nothing sent from a page" : b?.phone !== null && b?.phone !== undefined ? `${pct(b.phone)} the ${days()} days before` : "of what the pages sent"),
  );
}

const xlabel = (series) => (i) => {
  const d = series[i].day, n = series.length;
  if (n <= 7) return WEEK[(dayDate(d).getDay() + 6) % 7];
  return dayDate(d).getDay() === 1 || i === 0 ? shortDay(d) : "";
};
const weekend = (d) => [0, 6].includes(dayDate(d).getDay());

/** A project's color: the session list's; one no longer there takes a hue by its rank, so the stack's
 *  parts stay apart; Other is faint, no project muted. */
function stackColor(id) {
  if (id === "other") return "var(--faint)";
  const i = data.now.projects.findIndex((x) => x.id === id), p = data.now.projects[i];
  if (!p || !p.id || (p.color !== null && p.color !== undefined)) return inkOf(p);
  return hueColor(PROJECT_HUES[i % PROJECT_HUES.length]);
}

function drawPerDay() {
  const s = data.now.series, stack = data.now.stacked, today = s.length - 1;
  const bands = s.map((d, i) => ({
    parts: mode === "n" ? [...stack, "other"].map((id) => ({ value: d.by[id] ?? 0, color: stackColor(id) })) : [{ value: d.active_ms, color: "var(--ring)" }],
    band: weekend(d.day), faint: i === today,
  }));
  const sum = mode === "n" ? `${count(data.now.tiles.messages)} messages · ${days()} days` : `${hours(data.now.tiles.active_ms)} active · ${days()} days`;
  const say = (i) => {
    if (i < 0) { $("perdaysay").textContent = sum; $("perdaygo").hidden = true; return; }
    const d = s[i], parts = mode === "n" ? Object.entries(d.by).sort((x, y) => y[1] - x[1]).slice(0, 3).map(([id, n]) => `${id === "other" ? "Other" : projectLabel(data.now.projects.find((p) => p.id === id) ?? { id })} ${n}`) : [];
    $("perdaysay").textContent = [`${dayLabel(d.day)}${i === today ? " so far" : ""}`, mode === "n" ? `${d.n} messages` : hours(d.active_ms), ...parts].join(" · ");
    $("perdaygo").href = historyUrl(d.day, d.day);
    $("perdaygo").hidden = !d.n;
  };
  columns($("perday"), bands, { height: 220, fmt: mode === "n" ? count : (v) => `${Math.round(v / 3_600_000)} h`, xlabels: xlabel(s), select: say,
    open: (i) => { if (s[i].n) location.href = historyUrl(s[i].day, s[i].day); } });
  say(-1);
  const legend = mode === "n" ? [...stack.map((id) => data.now.projects.find((p) => p.id === id)), ...(s.some((d) => d.by.other) ? [{ id: "other", name: "Other" }] : [])] : [];
  $("perdaylegend").replaceChildren(...legend.filter(Boolean).map((p) => {
    const chip = el(p.id === "other" ? "span" : "a", "chip");
    chip.append(dotOf(stackColor(p.id)), el("span", "pname", projectLabel(p)), el("span", "rnum", count(p.n ?? s.reduce((n, d) => n + (d.by.other ?? 0), 0))));
    if (p.id !== "other") { chip.href = `?${new URLSearchParams({ ...(state.days ? { days: state.days } : {}), project: p.id || NONE })}`; chip.title = "Only this project"; }
    return chip;
  }));
}

function drawDays() {
  const s = data.now.series, t = data.now.tiles;
  const say = (i) => {
    const d = s[i];
    $("dayssay").textContent = i < 0 ? (t.starts === null ? "No active day in this period" : `Usually ${clock(t.starts)} to ${clock(t.ends)}`)
      : d.spans.length ? `${dayLabel(d.day)} · ${clock(d.spans[0][0])}–${clock(d.spans.at(-1)[1])} · ${hours(d.active_ms)} active` : `${dayLabel(d.day)} · nothing`;
  };
  workingDays($("days"), s, { label: xlabel(s), select: say });
  say(-1);
}

function drawHeat() {
  $("heathint").textContent = days() <= 7 ? "active minutes, this one week" : "active minutes, an average week";
  const say = (c) => {
    $("heatsay").textContent = c ? `${WEEK[c.wd]} ${String(c.hour).padStart(2, "0")}:00–${String(c.hour + c.hours).padStart(2, "0")}:00 · ${c.minutes} min` : "Darker: more active minutes in that hour";
  };
  heatMap($("heat"), data.now.heat, { select: say });
  say(null);
}

function drawProjects() {
  const s = data.now.series;
  ranked($("projects"), data.now.projects, { ink: (p) => stackColor(p.id), open: (p) => { location.href = historyUrl(s[0].day, s.at(-1).day, p.id || NONE); } });
}

function drawSessions() {
  const s = data.now.series;
  const say = (i) => {
    $("sessionssay").textContent = i < 0 ? `Median ${data.now.tiles.sessions ?? 0} a day · most in an hour ${data.now.tiles.peak ?? 0}` : `${dayLabel(s[i].day)} · ${s[i].sessions} sessions · most in an hour ${s[i].peak}`;
  };
  columns($("sessions"), s.map((d, i) => ({ parts: [{ value: d.sessions, color: "color-mix(in srgb, var(--ring) 45%, transparent)" }], dot: d.peak, band: weekend(d.day), faint: i === s.length - 1 })),
    { height: 150, xlabels: xlabel(s), select: say, open: (i) => { if (s[i].sessions) location.href = historyUrl(s[i].day, s[i].day); } });
  say(-1);
}

function draw() {
  if (!data) return;
  const empty = !data.now.tiles.messages && !data.now.tiles.active_ms;
  drawTiles();
  for (const f of [drawPerDay, drawDays, drawHeat, drawProjects, drawSessions]) f();
  $("cards").classList.toggle("empty", empty);
}

function drawKeys() {
  for (const b of $("dperiod").querySelectorAll("button")) b.setAttribute("aria-pressed", String(Number(b.dataset.days) === days()));
  const p = meta.projects.find((x) => (x.id || NONE) === state.project);
  $("dproject").querySelector("summary").textContent = state.project ? projectLabel(p ?? { id: state.project === NONE ? "" : state.project }) : "All projects";
  $("dproject").classList.toggle("on", !!state.project);
  const row = (label, value, color) => ({ label, color, on: value === (state.project ?? ""), pick: () => change({ project: value }) });
  menu($("dproject"), [row("All projects", ""), ...meta.projects.map((x) => row(projectLabel(x), x.id || NONE, inkOf(x)))]);
}

function change(next) {
  state = { ...state, ...next };
  writeQuery(state, KEYS, true);
  drawKeys();
  load();
}

async function load() {
  const mine = ++loading;
  $("cards").classList.add("busy");
  try {
    const q = new URLSearchParams({ days: String(days()) });
    if (state.project) q.set("project", state.project);
    const got = await api(`/api/dashboard?${q}`);
    if (mine !== loading) return;
    data = got;
    showError($("derr"), "");
    draw();
  } catch (e) {
    if (mine !== loading) return;
    showError($("derr"), `The numbers could not be read: ${e.message}`);
  } finally {
    if (mine === loading) $("cards").classList.remove("busy");
  }
}

$("dperiod").addEventListener("click", (e) => { const b = e.target.closest("button"); if (b) change({ days: b.dataset.days === "30" ? "" : b.dataset.days }); });
$("perdaymode").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  mode = b.dataset.mode;
  for (const x of $("perdaymode").querySelectorAll("button")) x.setAttribute("aria-pressed", String(x === b));
  if (data) drawPerDay();
});
addEventListener("popstate", () => { state = readQuery(KEYS); drawKeys(); load(); });
document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") load(); });
// the charts fit their width: drawn again when it changes, not when a card's height does
let wait = 0, width = 0;
new ResizeObserver(([e]) => {
  if (Math.round(e.contentRect.width) === width) return;
  width = Math.round(e.contentRect.width);
  clearTimeout(wait);
  wait = setTimeout(() => { if (data) for (const f of [drawPerDay, drawDays, drawHeat, drawSessions]) f(); }, 120);
}).observe($("cards"));

try { meta = await loadMeta(); } catch (e) { showError($("derr"), `The projects could not be read: ${e.message}`); }
drawKeys();
load();
