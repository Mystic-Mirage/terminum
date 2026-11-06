// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The output widgets' own elements (AC-56). A widget lives in a shadow root on a terminal row
// (widgets.js), so the row's text, its selection and copying, the links' underlines and the
// cursor's maths see only the terminal's own text. Content from the terminal is put in as text,
// or as the panel's Markdown (raw HTML off); a diagram is an image, so nothing in it runs.
import { icon } from "./icons.js";
import { MERMAID_LANG } from "./outparse.js";
import { renderers } from "./renderlib.js";
import { copyTablePng } from "./tablepng.js";
import { openZoom } from "./zoom.js";

/** `md` drawn as Markdown into `el` by the panel's rules (raw HTML off), as text when the
 *  renderers do not load. */
export async function markdownInto(el, md) {
  try { el.innerHTML = (await renderers()).markdownHtml(md); } catch { el.textContent = md; }
}

// The widgets' style sheet, shared by every shadow root (widgets.css), loaded with the page:
// widgets are attached only once it is in, as one drawn unstyled first changes its height
// after the page placed the view.
let sheet = null, loaded = null;
export function loadStyles() {
  sheet ??= new CSSStyleSheet();
  // with Markdown's look, which the page's panes share (markdown.css)
  return (loaded ??= Promise.all(["widgets.css", "markdown.css"].map((f) => fetch(f).then((r) => r.text()).catch(() => ""))).then((css) => sheet.replace(css.join("\n")))
    .catch(() => { /* unstyled, but working */ }));
}

/** The shadow root of row element `el` (made once: a row's text shows through its slot). */
export function hostOf(el, onClick) {
  if (el.shadowRoot) return el.shadowRoot;
  const root = el.attachShadow({ mode: "open" });
  root.adoptedStyleSheets = [sheet];
  root.append(document.createElement("slot"));
  root.addEventListener("click", (e) => {
    // the row's own text shows through the slot: its clicks are the terminal's (a path, ⌥-click)
    if (e.composedPath()[0]?.getRootNode?.() !== root) return;
    e.stopPropagation();                           // the terminal's own clicks (⌘-click, ⌥-click) are not for it
    const a = e.target.closest("a[href]");
    if (a) {
      e.preventDefault();                          // the page never navigates away
      if (/^https?:/i.test(a.getAttribute("href"))) window.open(a.href, "_blank", "noopener,noreferrer");
      return;
    }
    const b = e.target.closest("[data-act]");
    if (b) onClick(b, root);
  });
  return root;
}

/** `md` drawn into `host`'s shadow root as a Markdown widget's body (widgets.css), its links the
 *  page's as a widget's: the bookmarks' navigator shows a record as the terminal shows Markdown. */
export async function markdownView(host, md) {
  const styled = loadStyles();                    // makes the sheet the root adopts
  const root = hostOf(host, () => {});
  await styled;
  let body = root.querySelector(".body");
  if (!body) root.append(body = Object.assign(document.createElement("div"), { className: "body markdown wrap" }));   // long lines wrap (widgets.css)
  await markdownInto(body, md);
}

/** The event came from inside a widget (its taps are not the terminal's). */
export const inWidget = (e) => e.composedPath?.().some((n) => n instanceof Element && n.hasAttribute("data-wg"));

const NAMES = { code: "Code", markdown: "Markdown", mermaid: "Diagram", table: "Table" };

function button(act, label, title, iconName) {
  const b = document.createElement("button");
  b.type = "button";
  b.dataset.act = act;
  if (iconName) b.append(icon(iconName, 14));
  if (label) b.append(label);
  if (title) { b.title = title; b.setAttribute("aria-label", title); }
  return b;
}

/** A region's widget: its bar (name, Raw | rendered, Copy, PNG or Full screen) and its body, filled
 *  when the renderer answers. wg.draw() draws the body (once); the page calls it when the widget
 *  is first shown rendered. ctx: {key, dark, change(fn), raw()}: change runs a change to the
 *  page so the reader's place is kept; raw: it could not be drawn, so the region shows as text,
 *  with the error in the bar, and the widget is marked failed (data-failed: made again on the
 *  next try). */
