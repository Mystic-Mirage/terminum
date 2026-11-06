// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Dragging in the session list (AC-07, AC-52, AC-92): a row by its body within its group (tmux
// reorders that session's windows; a project's sessions in the project), a project or a tmux group
// by its header anywhere among them (every page's). What can move carries data-drag (nav.js). A mouse drags once it moves 6 px (a
// press that does not still clicks); a finger rests 350 ms to lift it, then drags it, or lifts
// without moving for the row's menu; moving sooner scrolls. The element itself is the landing
// slot (dashed, listdrag.css); a copy of it follows the pointer. Esc puts it back.

const MOVE = 6, STILL = 10, LIFT = 350, EDGE = 32;
const FREE = ".clone, .acts, input, textarea, select, [contenteditable], .rename";   // presses that are theirs

/** `done(kind, container, ids)` gets the new order; `ended()` after any drag; `held(li, x, y)`:
 *  a finger lifted a row and let go without moving (its menu). */
export function bindReorder(list, done, ended = () => {}, held = () => {}) {
  let press = null, drag = null, swallow = false, raf = 0;
  const swallowFor = (ms) => { swallow = true; setTimeout(() => { swallow = false; }, ms); };   // the click this press ends in
  const others = (el) => [...el.parentElement.children].filter((c) => c !== el && !c.classList.contains("dragghost"));
  const target = (e) => {
    if (e.target.closest(FREE)) return null;
    const h = e.target.closest("h2[data-drag]");
    if (h) return { el: h.closest("section"), kind: h.dataset.drag };
    const li = e.target.closest("li");
    return li?.dataset.drag ? { el: li, kind: "row" } : null;
  };
  list.addEventListener("pointerdown", (e) => {
    if (drag || e.button > 0) return;
    clear();                                       // a press left from before (its lift went elsewhere)
    const t = target(e);
    if (!t) return;
    press = { ...t, id: e.pointerId, x: e.clientX, y: e.clientY, touch: e.pointerType !== "mouse", lifted: false };
    if (press.touch) press.timer = setTimeout(() => { press.lifted = true; press.el.classList.add("lifted"); }, LIFT);
  });
  function start(e) {
    const { el, kind } = press, r = el.getBoundingClientRect();
    const start = [...el.parentElement.children].indexOf(el), ghost = el.cloneNode(true);
    for (const n of [ghost, ...ghost.querySelectorAll("[id]")]) n.removeAttribute("id");
    ghost.classList.remove("lifted");
    ghost.classList.add("dragghost");
    ghost.setAttribute("aria-hidden", "true");
    ghost.inert = true;
    ghost.style.width = `${r.width}px`;
    el.classList.remove("lifted");
    el.classList.add("dragging");
    el.parentElement.append(ghost);
    list.classList.add("reordering");
    try { list.setPointerCapture(e.pointerId); } catch { /* the pointer is gone */ }
    let sc = list;                                 // what scrolls under the dragged row near an edge
    for (let n = list; n; n = n.parentElement) if (/auto|scroll/.test(getComputedStyle(n).overflowY) && n.scrollHeight > n.clientHeight) { sc = n; break; }
    drag = { el, kind, ghost, dx: press.x - r.left, dy: press.y - r.top, y: e.clientY, start, sc, box: sc.getBoundingClientRect() };
    press = null;
    place(e);
    raf = requestAnimationFrame(scroll);
  }
  function place(e) {
    drag.y = e.clientY;
    let r = null;                                  // the rows read before the copy is moved (one layout)
    const over = others(drag.el).find((c) => { r = c.getBoundingClientRect(); return e.clientY >= r.top && e.clientY <= r.bottom; });
    drag.ghost.style.left = `${e.clientX - drag.dx}px`;
    drag.ghost.style.top = `${e.clientY - drag.dy}px`;
    if (!over) return;
    const before = e.clientY < r.top + r.height / 2 ? over : over.nextSibling;
    if (before !== drag.el && before !== drag.el.nextSibling) drag.el.parentElement.insertBefore(drag.el, before);
  }
  // near the list's top or bottom edge the list scrolls under the dragged row
  function scroll() {
    if (!drag) return;
    const r = drag.box, d = drag.y < r.top + EDGE ? -8 : drag.y > r.bottom - EDGE ? 8 : 0;
    if (d) drag.sc.scrollTop += d;
    raf = requestAnimationFrame(scroll);
  }
  list.addEventListener("pointermove", (e) => {
    if (drag) { place(e); return; }
    if (!press || e.pointerId !== press.id) return;
    if (!press.el.isConnected) return clear();     // a new layout redrew the list under the press
    const far = Math.hypot(e.clientX - press.x, e.clientY - press.y);
    if (!press.touch) { if (far > MOVE) start(e); }
    else if (press.lifted) { if (far > STILL) start(e); }
    else if (far > STILL) clear();                 // moved before the lift: the list scrolls
  });
  // once lifted, a finger's move is the drag's, not the list's scroll
  list.addEventListener("touchmove", (e) => { if (drag || press?.lifted) e.preventDefault(); }, { passive: false });
  function clear() {
    if (!press) return;
    clearTimeout(press.timer);
    press.el.classList.remove("lifted");
    press = null;
  }
  function end(commit) {
    cancelAnimationFrame(raf);
    const { el, kind, ghost, start } = drag;
    drag = null;
    swallowFor(0);                                 // the click this press ends in is the drag's
    ghost.remove();
    el.classList.remove("dragging");
    list.classList.remove("reordering");
    const parent = el.parentElement;
    if (!commit) parent.insertBefore(el, others(el)[start] ?? null);   // back where it was
    else if ([...parent.children].indexOf(el) !== start) {
      const ids = [...parent.children].map((c) => (kind === "part" ? c.dataset.part : c.querySelector(".item")?.dataset.id)).filter(Boolean);
      done(kind, parent, ids);
    }
    ended();
  }
  list.addEventListener("pointerup", (e) => {
    if (drag) return end(true);
    if (press?.lifted && press.kind === "row") {   // lifted and let go: the row's menu
      const { el } = press;
      clear();
      swallowFor(400);
      held(el, e.clientX, e.clientY);
    } else clear();
  });
  list.addEventListener("pointercancel", () => { if (drag) end(false); else clear(); });   // the system took the touch
  list.addEventListener("click", (e) => { if (swallow) { swallow = false; e.preventDefault(); e.stopImmediatePropagation(); } }, true);
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || !drag) return;
    e.preventDefault();
    e.stopPropagation();
    end(false);
  }, true);
  return { dragging: () => drag !== null };
}

