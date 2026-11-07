// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The list of every `<select class="field">` on the board, drawn by the page instead of the platform,
// so it wears the page's finish. The select stays the control: it keeps its value and label, sits in
// the tab order and fires `change` once, on commit (Enter, Space, a press on a row, Tab), only if the
// value moved; Esc, a press outside, a scroll or a lost window put it back. A coarse pointer keeps the
// platform's picker: a finger is better served by it.
const fine = matchMedia("(pointer: fine)");
const isSelect = (e) => e?.tagName === "SELECT" && e.classList.contains("field") && !e.disabled;
let list = null, owner = null, origin = -1;

function highlight(i) {
  if (!owner || i < 0) return;
  list.children[owner.selectedIndex]?.classList.remove("active");
  owner.selectedIndex = i;
  list.children[i]?.classList.add("active");
  list.children[i]?.scrollIntoView({ block: "nearest" });
}

function place() {
  const r = owner.getBoundingClientRect(), gap = 4;
  list.style.minWidth = `${r.width}px`;
  list.style.left = `${Math.max(8, Math.min(r.left, innerWidth - list.offsetWidth - 8))}px`;
  const below = r.bottom + gap;
  list.style.top = `${below + list.offsetHeight <= innerHeight - 8 ? below : Math.max(8, r.top - gap - list.offsetHeight)}px`;
}

function ensure() {
  if (list) return;
  list = document.createElement("div");
  list.className = "dropdown";
  list.setAttribute("role", "listbox");
  list.hidden = true;
  list.addEventListener("mousedown", (e) => e.preventDefault());   // the select keeps the focus
  list.addEventListener("mouseover", (e) => { const row = e.target.closest(".dropdown-option"); if (row) highlight(Number(row.dataset.index)); });
  list.addEventListener("click", (e) => { const row = e.target.closest(".dropdown-option"); if (row) { highlight(Number(row.dataset.index)); close(false); } });
}

function open(select) {
  ensure();
  (select.closest("dialog") ?? document.body).append(list);   // a modal's top layer holds its list too
  owner = select;
  origin = select.selectedIndex;
  list.replaceChildren(...[...select.options].map((o, i) => {
    const row = document.createElement("div");
    row.className = `dropdown-option${i === origin ? " active" : ""}`;
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", String(i === origin));
    row.dataset.index = i;
    row.textContent = o.textContent;
    return row;
  }));
  list.hidden = false;
  select.setAttribute("aria-expanded", "true");
  place();
  list.children[origin]?.scrollIntoView({ block: "nearest" });
}

function close(back) {
  if (!owner) return;
  const select = owner;
  owner = null;
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
  if (!isSelect(e.target) || !fine.matches) return;
  e.preventDefault();
  e.target.focus();
  open(e.target);
});

document.addEventListener("keydown", (e) => {
  const s = e.target;
  if (!isSelect(s) || !fine.matches) return;
  if (owner !== s) {
    if (["Enter", " ", "ArrowDown", "ArrowUp"].includes(e.key)) { e.preventDefault(); open(s); }
    return;
  }
  const at = s.selectedIndex, last = s.options.length - 1;
  if (e.key === "ArrowDown") highlight(Math.min(at + 1, last));
  else if (e.key === "ArrowUp") highlight(Math.max(at - 1, 0));
  else if (e.key === "Home") highlight(0);
  else if (e.key === "End") highlight(last);
  else if (e.key === "Enter" || e.key === " ") close(false);
  else if (e.key === "Escape") { close(true); e.stopPropagation(); }   // the list, not its window
  else if (e.key === "Tab") { close(false); return; }
  else return;
  e.preventDefault();
}, true);

document.addEventListener("focusout", (e) => { if (owner && e.target === owner) close(false); });
document.addEventListener("scroll", (e) => { if (owner && e.target !== list) close(true); }, true);
addEventListener("blur", () => close(true));
addEventListener("resize", () => { if (owner) place(); });