export function regionWidget(region, ctx) {
  const wg = document.createElement("div");
  wg.className = "wg";
  wg.dataset.wg = "";
  wg.dataset.key = ctx.key;
  wg.dataset.kind = region.kind;
  const bar = document.createElement("div");
  bar.className = "bar";
  const name = document.createElement("span");
  name.className = "name";
  name.textContent = region.kind === "code" && region.lang ? region.lang : NAMES[region.kind];
  const seg = document.createElement("span");
  seg.className = "seg";
  seg.append(button("raw", "Raw", ""), button("on", NAMES[region.kind], ""));
  const err = document.createElement("span");
  err.className = "err";
  err.hidden = true;
  bar.append(name, seg, button("copy", "Copy", "Copy the source", "copy"));
  if (region.kind === "table") {
    bar.append(button("png", "PNG", "Copy the table as a PNG picture", "image"));
    wg.copyPng = (notify) => copyTablePng(wg, tableElement(region.table), notify);
  }
  if (region.kind === "mermaid") bar.append(button("zoom", "", "Full screen", "maximize-2"));
  bar.append(err);
  const body = document.createElement("div");
  body.className = `body ${region.kind}`;
  wg.append(bar, body);
  const say = (e) => ctx.change(() => { err.textContent = e.message || String(e); err.hidden = false; });
  const fail = (e) => {
    say(e);
    if (region.kind === "code") return;            // code shows as plain text; the others would show nothing
    wg.dataset.failed = "";
    ctx.raw();
  };
  wg.draw = () => {
    wg.draw = () => {};
    if (region.kind === "code") {
      const pre = document.createElement("pre"), code = document.createElement("code");
      code.textContent = region.text;
      pre.append(code);
      body.append(pre);
      renderers().then((r) => r.highlight(code, region.text, region.lang)).catch(fail);
    } else if (region.kind === "table") {
      body.append(tableElement(region.table));
    } else if (region.kind === "markdown") {
      renderers().then(async (r) => {
        ctx.change(() => { body.innerHTML = r.markdownHtml(region.text); });   // the panel's rules: raw HTML is off
        for (const code of body.querySelectorAll("pre > code[class*='language-']")) {
          const lang = [...code.classList].find((c) => c.startsWith("language-")).slice(9);
          // a diagram that does not draw keeps its source, and its bar says why
          if (MERMAID_LANG.test(lang)) void diagram(code.textContent, ctx.dark).then((img) => ctx.change(() => code.parentElement.replaceWith(img)), say);
          else await r.highlight(code, code.textContent, lang);
        }
      }).catch(fail);
    } else {
      diagram(region.text, ctx.dark).then((img) => ctx.change(() => body.replaceChildren(img)), fail);
    }
  };
  return wg;
}

/** A drawn table's cells (drawntable.js) as a table, put in as text. */
function tableElement({ head, rows, align, marks }) {
  const table = document.createElement("table");
  const line = (cells, tag, mark = "") => {
    const tr = document.createElement("tr");
    if (mark) tr.className = mark === "+" ? "add" : "del";   // a diff's row (difftable.js)
    cells.forEach((text, c) => {
      const cell = document.createElement(tag);
      cell.textContent = text;
      if (align[c]) cell.className = "num";
      tr.append(cell);
    });
    return tr;
  };
  if (head) table.createTHead().append(line(head, "th"));
  table.createTBody().append(...rows.map((r, i) => line(r, "td", marks?.[i])));
  return table;
}

/** A Mermaid diagram as an image (SVG data URL, the panel's own: mermaid.ts), opened full screen on a click. */
async function diagram(source, dark) {
  const img = await (await renderers()).diagramImage(source, dark);   // in place at its size at once: the page does not move later
  img.dataset.act = "zoom";
  return img;
}

/** Opens the widget's diagram (or the one clicked in a Markdown widget) full screen. */
export function zoomDiagram(b, onClose) {
  const img = b.tagName === "IMG" ? b : b.closest(".wg")?.querySelector("img.diagram");
  if (img) openZoom(img.src, { alt: img.alt, vector: true, onClose });
}
