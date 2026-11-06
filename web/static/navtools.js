// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The session list's tools (AC-07, AC-52, AC-81, AC-89): its width, "New session", "Add host" and
// "Attach tmux sessions" (addtmux.js) at its foot, "New session" also as [+] in its head (its form then under the head), a host group's menu (retry, off, on, remove, attach its
// tmux sessions) and an attached session's (detach), the line for a tmux server that does not
// answer, the pencil that names the shown pane, and the page's confirmation dialog.

import { addTmux } from "./addtmux.js";
import { hostAgents } from "./hostagents.js";
import { inside, placeModal } from "./fabpane.js";
import { banner } from "./banners.js";

const $ = (id) => document.getElementById(id);

// Every dialog's ×, as Esc: a dialog that refuses Esc (the bookmark's while it saves) refuses it too.
// The bookmarks' windows close their own way (marks.js, markdoc.js).
for (const x of document.querySelectorAll("dialog.dlg .dhead .x:not([id])")) {
  x.onclick = () => { const d = x.closest("dialog"); if (d.dispatchEvent(new Event("cancel", { cancelable: true }))) d.close("cancel"); };
}

/** Asks `text` with a dialog: true when the user pressed `yes` (else Cancel, ×, Esc). Its question
 *  ("Close the session api?") is the title, what follows it the text. */
export function askConfirm(text, yes) {
  const box = $("confirmbox"), [, title = text, more = ""] = /^(.+?\?)\s+(.+)$/s.exec(text) ?? [];
  $("confirmtitle").textContent = title;
  $("confirmtext").textContent = more;
  $("confirmtext").hidden = !more;
  $("confirmok").textContent = yes;
  box.returnValue = "";
  box.showModal();
  $("confirmok").focus();
  return new Promise((resolve) => box.addEventListener("close", () => resolve(box.returnValue === "ok"), { once: true }));
}

/** Close Session (the menu's, a row's): asked first, as the program in it ends; then sent. */
export async function closeSession(item, send, tasks = []) {
  const prog = item.sub?.[0] || "a program";
  const open = tasks.length ? ` ${tasks.join(", ")} ${tasks.length === 1 ? "is" : "are"} open in it.` : "";
  if (await askConfirm(`Close the session ${item.title}? The program running in it (${prog}) ends.${open}`, "Close Session")) send({ t: "kill", id: item.id });
}

/** Asks for a name with a dialog: the text typed (trimmed), or null for Cancel and Esc; `yes`
 *  names its button. Enter in the field is the button (the form's first, Cancel, would be). */
export function askName(text, value, yes = "Rename", most = 40) {
  const box = $("namebox");
  $("nametext").textContent = text;
  $("nameok").textContent = yes;
  $("namefield").maxLength = most;
  $("namefield").value = value;
  $("namefield").onkeydown = (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); box.close("ok"); } };
  box.returnValue = "";
  box.showModal();
  $("namefield").select();
  return new Promise((resolve) => box.addEventListener("close", () => resolve(box.returnValue === "ok" ? $("namefield").value.trim() : null), { once: true }));
}

