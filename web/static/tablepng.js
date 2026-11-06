// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A table widget as a picture on the clipboard (AC-31), in the shown theme's colors:
// a copy of its HTML table is laid out out of sight by the widgets' own style sheet, then painted on
// a canvas from that layout: each cell's background, bottom line, opacity, and its text's font, color,
// alignment and strike. Another look in markdown.css or widgets.css for tables is to be painted here.
// Not an SVG foreignObject in an image: WebKit's tests read such a canvas, but a first draw that comes
// out blank is reported in Safari, which the tests here cannot run; text on a canvas has no such risk.

const WIDEST = 800;          // CSS px: the table's natural width, up to this, whatever the pane's
const SCALE = 2;             // canvas pixels per CSS px, sharp in a document
const MOST = 16e6;           // canvas pixels: the scale gives way before a browser's canvas does
const SIDE = 16384;          // canvas pixels along a side, for the same reason

/** The cell's lines as laid out: each one's text, its box and its glyphs' middles. */
function linesOf(cell) {
  const text = cell.firstChild, lines = [];
  if (!text) return lines;
  const range = document.createRange();
  let at = 0;
  for (const ch of text.data) {
    range.setStart(text, at);
    range.setEnd(text, at += ch.length);
    const box = [...range.getClientRects()].pop(), space = /\s/.test(ch), last = lines.at(-1);
    if (space && !box?.width) continue;            // collapsed
    if (last && (!box || Math.abs(box.top - last.top) < box.height / 2)) {
      last.text += ch;
      if (box) last.glyphs.push({ ch, mid: (box.left + box.right) / 2 });
      else last.glyphs.at(-1).ch += ch;            // a mark on the glyph before it
      if (box && !space) {                         // a space hanging at the line's end does not count
        last.left = Math.min(last.left, box.left);   // right to left, its first letter is its rightmost
        last.right = Math.max(last.right, box.right);
      }
    } else if (box) lines.push({ text: ch, left: box.left, right: box.right, top: box.top, height: box.height, glyphs: [{ ch, mid: (box.left + box.right) / 2 }] });
  }
  return lines;
}

function fill(ctx, color, x, y, w, h) {
  ctx.fillStyle = color;
  ctx.fillRect(x, y, w, h);
}

function paintCell(ctx, cell, origin) {
  const style = getComputedStyle(cell), box = cell.getBoundingClientRect();
  const [x, y] = [box.left - origin.left, box.top - origin.top], hair = parseFloat(style.borderBottomWidth);
  ctx.globalAlpha = Number(style.opacity);
  fill(ctx, style.backgroundColor, x, y, box.width, box.height);
  if (hair) fill(ctx, style.borderBottomColor, x, y + box.height - hair, box.width, hair);
  ctx.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
  const right = style.textAlign === "right", struck = style.textDecorationLine.includes("line-through");
  const tabular = style.fontVariantNumeric.includes("tabular-nums");   // a canvas's font cannot say it: each glyph in its place
  const { fontBoundingBoxAscent: up, fontBoundingBoxDescent: down } = ctx.measureText("x");
  ctx.fillStyle = style.color;
  for (const line of linesOf(cell)) {
    const [left, end] = [line.left - origin.left, line.right - origin.left];
    const base = line.top - origin.top + (line.height - up - down) / 2 + up;
    if (tabular) {
      ctx.textAlign = "center";
      for (const glyph of line.glyphs) ctx.fillText(glyph.ch, glyph.mid - origin.left, base);
    } else {
      ctx.textAlign = right ? "right" : "left";
      ctx.fillText(line.text.trimEnd(), right ? end : left, base);
    }
    if (struck) ctx.fillRect(left, base - up * 0.3, end - left, 1);
  }
}

/** The table laid out in `box` on a canvas, on white. */
function paint(box) {
  const origin = box.getBoundingClientRect();
  // a table whose words do not fit WIDEST goes past the box: as wide as it is, with its margin
  const [w, h] = [Math.max(origin.width, box.firstChild.getBoundingClientRect().right - origin.left + parseFloat(getComputedStyle(box).paddingRight)), origin.height];
  const scale = Math.min(SCALE, Math.sqrt(MOST / (w * h)), SIDE / Math.max(w, h));
  const canvas = Object.assign(document.createElement("canvas"), { width: Math.ceil(w * scale), height: Math.ceil(h * scale) });
  const ctx = canvas.getContext("2d");
  ctx.scale(scale, scale);
  fill(ctx, getComputedStyle(box).getPropertyValue("--t-bg").trim() || "#ffffff", 0, 0, w, h);
  for (const cell of box.querySelectorAll("th, td")) paintCell(ctx, cell, origin);
  return { canvas, reduced: scale < 1 };
}

/** `table` laid out inside widget `wg`, whose style sheet it takes: its PNG blob (a promise), and whether it had to be drawn smaller than the table. */
function tablePng(wg, table) {
  const box = document.createElement("div");
  box.className = "body table";
  box.style.cssText = `display: block; box-sizing: border-box; position: fixed; left: -100000px; top: 0; width: max-content; max-width: ${WIDEST}px; padding: var(--sp-3); overflow: visible; visibility: hidden;
    color: var(--t-fg);`;
  table.style.display = "table";
  box.append(table);
  wg.append(box);
  try {
    const { canvas, reduced } = paint(box);
    return { reduced, png: new Promise((resolve, reject) => canvas.toBlob((png) => (png ? resolve(png) : reject(new Error("the browser made no picture"))), "image/png")) };
  } finally { box.remove(); }
}

/** PNG pressed: within the click, with the clipboard's own promise, as Safari asks. */
export function copyTablePng(wg, table, notify) {
  if (!window.isSecureContext || !navigator.clipboard?.write || typeof ClipboardItem === "undefined") {
    notify("This browser can copy a picture only over HTTPS or on localhost.");
    return;
  }
  let drawn;
  try { drawn = tablePng(wg, table); } catch (e) { notify(`The table was not copied: ${e.message}`); return; }
  navigator.clipboard.write([new ClipboardItem({ "image/png": drawn.png })])
    .then(() => notify(drawn.reduced ? "Copied the table as PNG, smaller than its size: it is very long." : "Copied the table as PNG."),
      (e) => notify(`The table was not copied: ${e.message}`));
}
