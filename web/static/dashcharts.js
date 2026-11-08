// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The Dashboard's own charts (terminum.md AC-152), inline SVG as charts.js's: the working day (a day a
// column, midnight on top as a calendar's, a thin line from the first to the last event and the
// active stretches solid on it), the hour × weekday heat map, the projects ranked.
import { el } from "./listparts.js";
import { svg } from "./charts.js";
import { WEEK } from "./msgpage.js";

/** Working days: `series` [{spans: [[from, to] minutes]}], `label(i)` under a column; select(i) as charts.js's. */
export function workingDays(box, series, { label = () => "", select = () => {} } = {}) {
  const width = Math.max(160, box.clientWidth || 600), height = 220, left = 34, bottom = 18, top = 4;
  const plotW = width - left, plotH = height - top - bottom, y = (min) => top + (min / 1440) * plotH;
  const s = svg("svg", { viewBox: `0 0 ${width} ${height}`, width: "100%", height, class: "chart", "aria-hidden": "true" });
  for (const h of [0, 6, 12, 18, 24]) {
    svg("line", { x1: left, x2: width, y1: y(h * 60), y2: y(h * 60), class: "grid" }, s);
    svg("text", { x: left - 6, y: y(h * 60) + 3, class: "ylab", "text-anchor": "end" }, s).textContent = `${String(h).padStart(2, "0")}:00`;
  }
  const band = plotW / Math.max(1, series.length), w = Math.max(2, Math.min(10, band * 0.6));
  const marks = [];
  let lastLabel = -Infinity;
  series.forEach((d, i) => {
    const cx = left + i * band + band / 2;
    if (d.spans.length) {
      const first = d.spans[0][0], last = d.spans.at(-1)[1];
      svg("line", { x1: cx, x2: cx, y1: y(first), y2: y(Math.min(1440, last)), class: "span" }, s);
      for (const [a, b] of d.spans) svg("rect", { x: cx - w / 2, y: y(a), width: w, height: Math.max(1.5, y(Math.min(1440, b)) - y(a)), rx: 1, class: "stretch" }, s);
    }
    const t = label(i);
    if (t && cx - lastLabel >= 44) { svg("text", { x: cx, y: height - 4, class: "xlab", "text-anchor": "middle" }, s).textContent = t; lastLabel = cx; }
    marks.push(svg("rect", { x: left + i * band, y: 0, width: band, height: top + plotH, class: "hit" }, s));
  });
  box.replaceChildren(s);
  return pointer(box, s, marks, select, (e, r) => Math.floor(((e.clientX - r.left) / r.width * width - left) / band));
}

/** The heat map: `heat[weekday 0 = Sunday][hour]` minutes, Monday first; select({wd, hour} | null). */
export function heatMap(box, heat, { select = () => {} } = {}) {
  const narrow = (box.clientWidth || 600) < 420;
  const cols = narrow ? 12 : 24, per = 24 / cols;       // a phone: two hours a cell
  const width = Math.max(200, box.clientWidth || 600), left = 34, top = 14, cell = (width - left) / cols, h = Math.min(22, cell * 0.9);
  const height = top + h * 7 + 4;
  const rows = [1, 2, 3, 4, 5, 6, 0].map((wd) => Array.from({ length: cols }, (_, c) => heat[wd].slice(c * per, (c + 1) * per).reduce((a, b) => a + b, 0)));
  const values = rows.flat().filter((v) => v > 0).sort((a, b) => a - b);
  const q = (p) => values[Math.min(values.length - 1, Math.floor(p * values.length))] ?? 0;
  const steps = [q(0.2), q(0.4), q(0.6), q(0.8)];
  const level = (v) => (v <= 0 ? 0 : 1 + steps.filter((t) => v > t).length);
  const s = svg("svg", { viewBox: `0 0 ${width} ${height}`, width: "100%", height, class: "chart heat", "aria-hidden": "true" });
  for (let c = 0; c < cols; c += 3) svg("text", { x: left + c * cell + 1, y: 10, class: "xlab" }, s).textContent = String(c * per).padStart(2, "0");
  const marks = [];
  rows.forEach((row, r) => {
    svg("text", { x: left - 6, y: top + r * h + h / 2 + 3, class: "ylab", "text-anchor": "end" }, s).textContent = WEEK[r];
    row.forEach((v, c) => {
      const m = svg("rect", { x: left + c * cell + 1, y: top + r * h + 1, width: Math.max(1, cell - 2), height: Math.max(1, h - 2), rx: 2, class: `cell l${level(v)}` }, s);
      m.dataset.at = `${r},${c}`;
      marks.push(m);
    });
  });
  box.replaceChildren(s);
  const info = (i) => (i < 0 ? null : { wd: Math.floor(i / cols), hour: (i % cols) * per, hours: per, minutes: rows[Math.floor(i / cols)][i % cols] });
  return pointer(box, s, marks, (i) => select(info(i)), (e) => {
    const m = document.elementFromPoint(e.clientX, e.clientY)?.closest?.("rect.cell");
    return m ? marks.indexOf(m) : -1;
  }, cols);
}