/** Wires the tools; `current()` is the shown pane's {group, item} or null, `onResize()` refits the terminal. */
export function navTools({ send, store, current, groups, onResize, onRenamed }) {
  // The session list's width, dragged at its border (wide screens), remembered per browser.
  const NAV_MIN = 200, NAV_MAX = 640;
  function setNavWidth(px, save) {
    const w = Math.round(Math.min(NAV_MAX, Math.max(NAV_MIN, px)));
    document.documentElement.style.setProperty("--nav-w", `${w}px`);
    $("navgrip").setAttribute("aria-valuenow", String(w));
    if (save) store.set("navWidth", w);
    onResize();
  }
  {
    const grip = $("navgrip");
    const saved = store.get("navWidth", null);
    if (saved) setNavWidth(saved, false);
    grip.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      grip.setPointerCapture(e.pointerId);
      const left = $("nav").getBoundingClientRect().left;
      const move = (ev) => setNavWidth(ev.clientX - left, false);
      const up = (ev) => {
        setNavWidth(ev.clientX - left, true);
        cancel();
      };
      const cancel = () => {
        grip.removeEventListener("pointermove", move);
        grip.removeEventListener("pointerup", up);
        grip.removeEventListener("pointercancel", cancel);
      };
      grip.addEventListener("pointermove", move);
      grip.addEventListener("pointerup", up);
      grip.addEventListener("pointercancel", cancel);
    });
    grip.addEventListener("keydown", (e) => {
      const step = { ArrowLeft: -16, ArrowRight: 16 }[e.key];
      if (step) { e.preventDefault(); setNavWidth($("nav").getBoundingClientRect().width + step, true); }
    });
  }

  // Inline forms at the list's foot: one open at a time; Esc or a second press closes it. The
  // form waits for terminumd: its error shows in the form, a new session closes it.
  let pending = null;                              // the form whose request is on its way
  const FORMS = ["newsess", "addhost", "addtmux"];
  function openForm(id, button) {
    for (const [f, b] of FORMS.map((f) => [f, `${f}btn`])) {
      const on = f === id && $(f).hidden;
      $(f).hidden = !on;
      $(b).setAttribute("aria-expanded", String(on));
      if (on) { placeModal($(f), () => closeForm(f)); $(`${f}err`).textContent = ""; $(f).querySelector("input").focus(); }
    }
    if (button && $(id).hidden) pending = null;
  }
  const closeForm = (id) => {
    $(id).hidden = true;
    $(`${id}btn`).setAttribute("aria-expanded", "false");
    if (id === "newsess") $("navnew").setAttribute("aria-expanded", "false");
    if (pending === id) pending = null;
  };
  // New session opens its form by the button pressed: under the list's head for [+], above the foot for the foot's
  const spot = $("newsess").nextElementSibling;
  $("newsessbtn").onclick = () => { if ($("newsess").hidden) spot.before($("newsess")); openForm("newsess", true); };
  $("navnew").onclick = () => {
    if ($("newsess").hidden) $("sessions").before($("newsess"));
    openForm("newsess", true);
    $("newsessbtn").setAttribute("aria-expanded", "false");   // the foot stays folded (foot.css)
    $("navnew").setAttribute("aria-expanded", String(!$("newsess").hidden));
  };
  $("addhostbtn").onclick = () => openForm("addhost", true);
  const agents = hostAgents(send);
  const tmux = addTmux({ send, groups, openForm, closeForm, setPending: (f) => { pending = f; } });
  $("addtmuxbtn").onclick = () => { if ($("addtmux").hidden) tmux.open(); else closeForm("addtmux"); };
  for (const f of FORMS) {
    $(f).addEventListener("keydown", (e) => { if (e.key === "Escape") { e.stopPropagation(); closeForm(f); } });
    $(`${f}cancel`).onclick = $(f).querySelector(".dhead .x").onclick = () => closeForm(f);
  }
  $("newsess").addEventListener("submit", (e) => {
    e.preventDefault();
    const name = $("newsessname").value.trim();
    $("newsesserr").textContent = "";
    pending = "newsess";
    send({ t: "newsession", name });
  });
  // Enter on the tick sends the form too (WebKit submits only from a text field)
  $("addhostfiles").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); $("addhost").requestSubmit(); } });
  $("addhost").addEventListener("submit", (e) => {
    e.preventDefault();
    $("addhosterr").textContent = "";
    pending = "addhost";
    send({ t: "addhost", name: $("addhostname").value.trim(), recipe: $("addhostssh").value.trim(), files: $("addhostfiles").checked });
  });
  /** An `error` from terminumd: true when it answers a form's request (shown there). */
  function formError(msg) {
    if (!pending) return false;
    $(`${pending}err`).textContent = msg;
    pending = null;
    return true;
  }
  /** A new layout: the session or host asked for is there, and its form closes. */
  function layoutCame(gs) {
    const has = (name) => gs.some((g) => g.wid === name || g.label === name || g.host === name);
    if (pending === "newsess" && has($("newsessname").value.trim())) { closeForm("newsess"); $("newsessname").value = ""; }
    tmux.layoutCame(gs);
  }
  /** terminumd added the host (its group follows in a layout): the form closes; once the host
   *  is attached its other tmux sessions are offered (AC-89). */
  function hostAdded(name) {
    if (pending !== "addhost") return;
    closeForm("addhost");
    $("addhostname").value = $("addhostssh").value = "";
    $("addhostfiles").checked = false;
    tmux.hostAdded(name);
  }
  /** terminumd attached the sessions: the form closes. */
  function tmuxAdded() {
    if (pending === "addtmux") tmux.added();
  }

  // A host group's menu (AC-81): retry now, off or on, remove (asked first); an attached
  // session's (AC-89): detach, which leaves the session running.
  function hostMenu(g, button) {
    const box = $("hostmenu");
    if (!box.hidden && box.dataset.host === g.wid) return hideHostMenu();
    box.dataset.host = g.wid;
    const act = (label, action, ask) => {
      const b = document.createElement("button");
      b.type = "button"; b.textContent = label; b.dataset.action = action;
      b.onclick = async () => {
        hideHostMenu();
        if (ask && !(await askConfirm(ask, label))) return;
        send({ t: "hostaction", name: g.host, action });
      };
      return b;
    };
    const item = (label, action, run) => {
      const b = document.createElement("button");
      b.type = "button"; b.textContent = label; b.dataset.action = action;
      b.onclick = () => { hideHostMenu(); run(); };
      return b;
    };
    if (g.attached) box.replaceChildren(item("Detach from terminum", "detach", () => send({ t: "detachtmux", server: g.server, session: g.label })));
    else box.replaceChildren(act("Retry now", "retry"), g.state === "off" ? act("On", "on") : act("Off", "off"),
      ...(g.state === "attached" ? [item("Attach tmux sessions…", "import", () => tmux.open(`${g.host}:default`))] : []),
      ...agents.item(g, hideHostMenu),
      act("Remove", "remove", `Remove ${g.host}? Its tmux sessions on the host keep running; terminum stops showing them.`));
    const r = button.getBoundingClientRect();
    box.hidden = false;
    if (!placeModal(box, hideHostMenu)) {   // under its button, above it where the window ends first, always inside the window
      const h = box.offsetHeight;
      box.style.top = `${inside(r.bottom + 4 + h <= innerHeight - 8 ? r.bottom + 4 : r.top - 4 - h, h, innerHeight)}px`;
      box.style.left = `${inside(r.right - 180, box.offsetWidth, innerWidth)}px`;
    }
    button.setAttribute("aria-expanded", "true");
  }
  function hideHostMenu() {
    $("hostmenu").hidden = true;
    for (const b of document.querySelectorAll("#sessions .hostbtn")) b.setAttribute("aria-expanded", "false");
  }
  document.addEventListener("click", (e) => { if (!$("hostmenu").hidden && !e.target.closest("#hostmenu")) hideHostMenu(); });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !$("hostmenu").hidden) hideHostMenu(); });   // Esc closes a popover

  // ---------- a tmux server not answering (AC-03, AC-14) ----------
  // Screens, keys and histories wait for it: say so, with the time it has not answered.
  const stalls = new Map();                        // server id -> [what, since (performance.now())]
  let stallTimer = 0;
  function showStall(m) {
    const key = m.key ?? m.server ?? "local", what = m.what ?? `tmux on ${(m.server ?? "local") === "local" ? "this Mac" : m.server}`;
    if (m.ok) stalls.delete(key);
    else if (!stalls.has(key)) stalls.set(key, [what, performance.now() - (m.ms ?? 0)]);
    clearInterval(stallTimer);
    if (!stalls.size) return banner("stall", "");
    const say = () => banner("stall", [...stalls.values()].map(([what, since]) =>
      `${what} is not responding (${Math.round((performance.now() - since) / 1000)} s)`).join(". "), [...stalls.keys()].join());   // the seconds are no news
    say();
    stallTimer = setInterval(say, 1000);
  }

  // The pencil names the shown pane's window (and the pane's title).
  function startRename() {
    const found = current();
    if (!found) return;
    $("renamename").value = found.item.title;
    $("current").hidden = $("rename").hidden = true;
    $("renamebox").hidden = false;
    $("renamename").focus();
    $("renamename").select();
  }
  // Done or cancelled: the keys go back to the terminal (on a touch screen the keyboard closes).
  function endRename(refocus = false) {
    $("renamename").blur();
    $("renamebox").hidden = true;
    $("current").hidden = false;
    $("rename").hidden = !current();
    if (refocus) onRenamed();
  }
  $("rename").onclick = (e) => { e.stopPropagation(); startRename(); };   // not the header's own click
  $("renamecancel").onclick = () => endRename(true);
  $("renamebox").addEventListener("submit", (e) => {
    e.preventDefault();
    send({ t: "rename", id: current()?.item.id, name: $("renamename").value });
    endRename(true);
  });
  $("renamename").addEventListener("keydown", (e) => { if (e.key === "Escape") { e.stopPropagation(); endRename(true); } });

  /** terminumd's answer to the host form or the agents dialog. */
  const hostAnswer = (m) => (m.t === "hostagents" ? agents.answered(m) : hostAdded(m.name));
  return { showStall, formError, layoutCame, hostAdded, hostAnswer, tmuxAdded, tmuxChoices: tmux.choices, importTmux: (g) => tmux.open(`${g.host}:default`), hostMenu, endRename, renaming: () => !$("renamebox").hidden };
}
