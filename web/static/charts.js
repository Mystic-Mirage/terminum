// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Charts drawn as inline SVG (terminum.md AC-151, AC-152): columns a band each, stacked or with a
// dot, a sparkline. No library: rects, one polyline, text. Colors are CSS variables, so a theme's
// change restyles them without a redraw. A plot is one key stop: ← and → choose a band, Home/End,
// Enter opens it, Esc lets it go; the pointer hovers and presses, a finger taps (and drags along).
const NS = "http://www.w3.org/2000/svg";

/** An SVG element with `attrs`, into `parent`. */
export function svg(tag, attrs = {}, parent) {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) if (v !== undefined && v !== null) e.setAttribute(k, String(v));
  parent?.append(e);
  return e;
}

/** A "nice" top for values up to `max` and its ticks (1, 2, 5 × 10ⁿ apart): the finest step that needs
 *  at most `most` + 1 of them over zero. */
export function niceTicks(max, most = 3) {
  if (!(max > 0)) return { top: 1, ticks: [0] };
  const pow = 10 ** Math.floor(Math.log10(max / (most + 1)));
  const step = [1, 2, 5, 10, 20].map((m) => m * pow).find((s) => Math.ceil(max / s - 1e-9) <= most + 1);
  const top = Math.ceil(max / step - 1e-9) * step;
  const ticks = [];
  for (let v = 0; v <= top + step / 2; v += step) ticks.push(Math.round(v * 1e6) / 1e6);
  return { top, ticks };
}

/** Columns over `bands` into `box`: each band {label (x), parts: [{value, color}], dot?, faint?, band?
 *  (a backdrop: a weekend)}. Options: height, sqrt (a root scale, no axis), axis (y labels by `fmt`),
 *  xlabels(i) → text or "", select(i | -1) on hover/focus/tap, open(i) on a press or Enter. */
export function columns(box, bands, opts = {}) {
  const { height = 200, sqrt = false, axis = !sqrt, fmt = String, xlabels = () => "", select = () => {}, open = () => {} } = opts;
  const width = Math.max(120, box.clientWidth || 600);
  const left = axis ? 34 : 0, bottom = bands.some((_, i) => xlabels(i)) ? 18 : 0, top = 4;
  const plotW = width - left, plotH = height - bottom - top;
  const totals = bands.map((b) => b.parts.reduce((s, p) => s + p.value, 0));
  const max = Math.max(1, ...totals, ...bands.map((b) => b.dot ?? 0));
  const { top: yTop, ticks } = sqrt ? { top: max, ticks: [] } : niceTicks(max);
  const y = (v) => top + plotH - (sqrt ? Math.sqrt(v / yTop) : v / yTop) * plotH;
  const s = svg("svg", { viewBox: `0 0 ${width} ${height}`, width: "100%", height, class: "chart", "aria-hidden": "true" });
  for (const t of ticks) {
    svg("line", { x1: left, x2: width, y1: y(t), y2: y(t), class: "grid" }, s);
    if (axis) svg("text", { x: left - 6, y: y(t) + 3, class: "ylab", "text-anchor": "end" }, s).textContent = fmt(t);
  }
  const band = plotW / Math.max(1, bands.length), gap = Math.max(1, band * 0.2), w = Math.max(1, band - gap);
  const marks = [];
  let lastLabel = -Infinity;
  bands.forEach((b, i) => {
    const x = left + i * band;
    if (b.band) svg("rect", { x, y: top, width: band, height: plotH, class: "backband" }, s);
    const g = svg("g", { class: `col${b.faint ? " faint" : ""}` }, s);
    let base = 0;
    for (const p of b.parts) {
      if (!(p.value > 0)) continue;
      const y0 = y(base), y1 = y(base + p.value);
      svg("rect", { x: x + gap / 2, y: y1, width: w, height: Math.max(1, y0 - y1 - (base > 0 ? 1 : 0)), rx: 1, style: `fill:${p.color}` }, g);
      base += p.value;
    }
    if (b.dot > 0) svg("circle", { cx: x + band / 2, cy: y(b.dot), r: Math.min(3.5, Math.max(2, w / 2)), class: "dot" }, g);
    const label = xlabels(i);
    if (label && x - lastLabel >= 44) {
      svg("text", { x: x + band / 2, y: height - 4, class: "xlab", "text-anchor": "middle" }, s).textContent = label;
      lastLabel = x;
    }
    marks.push(svg("rect", { x, y: 0, width: band, height: top + plotH, class: "hit" }, s));
  });
  svg("line", { x1: left, x2: width, y1: y(0), y2: y(0), class: "base" }, s);
  box.replaceChildren(s);
  return interact(box, s, marks, { select, open, width, left, band });
}

/** The keys, pointer and finger of a plot whose bands are `marks`. */
function interact(box, s, marks, { select, open, width, left, band }) {
  let at = -1;
  const pick = (i) => {
    if (i === at) return;
    marks[at]?.classList.remove("on");
    at = i;
    marks[at]?.classList.add("on");
    select(at);
  };
  const bandAt = (e) => {
    const r = s.getBoundingClientRect();
    const x = ((e.clientX - r.left) / r.width) * width;
    return Math.min(marks.length - 1, Math.max(0, Math.floor((x - left) / band)));
  };
  s.addEventListener("pointermove", (e) => { if (e.pointerType === "mouse" || e.buttons) pick(bandAt(e)); });
  s.addEventListener("pointerdown", (e) => { if (e.pointerType !== "mouse") pick(bandAt(e)); });
  s.addEventListener("pointerleave", (e) => { if (e.pointerType === "mouse" && document.activeElement !== box) pick(-1); });
  // a mouse's press opens; a finger's tap chooses, its Open key opens (a 3px column is no target)
  s.addEventListener("click", (e) => { if (e.pointerType === "mouse" || e.detail === 0) open(bandAt(e)); });
  box.onkeydown = (e) => {
    const n = marks.length;
    const to = { ArrowLeft: Math.max(0, at - 1), ArrowRight: Math.min(n - 1, at + 1), Home: 0, End: n - 1 }[e.key];
    if (to !== undefined) { e.preventDefault(); pick(at < 0 && e.key.startsWith("Arrow") ? n - 1 : to); }
    else if (e.key === "Enter" && at >= 0) { e.preventDefault(); open(at); }
    else if (e.key === "Escape" && at >= 0) { e.stopPropagation(); pick(-1); }
  };
  box.onblur = () => pick(-1);
  return { pick, at: () => at };
}

/** A sparkline of `values` into `box`: one line in the accent, its last point a dot. */
export function spark(box, values) {
  const w = 120, h = 28, max = Math.max(1, ...values);
  const s = svg("svg", { viewBox: `0 0 ${w} ${h}`, preserveAspectRatio: "none", class: "spark", "aria-hidden": "true" });
  if (values.length > 1) {
    const pts = values.map((v, i) => [(i / (values.length - 1)) * (w - 4) + 2, h - 3 - (v / max) * (h - 6)]);
    svg("polyline", { points: pts.map((p) => p.join(",")).join(" "), class: "sparkline" }, s);
    const [x, y] = pts.at(-1);
    svg("circle", { cx: x, cy: y, r: 2, class: "sparkdot" }, s);
  }
  box.replaceChildren(s);
}
