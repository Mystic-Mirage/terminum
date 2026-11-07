// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The session list's drawer: on a phone it slides over the terminal, on a computer it is a
// column the user can hide (remembered per browser); "/" opens it and filters.
const $ = (id) => document.getElementById(id);

/** `hiddenKey`: what this page remembers hiding by: the board's is its own (terminum-kanban.md AC-135). */
export function sessionDrawer({ narrow, store, drawNav, focusInput, peek, hiddenKey = "navHidden" }) {
  function openDrawer() {
    if (!narrow.matches) { $("app").classList.remove("nav-hidden"); store.set(hiddenKey, false); return; }
    $("app").classList.add("nav-open");
    $("navbtn").setAttribute("aria-expanded", "true");
    $("sessions").querySelector(".item.on")?.scrollIntoView({ block: "center" });
  }
  function closeDrawer() {
    $("app").classList.remove("nav-open");
    $("navbtn").setAttribute("aria-expanded", "false");
  }
  $("navbtn").onclick = openDrawer;
  // its buttons (✎, Plan, Copy) are theirs; a narrow window's title shows the full name and path (☰ opens the list)
  if ($("current")) $("current").onclick = (e) => { if (!e.target.closest("button")) narrow.matches ? peek() : $("filter").focus(); };

  $("scrim").onclick = closeDrawer;
  $("drawerclose").onclick = closeDrawer;
  // "/" filters the sessions, unless the keys go to a field or the terminal
  document.addEventListener("keydown", (e) => {
    if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey || e.target.closest?.("input, textarea, [contenteditable]")) return;
    e.preventDefault();
    openDrawer();
    $("filter").focus();
  });
  $("navclose").onclick = () => {
    if (narrow.matches) return closeDrawer();
    $("app").classList.add("nav-hidden"); store.set(hiddenKey, true);
  };
  $("filter").addEventListener("input", drawNav);
  $("filter").addEventListener("keydown", (e) => {
    if (e.key === "Enter") $("sessions").querySelector(".item")?.click();
    if (e.key === "Escape") { $("filter").value = ""; drawNav(); focusInput(); }
  });
  if (store.get(hiddenKey, false)) $("app").classList.add("nav-hidden");
  return { openDrawer, closeDrawer };
}