/** Projects ranked: [{id, name, n, color}], the first `most` and the rest as Other; a row presses `open(p)`. */
export function ranked(box, projects, { most = 8, ink, open, other = () => {} }) {
  const shown = projects.slice(0, most), rest = projects.slice(most);
  const total = projects.reduce((s, p) => s + p.n, 0) || 1, max = Math.max(1, ...shown.map((p) => p.n), rest.reduce((s, p) => s + p.n, 0));
  const row = (name, n, color, onPress) => {
    const b = el("button", "rrow");
    b.type = "button";
    const dot = el("span", "pdot");
    dot.style.setProperty("--proj", color);
    const bar = el("span", "rbar");
    bar.style.setProperty("--w", `${(n / max) * 100}%`);
    bar.style.setProperty("--proj", color);
    b.append(dot, el("span", "rname", name), bar, el("span", "rnum", `${Math.round((n / total) * 100)}%`), el("span", "rnum", n.toLocaleString("en-US")));
    b.addEventListener("click", onPress);
    return b;
  };
  const rows = shown.map((p) => row(p.name || (p.id ? "Gone project" : "No project"), p.n, ink(p), () => open(p)));
  if (rest.length) rows.push(row(`Other (${rest.length})`, rest.reduce((s, p) => s + p.n, 0), "var(--faint)", other));
  box.replaceChildren(...(rows.length ? rows : [el("p", "msay", "Nothing sent in this period.")]));
}

/** Pointer and keys over `marks` (a grid of `cols` when given): select(i | -1). */
function pointer(box, s, marks, select, at, cols = 0) {
  let i = -1;
  const pick = (to) => { if (to === i) return; marks[i]?.classList.remove("on"); i = to; marks[i]?.classList.add("on"); select(i); };
  s.addEventListener("pointermove", (e) => { if (e.pointerType === "mouse" || e.buttons) { const r = s.getBoundingClientRect(); const to = at(e, r); pick(to >= 0 && to < marks.length ? to : -1); } });
  s.addEventListener("pointerdown", (e) => { const r = s.getBoundingClientRect(); const to = at(e, r); pick(to >= 0 && to < marks.length ? to : -1); });
  s.addEventListener("pointerleave", (e) => { if (e.pointerType === "mouse" && document.activeElement !== box) pick(-1); });
  box.onkeydown = (e) => {
    const n = marks.length;
    const to = { ArrowLeft: i - 1, ArrowRight: i + 1, ArrowUp: cols ? i - cols : undefined, ArrowDown: cols ? i + cols : undefined, Home: 0, End: n - 1 }[e.key];
    if (to !== undefined) { e.preventDefault(); pick(i < 0 ? (e.key === "ArrowLeft" ? n - 1 : 0) : Math.min(n - 1, Math.max(0, to))); }
    else if (e.key === "Escape" && i >= 0) { e.stopPropagation(); pick(-1); }
  };
  box.onblur = () => pick(-1);
}