/** The session list's projects and tmux groups as one list, in terminumd's `order` ("p:<id>", "g:<group>"):
 *  [{id, project} or {id, group}]. One it does not name: a project after the last project (else first), a
 *  group at the end, as `groups` has them; none named: the projects first, as before they could mix. */
export function arranged(groups, projects, order = []) {
  const at = new Map(order.map((id, i) => [id, i]));
  const parts = [...projects.map((project) => ({ id: `p:${project.id}`, project })), ...groups.map((group) => ({ id: `g:${group.wid}`, group }))];
  const list = parts.filter((p) => at.has(p.id)).sort((a, b) => at.get(a.id) - at.get(b.id));
  for (const part of parts.filter((p) => !at.has(p.id))) {
    if (part.project) list.splice(list.findLastIndex((p) => p.project) + 1, 0, part);
    else list.push(part);
  }
  return list;
}

/** `ids`, the parts drawn in their new order, in the places such parts held in `all` (the list's ids,
 *  whatever a filter hides or a group left empty): a part not drawn stays where it is; one `all` does
 *  not know yet (a group renamed) comes with them. */
export function moved(all, ids) {
  const drawn = new Set(ids);
  let k = 0;
  const list = all.map((id) => (drawn.has(id) ? ids[k++] : id));
  return [...list, ...ids.slice(k)];
}

/** The list's projects and groups in the order `ids` (moved), drawn at once (`draw`); terminumd tells
 *  every page. st: app.js's state; all(): the list's ids now. */
export const listOrderer = (st, all, send, draw) => (ids) => {
  st.listOrder = moved(all(), ids);
  send({ t: "project", action: "order", ids: st.listOrder });
  draw();
};

/** Groups in the order this browser kept before the list had one order (by their ids): a tie-break
 *  among those terminumd's order does not name; those it has not seen go last. */
export function ordered(groups, order) {
  const at = (g) => { const i = order.indexOf(g.wid); return i < 0 ? Infinity : i; };
  return groups.map((g, i) => [g, i]).sort((a, b) => at(a[0]) - at(b[0]) || a[1] - b[1]).map(([g]) => g);
}
