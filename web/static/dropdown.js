// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The list of every `<select class="field">` on the board, drawn by the page instead of the platform,
// so it wears the page's finish. The select stays the control: it keeps its value and label, sits in
// the tab order and fires `change` once, on commit (Enter, Space, a press on a row, Tab), only if the
// value moved; Esc, a press outside, a scroll or a lost window put it back. A coarse pointer keeps the
// platform's picker: a finger is better served by it.
import { inside } from "./fabpane.js";
import { el } from "./listparts.js";

const fine = matchMedia("(pointer: fine)");
const isSelect = (e) => fine.matches && e?.tagName === "SELECT" && e.classList.contains("field") && !e.disabled;
const list = el("div", "dropdown");
list.setAttribute("role", "listbox");
list.hidden = true;
let owner = null, origin = -1;
// its options changed under it (a project gone): what it showed is no more
const watcher = new MutationObserver(() => close(false));

function highlight(i) {
  if (i < 0 || owner.options[i]?.disabled) return;
  list.children[owner.selectedIndex]?.classList.remove("active");
  owner.selectedIndex = i;
  list.children[i]?.classList.add("active");
  list.children[i]?.scrollIntoView({ block: "nearest" });
}

/** The next row from `at` by `by` that can be chosen; none: where it is. */
function step(at, by) {
  for (let i = at + by; i >= 0 && i < owner.options.length; i += by) if (!owner.options[i].disabled) return i;
  return at;
}

function place() {
  const r = owner.getBoundingClientRect(), gap = 4, h = list.offsetHeight;
  list.style.minWidth = `${r.width}px`;
  list.style.left = `${inside(r.left, list.offsetWidth, innerWidth)}px`;
  list.style.top = `${r.bottom + gap + h <= innerHeight - 8 ? r.bottom + gap : Math.max(8, r.top - gap - h)}px`;
}

const hit = (e) => {
  const row = e.target.closest(".dropdown-option");
  if (row) highlight(Number(row.dataset.index));
  return row && !row.hasAttribute("aria-disabled");
};
list.addEventListener("mousedown", (e) => e.preventDefault());   // the select keeps the focus
list.addEventListener("mouseover", hit);
list.addEventListener("click", (e) => { if (hit(e)) close(false); });

function open(select) {
  (select.closest("dialog") ?? document.body).append(list);   // a modal's top layer holds its list too
  owner = select;
  origin = select.selectedIndex;
  list.replaceChildren(...[...select.options].map((o, i) => {
    const row = el("div", "dropdown-option", o.textContent);
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", String(i === origin));
    if (o.disabled) row.setAttribute("aria-disabled", "true");
    row.dataset.index = i;
    return row;
  }));
  list.hidden = false;
  select.setAttribute("aria-expanded", "true");
  place();
  highlight(origin);
  watcher.observe(select, { childList: true });
}

function close(back) {
  if (!owner) return;
  const select = owner;
  owner = null;
  watcher.disconnect();
  list.hidden = true;
  select.removeAttribute("aria-expanded");
  if (back) select.selectedIndex = origin;
  else if (select.selectedIndex !== origin) select.dispatchEvent(new Event("change", { bubbles: true }));
}

document.addEventListener("mousedown", (e) => {
  if (owner) {
    if (list.contains(e.target)) return;
    const same = e.target === owner;
    close(true);
    if (same) { e.preventDefault(); return; }
  }
  if (!isSelect(e.target)) return;
  e.preventDefault();
  e.target.focus();
  open(e.target);
});

document.addEventListener("keydown", (e) => {
  const s = e.target;
  if (!isSelect(s)) return;
  if (owner !== s) {
    if (["Enter", " ", "ArrowDown", "ArrowUp"].includes(e.key)) { e.preventDefault(); open(s); }
    return;
  }
  const at = s.selectedIndex;
  if (e.key === "ArrowDown") highlight(step(at, 1));
  else if (e.key === "ArrowUp") highlight(step(at, -1));
  else if (e.key === "Home") highlight(step(-1, 1));
  else if (e.key === "End") highlight(step(s.options.length, -1));
  else if (e.key === "Enter" || e.key === " ") close(false);
  else if (e.key === "Escape") { close(true); e.stopPropagation(); }   // the list, not its window
  else if (e.key === "Tab") return close(false);
  else return;
  e.preventDefault();
}, true);

document.addEventListener("focusout", (e) => { if (owner && e.target === owner) close(!document.hasFocus()); });
document.addEventListener("scroll", (e) => { if (owner && e.target !== list) close(true); }, true);
addEventListener("blur", () => close(true));
addEventListener("resize", () => { if (owner) place(); });
